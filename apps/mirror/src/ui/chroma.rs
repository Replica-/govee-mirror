//! The card of a Razer device with app-drawn effects, and the window that
//! puts its keys or LEDs in groups and gives each group an effect.

use eframe::egui::{
    self, Align2, Color32, FontId, PointerButton, Rect, RichText, Sense, Stroke, Ui, vec2,
};

use crate::razer::chroma::devices::{Key, Macros, Spec};
use crate::razer::chroma::effects::{self, Effect, Group};
use crate::razer::chroma::{self, Handle, Mode};
use crate::settings::Settings;

/// The smallest key drawn, in points.
const MIN_KEY: f32 = 22.0;

pub struct Panel {
    spec: &'static Spec,
    handle: Handle,
    /// Whether the effects window is open.
    open: bool,
    /// The group that a click on a key adds the key to.
    selected: usize,
}

impl Panel {
    pub fn new(spec: &'static Spec, handle: Handle) -> Self {
        Self {
            spec,
            handle,
            open: false,
            selected: 0,
        }
    }

    pub fn shown(&self, settings: &mut Settings) -> bool {
        self.handle.lock().present || chroma::of_mut(settings, self.spec.device).lit()
    }

    /// Whether the window shows an effect that moves, and needs a redraw
    /// for each frame.
    pub fn animated(&self, settings: &mut Settings) -> bool {
        let effects = chroma::of_mut(settings, self.spec.device);
        self.open
            && effects.mode() == Mode::Effects
            && effects.groups.iter().any(|g| g.effect.moves())
    }

    /// Give the device its own effect back, and end the thread.
    pub fn stop(&mut self) {
        self.handle.stop();
    }

