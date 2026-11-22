//! The headset's card in the Razer section: which sound output plays while
//! the headset is on, and which one while it is off.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Ui};

use crate::audio::{self, Output};
use crate::razer::headset::{self, Shared};
use crate::settings::Settings;

/// How long the list of outputs holds before the panel reads it again.
const LIST: Duration = Duration::from_secs(3);

pub struct Panel {
    state: Shared,
    outputs: Vec<Output>,
    listed: Option<Instant>,
    error: Option<String>,
}

impl Panel {
    pub fn new(state: Shared) -> Self {
        Self {
            state,
            outputs: Vec::new(),
            listed: None,
            error: None,
        }
    }

    /// Whether the headset's dongle is plugged in.
    pub fn present(&self) -> bool {
        headset::lock(&self.state).present
    }

    fn refresh(&mut self) {
        if self.listed.is_some_and(|at| at.elapsed() < LIST) {
            return;
        }
        self.listed = Some(Instant::now());
        match audio::outputs() {
            Ok(outputs) => {
                self.outputs = outputs;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// The headset's card, after the light cards. Answers whether the
    /// settings changed.
    pub fn show(&mut self, ui: &mut Ui, settings: &mut Settings) -> bool {
        let mut changed = false;
        let state = headset::lock(&self.state);
        let (present, connected, failed) = (state.present, state.connected, state.failed);
        let message = state.message.clone();
        drop(state);
        egui::Frame::group(ui.style())
            .stroke(egui::Stroke::new(1.0, Color32::from_gray(70)))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let id = ui.make_persistent_id("headset");
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    false,
                )
                .show_header(ui, |ui| {
                    let dot = if failed {
                        Color32::from_rgb(220, 80, 80)
                    } else if settings.headset.enabled && connected == Some(true) {
                        Color32::from_rgb(80, 200, 120)
                    } else {
                        Color32::from_gray(110)
                    };
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, dot);
                    ui.label(RichText::new("BlackShark V3 Pro").strong());
                })
                .body(|ui| {
                    ui.label(RichText::new("Headset · no lights").weak());
                    changed |= self.body(ui, settings);
                    let line = match (present, connected) {
                        (false, _) => "The headset's dongle is not plugged in.",
                        _ if !settings.headset.enabled => "The switch is off.",
                        (true, None) => "Waiting for the headset to switch on or off.",
                        (true, Some(true)) => "The headset is on.",
                        (true, Some(false)) => "The headset is off.",
                    };
                    ui.label(RichText::new(line).weak());
                    if let Some(message) = &message {
                        ui.label(RichText::new(message).weak());
                    }
                    if let Some(error) = &self.error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                });
            });
        changed
    }

    fn body(&mut self, ui: &mut Ui, settings: &mut Settings) -> bool {
        let mut changed = false;
        let switch = &mut settings.headset;
        if ui
            .checkbox(
                &mut switch.enabled,
                "Switch the sound output with the headset",
            )
            .changed()
        {
            changed = true;
            if switch.enabled && switch.back_to.is_none() {
                switch.back_to = audio::default_output().ok();
            }
        }
        if !switch.enabled {
            return changed;
        }
        changed |= ui
            .checkbox(&mut switch.notify, "Show a notification at each switch")
            .changed();
        self.refresh();
        egui::Grid::new("headset-outputs")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("On");
                changed |= pick(ui, "headset-on", &self.outputs, &mut switch.output);
                ui.end_row();
                ui.label("Off");
                changed |= pick(ui, "headset-off", &self.outputs, &mut switch.back_to);
                ui.end_row();
            });
        changed
    }
}

/// A list of the outputs. Answers whether the pick changed.
fn pick(ui: &mut Ui, id: &str, outputs: &[Output], chosen: &mut Option<String>) -> bool {
    let text = chosen
        .as_deref()
        .map_or("Pick an output", |id| audio::name(outputs, id));
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(220.0)
        .show_ui(ui, |ui| {
            for output in outputs {
                let selected = chosen.as_deref() == Some(output.id.as_str());
                if ui.selectable_label(selected, &output.name).clicked() && !selected {
                    *chosen = Some(output.id.clone());
                    changed = true;
                }
            }
        });
    changed
}
