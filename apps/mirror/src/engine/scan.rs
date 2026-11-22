//! The scan: Govee lights over `lan`, and the ports of a Razer ARGB controller.

use std::net::IpAddr;

use govee_toolkit::{Config, Govee};

use super::{Context, explain, lock, targets};

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
        match start_sdk(&self.settings.lan_hosts).await {
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
        let result = targets::scan(&self.govee).await;
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
