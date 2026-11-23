//! One HID handle for every Razer thread.
//!
//! A list of the HID collections took about half a second and 30 ms of CPU,
//! measured on one PC with 78 Razer collections. A filter by vendor took
//! longer. So the threads share
//! one list, and an open reuses it instead of a new list.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use hidapi::{DeviceInfo, HidApi, HidDevice};

/// How old the shared list gets before the next caller lists again. A device
/// that is plugged in shows within this time.
const AGE: Duration = Duration::from_secs(10);

static SHARED: Mutex<Option<(HidApi, Instant)>> = Mutex::new(None);

/// Run `f` on the shared handle, with a list at most [`AGE`] old.
///
/// # Errors
///
/// What the HID stack answered to the first list.
fn with<T>(f: impl FnOnce(&HidApi) -> T) -> Result<T, String> {
    let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
    match shared.as_mut() {
        Some((api, listed)) if listed.elapsed() >= AGE => {
            api.refresh_devices().map_err(|e| e.to_string())?;
            *listed = Instant::now();
        }
        Some(_) => {}
        None => *shared = Some((HidApi::new().map_err(|e| e.to_string())?, Instant::now())),
    }
    let (api, _) = shared.as_ref().ok_or("no HID handle")?;
    Ok(f(api))
}

/// The collections of the list that match `wanted`.
pub fn find(wanted: impl Fn(&DeviceInfo) -> bool) -> Vec<DeviceInfo> {
    with(|api| api.device_list().filter(|d| wanted(d)).cloned().collect()).unwrap_or_default()
}

/// Open one collection of the list.
///
/// # Errors
///
/// What the HID stack answered.
pub fn open(info: &DeviceInfo) -> Result<HidDevice, String> {
    with(|api| info.open_device(api).map_err(|e| e.to_string()))?
}
