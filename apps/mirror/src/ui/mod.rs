//! The window.

mod brush;
mod devices;
mod grid;
mod palette;
mod profiles;
mod shell;

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions};

use crate::capture::{self, Request, Requests};
use crate::engine::{self, Command, Engine};
use crate::mapping::square_rows;
use crate::pattern::{self, Source};
use crate::settings::Settings;
use crate::startup;

pub use self::brush::{Brush, Tool, segment_color, text_on};
pub use self::profiles::Profiles;
pub use self::shell::Shell;

/// How often the window redraws the preview while nothing else asks it to.
const PREVIEW: Duration = Duration::from_millis(100);
/// How often the window redraws while the animation runs.
const ANIMATION: Duration = Duration::from_millis(33);
/// How long the settings wait after a change before they go to disk.
const SAVE_DELAY: Duration = Duration::from_millis(500);

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
    shell: Shell,
    profiles: Profiles,
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
        shell: Shell,
        profiles: Profiles,
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
            shell,
            profiles,
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
        let mut library = self.profiles.lock();
        self.profiles.store(&mut library, &self.settings);
        drop(library);
        self.engine
            .send(Command::Settings(Box::new(self.settings.clone())));
        self.unsaved = Some(Instant::now());
    }

    fn save(&mut self) {
        self.unsaved = None;
        let saved = {
            let mut library = self.profiles.lock();
            self.profiles.store(&mut library, &self.settings);
            library.save()
        };
        if let Err(error) = saved {
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
        self.shell.frame(&ctx, &mut self.with_windows);
        if let Some(settings) = self.profiles.switched() {
            // The watcher has handed these to the engine and the capture.
            self.settings = settings;
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
                egui::ComboBox::from_id_salt("source")
                    .selected_text(self.settings.source.label())
                    .show_ui(ui, |ui| {
                        for source in Source::ALL {
                            changed |= ui
                                .selectable_value(&mut self.settings.source, source, source.label())
                                .changed();
                        }
                    })
                    .response
                    .on_hover_text("What the lights mirror: the screen, or a pattern");
                if self.settings.source != Source::Screen {
                    changed |= ui.checkbox(&mut self.settings.moving, "Moving").changed();
                }
                if self.settings.source != Source::Screen && self.settings.moving {
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut self.settings.animation_speed, 0.05..=2.0)
                                .logarithmic(true)
                                .text("speed"),
                        )
                        .changed();
                }
                ui.separator();
                changed |= ui
                    .checkbox(&mut self.settings.mirror_on_launch, "Mirror on launch")
                    .changed();
                if ui
                    .checkbox(&mut self.with_windows, "Start with Windows")
                    .on_hover_text("Starts in the tray when you sign in")
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
                match self.profiles.panel(ui, &self.settings) {
                    profiles::Action::Switched => {
                        self.settings = self.profiles.lock().current();
                        changed = true;
                    }
                    profiles::Action::Edited => self.unsaved = Some(Instant::now()),
                    profiles::Action::None => {}
                }
                ui.separator();
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

        let seconds = if self.settings.moving {
            ctx.input(|i| i.time) as f32
        } else {
            0.0
        };
        let grid = self.settings.grid;
        let animation = pattern::draw(
            self.settings.source,
            grid.cols,
            grid.rows,
            seconds,
            self.settings.animation_speed,
        );
        let moving = animation.is_some() && self.settings.moving;
        egui::CentralPanel::default().show(ui, |ui| {
            changed |= grid::show(
                ui,
                &mut self.settings,
                &status.targets,
                self.preview.as_ref(),
                animation.as_deref(),
                &mut self.brush,
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
        // A moving pattern changes on every frame; the screen preview needs
        // fewer.
        ctx.request_repaint_after(if moving { ANIMATION } else { PREVIEW });
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
