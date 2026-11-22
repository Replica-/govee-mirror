//! A section per brand in the light list: a colored header, then the
//! settings that cover every light of the brand.

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, Ui};

use super::devices::Controls;
use crate::engine::{Command, Control, Status, Target};
use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Brand {
    Govee,
    Razer,
}

impl Brand {
    pub fn of(target: &Target) -> Self {
        if target.razer_port.is_some() {
            Self::Razer
        } else {
            Self::Govee
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Govee => "GOVEE",
            Self::Razer => "RAZER",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Self::Govee => Color32::from_rgb(70, 160, 240),
            Self::Razer => Color32::from_rgb(68, 214, 44),
        }
    }
}

/// A frame in the brand's color, with a filled header strip.
pub fn section(ui: &mut Ui, brand: Brand, lights: usize, add: impl FnOnce(&mut Ui)) {
    let color = brand.color();
    egui::Frame::new()
        .stroke(Stroke::new(1.5, color))
        .corner_radius(6)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Frame::new()
                .fill(color)
                .corner_radius(CornerRadius {
                    nw: 5,
                    ne: 5,
                    sw: 0,
                    se: 0,
                })
                .inner_margin(Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let ink = Color32::from_gray(15);
                        ui.label(RichText::new(brand.title()).strong().size(14.0).color(ink));
                        let noun = if lights == 1 { "light" } else { "lights" };
                        ui.label(RichText::new(format!("{lights} {noun}")).color(ink));
                    });
                });
            egui::Frame::new().inner_margin(6).show(ui, add);
        });
}

/// A folded "General" block.
pub fn general(ui: &mut Ui, brand: Brand, add: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new("General").strong())
        .id_salt(("general", brand.title()))
        .default_open(false)
        .show(ui, add);
    ui.add_space(4.0);
}

/// Power, color and brightness for every Govee light at once.
pub fn govee(ui: &mut Ui, status: &Status, controls: &mut Controls) -> Vec<Command> {
    let mut commands = Vec::new();
    let lights: Vec<&Target> = status
        .targets
        .iter()
        .filter(|t| Brand::of(t) == Brand::Govee)
        .collect();
    let all = |commands: &mut Vec<Command>, control: &dyn Fn(&Target) -> Control| {
        for target in &lights {
            commands.push(Command::Control(target.id.clone(), control(target)));
        }
    };
    // A power command ends the segment channel of some models while it is
    // armed.
    ui.add_enabled_ui(!status.running && !lights.is_empty(), |ui| {
        ui.horizontal(|ui| {
            if ui
                .button("All on")
                .on_disabled_hover_text("Stop the mirror first")
                .clicked()
            {
                all(&mut commands, &|_| Control::Power(true));
            }
            if ui
                .button("All off")
                .on_disabled_hover_text("Stop the mirror first")
                .clicked()
            {
                all(&mut commands, &|_| Control::Power(false));
            }
        });
        ui.horizontal(|ui| {
            egui::color_picker::color_edit_button_srgb(ui, &mut controls.all_color);
            if ui.button("Set every color").clicked() {
                let color = controls.all_color;
                all(&mut commands, &|_| Control::Color(color));
            }
        });
    });
    let slider = ui.add_enabled(
        !lights.is_empty(),
        egui::Slider::new(&mut controls.all_brightness, 1..=100)
            .suffix("%")
            .text("brightness, all"),
    );
    if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
        let percent = i64::from(controls.all_brightness);
        all(&mut commands, &|target| {
            let [low, high] = target.brightness;
            Control::Brightness(low + (high - low) * percent / 100)
        });
    }
    commands
}

/// The LED count of each port of the Razer ARGB controller. A port with 0 is
/// not a light. Answers whether the settings changed.
pub fn razer(ui: &mut Ui, status: &Status, settings: &mut Settings) -> bool {
    let mut changed = false;
    if !status.razer {
        ui.label(RichText::new("No ARGB controller is connected.").weak());
        return false;
    }
    ui.label(RichText::new("ARGB controller").strong());
    ui.label(RichText::new("LEDs on each port, as set in Synapse. 0 leaves the port out.").weak());
    egui::Grid::new("razer-ports")
        .num_columns(2)
        .show(ui, |ui| {
            for (port, leds) in settings.razer_leds.iter_mut().enumerate() {
                ui.label(format!("Port {}", port + 1));
                changed |= ui
                    .add(egui::DragValue::new(leds).range(0..=crate::razer::protocol::LEDS))
                    .changed();
                ui.end_row();
            }
        });
    changed
}
