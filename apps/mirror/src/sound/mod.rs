//! The sound that plays on the default output: a spectrum for the window, and
//! the bass hits that raise the lights to their peak level.
//!
//! A thread captures the output while the settings that the engine hands it
//! are on, and the mirror or the window reads the result. The thread finds the hits at each analysis, and the engine takes
//! them at its own frame rate, so that a hit between two frames still counts.

mod detect;
#[cfg(windows)]
mod loopback;
pub mod spectrum;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use self::detect::Detector;
pub use self::detect::{Band, Detect};
use self::spectrum::BANDS;

/// The sound reaction. Which lights follow it, and their two levels, are in
/// `Settings::levels`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub detect: Detect,
    /// How small a jump counts as a hit in [`Detect::Adaptive`], from 0 to 1.
    pub sensitivity: f32,
    pub band: Band,
    /// The bass level, from 0 to 1, that counts as a hit in
    /// [`Detect::Fixed`].
    pub threshold: f32,
    /// How long a light takes to fall from its peak back to its rest level,
    /// in seconds.
    pub fade: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            detect: Detect::default(),
            sensitivity: 0.5,
            band: Band::default(),
            threshold: 0.7,
            fade: 0.25,
        }
    }
}

#[derive(Debug, Default)]
pub struct State {
    /// The engine copies them from the app's settings. The thread captures
    /// while they are on.
    pub settings: Settings,
    /// Who reads the analysis. With neither, the thread closes the capture.
    pub demand: crate::capture::Demand,
    /// Each band from 0 to 1, lowest first, with a slow fall for the eye.
    /// Empty before the first analysis.
    pub bands: Vec<f32>,
    /// The newest level of the detection's bands, from 0 to 1.
    pub bass: f32,
    /// The level that a hit must reach now.
    pub trigger: f32,
    /// Whether a hit came since the engine last took it.
    hit: bool,
    detector: Detector,
    pub error: Option<String>,
}

pub type Shared = Arc<Mutex<State>>;

pub fn lock(shared: &Shared) -> MutexGuard<'_, State> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

impl State {
    /// Record one analysis.
    fn update(&mut self, bands: &[f32; BANDS]) {
        /// How far a band falls per analysis, for the eye only.
        const FALL: f32 = 0.04;
        self.bands.resize(BANDS, 0.0);
        for (shown, &level) in self.bands.iter_mut().zip(bands) {
            *shown = level.max(*shown - FALL);
        }
        self.bass = bands[self.settings.band.range()]
            .iter()
            .copied()
            .fold(0.0, f32::max);
        let (hit, trigger) = self.detector.step(self.bass, &self.settings);
        self.hit |= hit;
        self.trigger = trigger;
        self.error = None;
    }

    fn silence(&mut self) {
        self.bands.clear();
        self.bass = 0.0;
        self.hit = false;
        self.detector = Detector::default();
    }
}

/// Whether a hit came since the last call.
pub fn take_hit(shared: &Shared) -> bool {
    std::mem::take(&mut lock(shared).hit)
}

/// Start the capture thread. It idles until [`State::settings`] are on.
pub fn spawn() -> Shared {
    let shared = Shared::default();
    let state = Arc::clone(&shared);
    let _ = thread::Builder::new()
        .name("sound".to_owned())
        .spawn(move || run(&state));
    shared
}

#[cfg(windows)]
use self::loopback::run;

#[cfg(not(windows))]
fn run(shared: &Shared) {
    lock(shared).error = Some("sound capture is available on Windows only".to_owned());
}

/// The level of the sound reaction: 1 at a hit, falling to 0 over the fade.
pub struct Pulse {
    level: f32,
    at: Instant,
}

impl Default for Pulse {
    fn default() -> Self {
        Self {
            level: 0.0,
            at: Instant::now(),
        }
    }
}

impl Pulse {
    /// The level now, given whether a hit came since the last call. `fade`
    /// is in seconds.
    pub fn next(&mut self, hit: bool, fade: f32) -> f32 {
        let elapsed = self.at.elapsed();
        self.at = Instant::now();
        self.step(hit, fade, elapsed)
    }

    fn step(&mut self, hit: bool, fade: f32, elapsed: Duration) -> f32 {
        self.level = if hit {
            1.0
        } else {
            (self.level - elapsed.as_secs_f32() / fade.max(0.01)).max(0.0)
        };
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hit_raises_the_pulse_and_the_fade_brings_it_down() {
        let mut pulse = Pulse::default();
        let step = Duration::from_millis(100);
        assert!((pulse.step(true, 0.3, step) - 1.0).abs() < f32::EPSILON);
        let falling = pulse.step(false, 0.3, step);
        assert!(falling > 0.6 && falling < 0.7, "{falling}");
        pulse.step(false, 0.3, step);
        assert!(pulse.step(false, 0.3, step) < f32::EPSILON);
    }
}
