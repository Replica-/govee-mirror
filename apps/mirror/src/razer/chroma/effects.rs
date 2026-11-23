//! The effects that the app draws on a device, one per group of keys or LEDs.

use std::collections::BTreeSet;
use std::sync::OnceLock;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::devices::{Key, Spec};
use crate::pattern::hue;

/// One frame: a row of colors per matrix row.
pub type Frame = Vec<Vec<[u8; 3]>>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    #[default]
    Static,
    /// The color fades out and back in.
    Breathing,
    /// Every key goes through the hues together.
    Spectrum,
    /// The hues move across the device: left to right on the keyboard, up
    /// the strip of the mousepad.
    Wave,
    /// Each key or LED lights up on its own and fades out.
    Starlight,
}

impl Effect {
    pub const ALL: [Self; 5] = [
        Self::Static,
        Self::Breathing,
        Self::Spectrum,
        Self::Wave,
        Self::Starlight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "Static",
            Self::Breathing => "Breathing",
            Self::Spectrum => "Spectrum",
            Self::Wave => "Wave",
            Self::Starlight => "Starlight",
        }
    }

    pub fn moves(self) -> bool {
        self != Self::Static
    }

    /// Whether the group's color shows. Spectrum and Wave make their own.
    pub fn colored(self) -> bool {
        !matches!(self, Self::Spectrum | Self::Wave)
    }
}

/// Keys that show one effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub name: String,
    /// Matrix positions, as row and column.
    pub keys: BTreeSet<(u8, u8)>,
    pub effect: Effect,
    pub color: [u8; 3],
    /// Cycles a second.
    pub speed: f32,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            name: "Group".to_owned(),
            keys: BTreeSet::new(),
            effect: Effect::Static,
            color: [68, 214, 44],
            speed: 0.3,
        }
    }
}

/// Seconds since the first call. The window preview and the keyboard share
/// this clock, so the preview shows the phase that the keys show.
pub fn now() -> f64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// The frame at `seconds`. A key in no group is dark. A key in two groups
/// shows the later one. `brightness` scales every color, from 0 to 1.
pub fn draw(spec: &Spec, groups: &[Group], seconds: f64, brightness: f32) -> Frame {
    let mut frame = vec![vec![[0u8; 3]; spec.cols]; spec.rows];
    for group in groups {
        for key in spec
            .keys
            .iter()
            .filter(|k| group.keys.contains(&(k.row, k.col)))
        {
            let color = color(group, key, seconds);
            frame[usize::from(key.row)][usize::from(key.col)] = scale(color, brightness);
        }
    }
    frame
}

/// The color of one key of `group` at `seconds`.
pub fn color(group: &Group, key: &Key, seconds: f64) -> [u8; 3] {
    let phase = |offset: f64| (seconds * f64::from(group.speed) + offset).rem_euclid(1.0);
    #[allow(clippy::cast_possible_truncation)]
    match group.effect {
        Effect::Static => group.color,
        Effect::Breathing => {
            let level = 0.5 - 0.5 * (phase(0.0) * std::f64::consts::TAU).cos();
            scale(group.color, level as f32)
        }
        Effect::Spectrum => hue(phase(0.0) as f32),
        Effect::Wave => hue(phase(-f64::from(key.along)) as f32),
        Effect::Starlight => {
            // Each key fades out over one cycle, from a start of its own.
            let start = f64::from(scatter(key.row, key.col));
            let level = (1.0 - phase(start)).powi(3);
            scale(group.color, level as f32)
        }
    }
}

/// A fixed value from 0 to 1 per key, scattered over the keyboard.
fn scatter(row: u8, col: u8) -> f32 {
    let mut x = u32::from(row) * 31 + u32::from(col) * 131 + 7;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    #[allow(clippy::cast_precision_loss)]
    let value = (x % 1000) as f32 / 1000.0;
    value
}

fn scale(color: [u8; 3], level: f32) -> [u8; 3] {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    color.map(|c| (f32::from(c) * level.clamp(0.0, 1.0)).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::razer::chroma::devices::{KEYBOARD, MOUSEPAD};

    const KEYS: &[Key] = KEYBOARD.keys;

    fn group(effect: Effect, keys: &[(u8, u8)]) -> Group {
        Group {
            keys: keys.iter().copied().collect(),
            effect,
            color: [200, 100, 0],
            ..Group::default()
        }
    }

    #[test]
    fn a_key_in_no_group_is_dark_and_the_later_group_wins() {
        let groups = [
            group(Effect::Static, &[(0, 0), (1, 1)]),
            Group {
                color: [0, 0, 50],
                ..group(Effect::Static, &[(1, 1)])
            },
        ];
        let frame = draw(&KEYBOARD, &groups, 0.0, 1.0);
        assert_eq!(frame[0][0], [200, 100, 0]);
        assert_eq!(frame[1][1], [0, 0, 50]);
        assert_eq!(frame[2][2], [0, 0, 0]);
        assert_eq!(draw(&KEYBOARD, &groups, 0.0, 0.5)[0][0], [100, 50, 0]);
    }

    #[test]
    fn a_position_with_no_key_stays_dark() {
        // Row 0, column 1 is the gap between Esc and F1.
        let frame = draw(&KEYBOARD, &[group(Effect::Static, &[(0, 1)])], 0.0, 1.0);
        assert_eq!(frame[0][1], [0, 0, 0]);
    }

    #[test]
    fn breathing_fades_out_and_back_in_once_a_cycle() {
        let g = Group {
            speed: 1.0,
            ..group(Effect::Breathing, &[(0, 0)])
        };
        let esc = &KEYS[0];
        assert_eq!(color(&g, esc, 0.0), [0, 0, 0]);
        assert_eq!(color(&g, esc, 0.5), [200, 100, 0]);
        assert_eq!(color(&g, esc, 1.0), [0, 0, 0]);
    }

    #[test]
    fn the_wave_gives_the_left_and_right_keys_other_hues() {
        let g = group(Effect::Wave, &[]);
        let right = KEYS.iter().find(|k| k.label == "Vol").copied();
        assert!(right.is_some_and(|r| color(&g, &KEYS[0], 0.0) != color(&g, &r, 0.0)));
        let pad = MOUSEPAD.keys;
        assert_ne!(color(&g, &pad[0], 0.0), color(&g, &pad[9], 0.0));
    }

    #[test]
    fn the_pad_frame_is_one_row_of_19() {
        let frame = draw(&MOUSEPAD, &[group(Effect::Static, &[(0, 18)])], 0.0, 1.0);
        assert_eq!((frame.len(), frame[0].len()), (1, 19));
        assert_eq!(frame[0][18], [200, 100, 0]);
    }
}
