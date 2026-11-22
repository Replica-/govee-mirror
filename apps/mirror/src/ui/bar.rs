//! The top bar, on one row: the start button over the frame rate and the
//! usage, then the settings in titled groups.

use std::ops::RangeInclusive;
use std::time::Instant;

use eframe::egui::style::HandleShape;
use eframe::egui::{self, Color32, RichText, Ui, emath::Numeric};

use super::{App, profiles};
use crate::engine::{Command, Status};
use crate::pattern::Source;
use crate::{startup, usage};

const START: Color32 = Color32::from_rgb(38, 120, 70);
const STOP: Color32 = Color32::from_rgb(150, 50, 50);

/// What the bar asks of the window.
#[derive(Default)]
pub struct Outcome {
    pub commands: Vec<Command>,
    pub changed: bool,
    pub problem: Option<String>,
}

impl App {
    pub(super) fn bar(
        &mut self,
        ui: &mut Ui,
        status: &Status,
        screens: &[String],
        capture_error: Option<&str>,
    ) -> Outcome {
        let mut out = Outcome::default();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (label, fill) = if status.running {
                ("Stop mirror", STOP)
            } else {
                ("Start mirror", START)
            };
            let button = egui::Button::new(
                RichText::new(label)
                    .strong()
                    .size(15.0)
                    .color(Color32::WHITE),
            )
            .fill(fill)
            .min_size(egui::vec2(150.0, 28.0));
            ui.vertical(|ui| {
                if ui.add_enabled(status.busy.is_none(), button).clicked() {
                    out.commands.push(if status.running {
                        Command::Stop
                    } else {
                        Command::Start
                    });
                }
                ui.horizontal(|ui| {
                    if let Some(busy) = status.busy {
                        ui.spinner();
                        ui.label(busy);
                    } else {
                        self.stats(ui, status);
                    }
                });
            });
            group(ui, "Screen", |ui| {
                let screen = screens
                    .get(self.settings.display)
                    .cloned()
                    .unwrap_or_else(|| "Screen".to_owned());
                egui::ComboBox::from_id_salt("screen")
                    .selected_text(screen)
                    .width(150.0)
                    .truncate()
                    .show_ui(ui, |ui| {
                        for (index, name) in screens.iter().enumerate() {
                            out.changed |= ui
                                .selectable_value(&mut self.settings.display, index, name)
                                .changed();
                        }
                    });
                out.changed |= ui
                    .add(egui::DragValue::new(&mut self.settings.grid.cols).range(1..=96))
                    .on_hover_text("Columns. The rows follow, so that every cell is square.")
                    .changed();
                ui.label(RichText::new(format!("× {}", self.settings.grid.rows)).weak());
            });
            group(ui, "Output", |ui| {
                out.changed |= slider(
                    ui,
                    "Frame rate",
                    &mut self.settings.fps,
                    5..=60,
                    &|v| format!("{v:.0} fps"),
                    "How often the lights take a new frame",
                );
                out.changed |= slider(
                    ui,
                    "Smoothing",
                    &mut self.settings.smoothing,
                    0.0..=0.95,
                    &|v| format!("{:.0}%", v * 100.0),
                    "How much of the last frame stays: higher is calmer, lower follows the screen faster",
                );
                out.changed |= slider(
                    ui,
                    "Saturation",
                    &mut self.settings.saturation,
                    1.0..=2.0,
                    &|v| {
                        if v <= 1.0 {
                            "Off".to_owned()
                        } else {
                            format!("+{:.0}%", (v - 1.0) * 100.0)
                        }
                    },
                    "Pushes colors away from grey, so that the lights show solid colors",
                );
            });
            group(ui, "Source", |ui| {
                ui.vertical(|ui| {
                    egui::ComboBox::from_id_salt("source")
                        .selected_text(self.settings.source.label())
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for source in Source::ALL {
                                out.changed |= ui
                                    .selectable_value(
                                        &mut self.settings.source,
                                        source,
                                        source.label(),
                                    )
                                    .changed();
                            }
                        })
                        .response
                        .on_hover_text("What the lights mirror: the screen, or a pattern");
                    if self.settings.source != Source::Screen {
                        out.changed |= ui.checkbox(&mut self.settings.moving, "Moving").changed();
                    }
                });
                if self.settings.source != Source::Screen && self.settings.moving {
                    out.changed |= slider(
                        ui,
                        "Speed",
                        &mut self.settings.animation_speed,
                        0.05..=2.0,
                        &|v| format!("×{v:.2}"),
                        "How fast the pattern moves",
                    );
                }
            });
            group(ui, "Startup", |ui| {
                ui.vertical(|ui| {
                    out.changed |= ui
                        .checkbox(&mut self.settings.mirror_on_launch, "Mirror on launch")
                        .changed();
                    if ui
                        .checkbox(&mut self.with_windows, "Start with Windows")
                        .on_hover_text("Starts in the tray when you sign in")
                        .changed()
                        && let Err(error) = startup::set(self.with_windows)
                    {
                        self.with_windows = startup::enabled();
                        out.problem = Some(format!("Start with Windows did not change: {error}"));
                    }
                });
            });
        });
        if let Some(error) = capture_error {
            ui.colored_label(Color32::LIGHT_RED, format!("Capture: {error}"));
        }
        ui.add_space(4.0);
        out
    }

    /// The profile row above the grid. Answers whether the settings changed.
    pub(super) fn profile(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        group(ui, "Profile", |ui| {
            match self.profiles.panel(ui, &self.settings) {
                profiles::Action::Switched => {
                    self.settings = self.profiles.lock().current();
                    changed = true;
                }
                profiles::Action::Edited => self.unsaved = Some(Instant::now()),
                profiles::Action::None => {}
            }
        });
        changed
    }

    /// The rate the mirror reaches, then the CPU and GPU share.
    fn stats(&self, ui: &mut Ui, status: &Status) {
        let usage = *usage::lock(&self.usage);
        let percent =
            |value: Option<f32>| value.map_or("\u{2026}".to_owned(), |v| format!("{v:.1}%"));
        let stat = |ui: &mut Ui, name: &str, value: String| {
            ui.label(RichText::new(name).small().weak());
            ui.label(RichText::new(value).strong().monospace());
        };
        ui.spacing_mut().item_spacing.x = 4.0;
        if status.running {
            stat(ui, "FPS", status.fps.to_string());
            ui.add_space(4.0);
        }
        ui.scope(|ui| {
            stat(ui, "CPU", percent(usage.map(|u| u.cpu)));
            ui.add_space(4.0);
            stat(ui, "GPU", percent(usage.and_then(|u| u.gpu)));
        })
        .response
        .on_hover_text("This app's share of the machine, as Task Manager counts it");
    }
}

/// A slider under a line that holds its name and its value. A range that
/// spans more than one order of magnitude moves on a log scale.
pub(super) fn slider<N: Numeric>(
    ui: &mut Ui,
    name: &str,
    value: &mut N,
    range: RangeInclusive<N>,
    show: &dyn Fn(f64) -> String,
    tip: &str,
) -> bool {
    const WIDTH: f32 = 135.0;
    let log = range.start().to_f64() > 0.0 && range.end().to_f64() / range.start().to_f64() > 10.0;
    ui.vertical(|ui| {
        ui.set_width(WIDTH);
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(show(value.to_f64())).strong());
            });
        });
        let spacing = ui.spacing_mut();
        spacing.slider_width = WIDTH;
        spacing.slider_rail_height = 6.0;
        ui.add(
            egui::Slider::new(value, range)
                .show_value(false)
                .trailing_fill(true)
                .logarithmic(log)
                .handle_shape(HandleShape::Circle),
        )
        .on_hover_text(tip)
        .changed()
    })
    .inner
}

/// A titled frame around one row of controls.
fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title.to_uppercase()).small().weak());
            ui.horizontal(add);
        });
    });
}
