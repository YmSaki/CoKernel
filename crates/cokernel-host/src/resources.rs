use crate::{
    bridge::BridgeError,
    supervisor::{Host, ObservedState},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::OnceLock, time::Duration};

pub(crate) struct HostResources {
    root: PathBuf,
    sampler: OnceLock<cokernel_metrics::Sampler>,
}
impl HostResources {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            sampler: OnceLock::new(),
        }
    }
    pub async fn shutdown(&self) -> Result<(), BridgeError> {
        if let Some(sampler) = self.sampler.get() {
            sampler
                .shutdown()
                .await
                .map_err(|e| BridgeError::new("CK-HOST-METRICS", e))?;
        }
        Ok(())
    }
    pub async fn snapshot(&self, host: &Host) -> Value {
        let windows = self
            .sampler
            .get_or_init(|| cokernel_metrics::Sampler::start(self.root.clone()))
            .snapshot();
        let before = host.status();
        let (linux, reason) = if before.observed != ObservedState::Healthy {
            (None, Some("RUNTIME_UNAVAILABLE"))
        } else if !before
            .runtime_capabilities
            .iter()
            .any(|c| c == "linux-metrics")
        {
            (None, Some("NOT_SUPPORTED"))
        } else {
            match tokio::time::timeout(
                Duration::from_millis(750),
                host.call("runtime.get_linux_metrics", json!({})),
            )
            .await
            {
                Ok(Ok(response)) if response.ok => {
                    match response
                        .result
                        .and_then(|v| serde_json::from_value::<cokernel_metrics::Snapshot>(v).ok())
                    {
                        Some(snapshot) => (Some(snapshot), None),
                        None => (None, Some("INVALID_DATA")),
                    }
                }
                Err(_) => (None, Some("TIMED_OUT")),
                _ => (None, Some("READ_FAILED")),
            }
        };
        let after = host.status();
        // Never attach an earlier Runtime generation's data to the current Runtime.
        let (linux, reason) =
            if before.generation != after.generation || before.observed != after.observed {
                (None, Some("RUNTIME_CHANGED"))
            } else {
                (linux, reason)
            };
        json!({"host":windows,"linux":linux,"linux_unavailable":reason,"runtime_generation":after.generation,"runtime_state":after.observed,"storage_scope":{"host":"HOST_STATE_VOLUME","linux":"MANAGED_PROJECTS_FILESYSTEM"}})
    }
}
