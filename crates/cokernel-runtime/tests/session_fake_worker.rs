#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use cokernel_domain::{
    ExecutionOrigin, FailureRecord, FailureTrigger, NotebookId, OperationId, OperationStatus,
    Project, ProjectId, SessionState,
};
use cokernel_runtime::session::{
    SessionError, SessionEvent, SessionHandle, SessionSupervisor, SessionSupervisorConfig,
};
use tokio::sync::broadcast;
use tokio::time::{sleep, timeout};

const FAKE_UV: &str = include_str!("fixtures/fake_uv.py");

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

async fn wait_operation_finished(
    events: &mut broadcast::Receiver<SessionEvent>,
    operation_id: OperationId,
) -> Result<OperationStatus> {
    timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await {
                Ok(SessionEvent::OperationFinished {
                    operation_id: observed,
                    status,
                    ..
                }) if observed == operation_id => return Ok(status),
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    anyhow::bail!("Session event stream closed before OperationFinished")
                }
            }
        }
    })
    .await
    .with_context(|| format!("timed out waiting for operation {operation_id} to finish"))?
}

async fn wait_state(session: &SessionHandle, expected: SessionState) -> Result<()> {
    timeout(Duration::from_secs(3), async {
        loop {
            if session.state() == expected {
                return;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .with_context(|| {
        format!(
            "timed out waiting for Session {} to reach {expected:?}; current state is {:?}",
            session.session_id(),
            session.state()
        )
    })?;
    Ok(())
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

#[tokio::test]
async fn startup_rejects_worker_missing_required_handshake_capability() -> Result<()> {
    let temp = TempWorkspace::new("bad-handshake")?;
    let project = project(&temp.0.join("bad-handshake"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;

    let error = match supervisor.ensure_primary(&project, NotebookId::new()).await {
        Ok(_) => anyhow::bail!("worker missing get_variable capability unexpectedly started"),
        Err(error) => error,
    };
    assert!(matches!(error, SessionError::WorkerTransport(_)));
    assert!(supervisor.list().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn startup_rejects_ready_pid_that_disagrees_with_unix_peer_credentials() -> Result<()> {
    let temp = TempWorkspace::new("bad-ready-pid")?;
    let project = project(&temp.0.join("bad-ready-pid"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;

    let error = match supervisor.ensure_primary(&project, NotebookId::new()).await {
        Ok(_) => anyhow::bail!("ready pid mismatch unexpectedly started a Session"),
        Err(error) => error,
    };
    assert!(matches!(error, SessionError::InvalidReady(_)));
    assert!(supervisor.list().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn cooperative_stop_reaches_stopped_and_clears_queue_depth() -> Result<()> {
    let temp = TempWorkspace::new("stop")?;
    let project = project(&temp.0.join("lifecycle-stop"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let mut events = session.subscribe();

    session.stop().await?;
    wait_state(&session, SessionState::Stopped).await?;

    assert_eq!(session.snapshot().queue_depth, 0);
    while let Ok(event) = events.try_recv() {
        assert!(
            !matches!(event, SessionEvent::Failure { .. }),
            "cooperative stop emitted a failure record"
        );
    }
    Ok(())
}

#[tokio::test]
async fn explicit_restart_preserves_logical_session_and_adopts_environment_generation() -> Result<()> {
    let temp = TempWorkspace::new("restart")?;
    let project = project(&temp.0.join("lifecycle-restart"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let notebook_id = NotebookId::new();
    let first = supervisor.ensure_primary(&project, notebook_id).await?;
    let first_snapshot = first.snapshot();
    let updated_project = Project {
        project_id: project.project_id,
        name: project.name.clone(),
        root_path: project.root_path.clone(),
        environment_generation: 2,
    };

    let restarted = supervisor
        .restart_primary(&updated_project, notebook_id)
        .await?;
    let restarted_snapshot = restarted.snapshot();

    assert_eq!(first.state(), SessionState::Stopped);
    assert_eq!(restarted.session_id(), first.session_id());
    assert_eq!(
        restarted_snapshot.worker_generation,
        first_snapshot.worker_generation + 1
    );
    assert_eq!(restarted_snapshot.started_at, first_snapshot.started_at);
    assert_eq!(restarted_snapshot.environment_generation, 2);
    assert_eq!(restarted.state(), SessionState::Idle);
    let listed = supervisor.list().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id, restarted.session_id());
    assert_eq!(listed[0].worker_generation, restarted_snapshot.worker_generation);

    restarted.stop().await?;
    wait_state(&restarted, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn environment_stale_marks_only_matching_live_project_and_does_not_replace_primary() -> Result<()> {
    let temp = TempWorkspace::new("stale")?;
    let project_a = project(&temp.0.join("lifecycle-stale-a"))?;
    let project_b = project(&temp.0.join("lifecycle-stale-b"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let notebook_a = NotebookId::new();
    let notebook_b = NotebookId::new();
    let session_a = supervisor.ensure_primary(&project_a, notebook_a).await?;
    let session_b = supervisor.ensure_primary(&project_b, notebook_b).await?;

    supervisor
        .mark_project_environment_stale(project_a.project_id, 2)
        .await;
    wait_state(&session_a, SessionState::StaleEnvironment).await?;

    assert_eq!(session_b.state(), SessionState::Idle);
    let same_primary = supervisor.ensure_primary(&project_a, notebook_a).await?;
    assert_eq!(same_primary.session_id(), session_a.session_id());
    assert_eq!(same_primary.state(), SessionState::StaleEnvironment);

    session_a.stop().await?;
    session_b.stop().await?;
    wait_state(&session_a, SessionState::Stopped).await?;
    wait_state(&session_b, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn human_and_mcp_execution_requests_complete_in_fifo_order() -> Result<()> {
    let temp = TempWorkspace::new("fifo")?;
    let project = project(&temp.0.join("execute-fifo"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let mut events = session.subscribe();

    let first = session
        .execute_cell(ExecutionOrigin::Human, "human-cell", "sleep:0.5")
        .await?;
    wait_state(&session, SessionState::Executing).await?;
    let second = session
        .execute_cell(ExecutionOrigin::Mcp, "mcp-cell", "sleep:0.0")
        .await?;

    assert_eq!(session.snapshot().queue_depth, 1);
    assert_eq!(
        wait_operation_finished(&mut events, first).await?,
        OperationStatus::Succeeded
    );
    assert_eq!(
        wait_operation_finished(&mut events, second).await?,
        OperationStatus::Succeeded
    );
    wait_state(&session, SessionState::Idle).await?;

    session.stop().await?;
    wait_state(&session, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn interrupt_finishes_active_operation_and_same_worker_survives() -> Result<()> {
    let temp = TempWorkspace::new("interrupt")?;
    let project = project(&temp.0.join("execute-interrupt"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let mut events = session.subscribe();

    let interrupted = session
        .execute_cell(ExecutionOrigin::Human, "blocking-cell", "sleep:2.0")
        .await?;
    wait_state(&session, SessionState::Executing).await?;
    session.interrupt().await?;
    assert_eq!(
        wait_operation_finished(&mut events, interrupted).await?,
        OperationStatus::Interrupted
    );
    wait_state(&session, SessionState::Idle).await?;

    let survivor = session
        .execute_cell(ExecutionOrigin::Human, "survivor-cell", "sleep:0.0")
        .await?;
    assert_eq!(
        wait_operation_finished(&mut events, survivor).await?,
        OperationStatus::Succeeded
    );
    assert_eq!(session.state(), SessionState::Idle);

    session.stop().await?;
    wait_state(&session, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn forced_worker_exit_correlates_failure_and_other_session_survives() -> Result<()> {
    let temp = TempWorkspace::new("crash")?;
    let crash_project = project(&temp.0.join("execute-crash"))?;
    let survivor_project = project(&temp.0.join("execute-survivor"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let crashed = supervisor
        .ensure_primary(&crash_project, NotebookId::new())
        .await?;
    let survivor = supervisor
        .ensure_primary(&survivor_project, NotebookId::new())
        .await?;
    let mut crash_events = crashed.subscribe();
    let mut survivor_events = survivor.subscribe();

    let crashing_operation = crashed
        .execute_cell(ExecutionOrigin::Human, "crash-cell", "crash")
        .await?;
    let record = wait_failure(&mut crash_events).await?;

    assert_eq!(crashed.state(), SessionState::Crashed);
    assert_eq!(record.session_id, Some(crashed.session_id()));
    assert_eq!(record.operation_id, Some(crashing_operation));
    assert_eq!(record.exit_code, Some(23));
    assert!(matches!(
        record
            .runtime_event_context
            .as_ref()
            .map(|context| context.trigger),
        Some(FailureTrigger::ProcessExit | FailureTrigger::WorkerDisconnected)
    ));

    let survivor_operation = survivor
        .execute_cell(ExecutionOrigin::Human, "survivor-cell", "sleep:0.0")
        .await?;
    assert_eq!(
        wait_operation_finished(&mut survivor_events, survivor_operation).await?,
        OperationStatus::Succeeded
    );
    assert_eq!(survivor.state(), SessionState::Idle);

    survivor.stop().await?;
    wait_state(&survivor, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn independent_sessions_execute_concurrently() -> Result<()> {
    let temp = TempWorkspace::new("parallel")?;
    let project_a = project(&temp.0.join("execute-parallel-a"))?;
    let project_b = project(&temp.0.join("execute-parallel-b"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session_a = supervisor
        .ensure_primary(&project_a, NotebookId::new())
        .await?;
    let session_b = supervisor
        .ensure_primary(&project_b, NotebookId::new())
        .await?;
    let mut events_a = session_a.subscribe();
    let mut events_b = session_b.subscribe();

    let operation_a = session_a
        .execute_cell(ExecutionOrigin::Human, "slow-cell", "sleep:0.8")
        .await?;
    wait_state(&session_a, SessionState::Executing).await?;
    let operation_b = session_b
        .execute_cell(ExecutionOrigin::Mcp, "fast-cell", "sleep:0.05")
        .await?;

    assert_eq!(
        wait_operation_finished(&mut events_b, operation_b).await?,
        OperationStatus::Succeeded
    );
    assert_eq!(session_b.state(), SessionState::Idle);
    assert_eq!(session_a.state(), SessionState::Executing);
    assert_eq!(
        wait_operation_finished(&mut events_a, operation_a).await?,
        OperationStatus::Succeeded
    );
    wait_state(&session_a, SessionState::Idle).await?;

    session_a.stop().await?;
    session_b.stop().await?;
    wait_state(&session_a, SessionState::Stopped).await?;
    wait_state(&session_b, SessionState::Stopped).await?;
    Ok(())
}
