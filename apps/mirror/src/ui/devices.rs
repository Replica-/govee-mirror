//! The light list: a section per brand, then one card per light, with its
//! controls. A click on a card opens its segments in the palette.

use std::collections::HashMap;

use eframe::egui::{self, Color32, RichText, Sense, Ui, UiBuilder};

use super::brands::{self, Brand};
use crate::engine::{Command, Control, Status, Target};
use crate::settings::{Resolution, Settings};
use crate::ui::Brush;

pub struct Controls {
    brightness: HashMap<String, i64>,
    color: HashMap<String, [u8; 3]>,
    pub(super) all_color: [u8; 3],
    /// Percent of each light's own range.
    pub(super) all_brightness: u8,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            brightness: HashMap::new(),
            color: HashMap::new(),
            all_color: [255; 3],
            all_brightness: 100,
        }
    }
}

/// Answers the commands to send, and whether the settings changed.
pub fn show(
    ui: &mut Ui,
    status: &Status,
    settings: &mut Settings,
    brush: &mut Brush,
    controls: &mut Controls,
) -> (Vec<Command>, bool) {
    let mut commands = Vec::new();
    let mut changed = false;
    if brush.target.is_none() {
        brush.target = status.targets.first().map(|t| t.id.clone());
    }
    for brand in [Brand::Govee, Brand::Razer] {
        let lights: Vec<&Target> = status
            .targets
            .iter()
            .filter(|t| Brand::of(t) == brand)
            .collect();
        if brand == Brand::Razer && lights.is_empty() && !status.razer {
            continue;
        }
        brands::section(ui, brand, lights.len(), |ui| {
            brands::general(ui, brand, |ui| match brand {
                Brand::Govee => commands.extend(brands::govee(ui, status, controls)),
                Brand::Razer => changed |= brands::razer(ui, status, settings),
            });
            if lights.is_empty() && status.busy.is_none() {
                ui.label(RichText::new("No light yet. Press Scan.").weak());
            }
            for target in lights {
                changed |= card(ui, status, settings, brush, controls, target, &mut commands);
                ui.add_space(4.0);
            }
        });
        ui.add_space(8.0);
    }
    (commands, changed)
}

/// One light: a header that folds, and its settings. A click on the card
/// chooses the light for the palette. Answers whether the settings changed.
fn card(
    ui: &mut Ui,
    status: &Status,
    settings: &mut Settings,
    brush: &mut Brush,
    controls: &mut Controls,
    target: &Target,
    commands: &mut Vec<Command>,
) -> bool {
    let mut changed = false;
    let chosen = brush.target.as_ref() == Some(&target.id);
    let stroke = if chosen {
        egui::Stroke::new(2.0, Color32::WHITE)
    } else {
        egui::Stroke::new(1.0, Color32::from_gray(70))
    };
    // The sense goes on the scope, under the controls: a sense added after
    // them sits on top and takes their clicks.
    let card = ui.scope_builder(UiBuilder::new().sense(Sense::click()), |ui| {
        egui::Frame::group(ui.style()).stroke(stroke).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let id = ui.make_persistent_id(("light", &target.id));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                .show_header(ui, |ui| {
                    commands.extend(header(ui, status, target));
                })
                .body(|ui| {
                    let zones = settings.zones(target);
                    ui.label(RichText::new(format!("{} · {zones} segments", target.sku)).weak());
                    changed |= segments(ui, settings, target);
                    if target.razer_port.is_some() {
                        ui.label(
                            RichText::new(
                                "Lights while the mirror runs. The controller's own effect shows otherwise.",
                            )
                            .weak(),
                        );
                    } else {
                        commands.extend(govee_controls(ui, status, target, controls));
                    }
                    if let Some(stats) = status.stats.get(&target.id) {
                        ui.label(RichText::new(format!("{} frames sent", stats.sent)).weak());
                    }
                });
            if let Some(error) = problem(status, &target.id) {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        });
    });
    if card.response.clicked() && !chosen {
        brush.target = Some(target.id.clone());
        brush.segment = 0;
    }
    changed
}

/// The last failure of a command or of the stream to this light.
fn problem<'a>(status: &'a Status, id: &str) -> Option<&'a str> {
    status
        .messages
        .get(id)
        .or_else(|| status.stats.get(id)?.error.as_ref())
        .map(String::as_str)
}

