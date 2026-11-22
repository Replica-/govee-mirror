//! The settings of the app, in one JSON file.

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::engine::Target;
use crate::pattern::Source;

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
    /// The MAC a Govee light reports, or `razer-argb:<port>`.
    pub target: String,
    /// The first segment the cell feeds.
    pub segment: usize,
    /// How many segments the cell feeds, from `segment` up. A box fill that
    /// spreads more segments than it has cells gives each cell a run.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub count: usize,
}

impl Assignment {
    /// One segment of one light.
    pub fn one(target: &str, segment: usize) -> Self {
        Self {
            target: target.to_owned(),
            segment,
            count: 1,
        }
    }

    /// The segments the cell feeds.
    pub fn segments(&self) -> std::ops::Range<usize> {
        self.segment..self.segment + self.count.max(1)
    }
}

fn one() -> usize {
    1
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_one(count: &usize) -> bool {
    *count == 1
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

/// A Govee light that answered a scan, kept so that the next scan asks its
/// address directly. Its settings stay keyed by the same MAC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Light {
    pub ip: std::net::IpAddr,
    pub sku: String,
    pub name: String,
}

/// What the mirror does to the colors of one light, in percent of full scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Levels {
    /// The brightness of a Razer port. A Govee light takes the `brightness`
    /// command instead.
    pub brightness: u8,
    /// Whether the light follows the sound reaction.
    pub react: bool,
    /// The level of the sound reaction between two bass hits.
    pub rest: u8,
    /// The level of the sound reaction at a bass hit.
    pub peak: u8,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            brightness: 100,
            react: false,
            rest: 50,
            peak: 100,
        }
    }
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
    /// What each cell feeds: one segment per light, and any number of
    /// lights. An earlier file gives one assignment per cell, and it reads
    /// as a list of one.
    #[serde(deserialize_with = "one_or_many")]
    pub assignments: BTreeMap<Cell, Vec<Assignment>>,
    pub devices: BTreeMap<String, DeviceSettings>,
    /// Start the mirror once the first scan ends.
    pub mirror_on_launch: bool,
    /// LEDs on each port of the Razer ARGB controller, port 1 first. A port
    /// with 0 is not a light. The controller keeps its own counts, set in
    /// Synapse, and these must match them.
    pub razer_leds: [usize; crate::razer::protocol::PORTS],
    /// Addresses of Govee lights that every scan also asks directly, for a
    /// network that drops the multicast scan.
    pub lan_hosts: Vec<std::net::IpAddr>,
    /// Each Govee light that a scan found, by its MAC. A move to a new
    /// address replaces the old one.
    pub lights: BTreeMap<String, Light>,
    /// Switch the sound output with the Razer headset.
    pub headset: crate::razer::headset::Settings,
    /// How far each color moves from grey, 1 for none. Averaging a patch of
    /// screen pulls colors toward grey, and this pushes them back.
    pub saturation: f32,
    /// What the mirror samples: the screen, or a pattern.
    pub source: Source,
    /// Whether a pattern moves. A still pattern holds its first frame.
    pub moving: bool,
    /// How many times a second one ring or one wedge of a pattern takes the
    /// place of the next.
    pub animation_speed: f32,
    /// Per light, by the key of `devices`. A change does not restart the
    /// mirror: each frame reads them.
    pub levels: BTreeMap<String, Levels>,
    /// Raise the lights to their peak level on a bass hit.
    pub sound: crate::sound::Settings,
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
            razer_leds: [0; crate::razer::protocol::PORTS],
            lan_hosts: Vec::new(),
            lights: BTreeMap::new(),
            headset: crate::razer::headset::Settings::default(),
            saturation: 1.3,
            source: Source::Screen,
            moving: true,
            animation_speed: 0.25,
            levels: BTreeMap::new(),
            sound: crate::sound::Settings::default(),
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

    /// The typed addresses and the address of each found light: what every
    /// scan asks directly.
    pub fn hosts(&self) -> Vec<std::net::IpAddr> {
        let mut hosts = self.lan_hosts.clone();
        hosts.extend(self.lights.values().map(|light| light.ip));
        hosts.sort_unstable();
        hosts.dedup();
        hosts
    }

    /// Give `assignment.target` the segment in `cell`, in place of the one it
    /// had there. Other lights in the cell keep theirs. Answers whether the
    /// cell changed.
    pub fn set_cell(&mut self, cell: Cell, assignment: Assignment) -> bool {
        let entries = self.assignments.entry(cell).or_default();
        match entries.iter_mut().find(|a| a.target == assignment.target) {
            Some(existing) if *existing == assignment => false,
            Some(existing) => {
                *existing = assignment;
                true
            }
            None => {
                entries.push(assignment);
                true
            }
        }
    }

    /// Take `target` out of `cell`. Answers whether the cell changed.
    pub fn clear_cell(&mut self, cell: Cell, target: &str) -> bool {
        let Some(entries) = self.assignments.get_mut(&cell) else {
            return false;
        };
        let before = entries.len();
        entries.retain(|a| a.target != target);
        let changed = entries.len() != before;
        if entries.is_empty() {
            self.assignments.remove(&cell);
        }
        changed
    }

    /// Take `target` out of every cell.
    pub fn clear_target(&mut self, target: &str) {
        for entries in self.assignments.values_mut() {
            entries.retain(|a| a.target != target);
        }
        self.assignments.retain(|_, entries| !entries.is_empty());
    }

    /// Every assignment, with its cell.
    pub fn entries(&self) -> impl Iterator<Item = (Cell, &Assignment)> {
        self.assignments
            .iter()
            .flat_map(|(cell, entries)| entries.iter().map(move |a| (*cell, a)))
    }

    /// The levels of `id`, or the defaults.
    pub fn levels(&self, id: &str) -> Levels {
        self.levels.get(id).copied().unwrap_or_default()
    }

    /// The share of full scale that the mirror gives the colors of light
    /// `id`, with the sound reaction at `pulse`, from 0 to 1.
    pub fn scale(&self, id: &str, razer: bool, pulse: f32) -> f32 {
        let levels = self.levels(id);
        let mut scale = 1.0;
        if razer {
            scale *= f32::from(levels.brightness) / 100.0;
        }
        if self.sound.enabled && levels.react {
            let (rest, peak) = (f32::from(levels.rest), f32::from(levels.peak));
            scale *= (rest + (peak - rest) * pulse) / 100.0;
        }
        scale
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

/// An assignment list per cell, from a list or from the single assignment
/// that an earlier file holds.
fn one_or_many<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<Cell, Vec<Assignment>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Entry {
        One(Assignment),
        Many(Vec<Assignment>),
    }
    let cells = BTreeMap::<Cell, Entry>::deserialize(deserializer)?;
    Ok(cells
        .into_iter()
        .map(|(cell, entry)| match entry {
            Entry::One(assignment) => (cell, vec![assignment]),
            Entry::Many(list) => (cell, list),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_read_back_what_they_wrote() {
        let mut settings = Settings::default();
        settings.set_cell(Cell { row: 2, col: 5 }, Assignment::one("AA:BB", 3));
        let text = serde_json::to_string(&settings).unwrap_or_default();
        assert!(text.contains("\"2:5\""));
        let back: Settings = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(back, settings);
    }

    #[test]
    fn an_earlier_file_with_one_assignment_per_cell_reads_as_a_list_of_one() {
        let text = r#"{"assignments": {"0:1": {"target": "A", "segment": 4}}}"#;
        let settings: Settings = serde_json::from_str(text).unwrap_or_default();
        let cell = Cell { row: 0, col: 1 };
        assert_eq!(settings.assignments[&cell], vec![Assignment::one("A", 4)]);
    }

    #[test]
    fn a_cell_holds_one_segment_per_light_and_any_number_of_lights() {
        let mut settings = Settings::default();
        let cell = Cell { row: 0, col: 0 };
        let to = |target: &str, segment| Assignment::one(target, segment);
        assert!(settings.set_cell(cell, to("A", 1)));
        assert!(settings.set_cell(cell, to("B", 7)));
        assert!(settings.set_cell(cell, to("A", 2)));
        assert!(!settings.set_cell(cell, to("A", 2)));
        assert_eq!(settings.assignments[&cell], vec![to("A", 2), to("B", 7)]);
        assert!(settings.clear_cell(cell, "A"));
        assert_eq!(settings.assignments[&cell], vec![to("B", 7)]);
        settings.clear_target("B");
        assert!(settings.assignments.is_empty());
    }
}
