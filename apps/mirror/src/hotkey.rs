//! The global hotkeys that switch the monitor's input. The keyboard does not
//! go through the monitor's hub, so it stays on this PC after a switch.

use std::sync::mpsc::Sender;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::monitor::INPUTS;

/// The key of each input in [`INPUTS`], with Ctrl+Alt.
const KEYS: [(Code, &str); INPUTS.len()] = [(Code::F12, "F12"), (Code::F11, "F11")];

/// Keeps the hotkeys registered while it lives.
pub struct Hotkeys {
    _manager: GlobalHotKeyManager,
}

/// The hotkey of each input, as the card shows it.
pub fn labels() -> impl Iterator<Item = (&'static str, String)> {
    INPUTS
        .iter()
        .zip(KEYS)
        .map(|((_, name), (_, key))| (*name, format!("Ctrl+Alt+{key}")))
}

/// Register the hotkeys on the thread that runs the window's message loop.
/// `None` where Windows refuses them, such as when another program holds
/// the keys.
pub fn spawn(send: Sender<u32>) -> Option<Hotkeys> {
    let manager = GlobalHotKeyManager::new().ok()?;
    let mut ids = Vec::new();
    for ((value, _), (code, _)) in INPUTS.iter().zip(KEYS) {
        let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), code);
        manager.register(hotkey).ok()?;
        ids.push((hotkey.id(), *value));
    }
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state != HotKeyState::Pressed {
            return;
        }
        if let Some((_, value)) = ids.iter().find(|(id, _)| *id == event.id) {
            let _ = send.send(*value);
        }
    }));
    Some(Hotkeys { _manager: manager })
}
