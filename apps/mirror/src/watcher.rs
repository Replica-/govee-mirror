//! Switch profiles: by the programs that run, and on request.
//!
//! The thread reads the process list every [`POLL`], and only while a profile
//! names a program. A switch goes straight to the engine and the capture, so
//! it applies while the window is hidden, and the window reloads its copy of
//! the settings on its next frame.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use eframe::egui::Context;

use crate::process;
use crate::profiles::Library;
use crate::settings::Settings;

/// How often the process list is read. A program takes seconds to start, so
/// a slow poll costs nothing that a person notices.
const POLL: Duration = Duration::from_secs(5);

/// Hands a profile switch to the engine, the capture and the window, from
/// any thread.
#[derive(Clone)]
pub struct Switcher {
    library: Arc<Mutex<Library>>,
    apply: Arc<dyn Fn(Settings) + Send + Sync>,
    /// Counts the switches, so that the window sees one.
    switched: Arc<AtomicU64>,
    ctx: Context,
}

impl Switcher {
    /// `apply` hands a profile's settings to the engine and the capture.
    pub fn new(
        library: Arc<Mutex<Library>>,
        apply: impl Fn(Settings) + Send + Sync + 'static,
        switched: Arc<AtomicU64>,
        ctx: Context,
    ) -> Self {
        Self {
            library,
            apply: Arc::new(apply),
            switched,
            ctx,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Library> {
        self.library.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Make the profile named `name` the one in use and the one picked, as a
    /// pick in the window does. A name that no profile has does nothing.
    pub fn choose(&self, name: &str) {
        let mut library = self.lock();
        let Some(index) = library.profiles.iter().position(|p| p.name == name) else {
            return;
        };
        library.chosen = index;
        if library.active == index {
            let _ = library.save();
            return;
        }
        library.active = index;
        self.announce(library);
    }

    fn announce(&self, library: MutexGuard<'_, Library>) {
        let settings = library.current();
        let _ = library.save();
        drop(library);
        (self.apply)(settings);
        self.switched.fetch_add(1, Ordering::SeqCst);
        self.ctx.request_repaint();
    }
}

/// Start the thread that follows the programs. It runs for the life of the
/// process.
pub fn spawn(switcher: Switcher) {
    let _ = thread::Builder::new()
        .name("profiles".to_owned())
        .spawn(move || {
            loop {
                thread::sleep(POLL);
                if !switcher.lock().watches() {
                    continue;
                }
                let running = process::running();
                let mut library = switcher.lock();
                let wanted = library.wanted(&running);
                if wanted == library.active {
                    continue;
                }
                library.active = wanted;
                switcher.announce(library);
            }
        });
}
