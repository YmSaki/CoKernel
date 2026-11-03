use cokernel_host::{
    bridge::{Bridge, BridgeCommand, BridgePolicy},
    state::{DesiredState, StateStore},
    supervisor::{Host, ObservedState, RecoveryPolicy},
};
use serde_json::json;
use std::{path::PathBuf, time::Duration};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("ckhost-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn command(dir: &Directory, mode: &str) -> BridgeCommand {
    BridgeCommand {
        program: std::env::var_os("COKERNEL_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" })),
        args: vec![
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/runtime.py")
                .to_string_lossy()
                .into(),
            mode.into(),
            dir.0.to_string_lossy().into(),
        ],
    }
}
fn policy() -> BridgePolicy {
    BridgePolicy {
        handshake_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_secs(2),
        shutdown_timeout: Duration::from_millis(300),
    }
}
fn recovery() -> RecoveryPolicy {
    RecoveryPolicy {
        delays: vec![Duration::from_millis(60), Duration::from_millis(120)],
        stable_after: Duration::from_secs(60),
    }
}
async fn observed(host: &Host, wanted: ObservedState) {
    tokio::time::timeout(Duration::from_secs(6), async {
        let mut status = host.subscribe();
        loop {
            if status.borrow().observed == wanted {
                return;
            }
            status.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}
async fn exists(path: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn correlates_out_of_order_replies_and_separates_events_and_stderr() {
    let dir = Directory::new();
    let bridge = Bridge::connect(&command(&dir, "normal"), policy())
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        bridge.call("pair", json!({"caller":"a"})),
        bridge.call("pair", json!({"caller":"b"}))
    );
    assert_eq!(a.unwrap().result.unwrap()["caller"], "a");
    assert_eq!(b.unwrap().result.unwrap()["caller"], "b");
    let mut events = bridge.subscribe();
    bridge
        .call("event", json!({"session_id":"one"}))
        .await
        .unwrap();
    assert_eq!(events.recv().await.unwrap().payload["session_id"], "one");
    bridge.call("stderr", json!({})).await.unwrap();
    assert!(bridge.call("bridge.shutdown", json!({})).await.is_err());
    bridge.stop().await.unwrap();
    assert!(dir.0.join("exited").exists());
}

#[tokio::test]
async fn caller_cancellation_does_not_cancel_or_corrupt_connection() {
    let dir = Directory::new();
    let bridge = Bridge::connect(&command(&dir, "normal"), policy())
        .await
        .unwrap();
    let caller = bridge.clone();
    let task = tokio::spawn(async move { caller.call("slow", json!({"accepted":true})).await });
    exists(&dir.0.join("accepted")).await;
    task.abort();
    let _ = task.await;
    assert!(
        bridge
            .call("echo", json!({"later":true}))
            .await
            .unwrap()
            .result
            .unwrap()["later"]
            .as_bool()
            .unwrap()
    );
    bridge.stop().await.unwrap();
}

#[tokio::test]
async fn startup_events_precede_hello_and_preserve_sequence_across_handshake() {
    let dir = Directory::new();
    let bridge = Bridge::connect(&command(&dir, "early-events"), policy())
        .await
        .unwrap();
    let mut events = bridge.subscribe();
    bridge.call("event", json!({})).await.unwrap();
    assert_eq!(events.recv().await.unwrap().sequence, 3);
    bridge.stop().await.unwrap();
    let dir = Directory::new();
    let bridge = Bridge::connect(&command(&dir, "early-reset"), policy())
        .await
        .unwrap();
    assert!(bridge.call("event", json!({})).await.is_err());
}

#[tokio::test]
async fn startup_event_flood_bad_protocol_and_reordered_sequence_fail_closed() {
    for mode in ["early-flood", "early-protocol", "early-order"] {
        let dir = Directory::new();
        assert!(
            Bridge::connect(&command(&dir, mode), policy())
                .await
                .is_err(),
            "{mode}"
        );
    }
}

#[tokio::test]
async fn handshake_mismatch_oversize_and_timeout_fail_closed() {
    for mode in ["mismatch", "bad-protocol", "oversized", "hang"] {
        let dir = Directory::new();
        let mut limits = policy();
        if mode == "hang" {
            limits.handshake_timeout = Duration::from_millis(200);
        }
        let result = Bridge::connect(&command(&dir, mode), limits).await;
        assert!(result.is_err(), "{mode}");
        if matches!(mode, "mismatch" | "bad-protocol") {
            assert_eq!(result.err().unwrap().code, "CK-HOST-VERSION");
        }
    }
}

#[test]
fn bridge_launch_strips_host_credentials_and_wslenv() {
    if std::env::var_os("COKERNEL_HOST_ENV_CHILD").is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "bridge_launch_strips_host_credentials_and_wslenv",
            ])
            .env("COKERNEL_HOST_ENV_CHILD", "1")
            .env("COKERNEL_TEST_SECRET", "test-only-host-credential")
            .env("WSLENV", "COKERNEL_TEST_SECRET/u")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let dir = Directory::new();
        let bridge = Bridge::connect(&command(&dir, "normal"), policy())
            .await
            .unwrap();
        let environment = bridge
            .call("environment", json!({}))
            .await
            .unwrap()
            .result
            .unwrap();
        assert!(environment["WSLENV"].is_null());
        assert!(environment["COKERNEL_TEST_SECRET"].is_null());
        bridge.stop().await.unwrap();
    });
}