    /// The card, after the light cards. `profiles` names every profile, for
    /// the macro keys. Answers whether the settings changed.
    pub fn card(&mut self, ui: &mut Ui, settings: &mut Settings, profiles: &[String]) -> bool {
        let spec = self.spec;
        let mut changed = false;
        let state = self.handle.lock();
        let (present, driving, error) = (state.present, state.driving, state.error.clone());
        let listening = state.listening;
        drop(state);
        let macro_keys = &mut settings.macro_keys;
        let effects = match spec.device {
            chroma::devices::Device::Keyboard => &mut settings.keyboard,
            chroma::devices::Device::Mousepad => &mut settings.mousepad,
        };
        egui::Frame::group(ui.style())
            .stroke(Stroke::new(1.0, Color32::from_gray(70)))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let id = ui.make_persistent_id(spec.name);
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    false,
                )
                .show_header(ui, |ui| {
                    let dot = if error.is_some() {
                        Color32::from_rgb(220, 80, 80)
                    } else if driving {
                        Color32::from_rgb(80, 200, 120)
                    } else {
                        Color32::from_gray(110)
                    };
                    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, dot);
                    ui.label(RichText::new(spec.name).strong());
                })
                .body(|ui| {
                    ui.label(RichText::new(spec.kind).weak());
                    ui.horizontal(|ui| {
                        changed |= mode(ui, (spec.name, "card"), effects);
                        if ui.button("Edit effects…").clicked() {
                            self.open = true;
                        }
                    });
                    let line = match (present, effects.mode(), driving) {
                        (false, _, _) => "The device is not connected.".to_owned(),
                        (true, Mode::Own, _) => "The device shows its own effect.".to_owned(),
                        (true, _, false) => "Waiting for the device.".to_owned(),
                        (true, Mode::Off, true) => format!("The {} are dark.", spec.parts),
                        (true, Mode::Effects, true) => format!(
                            "The app lights the {0}. Those in no group are dark.",
                            spec.parts
                        ),
                    };
                    ui.label(RichText::new(line).weak());
                    ui.label(RichText::new("The lights belong to the profile in use.").weak());
                    if let Some(macros) = &spec.macros {
                        ui.add_space(4.0);
                        changed |= macro_keys_ui(ui, macros, macro_keys, profiles, listening);
                    }
                    if let Some(error) = &error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                });
            });
        changed
    }

    /// The effects window, while it is open. Answers whether the settings
    /// changed.
    pub fn window(&mut self, ctx: &egui::Context, settings: &mut Settings) -> bool {
        let mut changed = false;
        let mut open = self.open;
        let effects = chroma::of_mut(settings, self.spec.device);
        egui::Window::new(format!("{} effects", self.spec.name))
            .open(&mut open)
            .default_width(760.0)
            .show(ctx, |ui| changed = self.editor(ui, effects));
        self.open = open;
        changed
    }

    fn editor(&mut self, ui: &mut Ui, effects: &mut chroma::Settings) -> bool {
        let parts = self.spec.parts;
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= mode(ui, (self.spec.name, "window"), effects);
            ui.label("Brightness");
            changed |= ui
                .add(egui::Slider::new(&mut effects.brightness, 0..=100).suffix("%"))
                .changed();
        });
        ui.label(
            RichText::new(format!(
                "Pick a group, then click or drag over the {parts} to add them. The right button \
                 takes them out. Each one is in one group at a time. Those in no group are dark."
            ))
            .weak(),
        );
        ui.add_space(4.0);
        changed |= self.keys(ui, effects);
        ui.add_space(6.0);
        changed |= self.groups(ui, effects);
        changed
    }

    /// The drawing of the device, with the colors its keys or LEDs show now.
    fn keys(&mut self, ui: &mut Ui, effects: &mut chroma::Settings) -> bool {
        let spec = self.spec;
        let unit = (ui.available_width() / spec.width).max(MIN_KEY);
        let (area, response) = ui.allocate_exact_size(
            vec2(unit * spec.width, unit * spec.height),
            Sense::click_and_drag(),
        );
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 4.0, Color32::from_gray(18));
        let brightness = f32::from(effects.brightness.min(100)) / 100.0;
        let frame = effects::draw(spec, &effects.groups, effects::now(), brightness);
        let rect = |key: &Key| {
            Rect::from_min_size(
                area.min + vec2(key.x * unit, key.y * unit),
                vec2(key.w * unit, key.h * unit),
            )
            .shrink(2.0)
        };
        let selected = effects.groups.get(self.selected);
        for key in spec.keys {
            let [r, g, b] = frame[usize::from(key.row)][usize::from(key.col)];
            let fill = Color32::from_rgb(r, g, b);
            let mine = selected.is_some_and(|g| g.keys.contains(&(key.row, key.col)));
            let stroke = if mine {
                Stroke::new(2.0, Color32::WHITE)
            } else {
                Stroke::new(1.0, Color32::from_gray(60))
            };
            let at = rect(key);
            painter.rect_filled(at, 3.0, Color32::from_gray(25).lerp_to_gamma(fill, 0.9));
            painter.rect_stroke(at, 3.0, stroke, egui::StrokeKind::Inside);
            painter.text(
                at.center(),
                Align2::CENTER_CENTER,
                key.label,
                FontId::proportional(unit * 0.32),
                super::text_on(fill),
            );
        }
        if !response.is_pointer_button_down_on() || self.selected >= effects.groups.len() {
            return false;
        }
        let Some(pos) = response.interact_pointer_pos() else {
            return false;
        };
        let Some(key) = spec.keys.iter().find(|k| rect(k).contains(pos)) else {
            return false;
        };
        let position = (key.row, key.col);
        let remove = ui.input(|i| i.pointer.button_down(PointerButton::Secondary));
        let mut changed = false;
        for (index, group) in effects.groups.iter_mut().enumerate() {
            let wanted = index == self.selected && !remove;
            changed |= if wanted {
                group.keys.insert(position)
            } else if index == self.selected || !remove {
                group.keys.remove(&position)
            } else {
                false
            };
        }
        changed
    }

    /// One row per group: its name, effect, color and speed.
    fn groups(&mut self, ui: &mut Ui, effects: &mut chroma::Settings) -> bool {
        let spec = self.spec;
        let mut changed = false;
        let mut delete = None;
        egui::Grid::new((spec.name, "groups"))
            .num_columns(7)
            .spacing(vec2(8.0, 6.0))
            .show(ui, |ui| {
                for (index, group) in effects.groups.iter_mut().enumerate() {
                    if ui
                        .radio(self.selected == index, "")
                        .on_hover_text(format!("Clicks on {} add them to this group", spec.parts))
                        .clicked()
                    {
                        self.selected = index;
                    }
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut group.name).desired_width(110.0))
                        .changed();
                    changed |= effect(ui, (spec.name, index), group);
                    if group.effect.colored() {
                        changed |= egui::color_picker::color_edit_button_srgb(ui, &mut group.color)
                            .changed();
                    } else {
                        ui.label("");
                    }
                    if group.effect.moves() {
                        changed |= ui
                            .add(
                                egui::Slider::new(&mut group.speed, 0.05..=2.0)
                                    .logarithmic(true)
                                    .suffix("/s"),
                            )
                            .on_hover_text("Cycles a second")
                            .changed();
                    } else {
                        ui.label("");
                    }
                    let count = format!("{} {}", group.keys.len(), spec.parts);
                    ui.label(RichText::new(count).weak());
                    if ui.small_button("Delete").clicked() {
                        delete = Some(index);
                    }
                    ui.end_row();
                }
            });
        if let Some(index) = delete {
            effects.groups.remove(index);
            self.selected = self.selected.min(effects.groups.len().saturating_sub(1));
            changed = true;
        }
        ui.horizontal(|ui| {
            if ui.button("Add group").clicked() {
                effects.groups.push(Group {
                    name: format!("Group {}", effects.groups.len() + 1),
                    ..Group::default()
                });
                self.selected = effects.groups.len() - 1;
                changed = true;
            }
            if self.selected < effects.groups.len() {
                if ui.button(format!("All {}", spec.parts)).clicked() {
                    for (index, group) in effects.groups.iter_mut().enumerate() {
                        group.keys.clear();
                        if index == self.selected {
                            group.keys.extend(spec.keys.iter().map(|k| (k.row, k.col)));
                        }
                    }
                    changed = true;
                }
                if ui.button(format!("No {}", spec.parts)).clicked() {
                    effects.groups[self.selected].keys.clear();
                    changed = true;
                }
            }
        });
        changed
    }
}

