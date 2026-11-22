//! The light addresses: lights that every scan asks directly, for a network
//! that drops the multicast scan. The field that adds one sits beside the
//! Scan button; the list sits in Govee General.

use std::net::IpAddr;

use eframe::egui::{self, Color32, RichText, Ui};

use crate::settings::Settings;

/// The saved addresses, each with its Remove button. Answers whether the
/// settings changed.
pub fn list(ui: &mut Ui, settings: &mut Settings) -> bool {
    ui.add_space(4.0);
    ui.label(RichText::new("Light addresses").strong());
    if settings.lan_hosts.is_empty() {
        ui.label(
            RichText::new(
                "Type a light's IP address beside Scan, then Add, when the scan does not find it.",
            )
            .weak(),
        );
        return false;
    }
    ui.label(RichText::new("Each scan also asks these addresses directly.").weak());
    let mut remove = None;
    for (index, host) in settings.lan_hosts.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(host.to_string()).monospace());
            if ui.small_button("Remove").clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        settings.lan_hosts.remove(index);
    }
    remove.is_some()
}

/// The IP box and its Add button, on the row of the Scan button. `draft`
/// holds the address being typed. Answers whether the settings changed.
pub fn field(ui: &mut Ui, settings: &mut Settings, draft: &mut String) -> bool {
    let parsed = draft.trim().parse::<IpAddr>();
    let typo = !draft.trim().is_empty() && parsed.is_err();
    let mut edit = egui::TextEdit::singleline(draft)
        .hint_text("Light IP")
        .desired_width(110.0);
    if typo {
        edit = edit.text_color(Color32::LIGHT_RED);
    }
    let field = ui
        .add(edit)
        .on_hover_text("The IP address of a light that the scan does not find");
    let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let add = ui
        .add_enabled(parsed.is_ok(), egui::Button::new("Add"))
        .on_disabled_hover_text(if typo {
            "Not an IP address"
        } else {
            "Type a light's IP address first"
        });
    let Ok(ip) = parsed else {
        return false;
    };
    if !(add.clicked() || entered) {
        return false;
    }
    draft.clear();
    if settings.lan_hosts.contains(&ip) {
        return false;
    }
    settings.lan_hosts.push(ip);
    true
}