/// A dot for the state, the name, and the power buttons.
fn header(ui: &mut Ui, status: &Status, target: &Target) -> Vec<Command> {
    let mut commands = Vec::new();
    let dot = if problem(status, &target.id).is_some() {
        Color32::from_rgb(220, 80, 80)
    } else if status.stats.contains_key(&target.id) {
        Color32::from_rgb(80, 200, 120)
    } else {
        Color32::from_gray(110)
    };
    // The buttons keep their room; a long name is cut short.
    let room = ui.available_width()
        - if target.razer_port.is_some() {
            0.0
        } else {
            70.0
        };
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(room.max(0.0), height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, dot);
            // A selectable label takes the click that chooses the card.
            ui.add(
                egui::Label::new(RichText::new(&target.name).strong())
                    .truncate()
                    .selectable(false),
            );
        },
    );
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if target.razer_port.is_some() {
            return;
        }
        // A power command ends the segment channel of some models while it
        // is armed.
        ui.add_enabled_ui(!status.running, |ui| {
            for (text, on) in [("Off", false), ("On", true)] {
                if ui
                    .small_button(text)
                    .on_disabled_hover_text("Stop the mirror first")
                    .clicked()
                {
                    commands.push(Command::Control(target.id.clone(), Control::Power(on)));
                }
            }
        });
    });
    commands
}

/// Color and brightness of a Govee light.
fn govee_controls(
    ui: &mut Ui,
    status: &Status,
    target: &Target,
    controls: &mut Controls,
) -> Vec<Command> {
    let mut commands = Vec::new();
    ui.add_enabled_ui(!status.running, |ui| {
        ui.horizontal(|ui| {
            let color = controls
                .color
                .entry(target.id.clone())
                .or_insert([255, 255, 255]);
            egui::color_picker::color_edit_button_srgb(ui, color);
            if ui.button("Set color").clicked() {
                commands.push(Command::Control(target.id.clone(), Control::Color(*color)));
            }
        });
    });
    let [low, high] = target.brightness;
    let level = controls.brightness.entry(target.id.clone()).or_insert(high);
    let slider = ui.add(egui::Slider::new(level, low..=high).text("brightness"));
    if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
        commands.push(Command::Control(
            target.id.clone(),
            Control::Brightness(*level),
        ));
    }
    commands
}

fn label(target: &Target, resolution: Resolution, zones: usize) -> String {
    if target.razer_port.is_some() {
        return format!("One per LED ({zones})");
    }
    match resolution {
        Resolution::App => format!("As the Govee app ({zones})"),
        Resolution::Full => format!("Every zone the channel renders ({zones})"),
        Resolution::Native => format!("One per LED ({zones})"),
    }
}

/// The segment count: a resolution the device file offers, or a count set by
/// hand. Answers whether the settings changed.
fn segments(ui: &mut Ui, settings: &mut Settings, target: &Target) -> bool {
    if !target.streams() {
        ui.label(
            RichText::new("No device file gives this model the segment channel: one color.").weak(),
        );
        return false;
    }
    let mut chosen = settings
        .devices
        .get(&target.id)
        .cloned()
        .unwrap_or_default();
    chosen.resolution = settings.resolution(target);
    let mut manual = settings.manual_zones(target);
    let before = (chosen.resolution, manual, chosen.gradient);

    ui.add_enabled_ui(manual.is_none(), |ui| {
        egui::ComboBox::from_id_salt(("resolution", &target.id))
            .selected_text(label(
                target,
                chosen.resolution,
                target.zones(chosen.resolution),
            ))
            .show_ui(ui, |ui| {
                for (resolution, zones) in target.options() {
                    ui.selectable_value(
                        &mut chosen.resolution,
                        resolution,
                        label(target, resolution, zones),
                    );
                }
            });
    });
    ui.horizontal(|ui| {
        let mut by_hand = manual.is_some();
        if ui.checkbox(&mut by_hand, "Set by hand").changed() {
            manual = by_hand.then(|| target.zones(chosen.resolution));
        }
        if let Some(count) = manual.as_mut() {
            ui.add(egui::DragValue::new(count).range(1..=target.max_zones));
            ui.label(RichText::new(format!("of {}", target.max_zones)).weak());
        }
    });
    ui.checkbox(&mut chosen.gradient, "Blend between segments")
        .on_hover_text(
            "The light fades from one segment to the next, and from the last back to the first",
        );

    if (chosen.resolution, manual, chosen.gradient) == before {
        return false;
    }
    chosen.zones = manual;
    settings.devices.insert(target.id.clone(), chosen);
    true
}
