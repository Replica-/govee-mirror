//! What the mirror samples: the screen, or a pattern drawn on the grid.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    #[default]
    Screen,
    /// Rainbow rings around the center of the grid. They move outward.
    Rings,
    /// Twelve solid wedges around the center. They turn clockwise.
    Wheel,
    /// The twelve wedges, twisted half a turn from the center to a corner.
    Spiral,
}

impl Source {
    pub const ALL: [Self; 4] = [Self::Screen, Self::Rings, Self::Wheel, Self::Spiral];

    pub fn label(self) -> &'static str {
        match self {
            Self::Screen => "Screen",
            Self::Rings => "Rings",
            Self::Wheel => "Color wheel",
            Self::Spiral => "Spiral",
        }
    }
}

/// The wedges of [`Source::Wheel`] and [`Source::Spiral`], clockwise from 12
/// o'clock.
const WHEEL: [[u8; 3]; 12] = [
    [0xA3, 0xD3, 0x12],
    [0x52, 0xB6, 0x1A],
    [0x17, 0x8F, 0x6A],
    [0x45, 0x6E, 0xB0],
    [0x9E, 0x4C, 0xAE],
    [0xC8, 0x45, 0xA0],
    [0xFF, 0x3F, 0xA4],
    [0xFF, 0x5A, 0x00],
    [0xFF, 0x7F, 0x00],
    [0xFF, 0xAA, 0x00],
    [0xFF, 0xCC, 0x00],
    [0xEE, 0xE6, 0x00],
];

/// One color per cell, row by row, or `None` for [`Source::Screen`].
///
/// `seconds` is how far the pattern has moved, and `speed` how many times a
/// second one ring or one wedge takes the place of the next. A cell is square,
/// so a circle on the grid is a circle on the screen.
pub fn draw(
    source: Source,
    cols: usize,
    rows: usize,
    seconds: f32,
    speed: f32,
) -> Option<Vec<[u8; 3]>> {
    if source == Source::Screen {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let (cx, cy) = (cols as f32 / 2.0, rows as f32 / 2.0);
    let corner = cx.hypot(cy).max(f32::EPSILON);
    let step = seconds * speed;
    let cells = (0..rows)
        .flat_map(|row| (0..cols).map(move |col| (row, col)))
        .map(|(row, col)| {
            #[allow(clippy::cast_precision_loss)]
            let (dx, dy) = (col as f32 + 0.5 - cx, row as f32 + 0.5 - cy);
            let distance = dx.hypot(dy) / corner;
            // 0 at 12 o'clock, growing clockwise to 1.
            let turn = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
            match source {
                // Two full rainbows from the center to a corner.
                Source::Rings => hue((distance * 2.0 - step).rem_euclid(1.0)),
                Source::Wheel => wedge(turn - step / 12.0),
                Source::Spiral => wedge(turn - distance * 0.5 - step / 12.0),
                Source::Screen => [0; 3],
            }
        })
        .collect();
    Some(cells)
}

/// The color wheel as an icon: `size` × `size` RGBA pixels, transparent
/// outside the circle.
pub fn icon(size: u32) -> Vec<u8> {
    #[allow(clippy::cast_precision_loss)]
    let half = size as f32 / 2.0;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            #[allow(clippy::cast_precision_loss)]
            let (dx, dy) = (x as f32 + 0.5 - half, y as f32 + 0.5 - half);
            let turn = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
            let [r, g, b] = wedge(turn);
            let alpha = if dx.hypot(dy) <= half { 255 } else { 0 };
            rgba.extend_from_slice(&[r, g, b, alpha]);
        }
    }
    rgba
}

/// The wedge of [`WHEEL`] at `turn`, where a whole turn is 1.
fn wedge(turn: f32) -> [u8; 3] {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = (turn.rem_euclid(1.0) * 12.0) as usize;
    WHEEL[index.min(11)]
}

/// A fully saturated color at `hue`, from 0 to 1.
fn hue(hue: f32) -> [u8; 3] {
    let channel = |offset: f32| {
        let k = (hue * 6.0 + offset).rem_euclid(6.0);
        let value = 1.0 - (k.min(4.0 - k).clamp(0.0, 1.0));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = (value * 255.0).round() as u8;
        byte
    };
    [channel(5.0), channel(3.0), channel(1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_screen_draws_nothing() {
        assert_eq!(draw(Source::Screen, 4, 4, 0.0, 1.0), None);
    }

    #[test]
    fn the_rings_start_red_and_move_outward() {
        assert_eq!(hue(0.0), [255, 0, 0]);
        assert_eq!(hue(1.0 / 3.0), [0, 255, 0]);
        let still = draw(Source::Rings, 2, 2, 0.0, 1.0).unwrap_or_default();
        // The four cells sit at the same distance from the center.
        assert!(still.iter().all(|c| *c == still[0]));
        assert_ne!(
            draw(Source::Rings, 2, 2, 0.25, 1.0).unwrap_or_default()[0],
            still[0]
        );
    }

    #[test]
    fn the_wheel_runs_clockwise_from_12_o_clock() {
        // A 4 × 4 grid: the cell right of the top center, the top right
        // corner, then the cell below the right center.
        let wheel = draw(Source::Wheel, 4, 4, 0.0, 1.0).unwrap_or_default();
        assert_eq!(wheel[2], WHEEL[0]);
        assert_eq!(wheel[3], WHEEL[1]);
        assert_eq!(wheel[2 * 4 + 3], WHEEL[3]);
        // Twelve steps of speed 1 bring every wedge back to its place.
        assert_eq!(
            draw(Source::Wheel, 4, 4, 12.0, 1.0).unwrap_or_default(),
            wheel
        );
        assert_ne!(
            draw(Source::Wheel, 4, 4, 1.0, 1.0).unwrap_or_default(),
            wheel
        );
    }

    #[test]
    fn the_icon_is_a_disc_of_the_wheel() {
        let icon = icon(16);
        assert_eq!(icon.len(), 16 * 16 * 4);
        // A corner is outside the circle; the pixel right of the top center
        // is in the first wedge.
        assert_eq!(icon[3], 0);
        let top = (2 * 16 + 8) * 4;
        assert_eq!(
            &icon[top..top + 4],
            &[WHEEL[0][0], WHEEL[0][1], WHEEL[0][2], 255]
        );
    }

    #[test]
    fn the_spiral_twists_the_wheel_away_from_the_center() {
        let wheel = draw(Source::Wheel, 8, 8, 0.0, 1.0).unwrap_or_default();
        let spiral = draw(Source::Spiral, 8, 8, 0.0, 1.0).unwrap_or_default();
        assert_ne!(wheel, spiral);
    }
}
