//! The GPU pass that shrinks the desktop: one draw, in which each output
//! pixel is the mean of one square block of screen pixels. The pass reads
//! each screen pixel once and writes only the small image. A copy of the
//! screen and a mip chain would read and write the full screen again.
//!
//! Every call into Direct3D is `unsafe`: the `windows` crate marks all COM
//! calls so. Each one passes owned interfaces and stack values that outlive
//! the call.
#![allow(unsafe_code)]

use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
    D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_SAMPLER_DESC, D3D11_TEXTURE_ADDRESS_CLAMP,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11_VIEWPORT,
    ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader, ID3D11RenderTargetView,
    ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::core::{Error, PCSTR, Result};

/// One triangle that covers the target, and the mean of each block. Shader
/// model 4 runs on every Direct3D 10 GPU and later. `SCALE` (a power of 2),
/// `WIDTH` and `HEIGHT` are set per screen size, before the compile.
///
/// A bilinear sample at the corner of four pixels is their mean, so a block
/// of `SCALE` × `SCALE` pixels takes a quarter as many samples. A block of
/// one pixel takes a load.
const SHADER: &str = r"
Texture2D<float4> desktop : register(t0);
SamplerState bilinear : register(s0);

float4 vs(uint id : SV_VertexID) : SV_Position {
    float2 corner = float2((id << 1) & 2, id & 2);
    return float4(corner * float2(2, -2) + float2(-1, 1), 0, 1);
}

float4 ps(float4 position : SV_Position) : SV_Target {
    int2 origin = int2(position.xy) * SCALE;
    if (SCALE == 1) {
        return desktop.Load(int3(origin, 0));
    }
    float2 pixel = float2(1.0 / WIDTH, 1.0 / HEIGHT);
    float4 sum = 0;
    [loop] for (int y = 0; y < SCALE; y += 2) {
        [loop] for (int x = 0; x < SCALE; x += 2) {
            sum += desktop.SampleLevel(bilinear, (origin + int2(x, y) + 1) * pixel, 0);
        }
    }
    return sum / (SCALE * SCALE / 4);
}
";

/// The pass for one screen size.
pub struct Shrink {
    /// The screen size that the pass takes.
    pub source: (u32, u32),
    /// The size of the result: the screen divided by `2^level`.
    pub size: (u32, u32),
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    target: ID3D11Texture2D,
    view: ID3D11RenderTargetView,
    /// The result, where the CPU can map it.
    pub staging: ID3D11Texture2D,
    /// A copy of the desktop, made only where the driver hands a desktop
    /// texture that a shader cannot read.
    copy: Option<(ID3D11Texture2D, ID3D11ShaderResourceView)>,
}

impl Shrink {
    /// The pass for a `width` × `height` screen. Each block is `2^level`
    /// pixels on a side.
    ///
    /// # Errors
    ///
    /// What Direct3D answered, or the compiler's message where the shader
    /// does not compile.
    pub fn new(device: &ID3D11Device, width: u32, height: u32, level: u32) -> Result<Self> {
        let size = ((width >> level).max(1), (height >> level).max(1));
        let source = SHADER
            .replace("SCALE", &(1u32 << level).to_string())
            .replace("WIDTH", &format!("{width}.0"))
            .replace("HEIGHT", &format!("{height}.0"));
        let vs = compile(&source, c"vs", c"vs_4_0")?;
        let ps = compile(&source, c"ps", c"ps_4_0")?;
        let (mut vertex, mut pixel, mut sampler) = (None, None, None);
        let bilinear = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MaxLOD: f32::MAX,
            ..D3D11_SAMPLER_DESC::default()
        };
        unsafe {
            device.CreateVertexShader(bytes(&vs), None, Some(&mut vertex))?;
            device.CreatePixelShader(bytes(&ps), None, Some(&mut pixel))?;
            device.CreateSamplerState(&bilinear, Some(&mut sampler))?;
        }
        #[allow(clippy::cast_sign_loss)]
        let target_desc = texture(
            size,
            D3D11_USAGE_DEFAULT,
            D3D11_BIND_RENDER_TARGET.0 as u32,
            0,
        );
        #[allow(clippy::cast_sign_loss)]
        let staging_desc = texture(size, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ.0 as u32);
        let (mut target, mut staging, mut view) = (None, None, None);
        unsafe {
            device.CreateTexture2D(&target_desc, None, Some(&mut target))?;
            device.CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
        }
        let target = target.ok_or_else(Error::empty)?;
        unsafe { device.CreateRenderTargetView(&target, None, Some(&mut view)) }?;
        Ok(Self {
            source: (width, height),
            size,
            vertex: vertex.ok_or_else(Error::empty)?,
            pixel: pixel.ok_or_else(Error::empty)?,
            sampler: sampler.ok_or_else(Error::empty)?,
            target,
            view: view.ok_or_else(Error::empty)?,
            staging: staging.ok_or_else(Error::empty)?,
            copy: None,
        })
    }

    /// Shrink `desktop` into [`Self::staging`].
    ///
    /// # Errors
    ///
    /// What Direct3D answered.
    pub fn run(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        desktop: &ID3D11Texture2D,
    ) -> Result<()> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { desktop.GetDesc(&mut desc) };
        #[allow(clippy::cast_sign_loss)]
        let readable = desc.BindFlags & D3D11_BIND_SHADER_RESOURCE.0 as u32 != 0;
        let input = if readable {
            let mut input = None;
            unsafe { device.CreateShaderResourceView(desktop, None, Some(&mut input)) }?;
            input.ok_or_else(Error::empty)?
        } else {
            let (copy, input) = match self.copy.take() {
                Some(made) => made,
                None => copy_of(device, &desc)?,
            };
            unsafe { context.CopyResource(&copy, desktop) };
            self.copy = Some((copy, input.clone()));
            input
        };
        #[allow(clippy::cast_precision_loss)]
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: self.size.0 as f32,
            Height: self.size.1 as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        unsafe {
            context.OMSetRenderTargets(Some(&[Some(self.view.clone())]), None);
            context.RSSetViewports(Some(&[viewport]));
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex, None);
            context.PSSetShader(&self.pixel, None);
            context.PSSetShaderResources(0, Some(&[Some(input)]));
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.Draw(3, 0);
            // The desktop texture goes back to DXGI after this frame: no view
            // of it stays bound.
            context.PSSetShaderResources(0, Some(&[None]));
            context.OMSetRenderTargets(None, None);
            context.CopyResource(&self.staging, &self.target);
        }
        Ok(())
    }
}

