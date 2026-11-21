//! Mirror the screen onto Govee lights over the LAN: a grid over the screen,
//! one cell to one segment.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod engine;
mod mapping;
mod settings;
mod startup;
mod ui;

use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::capture::Request;
use crate::engine::Engine;
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
    log_panics();
    let (settings, load_problem) = Settings::load();
    let requests = Arc::new(Mutex::new(Request {
        display: settings.display,
        fps: settings.fps,
    }));
    let capture = capture::spawn(Arc::clone(&requests));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Govee Mirror")
            .with_inner_size([1320.0, 860.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Govee Mirror",
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
            Ok(Box::new(ui::App::new(
                runtime, engine, capture, requests, settings, minimized,
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
