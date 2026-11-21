//! Switch profiles by the programs that run.
//!
//! The thread reads the process list every [`POLL`], and only while a profile
//! names a program. A switch goes straight to the engine and the capture, so
//! it applies while the window is hidden, and the window reloads its copy of
//! the settings on its next frame.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use eframe::egui::Context;

use crate::process;
use crate::profiles::Library;
use crate::settings::Settings;

/// How often the process list is read. A program takes seconds to start, so
/// a slow poll costs nothing that a person notices.
const POLL: Duration = Duration::from_secs(5);

/// Start the thread. It runs for the life of the process.
///
/// `apply` hands a profile's settings to the engine and the capture.
/// `switched` counts the switches, so that the window sees one.
pub fn spawn(
    library: Arc<Mutex<Library>>,
    apply: impl Fn(Settings) + Send + 'static,
    switched: Arc<AtomicU64>,
    ctx: Context,
) {
    let _ = thread::Builder::new()
        .name("profiles".to_owned())
        .spawn(move || {
            loop {
                thread::sleep(POLL);
                let watches = library
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .watches();
                if !watches {
                    continue;
                }
                let running = process::running();
                let mut library = library.lock().unwrap_or_else(PoisonError::into_inner);
                let wanted = library.wanted(&running);
                if wanted == library.active {
                    continue;
                }
                library.active = wanted;
                let settings = library.current();
                let _ = library.save();
                drop(library);
                apply(settings);
                switched.fetch_add(1, Ordering::SeqCst);
                ctx.request_repaint();
            }
        });
}
