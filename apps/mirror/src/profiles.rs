//! Profiles: named mirror setups, each with a program that switches to it.
//!
//! The file holds the library. The app works on one [`Settings`] at a time:
//! [`Library::current`] builds it from the active profile and the settings
//! that every profile shares, and [`Library::store`] splits it back.

use std::collections::HashSet;
use std::{fs, io};

use serde::{Deserialize, Serialize};

use crate::razer::protocol::PORTS;
use crate::razer::{chroma, headset};
use crate::settings::Settings;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// The executable that switches to this profile while it runs, as
    /// Windows names the process: `game.exe`. Compared without case.
    #[serde(default)]
    pub process: Option<String>,
    pub settings: Settings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub profiles: Vec<Profile>,
    /// The profile in use.
    pub active: usize,
    /// The profile the person picked. A program that ends switches back to
    /// it.
    pub chosen: usize,
    /// Shared by every profile: they describe the hardware and the start.
    pub mirror_on_launch: bool,
    pub razer_leds: [usize; PORTS],
    #[serde(default)]
    pub lan_hosts: Vec<std::net::IpAddr>,
    #[serde(default)]
    pub lights: std::collections::BTreeMap<String, crate::settings::Light>,
    #[serde(default)]
    pub headset: headset::Settings,
    #[serde(default)]
    pub macro_keys: Vec<Option<String>>,
    /// Read from an earlier file, where every profile shared the effects of
    /// the Razer keyboard and mousepad. [`Library::parse`] gives them to each
    /// profile.
    #[serde(default, skip_serializing)]
    keyboard: Option<chroma::Settings>,
    #[serde(default, skip_serializing)]
    mousepad: Option<chroma::Settings>,
}

impl Default for Library {
    fn default() -> Self {
        Self::from_settings(Settings::default())
    }
}

impl Library {
    /// One profile, "Default", from a single settings record.
    fn from_settings(settings: Settings) -> Self {
        Self {
            mirror_on_launch: settings.mirror_on_launch,
            razer_leds: settings.razer_leds,
            lan_hosts: settings.lan_hosts.clone(),
            lights: settings.lights.clone(),
            headset: settings.headset.clone(),
            macro_keys: settings.macro_keys.clone(),
            keyboard: None,
            mousepad: None,
            profiles: vec![Profile {
                name: "Default".to_owned(),
                process: None,
                settings,
            }],
            active: 0,
            chosen: 0,
        }
    }

    /// The saved library, and what went wrong reading it. A missing file
    /// gives one default profile. A file of the earlier format, one settings
    /// record, becomes the "Default" profile. A file that does not parse is
    /// moved to `mirror.json.bad`, so that the first save does not overwrite
    /// it.
    pub fn load() -> (Self, Option<String>) {
        let path = Settings::path();
        let Ok(text) = fs::read_to_string(&path) else {
            return (Self::default(), None);
        };
        match Self::parse(&text) {
            Ok(library) => (library, None),
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

    fn parse(text: &str) -> Result<Self, serde_json::Error> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let mut library = if value.get("profiles").is_some() {
            serde_json::from_value::<Self>(value)?
        } else {
            Self::from_settings(serde_json::from_value(value)?)
        };
        if library.profiles.is_empty() {
            library = Self::default();
        }
        for profile in &mut library.profiles {
            if let Some(keyboard) = &library.keyboard {
                profile.settings.keyboard.clone_from(keyboard);
            }
            if let Some(mousepad) = &library.mousepad {
                profile.settings.mousepad.clone_from(mousepad);
            }
        }
        library.keyboard = None;
        library.mousepad = None;
        let last = library.profiles.len() - 1;
        library.active = library.active.min(last);
        library.chosen = library.chosen.min(last);
        Ok(library)
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Settings::path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, text)?;
        fs::rename(temporary, path)
    }