fn texture(size: (u32, u32), usage: D3D11_USAGE, bind: u32, cpu: u32) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: size.0,
        Height: size.1,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: usage,
        BindFlags: bind,
        CPUAccessFlags: cpu,
        MiscFlags: 0,
    }
}

/// A texture of the desktop's size that a shader reads, and its view.
fn copy_of(
    device: &ID3D11Device,
    desktop: &D3D11_TEXTURE2D_DESC,
) -> Result<(ID3D11Texture2D, ID3D11ShaderResourceView)> {
    #[allow(clippy::cast_sign_loss)]
    let desc = D3D11_TEXTURE2D_DESC {
        MipLevels: 1,
        ArraySize: 1,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        ..*desktop
    };
    let (mut copy, mut view) = (None, None);
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut copy)) }?;
    let copy = copy.ok_or_else(Error::empty)?;
    unsafe { device.CreateShaderResourceView(&copy, None, Some(&mut view)) }?;
    Ok((copy, view.ok_or_else(Error::empty)?))
}

/// The bytecode of one entry point of `source`.
fn compile(source: &str, entry: &std::ffi::CStr, target: &std::ffi::CStr) -> Result<ID3DBlob> {
    let (mut code, mut errors) = (None, None);
    let compiled = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry.as_ptr().cast()),
            PCSTR(target.as_ptr().cast()),
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(error) = compiled {
        let message = errors
            .as_ref()
            .map(|blob| String::from_utf8_lossy(bytes(blob)).into_owned())
            .unwrap_or_default();
        return Err(Error::new(error.code(), message));
    }
    code.ok_or_else(Error::empty)
}

fn bytes(blob: &ID3DBlob) -> &[u8] {
    // SAFETY: a blob holds `GetBufferSize` bytes at `GetBufferPointer` for as
    // long as it lives, and the slice borrows the blob.
    unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize()) }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
        D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA, D3D11CreateDevice,
    };

    use super::*;

    /// Shrinks a known image on this machine's GPU: `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs a Direct3D 10 GPU"]
    fn each_pixel_is_the_mean_of_its_block() {
        let (mut device, mut context) = (None, None);
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .unwrap_or_else(|e| panic!("{e}"));
        let (Some(device), Some(context)) = (device, context) else {
            panic!("no device");
        };
        // 16 × 8 pixels, BGRA. Blue counts the column, green the row, red is
        // 0 or 200 on alternate columns.
        let (width, height) = (16u32, 8u32);
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let red = if x % 2 == 0 { 0 } else { 200 };
                pixels.extend_from_slice(&[(x * 10) as u8, (y * 20) as u8, red, 255]);
            }
        }
        #[allow(clippy::cast_sign_loss)]
        let desc = texture(
            (width, height),
            D3D11_USAGE_DEFAULT,
            D3D11_BIND_SHADER_RESOURCE.0 as u32,
            0,
        );
        let data = D3D11_SUBRESOURCE_DATA {
            pSysMem: pixels.as_ptr().cast(),
            SysMemPitch: width * 4,
            SysMemSlicePitch: 0,
        };
        let mut desktop = None;
        unsafe { device.CreateTexture2D(&desc, Some(&data), Some(&mut desktop)) }
            .unwrap_or_else(|e| panic!("{e}"));
        let desktop = desktop.unwrap_or_else(|| panic!("no texture"));

        // Blocks of 4 × 4: a 4 × 2 result.
        let mut shrink = Shrink::new(&device, width, height, 2).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(shrink.size, (4, 2));
        shrink
            .run(&device, &context, &desktop)
            .unwrap_or_else(|e| panic!("{e}"));
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { context.Map(&shrink.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
            .unwrap_or_else(|e| panic!("{e}"));
        let pitch = mapped.RowPitch as usize;
        // SAFETY: the mapped staging texture holds 2 rows of `RowPitch` bytes.
        let out = unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), pitch * 2) };
        for row in 0..2usize {
            for col in 0..4usize {
                let pixel = &out[row * pitch + col * 4..row * pitch + col * 4 + 4];
                // Columns 4c..4c+3 give blue 10 × (4c + 1.5); rows 4r..4r+3
                // give green 20 × (4r + 1.5); red is half 0, half 200.
                let blue = 10.0 * (4.0 * col as f32 + 1.5);
                let green = 20.0 * (4.0 * row as f32 + 1.5);
                let expect = [blue, green, 100.0, 255.0];
                for (got, want) in pixel.iter().zip(expect) {
                    assert!(
                        (f32::from(*got) - want).abs() <= 1.0,
                        "{row}:{col} {pixel:?}"
                    );
                }
            }
        }
        unsafe { context.Unmap(&shrink.staging, 0) };
    }
}
