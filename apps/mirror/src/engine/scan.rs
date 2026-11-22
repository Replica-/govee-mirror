//! The scan: Govee lights over `lan`, and the ports of a Razer ARGB controller.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use govee_toolkit::transport::Event as TransportEvent;
use govee_toolkit::{Config, Event, Govee, Mode};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;

use super::{Context, explain, lock, targets};
use crate::settings::Light;

/// Start the SDK from the configuration file, with `hosts` added to what
/// every `lan` scan asks directly.
pub(super) async fn start_sdk(hosts: &[IpAddr]) -> govee_toolkit::Result<Govee> {
    let mut config = Config::load()?;
    config.lan.hosts.extend_from_slice(hosts);
    Govee::start_or_defaults(Some(config), None).await
}

impl Context {
    /// Start the SDK again with the addresses of the settings, then scan. The
    /// old SDK must let go of the reply port first. A mirror that ran starts
    /// again on the new one.
    pub(super) async fn restart_sdk(&mut self) {
        let running = self.mirror.is_some();
        self.stop().await;
        let _ = self.govee.shutdown().await;
        match start_sdk(&self.settings.hosts()).await {
            Ok(govee) => self.govee = govee,
            Err(error) => {
                lock(&self.status).problems =
                    vec![format!("The SDK did not start again: {}", explain(&error))];
                return;
            }
        }
        self.scan().await;
        if running {
            self.start().await;
        }
    }

    pub(super) async fn scan(&mut self) {
        self.busy(Some("Scanning"));
        let mut events = self.govee.events();
        let result = targets::scan(&self.govee).await;
        let addresses = match &result {
            Ok(targets) => {
                let ids: Vec<&str> = targets.iter().map(|t| t.id.as_str()).collect();
                answered(&mut events, &ids).await
            }
            Err(_) => BTreeMap::new(),
        };
        let razer = crate::razer::present();
        let synapse = razer && crate::razer::synapse_running();
        let mut status = lock(&self.status);
        status.busy = None;
        status.razer = razer;
        match result {
            Ok(targets) => {
                status.problems = if targets.is_empty() {
                    vec!["No Govee light answered. Check that LAN Control is on in the Govee Home app, or add the light's address in General.".to_owned()]
                } else {
                    Vec::new()
                };
                // The engine keeps them too, so that the settings the window
                // sends back hold the same addresses and start no new SDK.
                for target in &targets {
                    let Some(&ip) = addresses.get(&target.id) else {
                        continue;
                    };
                    let light = Light {
                        ip,
                        sku: target.sku.clone(),
                        name: target.name.clone(),
                    };
                    self.settings
                        .lights
                        .insert(target.id.clone(), light.clone());
                    status.found.insert(target.id.clone(), light);
                }
                status.targets = targets;
            }
            Err(error) => status.problems = vec![format!("The scan failed: {}", explain(&error))],
        }
        if synapse {
            status.problems.push(
                "Razer Synapse is running. It sends its own frames to the ARGB controller: exit it from its tray icon.".to_owned(),
            );
        }
        status
            .targets
            .extend(targets::razer(&self.settings.razer_leds));
    }
}

/// The address of each light in `wanted` that answered over `lan`, by MAC,
/// from the events of the scan. A `lan` endpoint is `ip:port`. The SDK can
/// forward an answer after the scan returns, so a missing one gets
/// [`LATE`] to arrive.
async fn answered(events: &mut Receiver<Event>, wanted: &[&str]) -> BTreeMap<String, IpAddr> {
    let mut addresses = BTreeMap::new();
    let deadline = tokio::time::Instant::now() + LATE;
    while wanted.iter().any(|id| !addresses.contains_key(*id)) {
        let event = match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Ok(event)) => event,
            Ok(Err(RecvError::Lagged(_))) => continue,
            Ok(Err(RecvError::Closed)) | Err(_) => break,
        };
        if let Event::Transport(TransportEvent::Discovered {
            mode: Mode::Lan,
            device,
            ..
        }) = event
            && let Ok(endpoint) = device.endpoint.parse::<SocketAddr>()
        {
            addresses.insert(device.id.as_str().to_owned(), endpoint.ip());
        }
    }
    addresses
}

const LATE: Duration = Duration::from_millis(250);
