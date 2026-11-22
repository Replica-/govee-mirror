//! What the window sends the engine, and what the engine reports back.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::oneshot;

use super::targets::Target;
use super::writer::Stats;
use crate::settings::{Light, Settings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Power(bool),
    Brightness(i64),
    Color([u8; 3]),
}

#[derive(Debug, Clone)]
pub enum Command {
    Scan,
    Start,
    Stop,
    Control(String, Control),
    Highlight(String, usize),
    Settings(Box<Settings>),
    /// A command must be `Clone` for the window, and a sender is not.
    Quit(Arc<Mutex<Option<oneshot::Sender<()>>>>),
}

/// What the window shows.
#[derive(Debug, Default)]
pub struct Status {
    pub targets: Vec<Target>,
    pub problems: Vec<String>,
    /// What went wrong at startup. A scan does not clear it.
    pub warnings: Vec<String>,
    pub busy: Option<&'static str>,
    pub running: bool,
    /// Whether a Razer ARGB controller is connected.
    pub razer: bool,
    pub fps: u32,
    pub stats: HashMap<String, Stats>,
    /// The last failure of a command, per target.
    pub messages: HashMap<String, String>,
    /// The lights the last scan found, by MAC, for the window to save. The
    /// window takes them.
    pub found: BTreeMap<String, Light>,
}

pub type Shared = Arc<Mutex<Status>>;

pub fn lock(status: &Shared) -> MutexGuard<'_, Status> {
    status.lock().unwrap_or_else(PoisonError::into_inner)
}