/// What the lights show. Answers whether it changed.
fn mode(ui: &mut Ui, id: (&str, &str), effects: &mut chroma::Settings) -> bool {
    let mut current = effects.mode();
    egui::ComboBox::from_id_salt(("mode", id))
        .selected_text(current.label())
        .width(100.0)
        .show_ui(ui, |ui| {
            for mode in Mode::ALL {
                ui.selectable_value(&mut current, mode, mode.label());
            }
        });
    let changed = current != effects.mode();
    effects.set_mode(current);
    changed
}

/// The profile that each macro key switches to. Answers whether one changed.
fn macro_keys_ui(
    ui: &mut Ui,
    macros: &Macros,
    actions: &mut Vec<Option<String>>,
    profiles: &[String],
    listening: bool,
) -> bool {
    let mut changed = false;
    actions.resize(macros.labels.len(), None);
    egui::Grid::new("macro-keys").num_columns(2).show(ui, |ui| {
        for (label, action) in macros.labels.iter().zip(actions.iter_mut()) {
            ui.label(*label);
            let text = action
                .as_deref()
                .map_or("Nothing".to_owned(), |name| format!("Switch to {name}"));
            egui::ComboBox::from_id_salt(("macro", *label))
                .selected_text(text)
                .width(170.0)
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(action, None, "Nothing").changed();
                    for name in profiles {
                        changed |= ui
                            .selectable_value(
                                action,
                                Some(name.clone()),
                                format!("Switch to {name}"),
                            )
                            .changed();
                    }
                });
            ui.end_row();
        }
    });
    let line = if listening {
        "The macro keys report to the app."
    } else if actions.iter().any(Option::is_some) {
        "Waiting for the keyboard."
    } else {
        "Give a key a profile, and the app takes the macro keys."
    };
    ui.label(RichText::new(line).weak());
    changed
}

fn effect(ui: &mut Ui, id: (&str, usize), group: &mut Group) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(("effect", id))
        .selected_text(group.effect.label())
        .width(100.0)
        .show_ui(ui, |ui| {
            for effect in Effect::ALL {
                changed |= ui
                    .selectable_value(&mut group.effect, effect, effect.label())
                    .changed();
            }
        });
    changed
}
