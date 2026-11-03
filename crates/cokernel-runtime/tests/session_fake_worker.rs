#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

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

async fn document_runtime(
    temp: &TempWorkspace,
    project: &Project,
) -> Result<(
    cokernel_runtime::notebook::NotebookRuntime,
    NotebookId,
    SessionHandle,
)> {
    use cokernel_runtime::notebook::{Edit, NotebookRuntime, NotebookService};
    let mut documents = NotebookService::default();
    let id = documents
        .create(project, "runtime.ipynb")?
        .document
        .notebook_id;
    let cell = |id: &str, source: &str| serde_json::json!({"id":id,"cell_type":"code","source":source,"metadata":{},"execution_count":null,"outputs":[]});
    documents.apply_edits(
        project,
        id,
        1,
        &[
            Edit::InsertCell {
                index: 0,
                cell: cell("a", "sleep:0.2"),
            },
            Edit::InsertCell {
                index: 1,
                cell: cell("b", "1"),
            },
        ],
    )?;
    documents.save(project, id, 2)?;
    let runtime = NotebookRuntime::new(documents, supervisor(temp, Duration::from_secs(3))?);
    let session = runtime
        .ensure_primary(project, id)
        .await
        .map_err(|e| anyhow::anyhow!(e.summary))?;
    Ok((runtime, id, session))
}

