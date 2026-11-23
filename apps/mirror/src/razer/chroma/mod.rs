//! The Razer devices of [`devices`]: effects that the app draws on groups of
//! keys or LEDs, and the macro keys of the keyboard.
//!
//! One thread per device runs for the life of the process, in [`thread`].

pub mod devices;
pub mod effects;
pub mod reports;
mod thread;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

use self::devices::{Device, Spec};
use self::effects::Group;
use crate::profiles::Library;
use crate::watcher::Switcher;

/// The lights of one device, in one profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The app draws the effects of `groups`.
    pub enabled: bool,
    /// Every key or LED is dark. Wins over `enabled`.
    pub dark: bool,
    /// From 0 to 100.
    pub brightness: u8,
    pub groups: Vec<Group>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            dark: false,
            brightness: 100,
            groups: Vec::new(),
        }
    }
}

/// What the lights of a device show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The effect that the device keeps in its own memory.
    Own,
    Effects,
    Off,
}

impl Mode {
    pub const ALL: [Self; 3] = [Self::Own, Self::Effects, Self::Off];

    pub fn label(self) -> &'static str {
        match self {
            Self::Own => "Own effect",
            Self::Effects => "App effects",
            Self::Off => "Off",
        }
    }
}

impl Settings {
    pub fn mode(&self) -> Mode {
        match (self.dark, self.enabled) {
            (true, _) => Mode::Off,
            (false, true) => Mode::Effects,
            (false, false) => Mode::Own,
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.dark = mode == Mode::Off;
        self.enabled = mode == Mode::Effects;
    }

    /// Whether the app drives the lights.
    pub fn lit(&self) -> bool {
        self.mode() != Mode::Own
    }
}

#[derive(Debug, Default)]
pub struct State {
    /// Whether the device is connected.
    pub present: bool,
    /// Whether the thread sends frames to the device.
    pub driving: bool,
    /// Whether the macro keys report to the app.
    pub listening: bool,
    pub error: Option<String>,
}

pub struct Handle {
    pub state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Handle {
    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stop the thread. The device gets its own effect and its macro keys
    /// back first.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The lights of `device` in the profile in use.
pub fn of(library: &Library, device: Device) -> Settings {
    let settings = &library.profiles[library.active].settings;
    match device {
        Device::Keyboard => settings.keyboard.clone(),
        Device::Mousepad => settings.mousepad.clone(),
    }
}

/// The lights of `device`, in the settings of the window.
pub fn of_mut(settings: &mut crate::settings::Settings, device: Device) -> &mut Settings {
    match device {
        Device::Keyboard => &mut settings.keyboard,
        Device::Mousepad => &mut settings.mousepad,
    }
}

/// Start the thread of `spec`. A macro key press goes to `switcher`.
pub fn spawn(spec: &'static Spec, library: Arc<Mutex<Library>>, switcher: Switcher) -> Handle {
    let state = Arc::new(Mutex::new(State::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let thread = {
        let (state, stop) = (Arc::clone(&state), Arc::clone(&stop));
        std::thread::Builder::new()
            .name(spec.name.to_owned())
            .spawn(move || thread::run(spec, &library, &switcher, &state, &stop))
            .ok()
    };
    Handle {
        state,
        stop,
        thread,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_wins_over_the_effects() {
        let mut settings = Settings {
            enabled: true,
            ..Settings::default()
        };
        assert_eq!(settings.mode(), Mode::Effects);
        settings.set_mode(Mode::Off);
        assert_eq!((settings.dark, settings.enabled), (true, false));
        assert!(settings.lit());
        settings.set_mode(Mode::Own);
        assert!(!settings.lit());
    }
}
