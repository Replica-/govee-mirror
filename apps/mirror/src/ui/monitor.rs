//! The monitor's card in the MSI section: its input, a button for each
//! input, and the hotkeys.

use std::sync::mpsc::Sender;

use eframe::egui::{self, Color32, RichText, Ui};

use crate::monitor::{self, INPUTS, Shared};

pub struct Panel {
    state: Shared,
    send: Sender<u32>,
    /// Whether the hotkeys are registered.
    hotkeys: bool,
}

impl Panel {
    pub fn new(state: Shared, send: Sender<u32>, hotkeys: bool) -> Self {
        Self {
            state,
            send,
            hotkeys,
        }
    }

    /// Whether the monitor answers DDC/CI.
    pub fn present(&self) -> bool {
        monitor::lock(&self.state).present
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let state = monitor::lock(&self.state);
        let (model, input) = (state.model.clone(), state.input);
        let (message, failed) = (state.message.clone(), state.failed);
        drop(state);
        egui::Frame::group(ui.style())
            .stroke(egui::Stroke::new(1.0, Color32::from_gray(70)))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    let dot = Color32::from_rgb(80, 200, 120);
                    ui.painter().circle_filled(rect.center(), 4.0, dot);
                    ui.label(RichText::new(model.as_deref().unwrap_or("Monitor")).strong());
                });
                let current = input.map_or_else(|| "Reading…".to_owned(), monitor::name);
                ui.label(format!("Input: {current}"));
                ui.horizontal(|ui| {
                    for (value, name) in INPUTS {
                        let shown = input == Some(value);
                        if ui.add_enabled(!shown, egui::Button::new(name)).clicked() {
                            let _ = self.send.send(value);
                        }
                    }
                });
                if self.hotkeys {
                    let keys: Vec<String> = crate::hotkey::labels()
                        .map(|(name, key)| format!("{key}: {name}"))
                        .collect();
                    ui.label(RichText::new(keys.join(" · ")).weak());
                } else {
                    ui.label(RichText::new("The hotkeys are held by another program.").weak());
                }
                ui.label(RichText::new("The monitor's USB goes with the input.").weak());
                if let Some(message) = &message {
                    if failed {
                        ui.colored_label(Color32::LIGHT_RED, message);
                    } else {
                        ui.label(RichText::new(message).weak());
                    }
                }
            });
    }
}