async fn document_result(
    runtime: &cokernel_runtime::notebook::NotebookRuntime,
    project: &Project,
    operation: OperationId,
) -> Result<cokernel_runtime::notebook::DocumentExecutionState> {
    use cokernel_runtime::notebook::DocumentExecutionState;
    timeout(Duration::from_secs(5), async {
        loop {
            let result = runtime
                .operation(project.project_id, operation)
                .await
                .map_err(|e| anyhow::anyhow!(e.summary))?
                .result;
            if !matches!(result, DocumentExecutionState::Pending) {
                return Ok(result);
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await?
}

#[tokio::test]
async fn runtime_shutdown_stops_workers_settles_captures_and_flushes_dirty_documents() -> Result<()>
{
    use cokernel_runtime::notebook::{DocumentState, Edit};
    let temp = TempWorkspace::new("runtime-shutdown")?;
    let project = project(&temp.0.join("execute-runtime-shutdown"))?;
    let (runtime, notebook, session) = document_runtime(&temp, &project).await?;
    runtime.documents.lock().await.apply_edits(
        &project,
        notebook,
        2,
        &[
            Edit::SetCellSource {
                cell_id: "a".into(),
                source: "sleep:10.0".into(),
            },
            Edit::SetNotebookMetadata {
                metadata: serde_json::json!({"shutdown_flush":true})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
        ],
    )?;
    let operation = runtime
        .execute_cell(
            &project,
            session.session_id(),
            notebook,
            "a",
            3,
            ExecutionOrigin::Human,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.summary))?;
    wait_state(&session, SessionState::Executing).await?;
    runtime
        .shutdown()
        .await
        .map_err(|e| anyhow::anyhow!(e.summary))?;
    assert_eq!(session.state(), SessionState::Stopped);
    assert!(!matches!(
        document_result(&runtime, &project, operation.operation_id).await?,
        cokernel_runtime::notebook::DocumentExecutionState::Pending
    ));
    let saved = runtime.documents.lock().await.get(&project, notebook)?;
    assert_eq!(saved.state, DocumentState::Clean);
    let bytes = std::fs::read(std::path::Path::new(&project.root_path).join("runtime.ipynb"))?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes)?["metadata"]["shutdown_flush"],
        true
    );
    Ok(())
}

#[tokio::test]
async fn runtime_shutdown_reports_document_conflict_after_stopping_session() -> Result<()> {
    let temp = TempWorkspace::new("shutdown-conflict")?;
    let project = project(&temp.0.join("execute-shutdown-conflict"))?;
    let (runtime, _notebook, session) = document_runtime(&temp, &project).await?;
    let path = std::path::Path::new(&project.root_path).join("runtime.ipynb");
    std::fs::write(&path, b"external edit")?;
    let error = runtime.shutdown().await.unwrap_err();
    assert_eq!(error.code, "CK-RUN-SHUTDOWN");
    assert_eq!(session.state(), SessionState::Stopped);
    assert_eq!(std::fs::read(&path)?, b"external edit");
    Ok(())
}

#[tokio::test]
async fn document_runtime_queues_shared_revision_and_keeps_persistence_after_caller_drop()
-> Result<()> {
    use cokernel_runtime::notebook::DocumentExecutionState;
    let temp = TempWorkspace::new("doc-queue")?;
    let project = project(&temp.0.join("execute-doc-queue"))?;
    let (runtime, notebook, session) = document_runtime(&temp, &project).await?;
    let caller = runtime.clone();
    let first = caller
        .execute_cell(
            &project,
            session.session_id(),
            notebook,
            "a",
            2,
            ExecutionOrigin::Human,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.summary))?;
    let second = caller
        .execute_cell(
            &project,
            session.session_id(),
            notebook,
            "b",
            2,
            ExecutionOrigin::Mcp,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.summary))?;
    drop(caller);
    assert!(matches!(
        document_result(&runtime, &project, first.operation_id).await?,
        DocumentExecutionState::Persisted { .. }
    ));
    assert!(matches!(
        document_result(&runtime, &project, second.operation_id).await?,
        DocumentExecutionState::Persisted { .. }
    ));
    let mut documents = runtime.documents.lock().await;
    let saved = documents.get(&project, notebook)?;
    assert_eq!(saved.document.revision, 4);
    assert_eq!(saved.notebook.value()["cells"][0]["execution_count"], 1);
    assert_eq!(saved.notebook.value()["cells"][1]["execution_count"], 2);
    documents.close(&project, notebook, 4)?;
    assert_eq!(
        documents
            .open(&project, "runtime.ipynb")?
            .document
            .notebook_id,
        notebook
    );
    drop(documents);
    assert_eq!(
        runtime
            .ensure_primary(&project, notebook)
            .await
            .map_err(|e| anyhow::anyhow!(e.summary))?
            .session_id(),
        session.session_id()
    );
    session.stop().await?;
    wait_state(&session, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn unrelated_edit_does_not_conflict_but_source_aba_does() -> Result<()> {
    use cokernel_runtime::notebook::{DocumentExecutionState, Edit};
    let temp = TempWorkspace::new("doc-source")?;
    let project = project(&temp.0.join("execute-doc-source"))?;
    let (runtime, notebook, session) = document_runtime(&temp, &project).await?;
    let first = runtime
        .execute_cell(
            &project,
            session.session_id(),
            notebook,
            "a",
            2,
            ExecutionOrigin::Human,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.summary))?;
    runtime.documents.lock().await.apply_edits(
        &project,
        notebook,
        2,
        &[Edit::SetCellSource {
            cell_id: "b".into(),
            source: "other edit".into(),
        }],
    )?;
    assert!(matches!(
        document_result(&runtime, &project, first.operation_id).await?,
        DocumentExecutionState::Persisted { .. }
    ));
    let revision = runtime
        .documents
        .lock()
        .await
        .get(&project, notebook)?
        .document
        .revision;
    let second = runtime
        .execute_cell(
            &project,
            session.session_id(),
            notebook,
            "a",
            revision,
            ExecutionOrigin::Mcp,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.summary))?;
    runtime.documents.lock().await.apply_edits(
        &project,
        notebook,
        revision,
        &[
            Edit::SetCellSource {
                cell_id: "a".into(),
                source: "changed".into(),
            },
            Edit::SetCellSource {
                cell_id: "a".into(),
                source: "sleep:0.2".into(),
            },
        ],
    )?;
    assert!(
        matches!(document_result(&runtime,&project,second.operation_id).await?,DocumentExecutionState::NotPersisted { error,.. } if error.code=="CK-NB-STALE-EXECUTION")
    );
    assert_eq!(
        runtime
            .documents
            .lock()
            .await
            .get(&project, notebook)?
            .notebook
            .value()["cells"][0]["execution_count"],
        1
    );
    session.stop().await?;
    wait_state(&session, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn replacement_reload_and_external_changes_reject_old_results() -> Result<()> {
    use cokernel_runtime::notebook::{DocumentExecutionState, Edit};
    for scenario in ["replace", "reload", "external"] {
        let temp = TempWorkspace::new("doc-conflict")?;
        let project = project(&temp.0.join("execute-doc-conflict"))?;
        let (runtime, notebook, session) = document_runtime(&temp, &project).await?;
        let operation = runtime
            .execute_cell(
                &project,
                session.session_id(),
                notebook,
                "a",
                2,
                ExecutionOrigin::Human,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e.summary))?;
        match scenario {
            "replace" => {
                runtime.documents.lock().await.apply_edits(&project,notebook,2,&[
                Edit::DeleteCell {cell_id:"a".into()},
                Edit::InsertCell {index:0,cell:serde_json::json!({"id":"a","cell_type":"code","source":"sleep:0.2","metadata":{},"execution_count":null,"outputs":[]})},
            ])?;
            }
            "reload" => {
                runtime
                    .documents
                    .lock()
                    .await
                    .reload(&project, notebook, 2)?;
            }
            _ => {
                fs::write(
                    Path::new(&project.root_path).join("runtime.ipynb"),
                    b"external bytes",
                )?;
            }
        }
        assert!(
            matches!(
                document_result(&runtime, &project, operation.operation_id).await?,
                DocumentExecutionState::NotPersisted { .. }
            ),
            "{scenario}"
        );
        if scenario == "external" {
            assert_eq!(
                fs::read(Path::new(&project.root_path).join("runtime.ipynb"))?,
                b"external bytes"
            );
        }
        session.stop().await?;
        wait_state(&session, SessionState::Stopped).await?;
    }
    Ok(())
}

#[tokio::test]
async fn document_admission_rejects_wrong_identity_revision_and_cell_without_execution()
-> Result<()> {
    let temp = TempWorkspace::new("doc-admission")?;
    let project = project(&temp.0.join("execute-doc-admission"))?;
    let (runtime, notebook, session) = document_runtime(&temp, &project).await?;
    assert!(
        runtime
            .execute_cell(
                &project,
                session.session_id(),
                notebook,
                "a",
                1,
                ExecutionOrigin::Human
            )
            .await
            .is_err()
    );
    assert!(
        runtime
            .execute_cell(
                &project,
                session.session_id(),
                NotebookId::new(),
                "a",
                2,
                ExecutionOrigin::Human
            )
            .await
            .is_err()
    );
    assert!(
        runtime
            .execute_cell(
                &project,
                session.session_id(),
                notebook,
                "missing",
                2,
                ExecutionOrigin::Human
            )
            .await
            .is_err()
    );
    let mut wrong = project.clone();
    wrong.project_id = ProjectId::new();
    assert!(
        runtime
            .execute_cell(
                &wrong,
                session.session_id(),
                notebook,
                "a",
                2,
                ExecutionOrigin::Mcp
            )
            .await
            .is_err()
    );
    assert_eq!(session.snapshot().queue_depth, 0);
    assert!(session.snapshot().current_operation_id.is_none());
    session.stop().await?;
    wait_state(&session, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn wrong_project_cannot_reuse_or_restart_an_existing_primary_session() -> Result<()> {
    let temp = TempWorkspace::new("ownership")?;
    let a = project(&temp.0.join("execute-owner"))?;
    let b = project(&temp.0.join("execute-other"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let notebook = NotebookId::new();
    let session = supervisor.ensure_primary(&a, notebook).await?;
    assert!(matches!(
        supervisor.ensure_primary(&b, notebook).await,
        Err(SessionError::OwnershipMismatch)
    ));
    assert!(matches!(
        supervisor.restart_primary(&b, notebook).await,
        Err(SessionError::OwnershipMismatch)
    ));
    let receipt = session
        .execute_cell_captured(ExecutionOrigin::Human, "cell", "1")
        .await?;
    assert_eq!(
        timeout(Duration::from_secs(5), receipt.completed())
            .await??
            .status,
        OperationStatus::Succeeded
    );
    session.stop().await?;
    wait_state(&session, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn captured_outputs_survive_broadcast_lag_and_document_save() -> Result<()> {
    use cokernel_runtime::notebook::{Edit, NotebookOutputs, NotebookService};
    let temp = TempWorkspace::new("capture")?;
    let project = project(&temp.0.join("execute-capture"))?;
    let mut documents = NotebookService::default();
    let notebook = documents
        .create(&project, "capture.ipynb")?
        .document
        .notebook_id;
    let edited = documents.apply_edits(&project,notebook,1,&[Edit::InsertCell { index:0, cell:serde_json::json!({
        "id":"cell","cell_type":"code","metadata":{},"source":"many_outputs","execution_count":null,"outputs":[]
    }) }])?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor.ensure_primary(&project, notebook).await?;
    let mut deliberately_slow_listener = session.subscribe();
    let receipt = session
        .execute_cell_captured(
            ExecutionOrigin::Human,
            "cell",
            edited.notebook.cell_source("cell")?.1,
        )
        .await?;
    let operation = receipt.operation_id;
    let capture = timeout(Duration::from_secs(5), receipt.completed()).await??;
    assert_eq!(capture.operation_id, operation);
    assert_eq!(capture.status, OperationStatus::Succeeded);
    assert!(matches!(
        deliberately_slow_listener.try_recv(),
        Err(broadcast::error::TryRecvError::Lagged(_))
    ));
    let outputs =
        NotebookOutputs::from_frames(operation, &capture.frames.map_err(anyhow::Error::msg)?)?;
    assert_eq!(outputs.outputs().len(), 2048);
    assert_eq!(outputs.outputs()[2047]["text"], "2048\n");
    let changed = documents.apply_outputs(&project, notebook, 2, "cell", &outputs)?;
    documents.save(&project, notebook, changed.document.revision)?;
    let saved = cokernel_runtime::notebook::Notebook::parse(&fs::read(
        Path::new(&project.root_path).join("capture.ipynb"),
    )?)?;
    assert_eq!(
        saved.value()["cells"][0]["outputs"]
            .as_array()
            .unwrap()
            .len(),
        2048
    );
    session.stop().await?;
    wait_state(&session, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn captured_worker_crash_resolves_receipt_as_failure() -> Result<()> {
    let temp = TempWorkspace::new("capture-crash")?;
    let project = project(&temp.0.join("execute-capture-crash"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let receipt = session
        .execute_cell_captured(ExecutionOrigin::Mcp, "cell", "crash")
        .await?;
    assert!(
        timeout(Duration::from_secs(5), receipt.completed())
            .await?
            .is_err()
    );
    wait_state(&session, SessionState::Crashed).await?;
    assert!(session.last_failure().is_some());
    Ok(())
}

struct TestDescendant(u32);

impl TestDescendant {
    fn read(project: &Project) -> Result<Self> {
        let pid = fs::read_to_string(Path::new(&project.root_path).join("child.pid"))?;
        Ok(Self(pid.parse()?))
    }

    fn running(&self) -> bool {
        fs::read_to_string(format!("/proc/{}/stat", self.0))
            .ok()
            .and_then(|stat| {
                stat.rsplit_once(") ")
                    .map(|(_, tail)| tail.starts_with('Z'))
            })
            .is_some_and(|zombie| !zombie)
    }

    async fn expect_exited(&self) -> Result<()> {
        timeout(Duration::from_secs(3), async {
            while self.running() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .context("Session descendant survived cleanup")
    }
}

impl Drop for TestDescendant {
    fn drop(&mut self) {
        if self.running() {
            // Only the PID recorded by this test's own child is targeted.
            unsafe {
                libc::kill(self.0 as i32, libc::SIGKILL);
            }
        }
    }
}

fn project_with_child(root: &Path) -> Result<Project> {
    let project = project(root)?;
    fs::write(root.join("spawn-child"), "")?;
    Ok(project)
}

#[tokio::test]
async fn crash_evidence_survives_without_subscribers_and_after_restart() -> Result<()> {
    let temp = TempWorkspace::new("durable-failure")?;
    let project = project(&temp.0.join("execute-crash"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let notebook = NotebookId::new();
    let session = supervisor.ensure_primary(&project, notebook).await?;
    let operation = session
        .execute_cell(ExecutionOrigin::Human, "durable-cell", "crash")
        .await?;
    wait_state(&session, SessionState::Crashed).await?;
    let failure = session
        .last_failure()
        .context("failure missing without subscriber")?;
    assert!(failure.persistence_error.is_none());
    assert_eq!(failure.record.operation_id, Some(operation));
    let saved = supervisor.failures(session.session_id()).await?;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].failure_id, failure.record.failure_id);
    let reopened =
        cokernel_runtime::failure_store::FailureStore::new(temp.0.join("sockets/failures"));
    assert_eq!(
        reopened.get(failure.record.failure_id)?.unwrap().exit_code,
        Some(23)
    );
    let restarted = supervisor.restart_primary(&project, notebook).await?;
    assert_eq!(supervisor.failures(restarted.session_id()).await?.len(), 1);
    restarted.stop().await?;
    wait_state(&restarted, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn failure_store_error_is_observable_and_does_not_prevent_cleanup() -> Result<()> {
    let temp = TempWorkspace::new("failure-write-error")?;
    let project = project_with_child(&temp.0.join("execute-crash"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let child = TestDescendant::read(&project)?;
    fs::write(temp.0.join("sockets/failures"), "storage unavailable")?;
    let mut events = session.subscribe();
    session
        .execute_cell(ExecutionOrigin::Human, "crash", "crash")
        .await?;
    wait_state(&session, SessionState::Crashed).await?;
    child.expect_exited().await?;
    assert!(session.last_failure().unwrap().persistence_error.is_some());
    assert!(supervisor.failures(session.session_id()).await.is_err());
    let mut saw_storage_error = false;
    while let Ok(event) = events.try_recv() {
        if matches!(event, SessionEvent::FailurePersistenceFailed { .. }) {
            saw_storage_error = true;
        }
    }
    assert!(saw_storage_error);
    Ok(())
}

#[tokio::test]
async fn stop_and_restart_terminate_descendants_without_harming_other_session() -> Result<()> {
    let temp = TempWorkspace::new("descendants")?;
    let owned_project = project_with_child(&temp.0.join("lifecycle-children"))?;
    let other_project = project(&temp.0.join("healthy"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let notebook = NotebookId::new();
    let owned = supervisor.ensure_primary(&owned_project, notebook).await?;
    let first_child = TestDescendant::read(&owned_project)?;
    assert!(first_child.running());
    let other = supervisor
        .ensure_primary(&other_project, NotebookId::new())
        .await?;
    let restarted = supervisor.restart_primary(&owned_project, notebook).await?;
    assert_eq!(owned.session_id(), restarted.session_id());
    first_child.expect_exited().await?;
    let second_child = TestDescendant::read(&owned_project)?;
    assert_ne!(first_child.0, second_child.0);
    assert!(second_child.running());
    restarted.stop().await?;
    wait_state(&restarted, SessionState::Stopped).await?;
    second_child.expect_exited().await?;
    let mut other_events = other.subscribe();
    let operation = other
        .execute_cell(ExecutionOrigin::Human, "survives", "sleep:0.0")
        .await?;
    assert_eq!(
        wait_operation_finished(&mut other_events, operation).await?,
        OperationStatus::Succeeded
    );
    other.stop().await?;
    wait_state(&other, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn crash_after_group_leader_exits_still_terminates_descendants() -> Result<()> {
    let temp = TempWorkspace::new("crash-child")?;
    let project = project_with_child(&temp.0.join("execute-crash"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let child = TestDescendant::read(&project)?;
    let mut events = session.subscribe();
    session
        .execute_cell(ExecutionOrigin::Human, "crash", "crash")
        .await?;
    let record = wait_failure(&mut events).await?;
    assert_eq!(record.exit_code, Some(23));
    child.expect_exited().await?;
    Ok(())
}

#[tokio::test]
async fn rejected_startup_terminates_descendants() -> Result<()> {
    let temp = TempWorkspace::new("startup-child")?;
    let project = project_with_child(&temp.0.join("bad-handshake"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    assert!(
        supervisor
            .ensure_primary(&project, NotebookId::new())
            .await
            .is_err()
    );
    let child = TestDescendant::read(&project)?;
    child.expect_exited().await?;
    Ok(())
}

struct TempWorkspace(PathBuf);

impl TempWorkspace {
    fn new(_label: &str) -> Result<Self> {
        // Concurrent tests must never reuse or overwrite another test's launcher.
        // Clock resolution is not an exclusive directory reservation.
        let path =
            std::env::temp_dir().join(format!("cks-{}-{}", std::process::id(), ProjectId::new()));
        fs::create_dir(&path).with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self(path))
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fake_uv() -> Result<PathBuf> {
    static LAUNCHER: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    // Finish creating the executable before any test in this process can spawn
    // it. Per-test executable writes can overlap other tests' fork/exec windows.
    // Keep this immutable fixture beside the test binary as a build artifact;
    // each invocation has its own filename and never rewrites a running script.
    LAUNCHER
        .get_or_init(|| {
            (|| -> Result<PathBuf> {
                let executable = std::env::current_exe()?;
                let directory = executable.parent().context("test binary has no parent")?;
                let path = directory.join(format!("fake-uv-{}.py", ProjectId::new()));
                let mut file = fs::File::create_new(&path)?;
                file.write_all(FAKE_UV.as_bytes())?;
                file.set_permissions(fs::Permissions::from_mode(0o500))?;
                drop(file);
                Ok(path)
            })()
            .map_err(|error| format!("failed to initialize immutable fake uv: {error:#}"))
        })
        .clone()
        .map_err(anyhow::Error::msg)
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

fn supervisor(temp: &TempWorkspace, heartbeat_timeout: Duration) -> Result<SessionSupervisor> {
    let fake_uv = fake_uv()?;
    let mut config =
        SessionSupervisorConfig::development("fake-worker-package", temp.0.join("sockets"));
    config.uv_executable = fake_uv;
    config.startup_timeout = Duration::from_secs(2);
    config.shutdown_timeout = Duration::from_millis(200);
    config.heartbeat_timeout = heartbeat_timeout;
    Ok(SessionSupervisor::new(config))
}

async fn wait_failure(events: &mut broadcast::Receiver<SessionEvent>) -> Result<FailureRecord> {
    timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await {
                Ok(SessionEvent::Failure { record }) => return Ok(*record),
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
async fn explicit_restart_preserves_logical_session_and_adopts_environment_generation() -> Result<()>
{
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
    assert_eq!(
        listed[0].worker_generation,
        restarted_snapshot.worker_generation
    );

    restarted.stop().await?;
    wait_state(&restarted, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn environment_stale_marks_only_matching_live_project_and_does_not_replace_primary()
-> Result<()> {
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
    interrupt_survival("sleep:2.0").await
}

#[tokio::test]
async fn interrupt_before_worker_start_waits_for_execution_acknowledgement() -> Result<()> {
    interrupt_survival("delayed-start").await
}

async fn interrupt_survival(source: &str) -> Result<()> {
    let temp = TempWorkspace::new("interrupt")?;
    let project = project(&temp.0.join("execute-interrupt"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let mut events = session.subscribe();

    let interrupted = session
        .execute_cell(ExecutionOrigin::Human, "blocking-cell", source)
        .await?;
    wait_state(&session, SessionState::Executing).await?;
    if source == "delayed-start" {
        timeout(Duration::from_secs(2), async {
            while !Path::new(&project.root_path).join("before-start").exists() {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await?;
    }
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

#[tokio::test]
async fn stop_during_execution_cancels_active_and_queued_operations() -> Result<()> {
    let temp = TempWorkspace::new("stop-active")?;
    let project = project(&temp.0.join("execute-stop-active"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let mut active_events = session.subscribe();
    let mut queued_events = session.subscribe();
    let mut all_events = session.subscribe();

    let active = session
        .execute_cell(ExecutionOrigin::Human, "active-cell", "sleep:2.0")
        .await?;
    wait_state(&session, SessionState::Executing).await?;
    let queued = session
        .execute_cell(ExecutionOrigin::Mcp, "queued-cell", "sleep:0.0")
        .await?;
    assert_eq!(session.snapshot().queue_depth, 1);

    session.stop().await?;
    assert_eq!(
        wait_operation_finished(&mut active_events, active).await?,
        OperationStatus::Cancelled
    );
    assert_eq!(
        wait_operation_finished(&mut queued_events, queued).await?,
        OperationStatus::Cancelled
    );
    wait_state(&session, SessionState::Stopped).await?;
    assert_eq!(session.snapshot().queue_depth, 0);
    while let Ok(event) = all_events.try_recv() {
        assert!(
            !matches!(event, SessionEvent::Failure { .. }),
            "explicit stop during execution emitted FailureRecord"
        );
    }
    Ok(())
}

#[tokio::test]
async fn crash_during_execution_fails_active_and_cancels_queued_operation() -> Result<()> {
    let temp = TempWorkspace::new("crash-queued")?;
    let project = project(&temp.0.join("execute-crash-queued"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let mut active_events = session.subscribe();
    let mut queued_events = session.subscribe();
    let mut failure_events = session.subscribe();

    let active = session
        .execute_cell(ExecutionOrigin::Human, "crashing-cell", "crash_after:0.35")
        .await?;
    wait_state(&session, SessionState::Executing).await?;
    let queued = session
        .execute_cell(ExecutionOrigin::Mcp, "queued-cell", "sleep:0.0")
        .await?;
    assert_eq!(session.snapshot().queue_depth, 1);

    let record = wait_failure(&mut failure_events).await?;
    assert_eq!(record.operation_id, Some(active));
    assert_eq!(record.exit_code, Some(23));
    assert_eq!(
        wait_operation_finished(&mut active_events, active).await?,
        OperationStatus::Failed
    );
    assert_eq!(
        wait_operation_finished(&mut queued_events, queued).await?,
        OperationStatus::Cancelled
    );
    assert_eq!(session.state(), SessionState::Crashed);
    assert_eq!(session.snapshot().queue_depth, 0);
    Ok(())
}

#[tokio::test]
async fn restart_during_execution_cancels_old_work_and_replaces_worker_generation() -> Result<()> {
    let temp = TempWorkspace::new("restart-active")?;
    let project = project(&temp.0.join("execute-restart-active"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let notebook_id = NotebookId::new();
    let first = supervisor.ensure_primary(&project, notebook_id).await?;
    let first_snapshot = first.snapshot();
    let mut active_events = first.subscribe();
    let mut queued_events = first.subscribe();

    let active = first
        .execute_cell(ExecutionOrigin::Human, "active-cell", "sleep:2.0")
        .await?;
    wait_state(&first, SessionState::Executing).await?;
    let queued = first
        .execute_cell(ExecutionOrigin::Mcp, "queued-cell", "sleep:0.0")
        .await?;
    let updated_project = Project {
        project_id: project.project_id,
        name: project.name.clone(),
        root_path: project.root_path.clone(),
        environment_generation: 2,
    };

    let restarted = supervisor
        .restart_primary(&updated_project, notebook_id)
        .await?;
    assert_eq!(
        wait_operation_finished(&mut active_events, active).await?,
        OperationStatus::Cancelled
    );
    assert_eq!(
        wait_operation_finished(&mut queued_events, queued).await?,
        OperationStatus::Cancelled
    );
    assert_eq!(first.state(), SessionState::Stopped);
    assert_eq!(restarted.session_id(), first.session_id());
    assert_eq!(restarted.snapshot().started_at, first_snapshot.started_at);
    assert_eq!(
        restarted.snapshot().worker_generation,
        first_snapshot.worker_generation + 1
    );
    assert_eq!(restarted.snapshot().environment_generation, 2);
    assert_eq!(restarted.state(), SessionState::Idle);

    let mut restarted_events = restarted.subscribe();
    let survivor = restarted
        .execute_cell(ExecutionOrigin::Human, "replacement-cell", "sleep:0.0")
        .await?;
    assert_eq!(
        wait_operation_finished(&mut restarted_events, survivor).await?,
        OperationStatus::Succeeded
    );
    restarted.stop().await?;
    wait_state(&restarted, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn crashed_primary_restarts_explicitly_with_same_logical_session() -> Result<()> {
    let temp = TempWorkspace::new("restart-crashed")?;
    let project = project(&temp.0.join("execute-restart-crashed"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let notebook_id = NotebookId::new();
    let first = supervisor.ensure_primary(&project, notebook_id).await?;
    let first_snapshot = first.snapshot();
    let mut failure_events = first.subscribe();

    let crashing = first
        .execute_cell(ExecutionOrigin::Human, "crash-cell", "crash")
        .await?;
    let record = wait_failure(&mut failure_events).await?;
    assert_eq!(record.operation_id, Some(crashing));
    assert_eq!(first.state(), SessionState::Crashed);

    let restarted = supervisor.restart_primary(&project, notebook_id).await?;
    assert_eq!(restarted.session_id(), first.session_id());
    assert_eq!(restarted.snapshot().started_at, first_snapshot.started_at);
    assert_eq!(
        restarted.snapshot().worker_generation,
        first_snapshot.worker_generation + 1
    );
    assert_eq!(restarted.state(), SessionState::Idle);

    let mut events = restarted.subscribe();
    let survivor = restarted
        .execute_cell(ExecutionOrigin::Human, "survivor-cell", "sleep:0.0")
        .await?;
    assert_eq!(
        wait_operation_finished(&mut events, survivor).await?,
        OperationStatus::Succeeded
    );
    restarted.stop().await?;
    wait_state(&restarted, SessionState::Stopped).await?;
    Ok(())
}

#[tokio::test]
async fn environment_change_during_execution_becomes_stale_after_completion_and_remains_usable()
-> Result<()> {
    let temp = TempWorkspace::new("stale-active")?;
    let project = project(&temp.0.join("execute-stale-active"))?;
    let supervisor = supervisor(&temp, Duration::from_secs(3))?;
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let mut first_events = session.subscribe();

    let first = session
        .execute_cell(ExecutionOrigin::Human, "active-cell", "sleep:0.35")
        .await?;
    wait_state(&session, SessionState::Executing).await?;
    supervisor
        .mark_project_environment_stale(project.project_id, 2)
        .await;
    assert_eq!(
        wait_operation_finished(&mut first_events, first).await?,
        OperationStatus::Succeeded
    );
    wait_state(&session, SessionState::StaleEnvironment).await?;
    assert_eq!(session.snapshot().environment_generation, 1);

    let mut second_events = session.subscribe();
    let second = session
        .execute_cell(ExecutionOrigin::Mcp, "stale-cell", "sleep:0.0")
        .await?;
    assert_eq!(
        wait_operation_finished(&mut second_events, second).await?,
        OperationStatus::Succeeded
    );
    wait_state(&session, SessionState::StaleEnvironment).await?;

    session.stop().await?;
    wait_state(&session, SessionState::Stopped).await?;
    Ok(())
}
