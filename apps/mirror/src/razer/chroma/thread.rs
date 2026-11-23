//! The thread of one device. While the profile in use lights the device, it
//! sends a frame each time the colors change. While a macro key has an
//! action, it holds the keyboard in driver mode and reads the keys. When
//! neither holds or the app ends, it puts back the effect that the device
//! keeps in memory, and normal mode.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::devices::{Macros, Spec};
use super::effects::{self, Frame};
use super::{State, of, reports};
use crate::profiles::Library;
use crate::watcher::Switcher;

/// How often the thread draws a frame and reads the macro keys. A frame that
/// did not change is not sent.
const TICK: Duration = Duration::from_millis(33);
/// How often the thread looks for the device while it holds none.
const LOOK: Duration = Duration::from_secs(5);
/// How long the device takes a report before the answer is read.
const ANSWER_DELAY: Duration = Duration::from_millis(2);

/// The device while the thread holds it.
struct Held {
    device: hidapi::HidDevice,
    /// The report that puts the stored effect back.
    restore: Option<Vec<u8>>,
    /// The last frame sent. `None` while the device shows its own effect.
    last: Option<Frame>,
    /// The collections that the macro keys report on. Empty in normal mode.
    keys: Vec<hidapi::HidDevice>,
}

pub fn run(
    spec: &Spec,
    library: &Mutex<Library>,
    switcher: &Switcher,
    state: &Mutex<State>,
    stop: &AtomicBool,
) {
    let lock = || state.lock().unwrap_or_else(PoisonError::into_inner);
    let mut held: Option<Held> = None;
    let mut looked: Option<Instant> = None;
    let mut wanted_before = false;
    while !stop.load(Ordering::SeqCst) {
        let (settings, actions) = {
            let library = library.lock().unwrap_or_else(PoisonError::into_inner);
            (of(&library, spec.device), library.macro_keys.clone())
        };
        let listen = spec.macros.is_some() && actions.iter().any(Option::is_some);
        let wanted = settings.lit() || listen;
        if wanted && !wanted_before {
            looked = None;
        }
        wanted_before = wanted;
        if !wanted && let Some(current) = held.take() {
            release(&current);
        }
        if held.is_none() && looked.is_none_or(|at| at.elapsed() >= LOOK) {
            looked = Some(Instant::now());
            let found = find(spec);
            lock().present = found.is_some();
            if wanted && let Some(info) = found {
                match open(spec, &info) {
                    Ok(current) => held = Some(current),
                    Err(error) => lock().error = Some(error),
                }
            }
        }
        let mut pressed = Vec::new();
        if let Some(current) = held.as_mut() {
            let result = step(spec, current, &settings, listen, &mut pressed);
            let mut state = lock();
            state.driving = settings.lit() && result.is_ok();
            state.listening = !current.keys.is_empty() && result.is_ok();
            match result {
                Ok(()) => state.error = None,
                Err(error) => {
                    // Unplugged, or another program took it: look again.
                    state.error = Some(error);
                    held = None;
                }
            }
        } else {
            let mut state = lock();
            state.driving = false;
            state.listening = false;
        }
        for key in pressed {
            if let Some(Some(profile)) = actions.get(key) {
                switcher.choose(profile);
            }
        }
        thread::sleep(TICK);
    }
    if let Some(current) = held {
        release(&current);
    }
}

/// One tick: the mode of the macro keys, the frame, and the keys pressed.
fn step(
    spec: &Spec,
    current: &mut Held,
    settings: &super::Settings,
    listen: bool,
    pressed: &mut Vec<usize>,
) -> Result<(), String> {
    match (&spec.macros, listen, current.keys.is_empty()) {
        (Some(macros), true, true) => {
            request(&current.device, &reports::driver_mode(true))?;
            current.keys = open_keys(spec, macros);
        }
        (_, false, false) => {
            current.keys.clear();
            request(&current.device, &reports::driver_mode(false))?;
        }
        _ => {}
    }
    if settings.lit() {
        let frame = if settings.dark {
            vec![vec![[0u8; 3]; spec.cols]; spec.rows]
        } else {
            let brightness = f32::from(settings.brightness.min(100)) / 100.0;
            effects::draw(spec, &settings.groups, effects::now(), brightness)
        };
        if current.last.as_ref() != Some(&frame) {
            send(&current.device, &frame)?;
            current.last = Some(frame);
        }
    } else if current.last.take().is_some() {
        give_back(current);
    }
    if let Some(macros) = &spec.macros {
        let mut report = [0u8; 64];
        for keys in &current.keys {
            while let Ok(len) = keys.read(&mut report) {
                if len == 0 {
                    break;
                }
                pressed.extend(reports::pressed(macros, &report[..len]));
            }
        }
    }
    Ok(())
}

fn find(spec: &Spec) -> Option<hidapi::DeviceInfo> {
    let api = hidapi::HidApi::new().ok()?;
    api.device_list()
        .find(|d| {
            d.vendor_id() == reports::VENDOR
                && d.product_id() == spec.product
                && d.interface_number() == spec.interface
        })
        .cloned()
}

fn open(spec: &Spec, info: &hidapi::DeviceInfo) -> Result<Held, String> {
    let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let device = info.open_device(&api).map_err(|e| e.to_string())?;
    let answer = request(&device, &reports::read_stored(spec.led))?;
    Ok(Held {
        device,
        restore: reports::restore(&answer),
        last: None,
        keys: Vec::new(),
    })
}

/// Every collection that the macro keys can report on, read without a wait.
/// The device lists more than one with the same usage.
fn open_keys(spec: &Spec, macros: &Macros) -> Vec<hidapi::HidDevice> {
    let Ok(api) = hidapi::HidApi::new() else {
        return Vec::new();
    };
    api.device_list()
        .filter(|d| {
            d.vendor_id() == reports::VENDOR
                && d.product_id() == spec.product
                && d.interface_number() == macros.interface
                && d.usage_page() == macros.usage_page
                && d.usage() == macros.usage
        })
        .filter_map(|d| d.open_device(&api).ok())
    .filter(|d| d.set_blocking_mode(false).is_ok())
    .collect()
}

/// Send a report and read the answer. The device answers each report, and
/// the next report must wait for that answer.
fn request(device: &hidapi::HidDevice, report: &[u8]) -> Result<Vec<u8>, String> {
    device
        .send_feature_report(report)
        .map_err(|e| e.to_string())?;
    thread::sleep(ANSWER_DELAY);
    let mut answer = vec![0u8; reports::ANSWER_LEN];
    device
        .get_feature_report(&mut answer)
        .map_err(|e| e.to_string())?;
    Ok(answer)
}

fn send(device: &hidapi::HidDevice, frame: &Frame) -> Result<(), String> {
    for (row, colors) in frame.iter().enumerate() {
        request(device, &reports::row(row, colors))?;
    }
    let answer = request(device, &reports::show())?;
    if reports::taken(&answer) {
        Ok(())
    } else {
        Err(format!(
            "the device refused the frame (status {:02X})",
            answer[1]
        ))
    }
}

/// Put back the stored effect. Best effort: an unplugged device takes
/// nothing, and runs its stored effect when it comes back.
fn give_back(current: &Held) {
    if let Some(report) = &current.restore {
        let _ = request(&current.device, report);
    }
}

/// Hand the device back: its own effect, and normal mode. Best effort, as
/// for [`give_back`].
fn release(current: &Held) {
    if current.last.is_some() {
        give_back(current);
    }
    if !current.keys.is_empty() {
        let _ = request(&current.device, &reports::driver_mode(false));
    }
}
