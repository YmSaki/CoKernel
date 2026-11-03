//! Desktop transport only: all domain decisions remain in Host/Runtime.
use cokernel_host::ipc_pipe::{PipeClient, default_name};
use cokernel_protocol::ResponseEnvelope;
use serde::Serialize;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Semaphore, watch};

#[derive(Clone, Debug, Serialize)]
pub struct Observation {
    pub connected: bool,
    pub revision: u64,
    pub error_code: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct ClientError {
    pub code: &'static str,
    pub summary: &'static str,
    pub outcome_unknown: bool,
}
fn unavailable(unknown: bool) -> ClientError {
    ClientError {
        code: if unknown {
            "CK-DESKTOP-OUTCOME-UNKNOWN"
        } else {
            "CK-DESKTOP-HOST"
        },
        summary: if unknown {
            "応答を確認できません。操作を再送せず、状態を更新してください。"
        } else {
            "Hostへ接続できません。CoKernel Hostの起動状態を確認してください。"
        },
        outcome_unknown: unknown,
    }
}

pub fn validate_call(method: &str, params: &Value) -> Result<(), ClientError> {
    // This is a presentation capability list, not a second domain validator.
    let permitted = matches!(
        method,
        "runtime.get_status"
            | "runtime.get_snapshot"
            | "resources.get_snapshot"
            | "remote_access.get_status"
            | "remote_access.get_configuration"
            | "remote_access.start"
            | "remote_access.stop"
            | "runtime.start"
            | "host.autostart.get"
            | "host.autostart.set"
            | "runtime.get_diagnostics"
            | "projects.list"
            | "projects.get"
            | "projects.create"
            | "projects.register"
            | "environment.get_status"
            | "sessions.list"
            | "notebooks.list"
            | "operations.get"
            | "operations.cancel"
    );
    if !permitted
        || !params.is_object()
        || serde_json::to_vec(params).map_or(true, |b| b.len() > 64 * 1024)
    {
        return Err(ClientError {
            code: "CK-DESKTOP-REQUEST",
            summary: "この画面からは呼び出せない操作、または入力上限を超えた要求です。",
            outcome_unknown: false,
        });
    }
    Ok(())
}

pub struct DesktopClient {
    name: String,
    observation: watch::Receiver<Observation>,
    slots: Arc<Semaphore>,
    observer: tokio::task::JoinHandle<()>,
}
impl DesktopClient {
    pub fn start() -> std::io::Result<Self> {
        Ok(Self::start_at(default_name()?))
    }
    fn start_at(name: String) -> Self {
        let (tx, observation) = watch::channel(Observation {
            connected: false,
            revision: 0,
            error_code: None,
        });
        let observer_name = name.clone();
        let observer = tokio::spawn(async move {
            loop {
                if let Ok(mut client) = PipeClient::connect(&observer_name).await {
                    // Only a completed Host snapshot establishes this subscription.
                    let first =
                        tokio::time::timeout(Duration::from_secs(10), client.next_event()).await;
                    if matches!(first, Ok(Ok(ref e)) if e.event == "host.snapshot" && e.protocol == 1)
                    {
                        tx.send_modify(|s| {
                            s.connected = true;
                            s.error_code = None;
                            s.revision = s.revision.wrapping_add(1);
                        });
                        while let Ok(event) = client.next_event().await {
                            if event.protocol != 1 {
                                break;
                            }
                            // Coalesce invalidations. Never queue notebook outputs in the WebView.
                            // Resync markers and reconnects use exactly the same refresh rule.
                            tx.send_modify(|s| s.revision = s.revision.wrapping_add(1));
                        }
                    }
                }
                tx.send_modify(|s| {
                    s.connected = false;
                    s.error_code = Some("CK-DESKTOP-HOST");
                    s.revision = s.revision.wrapping_add(1);
                });
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
        Self {
            name,
            observation,
            slots: Arc::new(Semaphore::new(8)),
            observer,
        }
    }
    pub fn observation(&self) -> Observation {
        self.observation.borrow().clone()
    }
    pub async fn call(&self, method: &str, params: Value) -> Result<ResponseEnvelope, ClientError> {
        validate_call(method, &params)?;
        let _slot = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| ClientError {
                code: "CK-DESKTOP-BUSY",
                summary: "画面の要求が処理中です。完了を待ってください。",
                outcome_unknown: false,
            })?;
        // Requests do not share a lock with the event subscription or slow operations.
        let mut client = PipeClient::connect(&self.name)
            .await
            .map_err(|_| unavailable(false))?;
        client
            .call(method, params)
            .await
            .map_err(|_| unavailable(true))
    }
}
impl Drop for DesktopClient {
    fn drop(&mut self) {
        self.observer.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cokernel_host::{
        bridge::{BridgeCommand, BridgePolicy},
        ipc_pipe::PipeListener,
        supervisor::{Host, RecoveryPolicy},
    };
    use serde_json::json;

    #[test]
    fn webview_cannot_reach_credentials_launch_or_arbitrary_methods() {
        for method in [
            "remote_access.configure",
            "bridge.hello",
            "runtime.launch",
            "sessions.execute_cell",
            "notebooks.import_file",
            "unknown",
        ] {
            assert!(validate_call(method, &json!({})).is_err());
        }
        assert!(validate_call("projects.list", &json!([])).is_err());
        assert!(validate_call("projects.create", &json!({"name":"x".repeat(65536)})).is_err());
        assert!(validate_call("projects.create", &json!({"name":"demo"})).is_ok());
    }
    async fn connected(client: &DesktopClient, desired: bool) {
        tokio::time::timeout(Duration::from_secs(12), async {
            while client.observation().connected != desired {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn reconnect_refreshes_and_ui_drop_leaves_host_alive() {
        let dir = std::env::temp_dir().join(format!("ck-desktop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        // Host stays STOPPED: this command must never launch in this test.
        let host = Host::start(
            &dir,
            BridgeCommand {
                program: "must-not-run".into(),
                args: vec![],
            },
            BridgePolicy::default(),
            RecoveryPolicy::default(),
        )
        .unwrap();
        let name = format!(
            r"\\.\pipe\cokernel-v1-desktop-test-{}",
            uuid::Uuid::new_v4()
        );
        let server = PipeListener::bind_at(&name).unwrap().serve(host.clone());
        let client = DesktopClient::start_at(name.clone());
        connected(&client, true).await;
        let revision = client.observation().revision;
        assert!(
            client
                .call("runtime.get_status", json!({}))
                .await
                .unwrap()
                .ok
        );
        server.shutdown().await.unwrap();
        connected(&client, false).await;
        let server = PipeListener::bind_at(&name).unwrap().serve(host.clone());
        connected(&client, true).await;
        assert!(client.observation().revision > revision);
        drop(client);
        let mut independent = PipeClient::connect(&name).await.unwrap();
        assert!(
            independent
                .call("runtime.get_status", json!({}))
                .await
                .unwrap()
                .ok
        );
        drop(independent);
        server.shutdown().await.unwrap();
        host.shutdown().await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
