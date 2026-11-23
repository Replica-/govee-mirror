//! The window.

mod bar;
mod brands;
mod brush;
mod devices;
mod grid;
mod headset;
mod hosts;
mod levels;
mod monitor;
mod palette;
mod profiles;
mod shell;
mod sound;

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};

use crate::capture::{self, Request, Requests};
use crate::engine::{self, Command, Engine};
use crate::mapping::square_rows;
use crate::pattern;
use crate::settings::Settings;
use crate::{razer, startup, usage};

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
    headset: headset::Panel,
    monitor: monitor::Panel,
    _hotkeys: Option<crate::hotkey::Hotkeys>,
    shell: Shell,
    profiles: Profiles,
    usage: usage::Shared,
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
        let usage = usage::spawn(std::sync::Arc::clone(&capture));
        let headset = headset::Panel::new(razer::headset::spawn(std::sync::Arc::clone(
            &profiles.library,
        )));
        let (monitor_state, monitor_send) = crate::monitor::spawn();
        let hotkeys = crate::hotkey::spawn(monitor_send.clone());
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
            headset,
            monitor: monitor::Panel::new(monitor_state, monitor_send, hotkeys.is_some()),
            _hotkeys: hotkeys,
            shell,
            profiles,
            usage,
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
        // eframe runs no frame for a hidden or minimized window, so the last
        // frame before it hides sets this to false.
        let shown = self.shell.frame(&ctx, &mut self.with_windows);
        capture::lock(&self.capture).demand.window = shown;
        if let Some(settings) = self.profiles.switched() {
            // The watcher has handed these to the engine and the capture.
            self.settings = settings;
        }
        let (capture_error, screens) = self.refresh_preview(&ctx);
        let found = std::mem::take(&mut engine::lock(&self.engine.status).found);
        let status_handle = self.engine.status.clone();
        let status = engine::lock(&status_handle);
        let mut commands = Vec::new();
        let mut changed = false;
        for (id, light) in found {
            if self.settings.lights.get(&id) != Some(&light) {
                self.settings.lights.insert(id, light);
                changed = true;
            }
        }
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
            let out = self.bar(ui, &status, &screens, capture_error.as_deref());
            commands.extend(out.commands);
            changed |= out.changed;
            problem = out.problem;
        });

        egui::Panel::left("devices")
            .default_size(320.0)
            .min_size(280.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Lights");
                    if ui
                        .add_enabled(status.busy.is_none(), egui::Button::new("Scan"))
                        .clicked()
                    {
                        commands.push(Command::Scan);
                    }
                    changed |= hosts::field(ui, &mut self.settings, &mut self.controls.host);
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
                        &mut self.headset,
                        &mut self.monitor,
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

        egui::Panel::bottom("sound").show(ui, |ui| {
            changed |= sound::show(
                ui,
                &mut self.settings,
                &self.engine.sound,
                status.running,
                &status.targets,
            );
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
            changed |= self.profile(ui);
            ui.add_space(6.0);
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
        // A moving pattern and the spectrum change on every frame; the screen
        // preview needs fewer.
        let animated = moving || self.settings.sound.enabled;
        ctx.request_repaint_after(if animated { ANIMATION } else { PREVIEW });
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
