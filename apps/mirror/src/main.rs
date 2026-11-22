//! Mirror the screen onto Govee lights over the LAN: a grid over the screen,
//! one cell to one segment.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod engine;
mod mapping;
mod pattern;
mod process;
mod profiles;
mod razer;
mod settings;
mod startup;
mod tray;
mod ui;
mod usage;
mod watcher;

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui;

use crate::capture::Request;
use crate::engine::{Command, Engine};
use crate::profiles::Library;
use crate::settings::Settings;

fn main() -> eframe::Result {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("the async runtime did not start: {error}");
            std::process::exit(1);
        }
    };
    let minimized = std::env::args().any(|arg| arg == startup::MINIMIZED);
    // Best effort: a failure leaves the entry as it was, and the checkbox
    // shows it off.
    let _ = startup::migrate();
    log_panics();
    let (library, load_problem) = Library::load();
    let settings = library.current();
    let library = Arc::new(Mutex::new(library));
    let requests = Arc::new(Mutex::new(Request {
        display: settings.display,
        fps: settings.fps,
    }));
    let capture = capture::spawn(Arc::clone(&requests));
    capture::lock(&capture).demand.window = !minimized;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Govee Razer Mirror Windows 11 PC")
            .with_inner_size([1320.0, 860.0])
            .with_min_inner_size([900.0, 600.0])
            .with_icon(Arc::new(egui::IconData {
                rgba: pattern::icon(64),
                width: 64,
                height: 64,
            })),
        ..Default::default()
    };
    eframe::run_native(
        "Govee Razer Mirror Windows 11 PC",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let engine = Engine::spawn(
                &runtime,
                Arc::clone(&capture),
                settings.clone(),
                move || ctx.request_repaint(),
            );
            if let Some(problem) = load_problem {
                engine::lock(&engine.status).warnings.push(problem);
            }
            let quitting = Arc::new(AtomicBool::new(false));
            let tray = tray::spawn(&cc.egui_ctx, engine.remote(), Arc::clone(&quitting));
            let shell = ui::Shell::new(minimized, tray, quitting);
            let switched = Arc::new(AtomicU64::new(0));
            let send = engine.remote();
            let to_capture = Arc::clone(&requests);
            watcher::spawn(
                Arc::clone(&library),
                move |settings: Settings| {
                    *to_capture.lock().unwrap_or_else(PoisonError::into_inner) = Request {
                        display: settings.display,
                        fps: settings.fps,
                    };
                    send(Command::Settings(Box::new(settings)));
                },
                Arc::clone(&switched),
                cc.egui_ctx.clone(),
            );
            let profiles = ui::Profiles::new(library, switched);
            Ok(Box::new(ui::App::new(
                runtime, engine, capture, requests, settings, shell, profiles,
            )))
        }),
    )
}

/// Write a panic to `mirror-crash.log` beside the settings. The release build
/// has no console, so a panic would otherwise leave no trace.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let path = Settings::path().with_file_name("mirror-crash.log");
        let _ = std::fs::write(
            &path,
            format!("{info}\n{}", std::backtrace::Backtrace::force_capture()),
        );
        default(info);
    }));
}
