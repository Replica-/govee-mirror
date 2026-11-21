//! The programs that run, by executable name, through one Toolhelp snapshot.
//!
//! The snapshot is a Win32 call, so this module holds `unsafe` code: each call
//! takes a handle this module owns and a buffer that outlives it.

use std::collections::HashSet;

/// The executable names of the running processes, lowercased: `game.exe`.
/// Empty where the snapshot fails.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn running() -> HashSet<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let mut names = HashSet::new();
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return names;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        let len = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        names.insert(String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase());
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    let _ = unsafe { CloseHandle(snapshot) };
    names
}

#[cfg(not(windows))]
pub fn running() -> HashSet<String> {
    HashSet::new()
}
