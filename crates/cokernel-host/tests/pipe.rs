#![cfg(windows)]
use cokernel_host::{
    bridge::{BridgeCommand, BridgePolicy},
    ipc_pipe::{PipeClient, PipeListener, read_json, write_json},
    state::DesiredState,
    supervisor::{Host, ObservedState, RecoveryPolicy},
};
use cokernel_protocol::{RequestEnvelope, ResponseEnvelope};
use serde_json::json;
use std::{path::PathBuf, time::Duration};
use tokio::{io::AsyncWriteExt, net::windows::named_pipe::ClientOptions};

#[test]
fn remote_cli_rejects_extra_arguments_without_echo_or_opening_input() {
    let private = "synthetic-key-must-not-enter-argv";
    for action in ["configure", "status", "start", "stop", "restart", "clear"] {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_cokernel-host"))
            .args(["remote", action, private])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stdout).contains(private));
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(!stderr.contains(private));
        assert!(stderr.contains("no extra arguments"));
    }
}

struct Fixture {
    directory: PathBuf,
    host: Host,
    name: String,
    server: Option<cokernel_host::ipc_pipe::PipeServer>,
}
impl Fixture {
    fn command(directory: &std::path::Path, mode: &str) -> BridgeCommand {
        BridgeCommand {
            program: PathBuf::from("python"),
            args: vec![
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/runtime.py")
                    .to_string_lossy()
                    .into(),
                mode.into(),
                directory.to_string_lossy().into(),
            ],
        }
    }
    async fn restart_host(&mut self) {
        self.server.take().unwrap().shutdown().await.unwrap();
        self.host.shutdown().await.unwrap();
        self.host = Host::start(
            &self.directory,
            Self::command(&self.directory, "normal"),
            BridgePolicy::default(),
            RecoveryPolicy::default(),
        )
        .unwrap();
        self.server = Some(
            PipeListener::bind_at(&self.name)
                .unwrap()
                .serve(self.host.clone()),
        );
    }
    fn new() -> Self {
        Self::with_mode("normal")
    }
    fn with_mode(mode: &str) -> Self {
        let directory = std::env::temp_dir().join(format!("ckpipe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let command = Self::command(&directory, mode);
        let host = Host::start(
            &directory,
            command,
            BridgePolicy::default(),
            RecoveryPolicy::default(),
        )
        .unwrap();
        let name = format!(r"\\.\pipe\cokernel-v1-test-{}", uuid::Uuid::new_v4());
        let server = PipeListener::bind_at(&name).unwrap().serve(host.clone());
        Self {
            directory,
            host,
            name,
            server: Some(server),
        }
    }
    async fn finish(mut self) {
        self.server.take().unwrap().shutdown().await.unwrap();
        self.host.shutdown().await.unwrap();
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
    async fn ready(&self) {
        self.host.set_desired(DesiredState::Running).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut state = self.host.subscribe();
            loop {
                if state.borrow().observed == ObservedState::Healthy {
                    break;
                }
                state.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn unavailable_linux_metrics_do_not_hide_host_data_or_block_runtime_control() {
    for (mode, reason) in [
        ("metrics-silent", "TIMED_OUT"),
        ("metrics-invalid", "INVALID_DATA"),
        ("normal", "NOT_SUPPORTED"),
    ] {
        let fixture = Fixture::with_mode(mode);
        fixture.ready().await;
        let mut client = PipeClient::connect(&fixture.name).await.unwrap();
        let reply = tokio::time::timeout(
            Duration::from_secs(2),
            client.call("resources.get_snapshot", json!({})),
        )
        .await
        .unwrap()
        .unwrap();
        let result = reply.result.unwrap();
        assert_eq!(result["linux_unavailable"], reason);
        assert!(result["linux"].is_null());
        assert!(result["host"].is_object());
        assert_eq!(
            client
                .call("runtime.get_status", json!({}))
                .await
                .unwrap()
                .result
                .unwrap()["observed"],
            "HEALTHY"
        );
        drop(client);
        fixture.finish().await;
    }
}

#[tokio::test]
async fn tunnel_credentials_stay_in_windows_store_and_never_echo_over_pipe() {
    use cokernel_host::secrets::{SecretStore, WindowsSecretStore};
    struct Cleanup(WindowsSecretStore);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.clear();
        }
    }
    let fixture = Fixture::new();
    let mut cleanup = Cleanup(WindowsSecretStore::for_state(&fixture.directory).unwrap());
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    let key = "test-only-pipe-runtime-key-12345";
    let tunnel = "tunnel_0123456789abcdef0123456789abcdef";
    let initial = client
        .call("remote_access.get_configuration", json!({}))
        .await
        .unwrap();
    assert_eq!(
        initial.result.unwrap(),
        json!({"configured":false,"tunnel_id":null})
    );
    let credentials =
        cokernel_host::secrets::TunnelCredentials::new(tunnel.into(), key.as_bytes().to_vec())
            .unwrap();
    let saved = client.configure_remote(&credentials).await.unwrap();
    drop(credentials);
    assert!(!serde_json::to_string(&saved).unwrap().contains(key));
    assert_eq!(
        serde_json::to_value(saved).unwrap(),
        json!({"configured":true,"tunnel_id":tunnel})
    );
    drop(client);
    let mut reconnected = PipeClient::connect(&fixture.name).await.unwrap();
    assert!(
        !serde_json::to_string(&reconnected.next_event().await.unwrap())
            .unwrap()
            .contains(key)
    );
    let read = reconnected
        .call("remote_access.get_configuration", json!({}))
        .await
        .unwrap();
    assert!(read.ok && read.result.unwrap()["configured"] == true);
    assert_eq!(
        cleanup.0.load().unwrap().unwrap().key_for_tunnel(),
        key.as_bytes()
    );
    for (method, params) in [
        (
            "remote_access.configure",
            json!({"tunnel_id":tunnel,"runtime_key":key,"path":"C:/other"}),
        ),
        (
            "remote_access.configure",
            json!({"tunnel_id":"bad","runtime_key":key}),
        ),
        (
            "remote_access.configure",
            json!({"tunnel_id":tunnel,"runtime_key":null}),
        ),
        (
            "remote_access.get_configuration",
            json!({"include_key":true}),
        ),
        (
            "remote_access.clear_configuration",
            json!({"target":"other"}),
        ),
        ("remote_access.load_for_tunnel", json!({})),
    ] {
        let response = reconnected.call(method, params).await.unwrap();
        assert!(!response.ok);
        assert!(!serde_json::to_string(&response).unwrap().contains(key));
    }
    assert!(
        cleanup.0.load().unwrap().is_some(),
        "invalid mutation replaced valid credential"
    );
    assert!(
        !fixture.directory.join("starts").exists(),
        "secret configuration started Runtime"
    );
    for file in std::fs::read_dir(&fixture.directory).unwrap() {
        let file = file.unwrap();
        if file.file_name() != "host.lock" && file.file_type().unwrap().is_file() {
            assert!(!String::from_utf8_lossy(&std::fs::read(file.path()).unwrap()).contains(key));
        }
    }
    let cleared = reconnected
        .call("remote_access.clear_configuration", json!({}))
        .await
        .unwrap();
    assert!(cleared.ok && cleared.result.unwrap()["configured"] == false);
    assert!(cleanup.0.load().unwrap().is_none());
    assert!(
        reconnected
            .call("remote_access.clear_configuration", json!({}))
            .await
            .unwrap()
            .ok
    );
    drop(reconnected);
    fixture.finish().await;
}

async fn remote_until(
    client: &mut PipeClient,
    predicate: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let response = client
                .call("remote_access.get_status", json!({}))
                .await
                .unwrap();
            assert!(response.ok);
            let value = response.result.unwrap();
            if predicate(&value) {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("remote status did not converge")
}

#[tokio::test]
async fn remote_intent_handoff_rotation_and_host_runtime_recovery_share_one_owner() {
    use cokernel_host::secrets::{SecretStore, WindowsSecretStore};
    use sha2::{Digest, Sha256};
    struct Cleanup(WindowsSecretStore);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.clear();
        }
    }
    let mut fixture = Fixture::new();
    let mut cleanup = Cleanup(WindowsSecretStore::for_state(&fixture.directory).unwrap());
    let key1 = "synthetic-host-handoff-key-one";
    let key2 = "synthetic-host-handoff-key-two";
    let tunnel = "tunnel_0123456789abcdef0123456789abcdef";
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    assert!(
        !client
            .call("remote_access.start", json!({}))
            .await
            .unwrap()
            .ok
    );
    assert!(
        client
            .call(
                "remote_access.configure",
                json!({"tunnel_id":tunnel,"runtime_key":key1})
            )
            .await
            .unwrap()
            .ok
    );
    assert!(
        client
            .call("remote_access.start", json!({}))
            .await
            .unwrap()
            .ok
    );
    remote_until(&mut client, |s| {
        s["desired_enabled"] == true && s["state"] == "WAITING_FOR_RUNTIME"
    })
    .await;
    assert!(
        !fixture.directory.join("starts").exists(),
        "remote enable silently started stopped Runtime"
    );
    fixture.ready().await;
    remote_until(&mut client, |s| s["state"] == "CONNECTED").await;
    let read = || -> Vec<serde_json::Value> {
        std::fs::read_to_string(fixture.directory.join("handoffs.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    };
    assert_eq!(read().len(), 1);
    assert!(
        client
            .call("remote_access.start", json!({}))
            .await
            .unwrap()
            .ok
    );
    assert_eq!(
        read().len(),
        1,
        "unchanged enable caused another credential handoff"
    );
    assert!(
        client
            .call(
                "remote_access.configure",
                json!({"tunnel_id":tunnel,"runtime_key":key2})
            )
            .await
            .unwrap()
            .ok
    );
    remote_until(&mut client, |s| {
        s["state"] == "CONNECTED" && s["tunnel_generation"] == 2
    })
    .await;
    assert_eq!(read().len(), 2);
    let expected = format!("{:x}", Sha256::digest(key2));
    assert_eq!(read()[1]["fingerprint"], expected);
    fixture.host.call("crash", json!({})).await.unwrap_err();
    remote_until(&mut client, |s| {
        s["state"] == "CONNECTED" && s["runtime_generation"] == 2
    })
    .await;
    assert_eq!(read().last().unwrap()["fingerprint"], expected);
    fixture
        .host
        .set_desired(DesiredState::Stopped)
        .await
        .unwrap();
    remote_until(&mut client, |s| s["state"] == "WAITING_FOR_RUNTIME").await;
    fixture.ready().await;
    remote_until(&mut client, |s| {
        s["state"] == "CONNECTED" && s["runtime_generation"] == 3
    })
    .await;
    assert_eq!(read().last().unwrap()["fingerprint"], expected);
    drop(client);
    fixture.restart_host().await;
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    remote_until(&mut client, |s| {
        s["state"] == "CONNECTED" && s["runtime_generation"] == 1
    })
    .await;
    let latest: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(fixture.directory.join("handoffs.jsonl"))
            .unwrap()
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        latest["fingerprint"], expected,
        "Host restart did not load latest Windows credential"
    );
    assert!(
        client
            .call("remote_access.restart", json!({}))
            .await
            .unwrap()
            .ok
    );
    remote_until(&mut client, |s| {
        s["state"] == "CONNECTED" && s["tunnel_generation"] == 2
    })
    .await;
    assert!(
        client
            .call("remote_access.stop", json!({}))
            .await
            .unwrap()
            .ok
    );
    remote_until(&mut client, |s| {
        s["state"] == "DISABLED" && s["desired_enabled"] == false
    })
    .await;
    assert!(
        client
            .call("remote_access.clear_configuration", json!({}))
            .await
            .unwrap()
            .ok
    );
    assert!(cleanup.0.load().unwrap().is_none());
    for file in std::fs::read_dir(&fixture.directory).unwrap() {
        let file = file.unwrap();
        if file.file_name() != "host.lock" && file.file_type().unwrap().is_file() {
            let bytes = std::fs::read(file.path()).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            assert!(!text.contains(key1) && !text.contains(key2));
        }
    }
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn remote_clear_preserves_credential_until_cleanup_is_confirmed_and_local_control_stays_responsive()
 {
    use cokernel_host::secrets::{SecretStore, WindowsSecretStore};
    struct Cleanup(WindowsSecretStore);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.clear();
        }
    }
    let fixture = Fixture::with_mode("tunnel-stop-pending");
    let mut cleanup = Cleanup(WindowsSecretStore::for_state(&fixture.directory).unwrap());
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    assert!(
        client
            .call(
                "remote_access.configure",
                json!({"tunnel_id":"tunnel_test","runtime_key":"synthetic-pending-cleanup-key"})
            )
            .await
            .unwrap()
            .ok
    );
    fixture.ready().await;
    assert!(
        client
            .call("remote_access.start", json!({}))
            .await
            .unwrap()
            .ok
    );
    remote_until(&mut client, |s| s["state"] == "CONNECTED").await;
    let clearing = tokio::spawn(async move {
        client
            .call("remote_access.clear_configuration", json!({}))
            .await
            .unwrap()
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut independent = PipeClient::connect(&fixture.name).await.unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(800),
            independent.call("runtime.get_status", json!({}))
        )
        .await
        .unwrap()
        .unwrap()
        .ok
    );
    let reply = clearing.await.unwrap();
    assert!(!reply.ok);
    assert_eq!(reply.error.unwrap().code, "CK-REMOTE-CLEANUP");
    assert!(cleanup.0.load().unwrap().is_some());
    let intent: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fixture.directory.join("remote-access.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(intent["enabled"], false);
    assert!(
        independent
            .call(
                "remote_access.start",
                json!({"runtime_key":"must-not-echo"})
            )
            .await
            .unwrap()
            .error
            .is_some()
    );
    drop(independent);
    fixture.finish().await;
}

#[tokio::test]
async fn remote_rejects_false_connected_observations_and_explicit_reconnect_recovers() {
    use cokernel_host::secrets::{SecretStore, WindowsSecretStore};
    struct Cleanup(WindowsSecretStore);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.clear();
        }
    }
    let fixture = Fixture::with_mode("tunnel-malformed");
    let _cleanup = Cleanup(WindowsSecretStore::for_state(&fixture.directory).unwrap());
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    assert!(client.call("remote_access.configure",json!({"tunnel_id":"tunnel_invalid_status","runtime_key":"synthetic-status-proof-key"})).await.unwrap().ok);
    fixture.ready().await;
    assert!(
        client
            .call("remote_access.start", json!({}))
            .await
            .unwrap()
            .ok
    );
    let failed = remote_until(&mut client, |s| s["state"] == "ERROR").await;
    assert_eq!(failed["error_code"], "CK-REMOTE-PROTOCOL");
    assert!(
        !serde_json::to_string(&failed)
            .unwrap()
            .contains("synthetic-status-proof-key")
    );
    fixture
        .host
        .call("allow-shutdown", json!({}))
        .await
        .unwrap();
    assert!(
        client
            .call("remote_access.restart", json!({}))
            .await
            .unwrap()
            .ok
    );
    remote_until(&mut client, |s| s["state"] == "CONNECTED").await;
    assert!(
        client
            .call("remote_access.clear_configuration", json!({}))
            .await
            .unwrap()
            .ok
    );
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn resources_are_available_when_runtime_is_stopped_and_reject_client_paths() {
    let fixture = Fixture::new();
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    let invalid = client
        .call(
            "resources.get_snapshot",
            json!({"path":"C:\\secret","command":"other"}),
        )
        .await
        .unwrap();
    assert_eq!(invalid.error.unwrap().code, "CK-HOST-REQUEST");
    let first = client
        .call("resources.get_snapshot", json!({}))
        .await
        .unwrap()
        .result
        .unwrap();
    assert!(first["linux"].is_null());
    assert_eq!(first["linux_unavailable"], "RUNTIME_UNAVAILABLE");
    assert_eq!(first["runtime_state"], "STOPPED");
    assert_eq!(first["runtime_generation"], 0);
    assert!(!fixture.directory.join("starts").exists());
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn autostart_rejects_command_injection_and_requires_trusted_launch_binding() {
    let fixture = Fixture::new();
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    for (method, params) in [
        (
            "host.autostart.set",
            json!({"enabled":true,"executable":"C:\\other.exe"}),
        ),
        ("host.autostart.set", json!({"enabled":"true"})),
        ("host.autostart.get", json!({"state":"C:\\other"})),
    ] {
        let reply = client.call(method, params).await.unwrap();
        assert_eq!(reply.error.unwrap().code, "CK-HOST-REQUEST");
    }
    let reply = client
        .call("host.autostart.set", json!({"enabled":true}))
        .await
        .unwrap();
    assert_eq!(reply.error.unwrap().code, "CK-HOST-AUTOSTART");
    assert_eq!(fixture.host.status().desired, DesiredState::Stopped);
    assert_eq!(fixture.host.status().generation, 0);
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn current_user_handshake_snapshot_and_control_survive_reconnect() {
    let fixture = Fixture::new();
    assert!(
        PipeListener::bind_at(&fixture.name).is_err(),
        "another server claimed the pipe"
    );
    let mut first = PipeClient::connect(&fixture.name).await.unwrap();
    assert_eq!(first.next_event().await.unwrap().event, "host.snapshot");
    assert!(first.call("runtime.start", json!({})).await.unwrap().ok);
    fixture.ready().await;
    let generation = fixture.host.status().generation;
    let reply = first
        .call("projects.list", json!({"caller":"first"}))
        .await
        .unwrap();
    assert_eq!(reply.result.unwrap()["caller"], "first");
    drop(first);
    let mut second = PipeClient::connect(&fixture.name).await.unwrap();
    assert_eq!(
        second
            .call("runtime.get_status", json!({}))
            .await
            .unwrap()
            .result
            .unwrap()["generation"],
        generation
    );
    assert!(second.call("projects.list", json!({})).await.unwrap().ok);
    assert_eq!(fixture.host.status().generation, generation);
    drop(second);
    fixture.finish().await;
}

#[tokio::test]
async fn protocol_and_internal_methods_cannot_bypass_host_admission() {
    let fixture = Fixture::new();
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    for method in [
        "bridge.shutdown",
        "bridge.hello",
        "runtime.install_external_credential_handoff",
        "unknown",
    ] {
        let reply = client.call(method, json!({})).await.unwrap();
        assert!(!reply.ok);
        assert_eq!(reply.error.unwrap().code, "CK-HOST-NO-METHOD");
    }
    let reply = client
        .call("runtime.start", json!({"program":"other.exe"}))
        .await
        .unwrap();
    assert!(!reply.ok);
    assert_eq!(fixture.host.status().desired, DesiredState::Stopped);
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn malformed_and_oversized_clients_do_not_stop_other_clients() {
    let fixture = Fixture::new();
    for data in [u32::MAX.to_be_bytes().to_vec(), vec![0, 0, 0, 1, b'{']] {
        let mut bad = ClientOptions::new().open(&fixture.name).unwrap();
        bad.write_all(&data).await.unwrap();
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            read_json::<_, serde_json::Value>(&mut bad),
        )
        .await
        .unwrap();
        assert!(response.is_err());
        let mut good = PipeClient::connect(&fixture.name).await.unwrap();
        assert!(good.call("runtime.get_status", json!({})).await.unwrap().ok);
    }
    fixture.finish().await;
}

#[tokio::test]
async fn same_request_id_on_distinct_connections_is_correlated_independently() {
    let fixture = Fixture::new();
    fixture.ready().await;
    let exchange = |marker: &'static str| {
        let name = fixture.name.clone();
        async move {
            let mut pipe = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    match ClientOptions::new().open(&name) {
                        Ok(pipe) => break pipe,
                        Err(error) if error.raw_os_error() == Some(231) => {
                            tokio::time::sleep(Duration::from_millis(5)).await
                        }
                        Err(error) => panic!("{error}"),
                    }
                }
            })
            .await
            .unwrap();
            write_json(
                &mut pipe,
                &RequestEnvelope {
                    protocol: 1,
                    request_id: "hello".into(),
                    method: "host.hello".into(),
                    params: json!({"supported_protocols":[1]}),
                },
            )
            .await
            .unwrap();
            let hello: ResponseEnvelope = read_json(&mut pipe).await.unwrap();
            assert!(hello.ok);
            let _: cokernel_protocol::EventEnvelope = read_json(&mut pipe).await.unwrap();
            write_json(
                &mut pipe,
                &RequestEnvelope {
                    protocol: 1,
                    request_id: "same-client-id".into(),
                    method: "projects.list".into(),
                    params: json!({"caller":marker}),
                },
            )
            .await
            .unwrap();
            loop {
                let value: serde_json::Value = read_json(&mut pipe).await.unwrap();
                if value.get("event").is_none() {
                    assert_eq!(value["request_id"], "same-client-id");
                    assert_eq!(value["result"]["caller"], marker);
                    break;
                }
            }
        }
    };
    tokio::join!(exchange("a"), exchange("b"));
    fixture.finish().await;
}

#[tokio::test]
async fn cancelled_rpc_poisons_client_but_accepted_work_outlives_connection() {
    let fixture = Fixture::new();
    fixture.ready().await;
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    let accepted = fixture.directory.join("accepted");
    {
        let call = client.call("packages.sync", json!({"test_slow":true}));
        tokio::pin!(call);
        tokio::select! {
            result = &mut call => panic!("test must cancel after admission: {result:?}"),
            _ = tokio::time::timeout(Duration::from_secs(5), async {
                while !accepted.exists() {tokio::time::sleep(Duration::from_millis(5)).await;}
            }) => {}
        }
    }
    assert!(accepted.exists(), "test did not reach Runtime admission");
    assert!(
        client
            .call("projects.list", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("reconnect")
    );
    drop(client);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fixture.directory.join("completed").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let generation = fixture.host.status().generation;
    let mut reconnected = PipeClient::connect(&fixture.name).await.unwrap();
    assert!(
        reconnected
            .call("projects.list", json!({}))
            .await
            .unwrap()
            .ok
    );
    assert_eq!(generation, fixture.host.status().generation);
    drop(reconnected);
    fixture.finish().await;
}

#[tokio::test]
async fn failed_import_cancels_its_transaction_without_forwarding_windows_paths() {
    let fixture = Fixture::with_mode("import-refuse");
    fixture.ready().await;
    let path = fixture.directory.join("notebook.ipynb");
    let bytes = b"{}";
    std::fs::write(&path, bytes).unwrap();
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    let project = uuid::Uuid::new_v4();
    let response = client
        .call(
            "notebooks.import_file",
            json!({"project_id":project,"target_path":"target.ipynb","windows_path":path}),
        )
        .await
        .unwrap();
    assert_eq!(response.error.unwrap().code, "CK-NB-IMPORT");
    let requests: Vec<serde_json::Value> =
        std::fs::read_to_string(fixture.directory.join("import-requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert_eq!(
        requests
            .iter()
            .map(|r| r["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "notebooks.begin_import",
            "notebooks.import_chunk",
            "notebooks.cancel_import"
        ]
    );
    assert_eq!(requests[0]["params"]["size_bytes"], 2);
    assert_eq!(requests[1]["params"]["offset"], 0);
    assert_eq!(requests[1]["params"]["data_base64"], "e30=");
    assert_eq!(
        requests[1]["params"]["import_id"],
        requests[2]["params"]["import_id"]
    );
    for request in &requests {
        assert!(request["params"].get("windows_path").is_none());
        assert_eq!(request["params"]["project_id"], json!(project));
    }
    // Failure releases the read handle and does not disturb Host control.
    std::fs::write(&path, b"changed").unwrap();
    assert!(
        client
            .call("runtime.get_status", json!({}))
            .await
            .unwrap()
            .ok
    );
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn cancelled_event_read_requires_a_fresh_connection() {
    let fixture = Fixture::new();
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    let snapshot = client.next_event().await.unwrap();
    assert_eq!(snapshot.event, "host.snapshot");
    // Remote intent is loaded asynchronously. Establish an idle connection after
    // that legitimate startup event, then cancel an actually pending read.
    if snapshot.payload["remote_access"]["state"] == "LOADING" {
        let event = tokio::time::timeout(Duration::from_secs(2), client.next_event())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.event, "remote_access.state_changed");
        assert_eq!(event.payload["state"], "DISABLED");
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(20), client.next_event())
            .await
            .is_err()
    );
    assert!(
        client
            .call("runtime.get_status", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("reconnect")
    );
    drop(client);
    fixture.finish().await;
}

async fn raw_client(name: &str) -> tokio::net::windows::named_pipe::NamedPipeClient {
    let mut pipe = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match ClientOptions::new().open(name) {
                Ok(pipe) => break pipe,
                Err(error) if error.raw_os_error() == Some(231) => {
                    tokio::time::sleep(Duration::from_millis(5)).await
                }
                Err(error) => panic!("{error}"),
            }
        }
    })
    .await
    .unwrap();
    write_json(
        &mut pipe,
        &RequestEnvelope {
            protocol: 1,
            request_id: "hello".into(),
            method: "host.hello".into(),
            params: json!({"supported_protocols":[1]}),
        },
    )
    .await
    .unwrap();
    let hello: ResponseEnvelope = read_json(&mut pipe).await.unwrap();
    assert!(hello.ok);
    let _: cokernel_protocol::EventEnvelope = read_json(&mut pipe).await.unwrap();
    pipe
}

#[tokio::test]
async fn duplicate_id_is_confined_to_its_connection() {
    let fixture = Fixture::new();
    let mut pipe = raw_client(&fixture.name).await;
    write_json(
        &mut pipe,
        &RequestEnvelope {
            protocol: 1,
            request_id: "hello".into(),
            method: "runtime.get_status".into(),
            params: json!({}),
        },
    )
    .await
    .unwrap();
    // State notifications already queued before disconnect are legal. A reply
    // to the duplicate request, or a connection that stays open, is not.
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Ok(frame) = read_json::<_, serde_json::Value>(&mut pipe).await {
            serde_json::from_value::<cokernel_protocol::EventEnvelope>(frame)
                .expect("duplicate request received a non-event frame");
        }
    })
    .await
    .expect("duplicate request did not close its connection");
    let mut good = PipeClient::connect(&fixture.name).await.unwrap();
    assert!(good.call("runtime.get_status", json!({})).await.unwrap().ok);
    drop(good);
    drop(pipe);
    fixture.finish().await;
}

