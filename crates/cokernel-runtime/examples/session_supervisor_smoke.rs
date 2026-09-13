#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use anyhow::{bail, Context, Result};
#[cfg(unix)]
use cokernel_domain::{
    ExecutionOrigin, NotebookId, OperationId, OperationStatus, Project, ProjectId, SessionId,
    SessionState,
};
#[cfg(unix)]
use cokernel_protocol::worker::{self, WorkerFrame};
#[cfg(unix)]
use cokernel_runtime::session::{
    SessionEvent, SessionHandle, SessionSupervisor, SessionSupervisorConfig,
};
#[cfg(unix)]
use tokio::sync::broadcast;
#[cfg(unix)]
use tokio::time::{sleep, timeout};

#[cfg(unix)]
#[derive(Debug)]
struct ObservedOperation {
    status: OperationStatus,
    text_plain: Option<String>,
}

#[cfg(unix)]
struct TempWorkspace(PathBuf);

#[cfg(unix)]
impl TempWorkspace {
    fn new() -> Result<Self> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before UNIX_EPOCH")?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cokernel-session-supervisor-smoke-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self(path))
    }
}

#[cfg(unix)]
impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<()> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .context("could not locate repository root")?
        .to_path_buf();
    let worker_package = repo_root.join("worker");
    let temp = TempWorkspace::new()?;
    let project = prepare_project(&temp.0.join("project"))?;

    let supervisor = SessionSupervisor::new(SessionSupervisorConfig::development(
        worker_package,
        temp.0.join("sockets"),
    ));

    let notebook_a = NotebookId::new();
    let notebook_b = NotebookId::new();
    let session_a = supervisor.ensure_primary(&project, notebook_a).await?;
    let session_b = supervisor.ensure_primary(&project, notebook_b).await?;
    let mut events_a = session_a.subscribe();
    let mut events_b = session_b.subscribe();

    if session_a.snapshot().worker_generation != 1 || session_b.snapshot().worker_generation != 1 {
        bail!("fresh Sessions must start at worker generation 1");
    }

    // Canonical persistent-state proof.
    let set_x = session_a
        .execute_cell(ExecutionOrigin::Human, "cell-a1", "x = 123")
        .await?;
    expect_success(wait_operation(&mut events_a, set_x).await?, None)?;

    let read_x = session_a
        .execute_cell(ExecutionOrigin::Mcp, "cell-a2", "x + 1")
        .await?;
    expect_success(wait_operation(&mut events_a, read_x).await?, Some("124"))?;

    // Human and MCP executions must share one FIFO. Hold the first operation in-flight,
    // enqueue two callers from different origins, then prove acceptance order through state.
    let init_fifo = session_a
        .execute_cell(ExecutionOrigin::Human, "cell-a-fifo-init", "fifo_order = []")
        .await?;
    expect_success(wait_operation(&mut events_a, init_fifo).await?, None)?;

    let human_first = session_a
        .execute_cell(
            ExecutionOrigin::Human,
            "cell-a-fifo-human-1",
            "import time\ntime.sleep(1.0)\nfifo_order.append('human')\nlist(fifo_order)",
        )
        .await?;
    wait_for_state(&session_a, SessionState::Executing, Duration::from_secs(5)).await?;

    let mcp_second = session_a
        .execute_cell(
            ExecutionOrigin::Mcp,
            "cell-a-fifo-mcp-2",
            "fifo_order.append('mcp')\nlist(fifo_order)",
        )
        .await?;
    let human_third = session_a
        .execute_cell(
            ExecutionOrigin::Human,
            "cell-a-fifo-human-3",
            "fifo_order.append('human-2')\nlist(fifo_order)",
        )
        .await?;
    if session_a.snapshot().queue_depth != 2 {
        bail!(
            "shared FIFO queue depth mismatch while first operation runs: expected 2, got {}",
            session_a.snapshot().queue_depth
        );
    }

    expect_success(
        wait_operation(&mut events_a, human_first).await?,
        Some("['human']"),
    )?;
    expect_success(
        wait_operation(&mut events_a, mcp_second).await?,
        Some("['human', 'mcp']"),
    )?;
    expect_success(
        wait_operation(&mut events_a, human_third).await?,
        Some("['human', 'mcp', 'human-2']"),
    )?;

    // Namespace isolation: another Session must not see x from Session A.
    let isolated = session_b
        .execute_cell(ExecutionOrigin::Human, "cell-b1", "x")
        .await?;
    let isolated = wait_operation(&mut events_b, isolated).await?;
    if isolated.status != OperationStatus::Failed {
        bail!(
            "Session namespace leaked: Session B read Session A state with status {:?}",
            isolated.status
        );
    }

    // Parallelism proof: keep A busy, then complete B while A is still executing.
    let slow_a = session_a
        .execute_cell(
            ExecutionOrigin::Human,
            "cell-a3",
            "import time\ntime.sleep(4.0)\n'A-done'",
        )
        .await?;
    wait_for_state(&session_a, SessionState::Executing, Duration::from_secs(5)).await?;

    let fast_b = session_b
        .execute_cell(ExecutionOrigin::Human, "cell-b2", "21 * 2")
        .await?;
    expect_success(
        timeout(Duration::from_secs(2), wait_operation(&mut events_b, fast_b))
            .await
            .context("Session B did not finish while Session A was busy")??,
        Some("42"),
    )?;
    if session_a.state() != SessionState::Executing {
        bail!(
            "parallelism proof failed: Session A was {:?} before B finished",
            session_a.state()
        );
    }
    expect_success(wait_operation(&mut events_a, slow_a).await?, Some("'A-done'"))?;

    // Interrupt must preserve the worker and namespace rather than restart it.
    let remember_b = session_b
        .execute_cell(
            ExecutionOrigin::Human,
            "cell-b3",
            "restart_marker = 77\nrestart_marker",
        )
        .await?;
    expect_success(wait_operation(&mut events_b, remember_b).await?, Some("77"))?;
    let interrupted = session_b
        .execute_cell(
            ExecutionOrigin::Human,
            "cell-b4",
            "import time\ntime.sleep(30)\n'not-reached'",
        )
        .await?;
    wait_for_state(&session_b, SessionState::Executing, Duration::from_secs(5)).await?;
    session_b.interrupt().await?;
    let interrupted_result = wait_operation(&mut events_b, interrupted).await?;
    if interrupted_result.status != OperationStatus::Interrupted {
        bail!(
            "interrupt returned {:?}, expected INTERRUPTED",
            interrupted_result.status
        );
    }
    wait_for_state(&session_b, SessionState::Idle, Duration::from_secs(5)).await?;
    let survived_interrupt = session_b
        .execute_cell(ExecutionOrigin::Mcp, "cell-b5", "restart_marker")
        .await?;
    expect_success(
        wait_operation(&mut events_b, survived_interrupt).await?,
        Some("77"),
    )?;

    // Environment mutation marks an existing Session stale but leaves it executable.
    let mut project_v2 = project.clone();
    project_v2.environment_generation = 2;
    supervisor
        .mark_project_environment_stale(project.project_id, project_v2.environment_generation)
        .await;
    wait_for_state(
        &session_b,
        SessionState::StaleEnvironment,
        Duration::from_secs(5),
    )
    .await?;
    let stale_exec = session_b
        .execute_cell(ExecutionOrigin::Human, "cell-b6", "6 * 7")
        .await?;
    expect_success(wait_operation(&mut events_b, stale_exec).await?, Some("42"))?;
    wait_for_state(
        &session_b,
        SessionState::StaleEnvironment,
        Duration::from_secs(5),
    )
    .await?;

    // Restart preserves logical Session identity, increments worker generation, adopts
    // the new Project environment generation, and deliberately loses volatile namespace.
    let before_restart = session_b.snapshot();
    let restarted_b = supervisor.restart_primary(&project_v2, notebook_b).await?;
    let after_restart = restarted_b.snapshot();
    if restarted_b.session_id() != session_b.session_id() {
        bail!(
            "restart changed logical Session ID: {} -> {}",
            session_b.session_id(),
            restarted_b.session_id()
        );
    }
    if after_restart.worker_generation != before_restart.worker_generation + 1 {
        bail!(
            "worker generation did not increment: {} -> {}",
            before_restart.worker_generation,
            after_restart.worker_generation
        );
    }
    if after_restart.environment_generation != project_v2.environment_generation {
        bail!(
            "restart did not adopt environment generation {}: got {}",
            project_v2.environment_generation,
            after_restart.environment_generation
        );
    }
    if after_restart.started_at != before_restart.started_at {
        bail!("restart changed logical Session started_at");
    }
    wait_for_state(&session_b, SessionState::Stopped, Duration::from_secs(5)).await?;

    let ensured_b = supervisor.ensure_primary(&project_v2, notebook_b).await?;
    if ensured_b.snapshot().worker_generation != after_restart.worker_generation {
        bail!("ensure_primary replaced a healthy restarted worker");
    }

    let mut events_restarted_b = restarted_b.subscribe();
    let lost_namespace = restarted_b
        .execute_cell(ExecutionOrigin::Human, "cell-b7", "restart_marker")
        .await?;
    let lost_namespace = wait_operation(&mut events_restarted_b, lost_namespace).await?;
    if lost_namespace.status != OperationStatus::Failed {
        bail!("restart unexpectedly preserved volatile namespace");
    }
    let post_restart = restarted_b
        .execute_cell(ExecutionOrigin::Mcp, "cell-b8", "40 + 2")
        .await?;
    expect_success(
        wait_operation(&mut events_restarted_b, post_restart).await?,
        Some("42"),
    )?;

    // Forced worker crash must be contained to A and produce FailureRecord evidence.
    let crash_op = session_a
        .execute_cell(
            ExecutionOrigin::Internal,
            "cell-a4",
            "import os\nos._exit(23)",
        )
        .await?;
    let failure = wait_failure(&mut events_a, session_a.session_id()).await?;
    if failure.operation_id != Some(crash_op) {
        bail!(
            "FailureRecord operation mismatch: expected {crash_op}, got {:?}",
            failure.operation_id
        );
    }
    if failure.exit_code != Some(23) {
        bail!(
            "FailureRecord exit code mismatch: expected 23, got {:?}",
            failure.exit_code
        );
    }
    wait_for_state(&session_a, SessionState::Crashed, Duration::from_secs(5)).await?;

    // An innocuous ensure call must not silently replace a crashed worker; restart is explicit.
    let crashed_a = supervisor.ensure_primary(&project, notebook_a).await?;
    if crashed_a.state() != SessionState::Crashed
        || crashed_a.snapshot().worker_generation != session_a.snapshot().worker_generation
    {
        bail!("ensure_primary silently replaced a crashed Session");
    }

    let survivor = restarted_b
        .execute_cell(ExecutionOrigin::Mcp, "cell-b9", "20 + 22")
        .await?;
    expect_success(
        wait_operation(&mut events_restarted_b, survivor).await?,
        Some("42"),
    )?;

    restarted_b.stop().await?;
    wait_for_state(
        &restarted_b,
        SessionState::Stopped,
        Duration::from_secs(5),
    )
    .await?;

    println!("[session-supervisor-smoke] PASS");
    Ok(())
}

