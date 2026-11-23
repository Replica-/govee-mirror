//! The Razer Chroma Addressable RGB Controller, driven over USB with no
//! Synapse.
//!
//! One [`Controller`] holds the device. It takes the LEDs from the
//! controller's own effect when it opens, sends frames from a thread of its
//! own, and hands the LEDs back when the last holder drops it. [`acquire`]
//! shares that one controller between every port that the mirror or a
//! highlight drives.

pub mod chroma;
pub mod headset;
pub mod hid;
pub mod protocol;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use self::protocol::{LEDS, PORTS};

/// How often the thread looks for a new frame.
const TICK: Duration = Duration::from_millis(16);
/// A frame goes out at least this often. Synapse streams frames without a
/// pause, and the app keeps a floor rate the same way.
const KEEP_ALIVE: Duration = Duration::from_millis(250);

#[derive(Default)]
struct State {
    ports: [Vec<[u8; 3]>; PORTS],
    dirty: bool,
    error: Option<String>,
}

pub struct Controller {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

static SHARED: Mutex<Weak<Controller>> = Mutex::new(Weak::new());

/// The open controller, or a new one. The LEDs go back to the controller's
/// own effect when the last holder drops it.
///
/// # Errors
///
/// What the USB stack answered, where no controller is connected or another
/// program holds it.
pub fn acquire() -> Result<Arc<Controller>, String> {
    let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(controller) = shared.upgrade() {
        return Ok(controller);
    }
    let controller = Arc::new(Controller::open()?);
    *shared = Arc::downgrade(&controller);
    Ok(controller)
}

/// Whether a controller is connected. Opens nothing.
pub fn present() -> bool {
    !hid::find(|d| d.vendor_id() == protocol::VENDOR && d.product_id() == protocol::PRODUCT)
        .is_empty()
}

/// Whether Synapse runs. It streams its own frames to the controller and
/// overwrites the mirror's.
pub fn synapse_running() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window for the child.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq RazerAppEngine.exe", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("RazerAppEngine"))
    }
    #[cfg(not(windows))]
    false
}

