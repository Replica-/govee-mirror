//! The light list: one card per light, with its controls. A click on a card
//! opens its segments in the palette.

use std::collections::HashMap;

use eframe::egui::{self, Color32, RichText, Sense, Ui};

use crate::engine::{Command, Control, Status, Target};
use crate::settings::{Resolution, Settings};
use crate::ui::Brush;

#[derive(Default)]
pub struct Controls {
    brightness: HashMap<String, i64>,
    color: HashMap<String, [u8; 3]>,
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
    if status.targets.is_empty() && status.busy.is_none() {
        ui.label(RichText::new("No light yet. Press Scan.").weak());
    }
    if brush.target.is_none() {
        brush.target = status.targets.first().map(|t| t.id.clone());
    }
    for target in &status.targets {
        let chosen = brush.target.as_ref() == Some(&target.id);
        let stroke = if chosen {
            egui::Stroke::new(2.0, Color32::WHITE)
        } else {
            egui::Stroke::new(1.0, Color32::from_gray(70))
        };
        let card = egui::Frame::group(ui.style())
            .stroke(stroke)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&target.name).strong());
                let zones = settings.zones(target);
                ui.label(RichText::new(format!("{} · {zones} segments", target.sku)).weak());
                changed |= segments(ui, settings, target);

                // A power command ends the segment channel of some models while
                // it is armed.
                ui.add_enabled_ui(!status.running, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("On").clicked() {
                            commands
                                .push(Command::Control(target.id.clone(), Control::Power(true)));
                        }
                        if ui.button("Off").clicked() {
                            commands
                                .push(Command::Control(target.id.clone(), Control::Power(false)));
                        }
                        let color = controls
                            .color
                            .entry(target.id.clone())
                            .or_insert([255, 255, 255]);
                        egui::color_picker::color_edit_button_srgb(ui, color);
                        if ui.button("Set color").clicked() {
                            commands
                                .push(Command::Control(target.id.clone(), Control::Color(*color)));
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

                if let Some(text) = status.messages.get(&target.id) {
                    ui.colored_label(Color32::LIGHT_RED, text);
                } else if let Some(stats) = status.stats.get(&target.id) {
                    match &stats.error {
                        Some(error) => ui.colored_label(Color32::LIGHT_RED, error),
                        None => {
                            ui.label(RichText::new(format!("{} frames sent", stats.sent)).weak())
                        }
                    };
                }
            });
        if card.response.interact(Sense::click()).clicked() && !chosen {
            brush.target = Some(target.id.clone());
            brush.segment = 0;
        }
        ui.add_space(6.0);
    }
    (commands, changed)
}

fn label(resolution: Resolution, zones: usize) -> String {
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
            .selected_text(label(chosen.resolution, target.zones(chosen.resolution)))
            .show_ui(ui, |ui| {
                for (resolution, zones) in target.options() {
                    ui.selectable_value(
                        &mut chosen.resolution,
                        resolution,
                        label(resolution, zones),
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