#[cfg(unix)]
fn prepare_project(root: &Path) -> Result<Project> {
    fs::create_dir_all(root).with_context(|| format!("failed to create {}", root.display()))?;
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"cokernel-session-smoke\"\nversion = \"0.0.0\"\nrequires-python = \">=3.11\"\ndependencies = []\n",
    )?;

    let status = Command::new("uv")
        .arg("sync")
        .arg("--project")
        .arg(root)
        .env("UV_NO_PROGRESS", "1")
        .status()
        .context("failed to launch uv sync for Session Supervisor smoke")?;
    if !status.success() {
        bail!("uv sync failed with {status}");
    }

    Ok(Project {
        project_id: ProjectId::new(),
        name: "cokernel-session-smoke".into(),
        root_path: root.to_string_lossy().into_owned(),
        environment_generation: 1,
    })
}

#[cfg(unix)]
async fn wait_operation(
    events: &mut broadcast::Receiver<SessionEvent>,
    operation_id: OperationId,
) -> Result<ObservedOperation> {
    let operation_key = operation_id.to_string();
    timeout(Duration::from_secs(40), async {
        let mut text_plain = None;
        loop {
            match events.recv().await {
                Ok(SessionEvent::WorkerFrame {
                    frame:
                        WorkerFrame::Event {
                            event, payload, ..
                        },
                    ..
                }) if payload.get("operation_id").and_then(serde_json::Value::as_str)
                    == Some(operation_key.as_str())
                    && event == worker::event::EXECUTE_RESULT =>
                {
                    text_plain = payload
                        .get("data")
                        .and_then(|data| data.get("text/plain"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned);
                }
                Ok(SessionEvent::OperationFinished {
                    operation_id: finished,
                    status,
                    ..
                }) if finished == operation_id => {
                    return Ok(ObservedOperation { status, text_plain });
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    bail!("Session event stream closed while waiting for {operation_id}")
                }
            }
        }
    })
    .await
    .with_context(|| format!("timed out waiting for operation {operation_id}"))?
}

#[cfg(unix)]
fn expect_success(operation: ObservedOperation, expected_text: Option<&str>) -> Result<()> {
    if operation.status != OperationStatus::Succeeded {
        bail!("operation failed with status {:?}", operation.status);
    }
    if let Some(expected) = expected_text {
        if operation.text_plain.as_deref() != Some(expected) {
            bail!(
                "unexpected execute_result: expected {expected:?}, got {:?}",
                operation.text_plain
            );
        }
    }
    Ok(())
}

#[cfg(unix)]
async fn wait_failure(
    events: &mut broadcast::Receiver<SessionEvent>,
    session_id: SessionId,
) -> Result<cokernel_domain::FailureRecord> {
    timeout(Duration::from_secs(10), async {
        loop {
            match events.recv().await {
                Ok(SessionEvent::Failure { record }) if record.session_id == Some(session_id) => {
                    return Ok(record);
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    bail!("Session event stream closed while waiting for crash evidence")
                }
            }
        }
    })
    .await
    .context("timed out waiting for FailureRecord")?
}

#[cfg(unix)]
async fn wait_for_state(
    session: &SessionHandle,
    expected: SessionState,
    maximum: Duration,
) -> Result<()> {
    timeout(maximum, async {
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
            "timed out waiting for Session {} to reach {expected:?}; current={:?}",
            session.session_id(),
            session.state()
        )
    })?;
    Ok(())
}

#[cfg(not(unix))]
fn main() {
    println!("[session-supervisor-smoke] SKIP: Unix-domain Session runtime is exercised in WSL/Linux");
}
