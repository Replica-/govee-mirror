//! The window.

mod devices;
mod grid;
mod palette;

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions};

use crate::capture::{self, Request, Requests};
use crate::engine::{self, Command, Engine};
use crate::mapping::square_rows;
use crate::settings::Settings;
use crate::startup;

/// How often the window redraws the preview while nothing else asks it to.
const PREVIEW: Duration = Duration::from_millis(100);
/// How long the settings wait after a change before they go to disk.
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// The light whose segments the palette shows, and the segment that a click
/// on the grid paints.
#[derive(Debug, Default)]
pub struct Brush {
    pub target: Option<String>,
    pub segment: usize,
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

pub struct App {
    runtime: tokio::runtime::Runtime,
    engine: Engine,
    capture: capture::Shared,
    requests: Requests,
    settings: Settings,
    unsaved: Option<Instant>,
    preview: Option<TextureHandle>,
    shown: u64,
    brush: Brush,
    controls: devices::Controls,
    /// Minimize on the first frame: the window builder has no such option.
    minimize: bool,
    /// The `Run` entry as last read or written.
    with_windows: bool,
}

impl App {
    pub fn new(
        runtime: tokio::runtime::Runtime,
        engine: Engine,
        capture: capture::Shared,
        requests: Requests,
        settings: Settings,
        minimized: bool,
    ) -> Self {
        Self {
            runtime,
            engine,
            capture,
            requests,
            settings,
            unsaved: None,
            preview: None,
            shown: 0,
            brush: Brush::default(),
            controls: devices::Controls::default(),
            minimize: minimized,
            with_windows: startup::enabled(),
        }
    }

    /// Hand a change of the settings to the capture and the engine, and save
    /// it a moment later.
    fn changed(&mut self) {
        *self
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Request {
            display: self.settings.display,
            fps: self.settings.fps,
        };
        self.engine
            .send(Command::Settings(Box::new(self.settings.clone())));
        self.unsaved = Some(Instant::now());
    }

    fn save(&mut self) {
        self.unsaved = None;
        if let Err(error) = self.settings.save() {
            engine::lock(&self.engine.status)
                .problems
                .push(format!("The settings were not saved: {error}"));
        }
    }

    fn refresh_preview(&mut self, ctx: &egui::Context) -> (Option<String>, Vec<String>) {
        let latest = capture::lock(&self.capture);
        let (error, screens) = (latest.error.clone(), latest.screens.clone());
        let frame = latest.frame.clone();
        drop(latest);
        if let Some(frame) = frame.filter(|f| f.sequence != self.shown) {
            self.shown = frame.sequence;
            let image =
                ColorImage::from_rgba_unmultiplied([frame.width, frame.height], &frame.rgba);
            match &mut self.preview {
                Some(texture) => texture.set(image, TextureOptions::LINEAR),
                None => {
                    self.preview = Some(ctx.load_texture("screen", image, TextureOptions::LINEAR))
                }
            }
        }
        (error, screens)
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if std::mem::take(&mut self.minimize) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        let (capture_error, screens) = self.refresh_preview(&ctx);
        let status_handle = self.engine.status.clone();
        let status = engine::lock(&status_handle);
        let mut commands = Vec::new();
        let mut changed = false;
        let mut problem = None;

        // The rows follow the columns, so that every cell is square.
        if let Some(texture) = &self.preview {
            let [w, h] = texture.size();
            let rows = square_rows(self.settings.grid.cols, w as f32 / h.max(1) as f32);
            if rows != self.settings.grid.rows {
                self.settings.grid.rows = rows;
                changed = true;
            }
        }

        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let busy = status.busy.is_some();
                let label = if status.running {
                    "Stop mirror"
                } else {
                    "Start mirror"
                };
                if ui
                    .add_enabled(!busy, egui::Button::new(RichText::new(label).strong()))
                    .clicked()
                {
                    commands.push(if status.running {
                        Command::Stop
                    } else {
                        Command::Start
                    });
                }
                ui.separator();
                let screen = screens
                    .get(self.settings.display)
                    .cloned()
                    .unwrap_or_else(|| "Screen".to_owned());
                egui::ComboBox::from_id_salt("screen")
                    .selected_text(screen)
                    .show_ui(ui, |ui| {
                        for (index, name) in screens.iter().enumerate() {
                            changed |= ui
                                .selectable_value(&mut self.settings.display, index, name)
                                .changed();
                        }
                    });
                ui.label("Columns");
                changed |= ui
                    .add(egui::DragValue::new(&mut self.settings.grid.cols).range(1..=96))
                    .changed();
                ui.label(
                    RichText::new(format!("× {} rows, square cells", self.settings.grid.rows))
                        .weak(),
                );
                ui.separator();
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut self.settings.fps)
                            .range(5..=60)
                            .suffix(" fps"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut self.settings.smoothing, 0.0..=0.95)
                            .text("smoothing"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut self.settings.saturation, 1.0..=2.0)
                            .text("saturation"),
                    )
                    .on_hover_text(
                        "Pushes colors away from grey, so that the lights show solid colors",
                    )
                    .changed();
                ui.separator();
                changed |= ui
                    .checkbox(&mut self.settings.mirror_on_launch, "Mirror on launch")
                    .changed();
                if ui
                    .checkbox(&mut self.with_windows, "Start with Windows")
                    .on_hover_text("Starts minimized when you sign in")
                    .changed()
                    && let Err(error) = startup::set(self.with_windows)
                {
                    self.with_windows = startup::enabled();
                    problem = Some(format!("Start with Windows did not change: {error}"));
                }
                ui.separator();
                if let Some(busy) = status.busy {
                    ui.spinner();
                    ui.label(busy);
                } else if status.running {
                    ui.label(format!("{} frames per second", status.fps));
                }
                if let Some(error) = &capture_error {
                    ui.colored_label(Color32::LIGHT_RED, format!("Capture: {error}"));
                }
            });
        });

        egui::Panel::left("devices")
            .default_size(300.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Lights");
                    if ui
                        .add_enabled(status.busy.is_none(), egui::Button::new("Scan"))
                        .clicked()
                    {
                        commands.push(Command::Scan);
                    }
                });
                for warning in &status.warnings {
                    ui.colored_label(Color32::YELLOW, warning);
                }
                for problem in &status.problems {
                    ui.colored_label(Color32::LIGHT_RED, problem);
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let (sent, edited) = devices::show(
                        ui,
                        &status,
                        &mut self.settings,
                        &mut self.brush,
                        &mut self.controls,
                    );
                    commands.extend(sent);
                    changed |= edited;
                });
            });

        egui::Panel::right("segments")
            .default_size(280.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let (sent, edited) =
                        palette::show(ui, &status, &mut self.settings, &mut self.brush);
                    commands.extend(sent);
                    changed |= edited;
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            changed |= grid::show(
                ui,
                &mut self.settings,
                &status.targets,
                self.preview.as_ref(),
                &self.brush,
            );
        });

        drop(status);
        if let Some(problem) = problem {
            engine::lock(&self.engine.status).problems.push(problem);
        }
        for command in commands {
            self.engine.send(command);
        }
        if changed {
            self.changed();
        }
        if self.unsaved.is_some_and(|at| at.elapsed() >= SAVE_DELAY) {
            self.save();
        }
        ctx.request_repaint_after(PREVIEW);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.unsaved.is_some() {
            self.save();
        }
        // The streams disarm before the process ends, so each light goes back
        // to the color it held.
        let done = self.engine.quit();
        let _ = self
            .runtime
            .block_on(async { tokio::time::timeout(Duration::from_secs(3), done).await });
    }
}
