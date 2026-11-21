//! The scan: Govee lights over `lan`, and the ports of a Razer ARGB controller.

use super::{Context, explain, lock, targets};

impl Context {
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
                    vec!["No Govee light answered. Check that LAN Control is on in the Govee Home app.".to_owned()]
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
