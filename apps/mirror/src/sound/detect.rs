//! When the bass counts as a hit.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use super::Settings;
use super::spectrum;

/// How a hit is found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Detect {
    /// A jump above the average of the last second or so. It follows the
    /// loudness of the song and the volume.
    #[default]
    Adaptive,
    /// Every analysis at or above a fixed level. A bass that holds above it
    /// holds the light at its peak.
    Fixed,
}

impl Detect {
    pub const ALL: [Self; 2] = [Self::Adaptive, Self::Fixed];

    pub fn label(self) -> &'static str {
        match self {
            Self::Adaptive => "Adaptive",
            Self::Fixed => "Fixed line",
        }
    }
}

/// The bands that the detection reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Band {
    /// 40 to 100 Hz: the kick drum, with less of the bass line.
    #[default]
    Kick,
    /// Every band below 150 Hz.
    Bass,
}

impl Band {
    pub const ALL: [Self; 2] = [Self::Kick, Self::Bass];

    pub fn label(self) -> &'static str {
        match self {
            Self::Kick => "Kick (40–100 Hz)",
            Self::Bass => "All bass (below 150 Hz)",
        }
    }

    /// The bands of the spectrum, lowest first.
    pub fn range(self) -> Range<usize> {
        match self {
            Self::Kick => spectrum::within(40.0, 100.0),
            Self::Bass => spectrum::within(0.0, 150.0),
        }
    }
}

/// How fast the average follows the level, per analysis: about one second at
/// one analysis every 20 ms.
const FOLLOW: f32 = 0.02;
/// The lowest level an adaptive hit must reach, so that quiet noise does not
/// count.
const FLOOR: f32 = 0.25;

/// Follows the bass level from one analysis to the next.
#[derive(Debug, Default)]
pub struct Detector {
    average: f32,
    /// An adaptive hit needs the level to fall below the trigger after the
    /// one before, so that one long note is one hit.
    armed: bool,
}

impl Detector {
    /// Whether `level` is a hit, and the level that a hit must reach.
    pub fn step(&mut self, level: f32, settings: &Settings) -> (bool, f32) {
        if settings.detect == Detect::Fixed {
            return (level >= settings.threshold, settings.threshold);
        }
        let trigger = (self.average + margin(settings.sensitivity)).max(FLOOR);
        let hit = self.armed && level >= trigger;
        if hit {
            self.armed = false;
        } else if level < trigger {
            self.armed = true;
        }
        self.average += (level - self.average) * FOLLOW;
        (hit, trigger)
    }
}

/// How far above the average a hit must jump, on the 0 to 1 scale of the
/// spectrum: 0.05 is about 3.5 dB.
fn margin(sensitivity: f32) -> f32 {
    0.02 + (1.0 - sensitivity.clamp(0.0, 1.0)) * 0.13
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(detector: &mut Detector, levels: &[f32], settings: &Settings) -> usize {
        levels
            .iter()
            .filter(|&&level| detector.step(level, settings).0)
            .count()
    }

    #[test]
    fn a_kick_over_a_steady_bass_is_one_hit_and_a_held_note_is_one_hit() {
        let settings = Settings::default();
        let mut detector = Detector::default();
        // Six seconds of steady bass set the average.
        assert_eq!(run(&mut detector, &[0.5; 300], &settings), 0);
        assert_eq!(run(&mut detector, &[0.7, 0.7, 0.7, 0.5, 0.5], &settings), 1);
        assert_eq!(run(&mut detector, &[0.5, 0.7], &settings), 1);
    }

    #[test]
    fn the_adaptive_trigger_follows_a_louder_song() {
        let settings = Settings::default();
        let mut detector = Detector::default();
        run(&mut detector, &[0.8; 300], &settings);
        // A level that was a hit in a quiet song is no hit now.
        assert_eq!(run(&mut detector, &[0.82], &settings), 0);
    }

    #[test]
    fn the_fixed_line_counts_every_analysis_above_it() {
        let settings = Settings {
            detect: Detect::Fixed,
            threshold: 0.6,
            ..Settings::default()
        };
        let mut detector = Detector::default();
        assert_eq!(run(&mut detector, &[0.7, 0.7, 0.5, 0.7], &settings), 3);
    }

    #[test]
    fn silence_and_quiet_noise_are_no_hit() {
        let settings = Settings {
            sensitivity: 1.0,
            ..Settings::default()
        };
        let mut detector = Detector::default();
        assert_eq!(run(&mut detector, &[0.0, 0.1, 0.0, 0.2, 0.0], &settings), 0);
    }
}
