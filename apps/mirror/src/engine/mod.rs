//! The mirror engine: one task owns the SDK and the writers, and takes
//! commands from the window.
//!
//! A command runs to its end before the next one, so a scan or an arm pauses
//! the frames for as long as it takes. The window reads [`Status`] and never
//! waits on the engine.

mod explain;
mod scan;
mod status;
mod targets;
mod writer;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use govee_toolkit::{DeviceId, Govee, Mode};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::time::{MissedTickBehavior, interval};

use self::explain::explain;
pub use self::status::{Command, Control, Shared, Status, lock};
pub use self::targets::Target;
use self::writer::Writer;
use crate::capture;
use crate::mapping::{Sources, plan, render};
use crate::pattern;
use crate::settings::Settings;

pub struct Engine {
    commands: UnboundedSender<Command>,
    pub status: Shared,
}

impl Engine {
    /// Start the SDK and the engine task on `runtime`, then scan once.
    pub fn spawn(
        runtime: &tokio::runtime::Runtime,
        capture: capture::Shared,
        settings: Settings,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (commands, receiver) = unbounded_channel();
        let status = Shared::default();
        runtime.spawn(run(
            receiver,
            capture,
            settings,
            Arc::clone(&status),
            Box::new(repaint),
        ));
        let _ = commands.send(Command::Scan);
        Self { commands, status }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// A sender that outlives a borrow of the engine, for the tray.
    pub fn remote(&self) -> impl Fn(Command) + Send + Sync + 'static {
        let commands = self.commands.clone();
        move |command| {
            let _ = commands.send(command);
        }
    }

    /// Stop the mirror and the SDK. The receiver resolves once both are done.
    pub fn quit(&self) -> oneshot::Receiver<()> {
        let (done, receiver) = oneshot::channel();
        let _ = self
            .commands
            .send(Command::Quit(Arc::new(Mutex::new(Some(done)))));
        receiver
    }
}

struct Mirror {
    writers: HashMap<String, Writer>,
    plan: HashMap<String, Sources>,
    state: HashMap<String, Vec<[f32; 3]>>,
    /// The light and segment that blink, and when the blink started.
    highlight: Option<(String, usize, Instant)>,
    rendered: u32,
    reported: Instant,
    /// When the mirror started, the clock of the animation.
    started: Instant,
}

struct Context {
    govee: Govee,
    capture: capture::Shared,
    settings: Settings,
    status: Shared,
    repaint: Box<dyn Fn() + Send + Sync>,
    mirror: Option<Mirror>,
}

async fn run(
    mut commands: UnboundedReceiver<Command>,
    capture: capture::Shared,
    settings: Settings,
    status: Shared,
    repaint: Box<dyn Fn() + Send + Sync>,
) {
    let govee = match Govee::start_or_defaults(None, None).await {
        Ok(govee) => govee,
        Err(error) => {
            lock(&status).problems = vec![format!("The SDK did not start: {}", explain(&error))];
            repaint();
            while commands.recv().await.is_some() {}
            return;
        }
    };
    let mut tick = interval(period(settings.fps));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut cx = Context {
        govee,
        capture,
        settings,
        status,
        repaint,
        mirror: None,
    };
    // The window sends the first scan. The mirror on launch waits for it, so
    // that the writers open on lights the scan found.
    let mut launch = cx.settings.mirror_on_launch;
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { break };
                if let Command::Quit(done) = command {
                    cx.stop().await;
                    let _ = cx.govee.shutdown().await;
                    if let Some(done) = done.lock().unwrap_or_else(PoisonError::into_inner).take() {
                        let _ = done.send(());
                    }
                    return;
                }
                let fps = cx.settings.fps;
                let scanned = matches!(command, Command::Scan);
                cx.handle(command).await;
                if launch && scanned {
                    launch = false;
                    cx.start().await;
                    (cx.repaint)();
                }
                if cx.settings.fps != fps {
                    tick = interval(period(cx.settings.fps));
                    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
                }
            }
            _ = tick.tick(), if cx.mirror.is_some() => cx.frame(),
        }
    }
    cx.stop().await;
    let _ = cx.govee.shutdown().await;
}

fn period(fps: u32) -> Duration {
    Duration::from_secs(1) / fps.clamp(1, 120)
}

impl Context {
    async fn handle(&mut self, command: Command) {
        match command {
            Command::Scan => self.scan().await,
            Command::Start => self.start().await,
            Command::Stop => self.stop().await,
            Command::Control(id, control) => self.control(&id, control).await,
            Command::Highlight(id, segment) => self.highlight(id, segment).await,
            Command::Quit(_) => {}
            Command::Settings(settings) => {
                let razer = self.settings.razer_leds != settings.razer_leds;
                let rebuild = self.settings.grid != settings.grid
                    || self.settings.assignments != settings.assignments
                    || self.settings.devices != settings.devices
                    || razer;
                self.settings = *settings;
                if razer {
                    let mut status = lock(&self.status);
                    status.targets.retain(|t| t.razer_port.is_none());
                    status
                        .targets
                        .extend(targets::razer(&self.settings.razer_leds));
                }
                if rebuild && self.mirror.is_some() {
                    self.stop().await;
                    self.start().await;
                }
            }
        }
        (self.repaint)();
    }

    fn busy(&self, what: Option<&'static str>) {
        lock(&self.status).busy = what;
        (self.repaint)();
    }

    fn target(&self, id: &str) -> Option<Target> {
        lock(&self.status)
            .targets
            .iter()
            .find(|t| t.id == id)
            .cloned()
    }

