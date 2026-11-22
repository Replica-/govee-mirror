//! The sound panel under the screen: the switch of the sound reaction and its
//! options, the spectrum of the default output with the level a hit must
//! reach, and the lights that react with their two levels.

use std::ops::Range;

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2,
};

use super::bar::slider;
use crate::engine::Target;
use crate::settings::Settings;
use crate::sound::{self, Band, Detect};

const HEIGHT: f32 = 110.0;
/// The width of the light list beside the spectrum.
const LIGHTS: f32 = 330.0;
const BASS: Color32 = Color32::from_rgb(230, 120, 40);
const HIT: Color32 = Color32::from_rgb(255, 190, 80);
const OTHER: Color32 = Color32::from_rgb(70, 130, 200);
const LINE: Color32 = Color32::from_rgb(255, 211, 79);

/// Answers whether the settings changed.
pub fn show(
    ui: &mut Ui,
    settings: &mut Settings,
    shared: &sound::Shared,
    running: bool,
    targets: &[Target],
) -> bool {
    let mut changed = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| changed |= options(ui, settings, running));
    if !settings.sound.enabled {
        return changed;
    }
    let state = sound::lock(shared);
    let (mut bands, bass, trigger, error) = (
        state.bands.clone(),
        state.bass,
        state.trigger,
        state.error.clone(),
    );
    drop(state);
    bands.resize(sound::spectrum::BANDS, 0.0);
    if let Some(error) = error {
        ui.colored_label(Color32::LIGHT_RED, format!("Sound: {error}"));
    }
    ui.horizontal(|ui| {
        let width = (ui.available_width() - LIGHTS - 8.0).max(200.0);
        let sound = &mut settings.sound;
        let fixed = sound.detect == Detect::Fixed;
        let line = if fixed { sound.threshold } else { trigger };
        if let Some(threshold) = plot(ui, width, &bands, sound.band.range(), bass, line, fixed) {
            sound.threshold = threshold;
            changed = true;
        }
        ui.vertical(|ui| {
            ui.set_width(LIGHTS);
            changed |= lights(ui, settings, targets);
        });
    });
    changed
}

/// The switch, the detection, the band and the fade.
fn options(ui: &mut Ui, settings: &mut Settings, running: bool) -> bool {
    let sound = &mut settings.sound;
    let mut changed = ui
        .checkbox(&mut sound.enabled, RichText::new("Sound reaction").strong())
        .on_hover_text(
            "Shows the spectrum. A bass hit raises each light that reacts from its rest level to its peak level.",
        )
        .changed();
    if !sound.enabled {
        return changed;
    }
    egui::ComboBox::from_id_salt("detect")
        .selected_text(sound.detect.label())
        .show_ui(ui, |ui| {
            for detect in Detect::ALL {
                changed |= ui
                    .selectable_value(&mut sound.detect, detect, detect.label())
                    .changed();
            }
        })
        .response
        .on_hover_text(
            "Adaptive: a hit is a jump above the recent average, so it follows each song and the volume. Fixed line: a hit is any level above the line you drag.",
        );
    if sound.detect == Detect::Adaptive {
        changed |= slider(
            ui,
            "Sensitivity",
            &mut sound.sensitivity,
            0.0..=1.0,
            &|v| format!("{:.0}%", v * 100.0),
            "Higher counts smaller jumps as hits",
        );
    }
    egui::ComboBox::from_id_salt("band")
        .selected_text(sound.band.label())
        .show_ui(ui, |ui| {
            for band in Band::ALL {
                changed |= ui
                    .selectable_value(&mut sound.band, band, band.label())
                    .changed();
            }
        })
        .response
        .on_hover_text("The bands that a hit is read from: the kick drum alone, or every bass");
    changed |= slider(
        ui,
        "Fade",
        &mut sound.fade,
        0.05..=2.0,
        &|v| format!("{:.0} ms", v * 1000.0),
        "How long a light takes to fall back to its rest level. 150 to 300 ms shows each hit.",
    );
    if !running {
        ui.label(RichText::new("The lights react while the mirror runs.").weak());
    }
    changed
}

