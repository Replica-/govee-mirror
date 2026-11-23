//! DXGI Desktop Duplication, shrunk on the GPU.
//!
//! The desktop arrives as a GPU texture. One shader pass averages it down
//! ([`super::shrink`]), and the small image comes back to the CPU: about 256
//! pixels wide, so a frame costs well under a megabyte of readback. A screen that does not change delivers no frame, and costs
//! nothing.
//!
//! Every call into Direct3D is `unsafe`: the `windows` crate marks all COM
//! calls so. Each one passes owned interfaces and stack values that outlive
//! the call.
#![allow(unsafe_code)]

use std::time::Duration;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1,
    IDXGIFactory1, IDXGIOutput, IDXGIOutput1, IDXGIOutput5, IDXGIOutputDuplication, IDXGIResource,
};
use windows::core::{Interface, Result};

use super::shrink::Shrink;

/// The width the readback reaches at least.
const READBACK_WIDTH: u32 = 256;

pub struct Duplicator {
    /// One name per screen, in the order that `new` takes its index.
    pub screens: Vec<String>,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    chain: Option<Shrink>,
}

impl Duplicator {
    /// Duplicate the screen at `index`, over the adapter that drives it.
    pub fn new(index: usize) -> Result<Self> {
        let outputs = outputs()?;
        let screens = outputs.iter().map(|(_, _, name)| name.clone()).collect();
        let (adapter, output, _) = outputs
            .into_iter()
            .nth(index)
            .or_else(|| outputs_first().ok().flatten())
            .ok_or_else(|| {
                windows::core::Error::from_hresult(windows::Win32::Foundation::E_INVALIDARG)
            })?;

        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device: ID3D11Device = device.ok_or_else(windows::core::Error::empty)?;
        let context: ID3D11DeviceContext = context.ok_or_else(windows::core::Error::empty)?;

        // An HDR screen duplicates in 16-bit float unless the call names an
        // 8-bit format, which `DuplicateOutput1` can and `DuplicateOutput`
        // cannot.
        let duplication = match output.cast::<IDXGIOutput5>() {
            Ok(output5) => {
                unsafe { output5.DuplicateOutput1(&device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM]) }?
            }
            Err(_) => unsafe { output.cast::<IDXGIOutput1>()?.DuplicateOutput(&device) }?,
        };

        Ok(Self {
            screens,
            device,
            context,
            duplication,
            chain: None,
        })
    }

    /// The next frame, as width, height and RGBA bytes. `None` where the
    /// screen did not change within `wait`.
    pub fn next(&mut self, wait: Duration) -> Result<Option<(usize, usize, Vec<u8>)>> {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        let timeout = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
        match unsafe {
            self.duplication
                .AcquireNextFrame(timeout, &mut info, &mut resource)
        } {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            Err(error) => return Err(error),
        }
        let result = self.shrink(info, resource);
        unsafe { self.duplication.ReleaseFrame() }?;
        result
    }

    fn shrink(
        &mut self,
        info: DXGI_OUTDUPL_FRAME_INFO,
        resource: Option<IDXGIResource>,
    ) -> Result<Option<(usize, usize, Vec<u8>)>> {
        // A frame that moved the mouse pointer alone carries no new image.
        if info.LastPresentTime == 0 && self.chain.is_some() {
            return Ok(None);
        }
        let Some(resource) = resource else {
            return Ok(None);
        };
        let texture: ID3D11Texture2D = resource.cast()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        if self
            .chain
            .as_ref()
            .is_none_or(|c| c.source != (desc.Width, desc.Height))
        {
            let mut level = 0;
            while (desc.Width >> (level + 1)) >= READBACK_WIDTH {
                level += 1;
            }
            self.chain = Some(Shrink::new(&self.device, desc.Width, desc.Height, level)?);
        }
        let Some(chain) = self.chain.as_mut() else {
            return Ok(None);
        };
        chain.run(&self.device, &self.context, &texture)?;

        let (width, height) = chain.size;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(&chain.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }?;
        let (width, height) = (width as usize, height as usize);
        let mut rgba = vec![0u8; width * height * 4];
        let pitch = mapped.RowPitch as usize;
        // SAFETY: a mapped staging texture holds `height` rows of `RowPitch`
        // bytes, and each row holds `width` BGRA pixels.
        let source =
            unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), pitch * height) };
        for y in 0..height {
            let row = &source[y * pitch..y * pitch + width * 4];
            for (out, bgra) in rgba[y * width * 4..(y + 1) * width * 4]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(row.as_chunks::<4>().0)
            {
                out.copy_from_slice(&[bgra[2], bgra[1], bgra[0], 255]);
            }
        }
        unsafe { self.context.Unmap(&chain.staging, 0) };
        Ok(Some((width, height, rgba)))
    }
}

/// Every screen attached to the desktop, over every adapter.
fn outputs() -> Result<Vec<(IDXGIAdapter1, IDXGIOutput, String)>> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }?;
    let mut found = Vec::new();
    let mut a = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(a) } {
        let mut o = 0;
        while let Ok(output) = unsafe { adapter.EnumOutputs(o) } {
            let desc = unsafe { output.GetDesc() }?;
            if desc.AttachedToDesktop.as_bool() {
                let r = desc.DesktopCoordinates;
                let name = format!(
                    "Screen {} — {} × {}",
                    found.len() + 1,
                    r.right - r.left,
                    r.bottom - r.top
                );
                found.push((adapter.clone(), output, name));
            }
            o += 1;
        }
        a += 1;
    }
    Ok(found)
}

fn outputs_first() -> Result<Option<(IDXGIAdapter1, IDXGIOutput, String)>> {
    Ok(outputs()?.into_iter().next())
}
