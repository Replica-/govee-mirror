//! The icon in the notification area, and its menu.
//!
//! The handlers run outside the window's frame, so they act through the egui
//! context and the engine's command channel. A hidden window draws no frame,
//! so a handler asks for one after each viewport command.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{Context, ViewportCommand};

use crate::engine::Command;
use crate::{pattern, startup};

/// Keeps the icon in the notification area while it lives.
pub struct Tray {
    _icon: tray_icon::TrayIcon,
    startup: tray_icon::menu::CheckMenuItem,
}

impl Tray {
    /// Tick "Start with Windows" as the entry stands. The window's checkbox
    /// changes the entry too.
    pub fn sync(&self, with_windows: bool) {
        if self.startup.is_checked() != with_windows {
            self.startup.set_checked(with_windows);
        }
    }
}

/// Show the window, restored and in front.
pub fn show(ctx: &Context) {
    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(ViewportCommand::Focus);
    ctx.request_repaint();
}

/// Put the icon in the notification area. `None` where Windows refuses it:
/// the window then closes as a window does.
///
/// `quitting` is set before Exit closes the window, so that the close does
/// not hide it to the tray.
pub fn spawn(
    ctx: &Context,
    send: impl Fn(Command) + Send + Sync + 'static,
    quitting: Arc<AtomicBool>,
) -> Option<Tray> {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show_item = MenuItem::new("Show", true, None);
    let start = MenuItem::new("Start mirror", true, None);
    let stop = MenuItem::new("Stop mirror", true, None);
    let startup = CheckMenuItem::new("Start with Windows", true, startup::enabled(), None);
    let quit = MenuItem::new("Exit", true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &show_item,
        &PredefinedMenuItem::separator(),
        &start,
        &stop,
        &PredefinedMenuItem::separator(),
        &startup,
        &quit,
    ])
    .ok()?;

    const SIZE: u32 = 32;
    let icon = Icon::from_rgba(pattern::icon(SIZE), SIZE, SIZE).ok()?;
    let tray = TrayIconBuilder::new()
        .with_tooltip("Govee Mirror PC")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .ok()?;

    let clicked = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |event| match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
        | TrayIconEvent::DoubleClick { .. } => show(&clicked),
        _ => {}
    }));

    let ctx = ctx.clone();
    let ids = (
        show_item.id().clone(),
        start.id().clone(),
        stop.id().clone(),
        startup.id().clone(),
        quit.id().clone(),
    );
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let (show_id, start_id, stop_id, startup_id, quit_id) = &ids;
        if event.id == *show_id {
            show(&ctx);
        } else if event.id == *start_id {
            send(Command::Start);
        } else if event.id == *stop_id {
            send(Command::Stop);
        } else if event.id == *startup_id {
            // The menu item is not Send, so the handler reads the entry rather
            // than the tick, and the window ticks the item to match.
            let _ = startup::set(!startup::enabled());
            ctx.request_repaint();
        } else if event.id == *quit_id {
            quitting.store(true, Ordering::SeqCst);
            // A hidden window processes no command, so it shows first.
            show(&ctx);
            ctx.send_viewport_cmd(ViewportCommand::Close);
            ctx.request_repaint();
        }
    }));

    Some(Tray {
        _icon: tray,
        startup,
    })
}
