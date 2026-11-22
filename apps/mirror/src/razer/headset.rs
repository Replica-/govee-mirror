//! The BlackShark V3 Pro headset: switch the sound output when it connects
//! to its USB dongle, and switch back when it disconnects.
//!
//! The dongle stays plugged in, so the Windows sound outputs of the headset
//! stay listed while it is off. The dongle sends a 64-byte input report on
//! its vendor collection at each connection change ([`parse`]). It sends
//! nothing while the state holds, and no report answers a read of the state,
//! so the state is unknown until the first change after the app starts.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::profiles::Library;
use crate::{audio, notify};

/// The USB dongle of the BlackShark V3 Pro.
const PRODUCT: u16 = 0x0577;
/// The vendor collection that carries the connection report.
const USAGE_PAGE: u16 = 0xFF14;
/// The bytes before the state byte of the connection report, report ID
/// first. Byte 14 changes between two reports of one state.
const NOTICE: [u8; 13] = [
    0x02, 0x02, 0x60, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x20, 0x02, 0x02,
];
/// How long a read waits, so that the thread sees the option go off.
const READ: Duration = Duration::from_secs(1);
/// How long the thread waits before it looks for the dongle again.
const RETRY: Duration = Duration::from_secs(5);

/// The headset switch. It applies to every profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// The endpoint ID of the headset's output.
    pub output: Option<String>,
    /// The endpoint ID of the output that plays while the headset is off.
    pub back_to: Option<String>,
    /// Show a Windows notification at each switch.
    pub notify: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            output: None,
            back_to: None,
            notify: true,
        }
    }
}

#[derive(Debug, Default)]
pub struct State {
    /// Whether the dongle is plugged in.
    pub present: bool,
    /// Whether the headset is on. `None` until the first change after the
    /// thread opens the dongle.
    pub connected: Option<bool>,
    /// What the last switch did, or why it failed.
    pub message: Option<String>,
    /// Whether the last switch failed.
    pub failed: bool,
}

pub type Shared = Arc<Mutex<State>>;

pub fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The connection state in one input report: `Some(true)` when the headset
/// connects, `Some(false)` when it disconnects.
pub fn parse(report: &[u8]) -> Option<bool> {
    if report.get(..NOTICE.len())? != NOTICE {
        return None;
    }
    match report.get(NOTICE.len())? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// Start the thread. It runs for the life of the process, and opens the
/// dongle only while the switch is on. The window reads the state on its
/// next redraw.
pub fn spawn(library: Arc<Mutex<Library>>) -> Shared {
    let shared = Shared::default();
    let state = Arc::clone(&shared);
    let _ = thread::Builder::new()
        .name("headset".to_owned())
        .spawn(move || run(&library, &state));
    shared
}

fn settings(library: &Mutex<Library>) -> Settings {
    library
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .headset
        .clone()
}

fn run(library: &Mutex<Library>, state: &Shared) {
    loop {
        let present = dongle().is_some();
        lock(state).present = present;
        if !settings(library).enabled || !present {
            lock(state).connected = None;
            thread::sleep(RETRY);
            continue;
        }
        let Some(device) = open() else {
            thread::sleep(RETRY);
            continue;
        };
        listen(&device, library, state);
        lock(state).connected = None;
    }
}

fn dongle() -> Option<hidapi::DeviceInfo> {
    let api = hidapi::HidApi::new().ok()?;
    api.device_list()
        .find(|d| {
            d.vendor_id() == super::protocol::VENDOR
                && d.product_id() == PRODUCT
                && d.usage_page() == USAGE_PAGE
        })
        .cloned()
}

fn open() -> Option<hidapi::HidDevice> {
    let api = hidapi::HidApi::new().ok()?;
    dongle()?.open_device(&api).ok()
}

/// Read reports until the dongle goes away or the switch goes off.
fn listen(device: &hidapi::HidDevice, library: &Mutex<Library>, state: &Shared) {
    let timeout = i32::try_from(READ.as_millis()).unwrap_or(i32::MAX);
    let mut report = [0u8; 64];
    loop {
        let Ok(len) = device.read_timeout(&mut report, timeout) else {
            return;
        };
        let settings = settings(library);
        if !settings.enabled {
            return;
        }
        let Some(on) = parse(&report[..len]) else {
            continue;
        };
        if lock(state).connected == Some(on) {
            continue;
        }
        let outcome = switch(&settings, on);
        if settings.notify
            && let Some(outcome) = &outcome
        {
            let title = if on { "Headset on" } else { "Headset off" };
            let _ = notify::show(title, outcome.as_ref().unwrap_or_else(|e| e));
        }
        let mut state = lock(state);
        state.connected = Some(on);
        state.failed = matches!(outcome, Some(Err(_)));
        state.message = outcome.map(|o| o.unwrap_or_else(|e| e));
        drop(state);
    }
}

/// Play on the headset when it connects. Go back when it disconnects, unless
/// another output was picked while the headset played.
fn switch(settings: &Settings, on: bool) -> Option<Result<String, String>> {
    let current = audio::default_output().ok();
    let target = if on {
        settings.output.as_ref()?
    } else {
        let back_to = settings.back_to.as_ref()?;
        if current.is_some() && current != settings.output {
            return None;
        }
        back_to
    };
    if current.as_ref() == Some(target) {
        return None;
    }
    let outputs = audio::outputs().unwrap_or_default();
    let name = audio::name(&outputs, target);
    Some(match audio::set_default(target) {
        Ok(()) => Ok(format!("Sound went to {name}.")),
        Err(error) => Err(format!("Sound did not go to {name}: {error}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_connection_report_gives_the_state_and_other_reports_give_none() {
        let mut report = [0u8; 64];
        report[..NOTICE.len()].copy_from_slice(&NOTICE);
        report[13] = 1;
        report[14] = 1;
        assert_eq!(parse(&report), Some(true));
        report[13] = 0;
        assert_eq!(parse(&report), Some(false));
        report[13] = 7;
        assert_eq!(parse(&report), None);
        report[2] = 0;
        assert_eq!(parse(&report), None);
        assert_eq!(parse(&NOTICE), None);
    }
}