/// Each light with its switch and its two levels. Answers whether the
/// settings changed.
fn lights(ui: &mut Ui, settings: &mut Settings, targets: &[Target]) -> bool {
    let mut changed = false;
    if targets.is_empty() {
        ui.label(RichText::new("No light yet. Press Scan.").weak());
        return false;
    }
    egui::ScrollArea::vertical()
        .id_salt("sound-lights")
        .max_height(HEIGHT)
        .show(ui, |ui| {
            egui::Grid::new("sound-levels")
                .num_columns(3)
                .spacing(vec2(8.0, 2.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("Reacts").small().weak());
                    ui.label(RichText::new("At rest").small().weak());
                    ui.label(RichText::new("On bass").small().weak());
                    ui.end_row();
                    for target in targets {
                        let mut levels = settings.levels(&target.id);
                        let before = levels;
                        ui.add(egui::Checkbox::new(&mut levels.react, short(&target.name)))
                            .on_hover_text(&target.name);
                        ui.add_enabled_ui(levels.react, |ui| {
                            ui.add(percent(&mut levels.rest))
                                .on_hover_text("The level between two hits");
                        });
                        ui.add_enabled_ui(levels.react, |ui| {
                            ui.add(percent(&mut levels.peak))
                                .on_hover_text("The level at a hit");
                        });
                        ui.end_row();
                        if levels != before {
                            settings.levels.insert(target.id.clone(), levels);
                            changed = true;
                        }
                    }
                });
        });
    changed
}

fn percent(value: &mut u8) -> egui::DragValue<'_> {
    egui::DragValue::new(value).range(0..=100).suffix("%")
}

/// A name cut to fit the list.
fn short(name: &str) -> String {
    const MAX: usize = 22;
    if name.chars().count() <= MAX {
        return name.to_owned();
    }
    let cut: String = name.chars().take(MAX - 1).collect();
    format!("{cut}…")
}

/// The bars of the spectrum, and the level a hit must reach as a line. With
/// `draggable`, a drag moves the line: answers the new level.
fn plot(
    ui: &mut Ui,
    width: f32,
    bands: &[f32],
    read: Range<usize>,
    bass: f32,
    line: f32,
    draggable: bool,
) -> Option<f32> {
    let sense = if draggable {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let (area, response) = ui.allocate_exact_size(vec2(width, HEIGHT), sense);
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 4.0, Color32::from_gray(18));
    let hit = bass >= line;
    #[allow(clippy::cast_precision_loss)]
    let step = area.width() / bands.len() as f32;
    #[allow(clippy::cast_precision_loss)]
    let x = |band: usize| area.left() + band as f32 * step;
    for (band, level) in bands.iter().enumerate() {
        let color = match (read.contains(&band), hit) {
            (true, true) => HIT,
            (true, false) => BASS,
            (false, _) => OTHER,
        };
        let top = area.bottom() - level * area.height();
        let bar = Rect::from_min_max(
            pos2(x(band) + 1.0, top),
            pos2(x(band + 1) - 1.0, area.bottom()),
        );
        painter.rect_filled(bar, 1.0, color);
    }

    let mut moved = None;
    if draggable
        && response.is_pointer_button_down_on()
        && let Some(pos) = response.interact_pointer_pos()
    {
        moved = Some(((area.bottom() - pos.y) / area.height()).clamp(0.05, 1.0));
    }
    let level = moved.unwrap_or(line);
    let y = area.bottom() - level * area.height();
    painter.line_segment(
        [pos2(x(read.start), y), pos2(x(read.end), y)],
        Stroke::new(2.0, LINE),
    );
    painter.line_segment(
        [pos2(area.left(), y), pos2(area.right(), y)],
        Stroke::new(1.0, LINE.gamma_multiply(0.3)),
    );
    let text = if draggable {
        format!("Bass threshold {:.0}%", level * 100.0)
    } else {
        "Hit level, following the music".to_owned()
    };
    painter.text(
        pos2(area.right() - 6.0, y - 3.0),
        Align2::RIGHT_BOTTOM,
        text,
        FontId::proportional(12.0),
        LINE,
    );
    let tip = if draggable {
        "The orange bars are the bands a hit is read from. Drag up or down to set the level of a hit."
    } else {
        "The orange bars are the bands a hit is read from. The line is the level a hit must reach now: it follows the music."
    };
    let response = response.on_hover_text(tip);
    if draggable {
        response.on_hover_cursor(CursorIcon::ResizeVertical);
    }
    moved
}
