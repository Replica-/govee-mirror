//! A Windows notification under the app's own name.
//!
//! An unpackaged app names itself to the notification centre through a key
//! under `HKCU\Software\Classes\AppUserModelId`. [`show`] writes that key, and
//! the icon it points at, before the first notification of the process.

/// Show a notification with a title and one line of text.
///
/// # Errors
///
/// What Windows answered, where it refuses the notification.
pub fn show(title: &str, text: &str) -> Result<(), String> {
    imp::show(title, text)
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use std::sync::Once;
    use std::{fs, io};

    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use windows::core::HSTRING;
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    use crate::pattern;
    use crate::settings::Settings;

    /// The same name as the start-with-Windows entry.
    const APP_ID: &str = "GoveeMirrorPC";
    const NAME: &str = "Govee Razer Mirror Windows 11 PC";
    const ICON: u32 = 64;

    pub fn show(title: &str, text: &str) -> Result<(), String> {
        static REGISTER: Once = Once::new();
        // Best effort: without the key, Windows shows the app ID in place of
        // the name.
        REGISTER.call_once(|| {
            let _ = register();
        });
        let xml = format!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
            escape(title),
            escape(text),
        );
        let toast = || -> windows::core::Result<()> {
            // WinRT needs an apartment on this thread. A thread that holds
            // one keeps it.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let document = XmlDocument::new()?;
            document.LoadXml(&HSTRING::from(xml))?;
            let notification = ToastNotification::CreateToastNotification(&document)?;
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?
                .Show(&notification)
        };
        toast().map_err(|e| e.to_string())
    }

    fn register() -> io::Result<()> {
        let icon = Settings::path().with_file_name("icon.ico");
        if let Some(dir) = icon.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&icon, ico(ICON))?;
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))?;
        key.set_value("DisplayName", &NAME)?;
        key.set_value("IconUri", &icon.display().to_string())
    }

    /// The app icon as a `.ico` file: one 32-bit image, bottom row first, and
    /// an empty mask, since the alpha channel carries the shape.
    fn ico(size: u32) -> Vec<u8> {
        let rgba = pattern::icon(size);
        let mask = (size.div_ceil(32) * 4 * size) as usize;
        let image = 40 + rgba.len() + mask;
        let mut out = Vec::with_capacity(22 + image);
        out.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
        let side = u8::try_from(size).unwrap_or(0);
        out.extend_from_slice(&[side, side, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&u32::try_from(image).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(&22u32.to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(size * 2).to_le_bytes());
        out.extend_from_slice(&[1, 0, 32, 0]);
        out.extend_from_slice(&[0; 24]);
        let row = size as usize * 4;
        for line in rgba.chunks_exact(row).rev() {
            for [r, g, b, a] in line.as_chunks::<4>().0 {
                out.extend_from_slice(&[*b, *g, *r, *a]);
            }
        }
        out.resize(out.len() + mask, 0);
        out
    }

    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn show(_title: &str, _text: &str) -> Result<(), String> {
        Err("notifications are available on Windows only".to_owned())
    }
}
