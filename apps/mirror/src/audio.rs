//! The sound outputs of Windows, and the one that plays by default.
//!
//! Core Audio lists the outputs. Windows has no public call that sets the
//! default output: [`set_default`] goes through `IPolicyConfig`, the
//! interface that the Windows sound settings use. Every call is COM, so this
//! module holds `unsafe` code.

/// One active sound output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The endpoint ID that Windows gives the output. It outlives a restart.
    pub id: String,
    /// The name the Windows sound settings show.
    pub name: String,
}

/// The name of `id` in `outputs`, else the ID itself.
pub fn name<'a>(outputs: &'a [Output], id: &'a str) -> &'a str {
    outputs
        .iter()
        .find(|o| o.id == id)
        .map_or(id, |o| o.name.as_str())
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        DEVICE_STATE_ACTIVE, IMMDeviceEnumerator, MMDeviceEnumerator, eCommunications, eConsole,
        eMultimedia, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        STGM_READ,
    };
    use windows::core::{GUID, PCWSTR};

    use self::policy::IPolicyConfig;
    use super::Output;

    /// `CPolicyConfigClient`.
    const POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    /// The methods before `SetDefaultEndpoint` are declared only to keep the
    /// vtable order.
    #[allow(non_snake_case)]
    mod policy {
        use std::ffi::c_void;

        use windows::Win32::Media::Audio::ERole;
        use windows::core::{HRESULT, IUnknown, IUnknown_Vtbl, PCWSTR, interface};

        #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
        pub unsafe trait IPolicyConfig: IUnknown {
            fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut c_void) -> HRESULT;
            fn GetDeviceFormat(
                &self,
                id: PCWSTR,
                default: i32,
                format: *mut *mut c_void,
            ) -> HRESULT;
            fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
            fn SetDeviceFormat(
                &self,
                id: PCWSTR,
                endpoint: *mut c_void,
                mix: *mut c_void,
            ) -> HRESULT;
            fn GetProcessingPeriod(
                &self,
                id: PCWSTR,
                default: i32,
                default_period: *mut i64,
                min_period: *mut i64,
            ) -> HRESULT;
            fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT;
            fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
            fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
            fn GetPropertyValue(
                &self,
                id: PCWSTR,
                store: i32,
                key: *const c_void,
                value: *mut c_void,
            ) -> HRESULT;
            fn SetPropertyValue(
                &self,
                id: PCWSTR,
                store: i32,
                key: *const c_void,
                value: *mut c_void,
            ) -> HRESULT;
            pub fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
            fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
        }
    }

    /// COM on this thread. A thread that already holds an apartment, as the
    /// window thread does, keeps it, and the calls below work from either.
    fn enumerator() -> Result<IMMDeviceEnumerator, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())
        }
    }

    pub fn outputs() -> Result<Vec<Output>, String> {
        let enumerator = enumerator()?;
        let mut outputs = Vec::new();
        unsafe {
            let devices = enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
                .map_err(|e| e.to_string())?;
            for i in 0..devices.GetCount().map_err(|e| e.to_string())? {
                let Ok(device) = devices.Item(i) else {
                    continue;
                };
                let Ok(raw) = device.GetId() else { continue };
                let id = raw.to_string().unwrap_or_default();
                CoTaskMemFree(Some(raw.0.cast_const().cast()));
                let name = device
                    .OpenPropertyStore(STGM_READ)
                    .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
                    .map(|value| value.to_string())
                    .unwrap_or_else(|_| id.clone());
                outputs.push(Output { id, name });
            }
        }
        outputs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(outputs)
    }

    pub fn default_output() -> Result<String, String> {
        let enumerator = enumerator()?;
        unsafe {
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| e.to_string())?;
            let raw = device.GetId().map_err(|e| e.to_string())?;
            let id = raw.to_string().map_err(|e| e.to_string());
            CoTaskMemFree(Some(raw.0.cast_const().cast()));
            id
        }
    }

    pub fn set_default(id: &str) -> Result<(), String> {
        let _ = enumerator()?;
        let wide: Vec<u16> = id.encode_utf16().chain([0]).collect();
        unsafe {
            let policy: IPolicyConfig =
                CoCreateInstance(&POLICY_CONFIG, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
            for role in [eConsole, eMultimedia, eCommunications] {
                policy
                    .SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role)
                    .ok()
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Output;

    const ONLY: &str = "sound outputs are available on Windows only";

    pub fn outputs() -> Result<Vec<Output>, String> {
        Err(ONLY.to_owned())
    }

    pub fn default_output() -> Result<String, String> {
        Err(ONLY.to_owned())
    }

    pub fn set_default(_id: &str) -> Result<(), String> {
        Err(ONLY.to_owned())
    }
}

/// The active outputs, by name.
///
/// # Errors
///
/// What COM answered.
pub use imp::outputs;

/// The endpoint ID of the default output for the console role.
///
/// # Errors
///
/// What COM answered, where no output is active.
pub use imp::default_output;

/// Make `id` the default output for every role: console, multimedia and
/// communications.
///
/// # Errors
///
/// What COM answered, where the ID names no active output.
pub use imp::set_default;