#[tokio::test]
async fn desired_state_save_failure_does_not_launch_runtime() {
    let dir = Directory::new();
    let host = Host::start(&dir.0, command(&dir, "normal"), policy(), recovery()).unwrap();
    std::fs::create_dir(dir.0.join("desired.json")).unwrap();
    assert_eq!(
        host.set_desired(DesiredState::Running)
            .await
            .unwrap_err()
            .code,
        "CK-HOST-STATE"
    );
    assert_eq!(host.status().desired, DesiredState::Stopped);
    assert!(!dir.0.join("starts").exists());
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn wrong_response_id_invalidates_connection() {
    let dir = Directory::new();
    let bridge = Bridge::connect(&command(&dir, "normal"), policy())
        .await
        .unwrap();
    assert!(bridge.call("wrong-id", json!({})).await.is_err());
    assert!(bridge.stop().await.is_err());
}

#[tokio::test]
async fn request_timeout_is_not_replayed_and_reports_unknown_outcome() {
    let dir = Directory::new();
    let mut limits = policy();
    limits.request_timeout = Duration::from_millis(100);
    let bridge = Bridge::connect(&command(&dir, "normal"), limits)
        .await
        .unwrap();
    let error = bridge.call("hang", json!({})).await.unwrap_err();
    assert_eq!(error.code, "CK-HOST-TIMEOUT");
    assert!(bridge.stop().await.is_err());
    assert_eq!(
        std::fs::read_to_string(dir.0.join("mutations"))
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        vec!["executed"]
    );
}

#[tokio::test]
async fn shutdown_failure_is_not_reported_as_clean_stop() {
    let dir = Directory::new();
    let bridge = Bridge::connect(&command(&dir, "bad-shutdown"), policy())
        .await
        .unwrap();
    assert_eq!(bridge.stop().await.unwrap_err().code, "CK-HOST-SHUTDOWN");
}

#[test]
fn desired_state_is_locked_atomic_and_reopened_without_guessing() {
    let dir = Directory::new();
    let (store, initial) = StateStore::open(&dir.0).unwrap();
    assert_eq!(initial, DesiredState::Stopped);
    assert!(StateStore::open(&dir.0).is_err());
    store.save(DesiredState::Running).unwrap();
    store.save(DesiredState::Stopped).unwrap();
    drop(store);
    let (store, desired) = StateStore::open(&dir.0).unwrap();
    assert_eq!(desired, DesiredState::Stopped);
    drop(store);
    std::fs::write(dir.0.join("desired.json"), b"invalid").unwrap();
    assert!(StateStore::open(&dir.0).is_err());
    std::fs::write(dir.0.join("desired.json"), vec![b' '; 4097]).unwrap();
    assert!(StateStore::open(&dir.0).is_err());
}

#[tokio::test]
async fn refused_stop_and_host_exit_keep_document_connection_available() {
    let dir = Directory::new();
    let host = Host::start(
        &dir.0,
        command(&dir, "refuse-shutdown"),
        policy(),
        recovery(),
    )
    .unwrap();
    host.set_desired(DesiredState::Running).await.unwrap();
    observed(&host, ObservedState::Healthy).await;
    assert_eq!(
        host.set_desired(DesiredState::Stopped)
            .await
            .unwrap_err()
            .code,
        "CK-HOST-SHUTDOWN-REFUSED"
    );
    assert_eq!(host.status().desired, DesiredState::Stopped);
    assert_eq!(
        host.shutdown().await.unwrap_err().code,
        "CK-HOST-SHUTDOWN-REFUSED"
    );
    assert!(StateStore::open(&dir.0).is_err());
    assert!(
        host.call("echo", json!({"unsaved":"retained"}))
            .await
            .unwrap()
            .result
            .unwrap()["unsaved"]
            == "retained"
    );
    host.call("allow-shutdown", json!({})).await.unwrap();
    host.set_desired(DesiredState::Stopped).await.unwrap();
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn stopped_desire_does_not_reconnect_after_refused_stop_then_crash() {
    let dir = Directory::new();
    let host = Host::start(
        &dir.0,
        command(&dir, "refuse-shutdown"),
        policy(),
        recovery(),
    )
    .unwrap();
    host.set_desired(DesiredState::Running).await.unwrap();
    observed(&host, ObservedState::Healthy).await;
    assert!(host.set_desired(DesiredState::Stopped).await.is_err());
    assert!(host.call("crash", json!({})).await.is_err());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(host.status().observed, ObservedState::Error);
    assert_eq!(host.status().generation, 1);
    assert_eq!(
        std::fs::read_to_string(dir.0.join("starts"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn host_exit_propagates_runtime_shutdown_failure() {
    let dir = Directory::new();
    let host = Host::start(&dir.0, command(&dir, "bad-shutdown"), policy(), recovery()).unwrap();
    host.set_desired(DesiredState::Running).await.unwrap();
    observed(&host, ObservedState::Healthy).await;
    assert_eq!(host.shutdown().await.unwrap_err().code, "CK-HOST-SHUTDOWN");
    assert_eq!(host.status().observed, ObservedState::Error);
    let (_store, desired) = StateStore::open(&dir.0).unwrap();
    assert_eq!(desired, DesiredState::Running);
}

#[tokio::test]
async fn oversized_request_is_rejected_without_losing_the_healthy_connection() {
    let dir = Directory::new();
    let bridge = Bridge::connect(&command(&dir, "normal"), policy())
        .await
        .unwrap();
    let error = bridge
        .call(
            "echo",
            json!({"large":"z".repeat(cokernel_protocol::DEFAULT_MAX_FRAME_BYTES)}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "CK-HOST-LIMIT");
    assert!(bridge.call("echo", json!({})).await.is_ok());
    bridge.stop().await.unwrap();
}

#[tokio::test]
async fn host_survives_client_drop_preserves_explicit_stop_and_resumes_running() {
    let dir = Directory::new();
    let host = Host::start(&dir.0, command(&dir, "normal"), policy(), recovery()).unwrap();
    assert_eq!(host.status().desired, DesiredState::Stopped);
    host.set_desired(DesiredState::Running).await.unwrap();
    observed(&host, ObservedState::Healthy).await;
    let client = host.clone();
    client.call("echo", json!({})).await.unwrap();
    drop(client);
    assert_eq!(host.status().generation, 1);
    assert!(host.call("echo", json!({})).await.is_ok());
    host.shutdown().await.unwrap();
    let host = Host::start(&dir.0, command(&dir, "normal"), policy(), recovery()).unwrap();
    observed(&host, ObservedState::Healthy).await;
    host.set_desired(DesiredState::Stopped).await.unwrap();
    assert_eq!(host.status().observed, ObservedState::Stopped);
    host.shutdown().await.unwrap();
    let starts = std::fs::read_to_string(dir.0.join("starts")).unwrap();
    let host = Host::start(&dir.0, command(&dir, "normal"), policy(), recovery()).unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(host.status().observed, ObservedState::Stopped);
    assert_eq!(
        std::fs::read_to_string(dir.0.join("starts")).unwrap(),
        starts
    );
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn recovery_is_bounded_and_version_mismatch_never_loops() {
    for (mode, expected) in [("exit", 3), ("mismatch", 1)] {
        let dir = Directory::new();
        let host = Host::start(&dir.0, command(&dir, mode), policy(), recovery()).unwrap();
        host.set_desired(DesiredState::Running).await.unwrap();
        observed(&host, ObservedState::Error).await;
        assert_eq!(host.status().consecutive_failures, expected);
        let starts = std::fs::read_to_string(dir.0.join("starts")).unwrap();
        assert_eq!(starts.lines().count(), expected as usize);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            std::fs::read_to_string(dir.0.join("starts")).unwrap(),
            starts
        );
        host.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn unexpected_exit_reconnects_with_a_new_generation_and_visible_evidence() {
    let dir = Directory::new();
    let host = Host::start(&dir.0, command(&dir, "normal"), policy(), recovery()).unwrap();
    host.set_desired(DesiredState::Running).await.unwrap();
    observed(&host, ObservedState::Healthy).await;
    assert!(host.call("crash", json!({})).await.is_err());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let state = host.status();
            if state.generation == 2 && state.observed == ObservedState::Healthy {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(host.status().last_error.is_some());
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn runtime_events_follow_bridge_generation_and_slow_subscribers_do_not_block_calls() {
    let dir = Directory::new();
    let host = Host::start(&dir.0, command(&dir, "normal"), policy(), recovery()).unwrap();
    let mut events = host.subscribe_events();
    for generation in 1..=2 {
        host.set_desired(DesiredState::Running).await.unwrap();
        observed(&host, ObservedState::Healthy).await;
        host.call("event", json!({"session_id":"shared"}))
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.generation, generation);
        assert_eq!(event.runtime_sequence, Some(1));
        assert_eq!(event.payload["session_id"], "shared");
        host.set_desired(DesiredState::Stopped).await.unwrap();
    }
    host.set_desired(DesiredState::Running).await.unwrap();
    observed(&host, ObservedState::Healthy).await;
    for _ in 0..300 {
        host.call("event", json!({})).await.unwrap();
    }
    assert!(matches!(
        events.recv().await,
        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))
    ));
    assert!(host.call("echo", json!({})).await.unwrap().ok);
    host.shutdown().await.unwrap();
}
