#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use cokernel_domain::{FailureRecord, FailureTrigger, NotebookId, Project, ProjectId, SessionState};
use cokernel_runtime::session::{SessionEvent, SessionSupervisor, SessionSupervisorConfig};
use tokio::sync::broadcast;
use tokio::time::timeout;

struct TempWorkspace(PathBuf);

impl TempWorkspace {
    fn new(label: &str) -> Result<Self> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before UNIX_EPOCH")?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cokernel-session-fake-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self(path))
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const FAKE_UV: &str = r#"#!/usr/bin/env python3
import json
import os
import socket
import struct
import sys
import time


def arg_after(flag):
    index = sys.argv.index(flag)
    return sys.argv[index + 1]


def recv_exact(sock, size):
    data = bytearray()
    while len(data) < size:
        chunk = sock.recv(size - len(data))
        if not chunk:
            raise RuntimeError("socket closed mid-frame")
        data.extend(chunk)
    return bytes(data)


def recv_frame(sock):
    size = struct.unpack(">I", recv_exact(sock, 4))[0]
    return json.loads(recv_exact(sock, size))


def send_frame(sock, message):
    payload = json.dumps(message, separators=(",", ":")).encode("utf-8")
    sock.sendall(struct.pack(">I", len(payload)) + payload)


socket_path = arg_after("--socket")
session_id = arg_after("--session-id")
mode = os.path.basename(arg_after("--project"))

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(socket_path)
send_frame(
    sock,
    {
        "protocol": 1,
        "type": "event",
        "session_id": session_id,
        "event": "ready",
        "payload": {
            "worker_version": "fake-1",
            "python_version": "fake-3",
            "ipython_version": "fake-9",
            "pid": os.getpid(),
            "heartbeat_interval_ms": 50,
        },
    },
)

handshake = recv_frame(sock)
assert handshake["method"] == "handshake"
send_frame(
    sock,
    {
        "protocol": 1,
        "type": "response",
        "id": handshake["id"],
        "session_id": session_id,
        "ok": True,
        "result": {
            "protocol": 1,
            "worker_version": "fake-1",
            "capabilities": [
                "execute",
                "inspect_variables",
                "get_variable",
                "reset",
                "shutdown",
            ],
            "heartbeat_interval_ms": 50,
            "output_limits": {
                "max_event_bytes": 1024,
                "max_operation_bytes": 4096,
                "max_blob_bytes": 512,
            },
        },
    },
)

if mode == "heartbeat-timeout":
    time.sleep(30)
elif mode == "healthy":
    sequence = 1
    while True:
        send_frame(
            sock,
            {
                "protocol": 1,
                "type": "event",
                "session_id": session_id,
                "event": "heartbeat",
                "payload": {"monotonic_ns": sequence},
            },
        )
        sequence += 1
        time.sleep(0.05)
elif mode == "protocol-violation":
    # Give the Rust caller time to subscribe after ensure_primary returns.
    time.sleep(0.35)
    send_frame(
        sock,
        {
            "protocol": 1,
            "type": "response",
            "id": "stale-response",
            "session_id": session_id,
            "ok": True,
            "result": {"pong": True},
        },
    )
    time.sleep(30)
else:
    raise RuntimeError(f"unsupported fake worker mode: {mode}")
"#;

fn write_fake_uv(root: &Path) -> Result<PathBuf> {
    let path = root.join("fake-uv.py");
    fs::write(&path, FAKE_UV).with_context(|| format!("failed to write {}", path.display()))?;
    let mut permissions = fs::metadata(&path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&path, permissions)?;
    Ok(path)
}

fn project(root: &Path) -> Result<Project> {
    fs::create_dir_all(root)?;
    Ok(Project {
        project_id: ProjectId::new(),
        name: root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("fake-worker-test")
            .to_owned(),
        root_path: root.to_string_lossy().into_owned(),
        environment_generation: 1,
    })
}

fn supervisor(
    temp: &TempWorkspace,
    heartbeat_timeout: Duration,
) -> Result<SessionSupervisor> {
    let fake_uv = write_fake_uv(&temp.0)?;
    let mut config =
        SessionSupervisorConfig::development("fake-worker-package", temp.0.join("sockets"));
    config.uv_executable = fake_uv;
    config.startup_timeout = Duration::from_secs(2);
    config.shutdown_timeout = Duration::from_millis(200);
    config.heartbeat_timeout = heartbeat_timeout;
    Ok(SessionSupervisor::new(config))
}

async fn wait_failure(
    events: &mut broadcast::Receiver<SessionEvent>,
) -> Result<FailureRecord> {
    timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await {
                Ok(SessionEvent::Failure { record }) => return Ok(record),
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    anyhow::bail!("Session event stream closed before Failure")
                }
            }
        }
    })
    .await
    .context("timed out waiting for Session Failure")?
}

#[tokio::test]
async fn heartbeat_timeout_crashes_only_the_stalled_session_with_failure_evidence() -> Result<()> {
    let temp = TempWorkspace::new("heartbeat")?;
    let healthy_project = project(&temp.0.join("healthy"))?;
    let stalled_project = project(&temp.0.join("heartbeat-timeout"))?;
    let supervisor = supervisor(&temp, Duration::from_millis(350))?;

    let healthy = supervisor
        .ensure_primary(&healthy_project, NotebookId::new())
        .await?;
    let stalled = supervisor
        .ensure_primary(&stalled_project, NotebookId::new())
        .await?;
    let mut events = stalled.subscribe();

    let record = wait_failure(&mut events).await?;

    assert_eq!(stalled.state(), SessionState::Crashed);
    assert_eq!(healthy.state(), SessionState::Idle);
    assert_eq!(record.session_id, Some(stalled.session_id()));
    assert_eq!(record.operation_id, None);
    assert_eq!(
        record
            .runtime_event_context
            .as_ref()
            .map(|context| context.trigger),
        Some(FailureTrigger::HeartbeatTimeout)
    );
    assert!(record.worker_pid.is_some());
    assert_eq!(record.environment_generation, Some(1));
    Ok(())
}

#[tokio::test]
async fn unexpected_idle_transaction_frame_fails_closed_as_worker_protocol_crash() -> Result<()> {
    let temp = TempWorkspace::new("protocol")?;
    let project = project(&temp.0.join("protocol-violation"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let mut events = session.subscribe();

    let record = wait_failure(&mut events).await?;

    assert_eq!(session.state(), SessionState::Crashed);
    assert_eq!(record.session_id, Some(session.session_id()));
    assert_eq!(record.operation_id, None);
    assert_eq!(
        record
            .runtime_event_context
            .as_ref()
            .map(|context| context.trigger),
        Some(FailureTrigger::WorkerProtocol)
    );
    Ok(())
}
