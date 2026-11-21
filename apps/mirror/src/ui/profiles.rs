//! The profile section: pick, add, rename and delete profiles, and name the
//! program that switches to one.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui::{self, RichText, Ui};

use crate::process;
use crate::profiles::{Library, Profile};
use crate::settings::Settings;

/// What the person did. The caller applies a switch: it reloads its settings
/// from the library and hands them on.
pub enum Action {
    None,
    /// The active profile changed.
    Switched,
    /// The profile list or a program name changed.
    Edited,
}

/// The library the window and the watcher share, and the count of the
/// watcher's switches that the window has seen.
pub struct Profiles {
    pub library: Arc<Mutex<Library>>,
    switched: Arc<AtomicU64>,
    seen: u64,
    /// The profile the window's settings belong to. The watcher can switch
    /// between two frames, and a store into another profile would overwrite
    /// it.
    active: usize,
    state: State,
}

impl Profiles {
    pub fn new(library: Arc<Mutex<Library>>, switched: Arc<AtomicU64>) -> Self {
        let active = library
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .active;
        Self {
            library,
            active,
            switched,
            seen: 0,
            state: State::default(),
        }
    }

    /// The settings of the profile that the watcher switched to, once per
    /// switch.
    pub fn switched(&mut self) -> Option<Settings> {
        let count = self.switched.load(Ordering::SeqCst);
        if count == self.seen {
            return None;
        }
        self.seen = count;
        let library = self.library.lock().unwrap_or_else(PoisonError::into_inner);
        self.active = library.active;
        Some(library.current())
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, Library> {
        self.library.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keep `settings` as their profile's, unless the watcher switched away
    /// from it since the window loaded them.
    pub fn store(&self, library: &mut Library, settings: &Settings) {
        if library.active == self.active {
            library.store(settings);
        }
    }

    pub fn panel(&mut self, ui: &mut Ui, settings: &Settings) -> Action {
        let action = {
            let mut library = self.library.lock().unwrap_or_else(PoisonError::into_inner);
            self.store(&mut library, settings);
            drop(library);
            show(ui, &self.library, &mut self.state)
        };
        if matches!(action, Action::Switched) {
            self.active = self
                .library
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .active;
        }
        action
    }
}

#[derive(Default)]
struct State {
    /// The running programs, read when the picker opens.
    programs: Vec<String>,
}

fn show(ui: &mut Ui, library: &Mutex<Library>, state: &mut State) -> Action {
    let mut library = library.lock().unwrap_or_else(PoisonError::into_inner);
    let mut action = Action::None;

    ui.horizontal(|ui| {
        ui.heading("Profile");
        let mut picked = library.active;
        egui::ComboBox::from_id_salt("profile")
            .selected_text(library.profiles[library.active].name.clone())
            .show_ui(ui, |ui| {
                for (index, profile) in library.profiles.iter().enumerate() {
                    ui.selectable_value(&mut picked, index, &profile.name);
                }
            });
        if picked != library.active {
            library.active = picked;
            library.chosen = picked;
            action = Action::Switched;
        }
    });

    let active = library.active;
    ui.horizontal(|ui| {
        if ui
            .button("New")
            .on_hover_text("A copy of this profile")
            .clicked()
        {
            let copy = Profile {
                name: format!("Profile {}", library.profiles.len() + 1),
                process: None,
                settings: library.profiles[active].settings.clone(),
            };
            library.profiles.push(copy);
            library.active = library.profiles.len() - 1;
            library.chosen = library.active;
            action = Action::Switched;
        }
        let deletable = library.profiles.len() > 1;
        if ui
            .add_enabled(deletable, egui::Button::new("Delete"))
            .clicked()
        {
            library.profiles.remove(active);
            library.active = 0;
            library.chosen = library.chosen.min(library.profiles.len() - 1);
            if library.chosen == active {
                library.chosen = 0;
            }
            action = Action::Switched;
        }
    });
    if matches!(action, Action::Switched) {
        return action;
    }

    let profile = &mut library.profiles[active];
    if ui
        .add(egui::TextEdit::singleline(&mut profile.name).hint_text("Name"))
        .changed()
    {
        action = Action::Edited;
    }

    let mut program = profile.process.clone().unwrap_or_default();
    ui.label(RichText::new("Switch to this profile while this program runs:").weak());
    let mut picked = false;
    ui.horizontal(|ui| {
        let edit = ui.add(
            egui::TextEdit::singleline(&mut program)
                .hint_text("game.exe")
                .desired_width(140.0),
        );
        let picker = egui::ComboBox::from_id_salt("programs")
            .selected_text("Running…")
            .width(90.0)
            .show_ui(ui, |ui| {
                for name in &state.programs {
                    if ui.selectable_label(false, name).clicked() {
                        program.clone_from(name);
                        picked = true;
                    }
                }
            });
        // The list is read when the picker opens, never on a timer.
        if picker.response.clicked() {
            let mut names: Vec<String> = process::running().into_iter().collect();
            names.sort();
            state.programs = names;
        }
        if edit.changed() || picked {
            action = Action::Edited;
        }
    });
    let program = program.trim().to_owned();
    profile.process = (!program.is_empty()).then_some(program);

    if library.active != library.chosen {
        let by = library.profiles[library.active]
            .process
            .clone()
            .unwrap_or_default();
        ui.label(RichText::new(format!("Switched here because {by} runs.")).weak());
    }
    action
}
