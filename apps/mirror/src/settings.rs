//! The settings of the app, in one JSON file.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::{env, fs, io};

use serde::{Deserialize, Serialize};

use crate::engine::Target;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
}

/// One cell of the grid. Serialized as `row:col`, so that it can key a JSON
/// object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    /// The MAC the device reports.
    pub target: String,
    pub segment: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Resolution {
    /// The zones the phone controller exposes.
    #[default]
    App,
    /// One zone per addressable LED, where the device file measured them.
    Native,
    /// Every zone the segment channel renders, where the device file bounds
    /// the channel above the `app` count.
    Full,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceSettings {
    pub resolution: Resolution,
    /// A zone count set by hand. It wins over `resolution`.
    pub zones: Option<usize>,
    /// Ask the firmware to blend between segments, and to wrap from the last
    /// back to the first.
    pub gradient: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The index of the screen, in the order DXGI lists them.
    pub display: usize,
    pub grid: Grid,
    pub fps: u32,
    /// The share of the previous frame that stays, from 0 to 0.95.
    pub smoothing: f32,
    pub assignments: BTreeMap<Cell, Assignment>,
    pub devices: BTreeMap<String, DeviceSettings>,
    /// Start the mirror once the first scan ends.
    pub mirror_on_launch: bool,
    /// How far each color moves from grey, 1 for none. Averaging a patch of
    /// screen pulls colors toward grey, and this pushes them back.
    pub saturation: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            display: 0,
            grid: Grid { cols: 16, rows: 9 },
            fps: 30,
            smoothing: 0.4,
            assignments: BTreeMap::new(),
            devices: BTreeMap::new(),
            mirror_on_launch: false,
            saturation: 1.3,
        }
    }
}

impl Settings {
    /// `%APPDATA%\govee-toolkit\mirror.json`, or `mirror.json` beside the
    /// working directory where the variable is absent.
    pub fn path() -> PathBuf {
        env::var_os("APPDATA")
            .map(|dir| PathBuf::from(dir).join("govee-toolkit"))
            .unwrap_or_default()
            .join("mirror.json")
    }

    /// The saved settings, and what went wrong reading them. A missing file
    /// gives the defaults. A file that does not parse is moved to
    /// `mirror.json.bad`, so that the first save does not overwrite it.
    pub fn load() -> (Self, Option<String>) {
        let path = Self::path();
        let Ok(text) = fs::read_to_string(&path) else {
            return (Self::default(), None);
        };
        match serde_json::from_str(&text) {
            Ok(settings) => (settings, None),
            Err(error) => {
                let bad = path.with_extension("json.bad");
                let kept = match fs::rename(&path, &bad) {
                    Ok(()) => format!("it is kept as {}", bad.display()),
                    Err(_) => "it could not be set aside".to_owned(),
                };
                let problem = format!("The settings did not load ({error}): {kept}.");
                (Self::default(), Some(problem))
            }
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, text)?;
        fs::rename(temporary, path)
    }

    /// The saved resolution where the light offers it, else the light's
    /// finest.
    pub fn resolution(&self, target: &Target) -> Resolution {
        self.devices
            .get(&target.id)
            .map(|d| d.resolution)
            .filter(|r| target.options().iter().any(|(o, _)| o == r))
            .unwrap_or_else(|| target.finest())
    }

    /// The zone count set by hand, within what one frame carries.
    pub fn manual_zones(&self, target: &Target) -> Option<usize> {
        let zones = self.devices.get(&target.id)?.zones?;
        target.streams().then(|| zones.clamp(1, target.max_zones))
    }

    /// How many segments the light mirrors.
    pub fn zones(&self, target: &Target) -> usize {
        self.manual_zones(target)
            .unwrap_or_else(|| target.zones(self.resolution(target)))
    }

    /// The resolution a stream or a paint states to the SDK.
    pub fn frame(&self, target: &Target) -> govee_toolkit::Resolution {
        let exact =
            |n: usize| govee_toolkit::Resolution::Exact(u16::try_from(n).unwrap_or(u16::MAX));
        if let Some(zones) = self.manual_zones(target) {
            return exact(zones);
        }
        match self.resolution(target) {
            Resolution::App => govee_toolkit::Resolution::App,
            Resolution::Native => govee_toolkit::Resolution::Native,
            Resolution::Full => exact(target.zones(Resolution::Full)),
        }
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.row, self.col)
    }
}

impl FromStr for Cell {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (row, col) = text
            .split_once(':')
            .ok_or_else(|| format!("{text} is not row:col"))?;
        Ok(Self {
            row: row.parse().map_err(|_| format!("{text} is not row:col"))?,
            col: col.parse().map_err(|_| format!("{text} is not row:col"))?,
        })
    }
}

impl Serialize for Cell {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Cell {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_read_back_what_they_wrote() {
        let mut settings = Settings::default();
        settings.assignments.insert(
            Cell { row: 2, col: 5 },
            Assignment {
                target: "AA:BB".to_owned(),
                segment: 3,
            },
        );
        let text = serde_json::to_string(&settings).unwrap_or_default();
        assert!(text.contains("\"2:5\""));
        let back: Settings = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(back, settings);
    }
}