    /// The settings of the active profile, with the shared ones.
    pub fn current(&self) -> Settings {
        let mut settings = self.profiles[self.active].settings.clone();
        settings.mirror_on_launch = self.mirror_on_launch;
        settings.razer_leds = self.razer_leds;
        settings.lan_hosts.clone_from(&self.lan_hosts);
        settings.lights.clone_from(&self.lights);
        settings.headset = self.headset.clone();
        settings.macro_keys.clone_from(&self.macro_keys);
        settings
    }

    /// Keep `settings` as the active profile's, and its shared settings for
    /// every profile.
    pub fn store(&mut self, settings: &Settings) {
        self.mirror_on_launch = settings.mirror_on_launch;
        self.razer_leds = settings.razer_leds;
        self.lan_hosts.clone_from(&settings.lan_hosts);
        self.lights.clone_from(&settings.lights);
        self.headset = settings.headset.clone();
        self.macro_keys.clone_from(&settings.macro_keys);
        self.profiles[self.active].settings = settings.clone();
    }

    /// Whether any profile names a program, so that the process list is
    /// worth reading.
    pub fn watches(&self) -> bool {
        self.profiles.iter().any(|p| p.process.is_some())
    }

    /// The profile that the running programs ask for: the first one whose
    /// program runs, else the one the person picked.
    pub fn wanted(&self, running: &HashSet<String>) -> usize {
        self.profiles
            .iter()
            .position(|p| {
                p.process
                    .as_ref()
                    .is_some_and(|name| running.contains(&name.to_lowercase()))
            })
            .unwrap_or(self.chosen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, process: Option<&str>) -> Profile {
        Profile {
            name: name.to_owned(),
            process: process.map(str::to_owned),
            settings: Settings::default(),
        }
    }

    #[test]
    fn shared_razer_effects_of_an_earlier_file_go_to_every_profile() {
        let text = r#"{
            "profiles": [
                {"name": "A", "settings": {}},
                {"name": "B", "settings": {}}
            ],
            "active": 0, "chosen": 0, "mirror_on_launch": false,
            "razer_leds": [0, 0, 0, 0, 0, 0],
            "keyboard": {"enabled": true, "brightness": 30}
        }"#;
        let library = Library::parse(text).unwrap_or_default();
        assert!(library.profiles.iter().all(|p| p.settings.keyboard.enabled));
        assert_eq!(library.profiles[1].settings.keyboard.brightness, 30);
        let saved = serde_json::to_value(&library).unwrap_or_default();
        assert!(saved.get("keyboard").is_none());
    }

    #[test]
    fn an_earlier_settings_file_becomes_the_default_profile() {
        let library = Library::parse(r#"{"fps": 45, "razer_leds": [0, 30, 0, 60, 0, 0]}"#)
            .unwrap_or_default();
        assert_eq!(library.profiles.len(), 1);
        assert_eq!(library.profiles[0].name, "Default");
        assert_eq!(library.current().fps, 45);
        assert_eq!(library.razer_leds, [0, 30, 0, 60, 0, 0]);
    }

    #[test]
    fn shared_settings_follow_every_profile() {
        let mut library = Library::default();
        library.profiles.push(profile("Game", None));
        let mut settings = library.current();
        settings.razer_leds[3] = 60;
        settings.fps = 50;
        library.store(&settings);
        library.active = 1;
        assert_eq!(library.current().razer_leds[3], 60);
        assert_eq!(library.current().fps, Settings::default().fps);
    }

    #[test]
    fn a_running_program_picks_its_profile_and_its_end_returns_to_the_chosen_one() {
        let mut library = Library::default();
        library.profiles.push(profile("Game", Some("Game.exe")));
        library.profiles.push(profile("Films", None));
        library.chosen = 2;
        assert!(library.watches());
        let running: HashSet<String> = ["game.exe".to_owned()].into();
        assert_eq!(library.wanted(&running), 1);
        assert_eq!(library.wanted(&HashSet::new()), 2);
    }
}