impl Controller {
    fn open() -> Result<Self, String> {
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (opened, result) = mpsc::channel();
        let thread = {
            let (state, stop) = (Arc::clone(&state), Arc::clone(&stop));
            thread::Builder::new()
                .name("razer".to_owned())
                .spawn(move || run(&state, &stop, &opened))
                .map_err(|e| e.to_string())?
        };
        match result.recv() {
            Ok(Ok(())) => Ok(Self {
                state,
                stop,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => Err("the Razer thread ended before it opened the controller".to_owned()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Set the colors of one port, LED 0 first. The next frame carries them.
    pub fn set(&self, port: usize, colors: &[[u8; 3]]) {
        let mut state = self.lock();
        if let Some(leds) = state.ports.get_mut(port) {
            leds.clear();
            leds.extend_from_slice(&colors[..colors.len().min(LEDS)]);
            state.dirty = true;
        }
    }

    /// The last failure to send a frame, cleared by the next frame sent.
    pub fn error(&self) -> Option<String> {
        self.lock().error.clone()
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(state: &Mutex<State>, stop: &AtomicBool, opened: &mpsc::Sender<Result<(), String>>) {
    let open = || -> Result<(hidapi::HidDevice, hidapi::HidDevice), String> {
        let open = |interface| {
            let found = hid::find(|d| {
                d.vendor_id() == protocol::VENDOR
                    && d.product_id() == protocol::PRODUCT
                    && d.interface_number() == interface
            });
            let info = found
                .first()
                .ok_or_else(|| "no Razer ARGB controller is connected".to_owned())?;
            hid::open(info)
        };
        let control = open(protocol::CONTROL_INTERFACE)?;
        let frames = open(protocol::FRAME_INTERFACE)?;
        control
            .send_feature_report(&protocol::host_control(true))
            .map_err(|e| e.to_string())?;
        Ok((control, frames))
    };
    let (control, frames) = match open() {
        Ok(devices) => {
            let _ = opened.send(Ok(()));
            devices
        }
        Err(error) => {
            let _ = opened.send(Err(error));
            return;
        }
    };

    let lock = || state.lock().unwrap_or_else(PoisonError::into_inner);
    let mut last = Instant::now() - KEEP_ALIVE;
    while !stop.load(Ordering::SeqCst) {
        let ports = {
            let mut state = lock();
            if state.dirty || last.elapsed() >= KEEP_ALIVE {
                state.dirty = false;
                Some(state.ports.clone())
            } else {
                None
            }
        };
        if let Some(ports) = ports {
            let result = protocol::frame(&ports)
                .iter()
                .try_for_each(|report| frames.send_feature_report(report));
            let mut state = lock();
            match result {
                Ok(()) => state.error = None,
                Err(error) => state.error = Some(error.to_string()),
            }
            last = Instant::now();
        }
        thread::sleep(TICK);
    }
    let _ = control.send_feature_report(&protocol::host_control(false));
}

/// Spread `segments` over `leds` LEDs, each segment over an even run. With
/// `blend`, the colors fade from one segment's center to the next. With one
/// segment per LED, `blend` gives each LED the mean of it and its neighbours.
pub fn spread(segments: &[[u8; 3]], leds: usize, blend: bool) -> Vec<[u8; 3]> {
    if segments.is_empty() || (segments.len() == leds && !blend) {
        return segments.to_vec();
    }
    if segments.len() == leds {
        return smooth(segments);
    }
    let count = segments.len();
    (0..leds)
        .map(|led| {
            if !blend {
                return segments[(led * count / leds).min(count - 1)];
            }
            #[allow(clippy::cast_precision_loss)]
            let at = ((led as f32 + 0.5) * count as f32 / leds as f32 - 0.5)
                .clamp(0.0, (count - 1) as f32);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let low = at.floor() as usize;
            let high = (low + 1).min(count - 1);
            let t = at - at.floor();
            let (a, b) = (segments[low], segments[high]);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            std::array::from_fn(|i| {
                (f32::from(a[i]) * (1.0 - t) + f32::from(b[i]) * t).round() as u8
            })
        })
        .collect()
}

fn smooth(segments: &[[u8; 3]]) -> Vec<[u8; 3]> {
    (0..segments.len())
        .map(|led| {
            let near = &segments[led.saturating_sub(1)..(led + 2).min(segments.len())];
            #[allow(clippy::cast_possible_truncation)]
            std::array::from_fn(|i| {
                let sum: usize = near.iter().map(|c| usize::from(c[i])).sum();
                ((sum + near.len() / 2) / near.len()) as u8
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_segment_covers_an_even_run_of_leds() {
        let segments = [[255, 0, 0], [0, 0, 255]];
        let leds = spread(&segments, 6, false);
        assert_eq!(
            leds,
            vec![[255, 0, 0]; 3]
                .into_iter()
                .chain(vec![[0, 0, 255]; 3])
                .collect::<Vec<_>>()
        );
        assert_eq!(spread(&segments, 2, false), segments.to_vec());
    }

    #[test]
    fn a_blend_fades_between_segment_centers() {
        let leds = spread(&[[0, 0, 0], [200, 0, 0]], 4, true);
        // The ends hold each segment's color; the middle LEDs fade.
        assert_eq!(leds[0], [0, 0, 0]);
        assert_eq!(leds[3], [200, 0, 0]);
        assert!(leds[1][0] > 0 && leds[1][0] < leds[2][0] && leds[2][0] < 200);
    }

    #[test]
    fn a_blend_at_one_segment_per_led_averages_the_neighbours() {
        let segments = [[0, 0, 0], [90, 0, 0], [0, 0, 0], [0, 0, 30]];
        assert_eq!(spread(&segments, 4, false), segments.to_vec());
        assert_eq!(
            spread(&segments, 4, true),
            vec![[45, 0, 0], [30, 0, 0], [30, 0, 10], [0, 0, 15]]
        );
    }
}
