//! What the window does as a window: start hidden, hide to the tray on close,
//! and keep the tray's "Start with Windows" in step with the checkbox.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use eframe::egui::{Context, ViewportCommand};

use crate::startup;
use crate::tray::Tray;

/// How often the `Run` entry is read again. The tray can change it while the
/// window draws nothing.
const SYNC: Duration = Duration::from_secs(1);

pub struct Shell {
    /// Start out of sight: in the tray where there is one, else minimized.
    hide: bool,
    tray: Option<Tray>,
    /// Set by the tray's Exit. A close that it did not set hides the window.
    quitting: Arc<AtomicBool>,
    synced: Instant,
}

impl Shell {
    pub fn new(minimized: bool, tray: Option<Tray>, quitting: Arc<AtomicBool>) -> Self {
        Self {
            hide: minimized,
            tray,
            quitting,
            synced: Instant::now(),
        }
    }

    /// Run once per frame, before the window draws. `with_windows` is the
    /// checkbox's view of the `Run` entry.
    pub fn frame(&mut self, ctx: &Context, with_windows: &mut bool) {
        if std::mem::take(&mut self.hide) {
            ctx.send_viewport_cmd(match self.tray {
                Some(_) => ViewportCommand::Visible(false),
                None => ViewportCommand::Minimized(true),
            });
        }

        // Without a tray, a hidden window could not be brought back.
        let closing = ctx.input(|i| i.viewport().close_requested());
        if closing && self.tray.is_some() && !self.quitting.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }

        if self.synced.elapsed() >= SYNC {
            self.synced = Instant::now();
            *with_windows = startup::enabled();
        }
        if let Some(tray) = &self.tray {
            tray.sync(*with_windows);
        }
    }
}
