//! Start with Windows: one value under the user's `Run` key. It names this
//! executable with `--minimized`, so a sign-in starts the app in the taskbar.

/// The argument that starts the window minimized.
pub const MINIMIZED: &str = "--minimized";

#[cfg(windows)]
mod imp {
    use std::{env, io};

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "GoveeMirror";

    fn command() -> io::Result<String> {
        Ok(format!(
            "\"{}\" {}",
            env::current_exe()?.display(),
            super::MINIMIZED
        ))
    }

    /// Whether the entry names this executable. An entry left by a copy
    /// elsewhere on disk reads as off, so that a click writes this one.
    pub fn enabled() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN, KEY_READ)
            .and_then(|key| key.get_value::<String, _>(VALUE))
            .is_ok_and(|value| command().is_ok_and(|ours| value == ours))
    }

    pub fn set(on: bool) -> io::Result<()> {
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN, KEY_SET_VALUE)?;
        if on {
            key.set_value(VALUE, &command()?)
        } else {
            match key.delete_value(VALUE) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::io;

    pub fn enabled() -> bool {
        false
    }

    pub fn set(_on: bool) -> io::Result<()> {
        Err(io::Error::other(
            "start with Windows is available on Windows only",
        ))
    }
}

pub use imp::{enabled, set};