    /// Open a writer on every light that the grid uses. A light that fails
    /// to open is reported, and the others go ahead.
    async fn start(&mut self) {
        if self.mirror.is_some() {
            return;
        }
        self.busy(Some("Starting"));
        let mut used: Vec<String> = self
            .settings
            .entries()
            .map(|(_, a)| a.target.clone())
            .collect();
        used.sort();
        used.dedup();
        let mut writers = HashMap::new();
        let mut failed = HashMap::new();
        for id in used {
            let Some(target) = self.target(&id) else {
                failed.insert(id, "not in the last scan".to_owned());
                continue;
            };
            let opened = match target.razer_port {
                Some(port) => writer::open_razer(
                    port,
                    self.settings.zones(&target),
                    target.max_zones,
                    self.settings.devices.get(&id).is_some_and(|d| d.gradient),
                ),
                None => writer::open(
                    &self.govee,
                    &target,
                    self.settings.frame(&target),
                    self.settings.devices.get(&id).is_some_and(|d| d.gradient),
                )
                .await
                .map_err(|error| explain(&error)),
            };
            match opened {
                Ok(writer) => drop(writers.insert(id, writer)),
                Err(error) => drop(failed.insert(id, error)),
            }
        }
        let zones = writers
            .iter()
            .map(|(id, w)| (id.clone(), w.zones()))
            .collect();
        self.mirror = Some(Mirror {
            plan: plan(self.settings.entries(), &zones, self.settings.grid),
            writers,
            state: HashMap::new(),
            highlight: None,
            rendered: 0,
            reported: Instant::now(),
            started: Instant::now(),
        });
        let mut status = lock(&self.status);
        status.busy = None;
        status.running = true;
        status.stats.clear();
        status.messages = failed;
    }

    async fn stop(&mut self) {
        let Some(mirror) = self.mirror.take() else {
            return;
        };
        self.busy(Some("Stopping"));
        for (_, writer) in mirror.writers {
            writer.close().await;
        }
        let mut status = lock(&self.status);
        status.busy = None;
        status.running = false;
        status.fps = 0;
    }

    async fn control(&mut self, id: &str, control: Control) {
        if id.starts_with("razer-argb:") {
            return self.message(
                id,
                "A Razer port lights only while the mirror runs.".to_owned(),
            );
        }
        let handle = match self.govee.device(&DeviceId::new(id), Some(Mode::Lan)) {
            Ok(handle) => handle,
            Err(error) => return self.message(id, explain(&error)),
        };
        let result = match control {
            Control::Power(on) => handle.power(on).await,
            Control::Brightness(level) => handle.brightness(level).await,
            Control::Color(rgb) => handle.color(rgb).await,
        };
        match result {
            Ok(_) => drop(lock(&self.status).messages.remove(id)),
            Err(error) => self.message(id, explain(&error)),
        }
    }

    /// While the mirror runs, the highlight goes out in the frames: a command
    /// would end the segment channel.
    async fn highlight(&mut self, id: String, segment: usize) {
        if let Some(mirror) = self.mirror.as_mut().filter(|m| m.writers.contains_key(&id)) {
            mirror.highlight = Some((id, segment, Instant::now()));
            return;
        }
        let Some(target) = self.target(&id) else {
            return;
        };
        let (frame, zones) = (self.settings.frame(&target), self.settings.zones(&target));
        let result = match target.razer_port {
            Some(port) => writer::highlight_razer(port, zones, target.max_zones, segment).await,
            None => writer::highlight(&self.govee, &target, frame, zones, segment)
                .await
                .map_err(|error| explain(&error)),
        };
        if let Err(error) = result {
            self.message(&id, error);
        }
    }

    fn message(&self, id: &str, text: String) {
        lock(&self.status).messages.insert(id.to_owned(), text);
    }

    fn frame(&mut self) {
        let Some(mirror) = self.mirror.as_mut() else {
            return;
        };
        let grid = self.settings.grid;
        let seconds = if self.settings.moving {
            mirror.started.elapsed().as_secs_f32()
        } else {
            0.0
        };
        let (source, speed) = (self.settings.source, self.settings.animation_speed);
        let cells = pattern::draw(source, grid.cols, grid.rows, seconds, speed).or_else(|| {
            let frame = capture::lock(&self.capture).frame.clone();
            frame.map(|frame| frame.cells(grid.cols, grid.rows))
        });
        if let Some(cells) = cells {
            let mut frames = render(
                &mirror.plan,
                &cells,
                &mut mirror.state,
                self.settings.smoothing,
                self.settings.saturation,
            );
            if let Some((id, segment, started)) = &mirror.highlight {
                match writer::blink(started.elapsed()) {
                    Some(on) => {
                        if let Some(colors) = frames.get_mut(id) {
                            colors.fill([0; 3]);
                            if let Some(color) = colors.get_mut(*segment).filter(|_| on) {
                                *color = [255; 3];
                            }
                        }
                    }
                    None => mirror.highlight = None,
                }
            }
            for (id, colors) in &frames {
                if let Some(writer) = mirror.writers.get(id) {
                    writer.write(&self.govee, id, colors);
                }
            }
            mirror.rendered += 1;
        }
        if mirror.reported.elapsed() >= Duration::from_secs(1) {
            let mut status = lock(&self.status);
            status.fps = mirror.rendered;
            status.stats = mirror
                .writers
                .iter()
                .map(|(id, w)| (id.clone(), w.stats()))
                .collect();
            mirror.rendered = 0;
            mirror.reported = Instant::now();
            drop(status);
            (self.repaint)();
        }
    }
}
