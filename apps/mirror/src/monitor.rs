//! The MSI monitor's input source, over DDC/CI on the video cable.
//!
//! The monitor answers DDC/CI on this PC's input while it shows another
//! input, so a switch works in both directions. The monitor's KVM moves its
//! USB hub with the input.
//!
//! The calls go through dxva2, so this module holds `unsafe` code: each call
//! takes a monitor handle that this module gets and destroys.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

/// The VCP code of the input source.
const INPUT: u8 = 0x60;
/// The monitors whose description starts with this.
const BRAND: &str = "MSI ";
/// The inputs, as the MPG 321URX answered them. MCCS names 0x10
/// "DisplayPort 2", but on this monitor it is the USB-C input.
pub const INPUTS: [(u32, &str); 2] = [(0x0F, "This PC"), (0x10, "USB-C")];
/// How often the thread reads the input while nothing is asked.
const POLL: Duration = Duration::from_secs(3);

#[derive(Debug, Default)]
pub struct State {
    /// Whether the monitor answers DDC/CI.
    pub present: bool,
    /// The model, from the monitor's description.
    pub model: Option<String>,
    /// The value of the input source, as the monitor answered it.
    pub input: Option<u32>,
    /// What the last switch did, or why it failed.
    pub message: Option<String>,
    pub failed: bool,
}

pub type Shared = Arc<Mutex<State>>;

pub fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The name of an input value.
pub fn name(input: u32) -> String {
    INPUTS
        .iter()
        .find(|(value, _)| *value == input)
        .map_or_else(
            || format!("Input 0x{input:02X}"),
            |(_, name)| (*name).to_owned(),
        )
}

/// Start the thread. It runs for the life of the process. Send an input
/// value to switch the monitor to it.
pub fn spawn() -> (Shared, Sender<u32>) {
    let shared = Shared::default();
    let state = Arc::clone(&shared);
    let (send, requests) = mpsc::channel::<u32>();
    let _ = thread::Builder::new()
        .name("monitor".to_owned())
        .spawn(move || {
            loop {
                match requests.recv_timeout(POLL) {
                    Ok(input) => switch(input, &state),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                let read = ddc::get(INPUT);
                let mut state = lock(&state);
                state.present = read.is_some();
                state.model = read.as_ref().map(|(model, _)| model.clone());
                state.input = read.map(|(_, value)| value);
            }
        });
    (shared, send)
}

fn switch(input: u32, state: &Shared) {
    let name = name(input);
    let outcome = if ddc::set(INPUT, input) {
        Ok(format!("Switched to {name}."))
    } else {
        Err(format!("The monitor did not take the switch to {name}."))
    };
    let mut state = lock(state);
    state.failed = outcome.is_err();
    state.message = Some(outcome.unwrap_or_else(|e| e));
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod ddc {
    use windows::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, GetVCPFeatureAndVCPFeatureReply, PHYSICAL_MONITOR,
        SetVCPFeature,
    };
    use windows::Win32::Foundation::{HANDLE, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows::core::BOOL;

    use super::BRAND;

    /// The model and the value of `code`, from the first monitor that
    /// answers.
    pub fn get(code: u8) -> Option<(String, u32)> {
        each(|handle| {
            let mut value = 0;
            let read =
                unsafe { GetVCPFeatureAndVCPFeatureReply(handle, code, None, &mut value, None) };
            (read != 0).then_some(value)
        })
    }

    /// Whether a monitor took the value.
    pub fn set(code: u8, value: u32) -> bool {
        each(|handle| (unsafe { SetVCPFeature(handle, code, value) } != 0).then_some(())).is_some()
    }

    /// Call `f` on each physical monitor of the brand until it answers.
    fn each<T>(mut f: impl FnMut(HANDLE) -> Option<T>) -> Option<(String, T)> {
        let mut found = None;
        for monitor in monitors() {
            let mut count = 0;
            if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut count) }.is_err() {
                continue;
            }
            let mut physical = vec![PHYSICAL_MONITOR::default(); count as usize];
            if unsafe { GetPhysicalMonitorsFromHMONITOR(monitor, &mut physical) }.is_err() {
                continue;
            }
            for one in &physical {
                // The struct is packed, so its fields are copied out first.
                let (handle, text) = (one.hPhysicalMonitor, one.szPhysicalMonitorDescription);
                let len = text.iter().position(|c| *c == 0).unwrap_or(text.len());
                let description = String::from_utf16_lossy(&text[..len]);
                let Some(model) = description.strip_prefix(BRAND) else {
                    continue;
                };
                if let Some(answer) = f(handle) {
                    found = Some((model.to_owned(), answer));
                    break;
                }
            }
            let _ = unsafe { DestroyPhysicalMonitors(&physical) };
            if found.is_some() {
                break;
            }
        }
        found
    }

    fn monitors() -> Vec<HMONITOR> {
        unsafe extern "system" fn add(
            monitor: HMONITOR,
            _: HDC,
            _: *mut RECT,
            data: LPARAM,
        ) -> BOOL {
            // `data` is the `Vec` below, which outlives the enumeration.
            let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
            list.push(monitor);
            true.into()
        }
        let mut list: Vec<HMONITOR> = Vec::new();
        let data = LPARAM(&raw mut list as isize);
        let _ = unsafe { EnumDisplayMonitors(None, None, Some(add), data) };
        list
    }
}

#[cfg(not(windows))]
mod ddc {
    pub fn get(_: u8) -> Option<(String, u32)> {
        None
    }

    pub fn set(_: u8, _: u32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_input_has_its_name_and_another_has_its_value() {
        assert_eq!(name(0x0F), "This PC");
        assert_eq!(name(0x10), "USB-C");
        assert_eq!(name(0x11), "Input 0x11");
    }
}