#[tokio::test]
async fn ninth_inflight_request_is_rejected_without_cancelling_eight_accepted_requests() {
    let fixture = Fixture::new();
    fixture.ready().await;
    let mut pipe = raw_client(&fixture.name).await;
    for index in 0..9 {
        write_json(
            &mut pipe,
            &RequestEnvelope {
                protocol: 1,
                request_id: format!("r{index}"),
                method: "packages.sync".into(),
                params: json!({"test_hold":true}),
            },
        )
        .await
        .unwrap();
    }
    let rejected = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let value: serde_json::Value = read_json(&mut pipe).await.unwrap();
            if value.get("event").is_none() {
                break value;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(rejected["request_id"], "r8");
    assert_eq!(rejected["error"]["code"], "CK-HOST-BUSY");
    std::fs::write(fixture.directory.join("release"), b"yes").unwrap();
    let mut replies = std::collections::HashSet::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        while replies.len() < 8 {
            let value: serde_json::Value = read_json(&mut pipe).await.unwrap();
            if value.get("event").is_some() {
                continue;
            }
            assert_eq!(value["ok"], true);
            assert!(replies.insert(value["request_id"].as_str().unwrap().to_owned()));
        }
    })
    .await
    .unwrap();
    assert_eq!(replies, (0..8).map(|i| format!("r{i}")).collect());
    drop(pipe);
    fixture.finish().await;
}

#[tokio::test]
async fn draining_client_receives_a_burst_without_disconnect_or_response_interleaving() {
    let fixture = Fixture::new();
    fixture.ready().await;
    let mut client = PipeClient::connect(&fixture.name).await.unwrap();
    let response = client
        .call("projects.list", json!({"test_events":true}))
        .await
        .unwrap();
    assert_eq!(response.result.unwrap()["events"], 64);
    let mut found = Vec::new();
    let mut sequence = 0;
    tokio::time::timeout(Duration::from_secs(3), async {
        while found.len() < 64 {
            let event = client.next_event().await.unwrap();
            if event.event != "session.output" {
                continue;
            }
            assert!(event.sequence > sequence);
            sequence = event.sequence;
            assert_eq!(
                event.payload["generation"],
                fixture.host.status().generation
            );
            found.push(event.payload["data"]["index"].as_u64().unwrap());
        }
    })
    .await
    .unwrap();
    assert_eq!(found, (0..64).collect::<Vec<_>>());
    assert!(
        client
            .call("runtime.get_status", json!({}))
            .await
            .unwrap()
            .ok
    );
    drop(client);
    fixture.finish().await;
}
