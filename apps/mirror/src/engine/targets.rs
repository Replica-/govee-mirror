//! The Govee lights that answer a `lan` scan, and what each one can mirror.

use govee_toolkit::stream::reach;
use govee_toolkit::{Device, Govee, Mode};

use crate::settings::Resolution;

/// One light the app can drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The MAC the device reports.
    pub id: String,
    pub name: String,
    pub sku: String,
    /// Segments at the `app` resolution. `None` where the device file gives
    /// no segment channel over `lan`: the light then mirrors as one zone,
    /// through the `color` command.
    pub app_zones: Option<usize>,
    /// Segments at the `native` resolution, where the file measured them and
    /// the channel carries them all.
    pub native_zones: Option<usize>,
    /// The most zones the channel renders, where the file bounds it above
    /// the `app` count.
    pub full_zones: Option<usize>,
    /// The most zones one frame carries. Bounds a count set by hand.
    pub max_zones: usize,
    pub brightness: [i64; 2],
    /// Whether a device file describes the SKU at all.
    pub known: bool,
}

impl Target {
    pub fn streams(&self) -> bool {
        self.app_zones.is_some()
    }

    /// Each resolution the light offers, with its zone count.
    pub fn options(&self) -> Vec<(Resolution, usize)> {
        let Some(app) = self.app_zones else {
            return vec![(Resolution::App, 1)];
        };
        let mut options = vec![(Resolution::App, app)];
        options.extend(self.full_zones.map(|n| (Resolution::Full, n)));
        options.extend(self.native_zones.map(|n| (Resolution::Native, n)));
        options
    }

    /// The resolution with the most zones.
    pub fn finest(&self) -> Resolution {
        self.options()
            .into_iter()
            .max_by_key(|(_, zones)| *zones)
            .map_or(Resolution::App, |(r, _)| r)
    }

    pub fn zones(&self, resolution: Resolution) -> usize {
        self.options()
            .into_iter()
            .find(|(r, _)| *r == resolution)
            .map_or(1, |(_, zones)| zones)
    }
}

/// Scan `lan` and describe what answered.
pub async fn scan(govee: &Govee) -> govee_toolkit::Result<Vec<Target>> {
    let found = govee.scan(Some(&[Mode::Lan])).await?;
    Ok(found.iter().map(|device| describe(govee, device)).collect())
}

fn describe(govee: &Govee, device: &Device) -> Target {
    let spec = govee
        .device(&device.id, Some(Mode::Lan))
        .ok()
        .and_then(|handle| handle.spec().ok().cloned());
    let Some(spec) = spec else {
        return Target {
            id: device.id.as_str().to_owned(),
            name: device.name.clone().unwrap_or_else(|| device.sku.clone()),
            sku: device.sku.clone(),
            app_zones: None,
            native_zones: None,
            full_zones: None,
            max_zones: 1,
            brightness: [0, 100],
            known: false,
        };
    };
    let reach = reach(&spec, Mode::Lan);
    let count = |value: Option<u32>| value.and_then(|v| usize::try_from(v).ok());
    // `app` falls to what the channel carries where the file states more.
    let app_zones =
        reach.and_then(|r| count(spec.capabilities.segment_count()).map(|c| c.min(r.zones)));
    let native_zones = reach
        .filter(|r| r.native)
        .and_then(|_| count(spec.capabilities.native_pixels()))
        .filter(|&native| Some(native) != app_zones);
    let full_zones = reach
        .map(|r| r.zones)
        .filter(|&zones| app_zones.is_some_and(|app| zones > app) && Some(zones) != native_zones);
    Target {
        id: device.id.as_str().to_owned(),
        name: device.name.clone().unwrap_or_else(|| spec.name.clone()),
        sku: device.sku.clone(),
        app_zones,
        native_zones,
        full_zones,
        max_zones: reach.map_or(1, |r| r.zones),
        brightness: spec
            .capabilities
            .get("brightness")
            .and_then(|params| params.range)
            .unwrap_or([0, 100]),
        known: true,
    }
}
