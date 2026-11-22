//! The brightness of a Razer port, the sound levels of each light, and the
//! tag of a light that reacts to sound.

use eframe::egui::{self, Color32, RichText, Slider, Ui};

use super::brands::Brand;
use super::devices::Controls;
use crate::engine::{Status, Target};
use crate::settings::Settings;

/// The brightness slider on a Razer port's card. Answers whether the settings changed.
pub fn light(ui: &mut Ui, settings: &mut Settings, target: &Target) -> bool {
    let mut levels = settings.levels(&target.id);
    let before = levels;
    if target.razer_port.is_some() {
        ui.add(percent(&mut levels.brightness, "brightness"))
            .on_hover_text("Scales the colors that the mirror sends to this port");
    }
    if levels == before {
        return false;
    }
    settings.levels.insert(target.id.clone(), levels);
    true
}

/// One brightness for every Razer port. Answers whether the settings changed.
pub fn razer_all(
    ui: &mut Ui,
    status: &Status,
    settings: &mut Settings,
    controls: &mut Controls,
) -> bool {
    let ports: Vec<&Target> = status
        .targets
        .iter()
        .filter(|t| Brand::of(t) == Brand::Razer)
        .collect();
    if ports.is_empty() {
        return false;
    }
    if !ui
        .add(percent(&mut controls.all_razer, "brightness, all"))
        .changed()
    {
        return false;
    }
    for port in ports {
        settings
            .levels
            .entry(port.id.clone())
            .or_default()
            .brightness = controls.all_razer;
    }
    true
}

/// The tag on a card whose light reacts to sound.
pub fn badge(ui: &mut Ui) {
    egui::Frame::new()
        .fill(Color32::from_rgb(230, 120, 40))
        .corner_radius(3)
        .inner_margin(egui::Margin::symmetric(4, 0))
        .show(ui, |ui| {
            ui.label(
                RichText::new("SOUND")
                    .small()
                    .strong()
                    .color(Color32::from_gray(15)),
            );
        })
        .response
        .on_hover_text("This light reacts to sound");
}

pub fn percent<'a>(value: &'a mut u8, text: &str) -> Slider<'a> {
    Slider::new(value, 0..=100).suffix("%").text(text)
}
