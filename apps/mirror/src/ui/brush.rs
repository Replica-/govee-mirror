//! The brush: the light and segment a click paints, the tool a drag uses,
//! and the colors the segments show in.

use eframe::egui::{self, Color32};

/// The light whose segments the palette shows, and the segment that a click
/// on the grid paints.
#[derive(Debug, Default)]
pub struct Brush {
    pub target: Option<String>,
    pub segment: usize,
    pub tool: Tool,
    /// The box that the Box tool drew, and the corner it started from.
    pub boxed: Option<crate::mapping::Rect>,
    pub anchor: Option<crate::settings::Cell>,
    /// The range a box fill spreads, as segment indexes. `to` is the last
    /// segment where unset.
    pub from: usize,
    pub to: Option<usize>,
}

/// What a drag over the grid does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tool {
    /// Each cell crossed takes the brush's segment.
    #[default]
    Paint,
    /// The drag draws a box, which the palette fills with a range.
    Box,
}

/// One color per segment. The golden-ratio step keeps two neighbors apart in
/// hue, whatever the count.
pub fn segment_color(segment: usize, zones: usize) -> Color32 {
    let hue = if zones <= 12 {
        segment as f32 / zones.max(1) as f32
    } else {
        (segment as f32 * 0.618_034).fract()
    };
    egui::ecolor::Hsva::new(hue, 0.8, 0.95, 1.0).into()
}

/// Black or white, whichever reads on `background`.
pub fn text_on(background: Color32) -> Color32 {
    let [r, g, b, _] = background.to_array();
    let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    if luma > 150.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}
