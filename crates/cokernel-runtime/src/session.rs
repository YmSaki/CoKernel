use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use chrono::Utc;
use cokernel_domain::{
    ExecutionOrigin, ExecutionSession, FailureClassification, FailureId, FailureRecord, NotebookId,
    OperationId, OperationStatus, Project, ProjectId, SessionId, SessionState,
};
use cokernel_protocol::worker::{self, WorkerFrame, WORKER_PROTOCOL_V1};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, watch, Mutex};
use tokio::time::{sleep, timeout};

use crate::worker::{read_worker_frame, uv_worker_command, write_worker_frame, WorkerTransportError};

#[derive(Debug, Clone)]
pub struct SessionSupervisorConfig {
    pub uv_executable: PathBuf,
    pub worker_package: PathBuf,
    pub socket_dir: PathBuf,
    pub startup_timeout: Duration,
    pub shutdown_timeout: Duration,
    pub diagnostic_tail_bytes: usize,
}

impl SessionSupervisorConfig {
    pub fn development(worker_package: impl Into<PathBuf>, socket_dir: impl Into<PathBuf>) -> Self {
        Self {
            uv_executable: PathBuf::from("uv"),
            worker_package: worker_package.into(),
            socket_dir: socket_dir.into(),
            startup_timeout: Duration::from_secs(15),
            shutdown_timeout: Duration::from_secs(3),
            diagnostic_tail_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session runtime I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    WorkerTransport(#[from] WorkerTransportError),
    #[error("worker failed to connect before startup timeout")]
    StartupTimeout,
    #[error("worker exited during startup: {0}")]
    WorkerExitedDuringStartup(String),
    #[error("worker exited unexpectedly: {0}")]
    WorkerExited(String),
    #[error("worker disconnected unexpectedly")]
    WorkerDisconnected,
    #[error("worker did not send a valid ready event: {0}")]
    InvalidReady(String),
    #[error("session command channel is closed")]
    CommandChannelClosed,
    #[error("session did not reach a terminal state before restart timeout")]
    RestartTimeout,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    StateChanged {
        session_id: SessionId,
        state: SessionState,
    },
    OperationQueued {
        session_id: SessionId,
        operation_id: OperationId,
        origin: ExecutionOrigin,
    },
    WorkerFrame {
        session_id: SessionId,
        frame: WorkerFrame,
    },
    OperationFinished {
        session_id: SessionId,
        operation_id: OperationId,
        status: OperationStatus,
    },
    Failure {
        record: FailureRecord,
    },
}

#[derive(Clone)]
pub struct SessionHandle {
    session_id: SessionId,
    project_id: ProjectId,
    notebook_id: NotebookId,
    environment_generation: u64,
    started_at: chrono::DateTime<Utc>,
    state_rx: watch::Receiver<SessionState>,
    execute_tx: mpsc::Sender<ExecuteRequest>,
    control_tx: mpsc::Sender<ControlRequest>,
    events: broadcast::Sender<SessionEvent>,
    queue_depth: Arc<AtomicUsize>,
    current_operation: Arc<StdMutex<Option<OperationId>>>,
}

impl SessionHandle {
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn state(&self) -> SessionState {
        *self.state_rx.borrow()
    }

    pub fn snapshot(&self) -> ExecutionSession {
        ExecutionSession {
            session_id: self.session_id,
            project_id: self.project_id,
            notebook_id: self.notebook_id,
            state: self.state(),
            environment_generation: self.environment_generation,
            started_at: self.started_at,
            current_operation_id: *self
                .current_operation
                .lock()
                .expect("operation lock poisoned"),
            queue_depth: self.queue_depth.load(Ordering::Relaxed),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.events.subscribe()
    }

    pub async fn execute_cell(
        &self,
        origin: ExecutionOrigin,
        cell_id: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<OperationId, SessionError> {
        let operation_id = OperationId::new();
        self.queue_depth.fetch_add(1, Ordering::Relaxed);
        let request = ExecuteRequest {
            operation_id,
            origin,
            cell_id: Some(cell_id.into()),
            source: source.into(),
        };
        if self.execute_tx.send(request).await.is_err() {
            self.queue_depth.fetch_sub(1, Ordering::Relaxed);
            return Err(SessionError::CommandChannelClosed);
        }
        let _ = self.events.send(SessionEvent::OperationQueued {
            session_id: self.session_id,
            operation_id,
            origin,
        });
        Ok(operation_id)
    }

    pub async fn interrupt(&self) -> Result<(), SessionError> {
        self.control_tx
            .send(ControlRequest::Interrupt)
            .await
            .map_err(|_| SessionError::CommandChannelClosed)
    }

    pub async fn stop(&self) -> Result<(), SessionError> {
        self.control_tx
            .send(ControlRequest::Stop)
            .await
            .map_err(|_| SessionError::CommandChannelClosed)
    }

    pub async fn mark_environment_stale(&self) -> Result<(), SessionError> {
        self.control_tx
            .send(ControlRequest::MarkEnvironmentStale)
            .await
            .map_err(|_| SessionError::CommandChannelClosed)
    }
}

#[derive(Clone)]
pub struct SessionSupervisor {
    config: SessionSupervisorConfig,
    sessions: Arc<Mutex<HashMap<SessionId, SessionHandle>>>,
    primary_by_notebook: Arc<Mutex<HashMap<NotebookId, SessionId>>>,
    start_lock: Arc<Mutex<()>>,
}

impl SessionSupervisor {
    pub fn new(config: SessionSupervisorConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            primary_by_notebook: Arc::new(Mutex::new(HashMap::new())),
            start_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn ensure_primary(
        &self,
        project: &Project,
        notebook_id: NotebookId,
    ) -> Result<SessionHandle, SessionError> {
        let _guard = self.start_lock.lock().await;
        if let Some(existing) = self.primary_handle(notebook_id).await {
            if is_live_state(existing.state()) {
                return Ok(existing);
            }
        }

        let handle = self
            .start_worker(project, notebook_id, SessionId::new())
            .await?;
        self.primary_by_notebook
            .lock()
            .await
            .insert(notebook_id, handle.session_id());
        self.sessions
            .lock()
            .await
            .insert(handle.session_id(), handle.clone());
        Ok(handle)
    }

    pub async fn get(&self, session_id: SessionId) -> Option<SessionHandle> {
        self.sessions.lock().await.get(&session_id).cloned()
    }

    pub async fn list(&self) -> Vec<ExecutionSession> {
        self.sessions
            .lock()
            .await
            .values()
            .map(SessionHandle::snapshot)
            .collect()
    }

    pub async fn restart_primary(
        &self,
        project: &Project,
        notebook_id: NotebookId,
    ) -> Result<SessionHandle, SessionError> {
        let _guard = self.start_lock.lock().await;
        let existing = self.primary_handle(notebook_id).await;
        let session_id = match existing {
            Some(existing) => {
                if is_live_state(existing.state()) {
                    if existing.state() != SessionState::Stopping {
                        match existing.stop().await {
                            Ok(()) => {}
                            Err(error) if !is_live_state(existing.state()) => {}
                            Err(error) => return Err(error),
                        }
                    }
                    if !wait_until_terminal(&existing, self.config.shutdown_timeout).await {
                        return Err(SessionError::RestartTimeout);
                    }
                }
                existing.session_id()
            }
            None => SessionId::new(),
        };

        let handle = self.start_worker(project, notebook_id, session_id).await?;
        self.primary_by_notebook
            .lock()
            .await
            .insert(notebook_id, handle.session_id());
        self.sessions
            .lock()
            .await
            .insert(handle.session_id(), handle.clone());
        Ok(handle)
    }

    pub async fn mark_project_environment_stale(
        &self,
        project_id: ProjectId,
        current_generation: u64,
    ) {
        let handles = self
            .sessions
            .lock()
            .await
            .values()
            .filter(|handle| {
                handle.project_id == project_id
                    && handle.environment_generation != current_generation
                    && is_live_state(handle.state())
            })
            .cloned()
            .collect::<Vec<_>>();
        for handle in handles {
            let _ = handle.mark_environment_stale().await;
        }
    }

    async fn primary_handle(&self, notebook_id: NotebookId) -> Option<SessionHandle> {
        let session_id = self
            .primary_by_notebook
            .lock()
            .await
            .get(&notebook_id)
            .copied()?;
        self.get(session_id).await
    }

    async fn start_worker(
        &self,
        project: &Project,
        notebook_id: NotebookId,
        session_id: SessionId,
    ) -> Result<SessionHandle, SessionError> {
        prepare_socket_dir(&self.config.socket_dir)?;
        let socket_path = self.config.socket_dir.join(format!("{session_id}.sock"));
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;

        let mut command = uv_worker_command(
            &self.config.uv_executable,
            Path::new(&project.root_path),
            &self.config.worker_package,
            &socket_path,
            session_id,
        );
        let mut child = command.spawn()?;
        let stderr_tail = Arc::new(Mutex::new(ByteTail::new(
            self.config.diagnostic_tail_bytes,
        )));
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(drain_tail(
                stdout,
                Arc::new(Mutex::new(ByteTail::new(8 * 1024))),
            ));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(drain_tail(stderr, stderr_tail.clone()));
        }

        let (stream, _) = tokio::select! {
            accepted = listener.accept() => accepted?,
            status = child.wait() => {
                let status = status?;
                let _ = fs::remove_file(&socket_path);
                return Err(SessionError::WorkerExitedDuringStartup(format_exit_status(status)));
            }
            _ = sleep(self.config.startup_timeout) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                let _ = fs::remove_file(&socket_path);
                return Err(SessionError::StartupTimeout);
            }
        };

        let mut stream = stream;
        let ready = timeout(self.config.startup_timeout, read_worker_frame(&mut stream))
            .await
            .map_err(|_| SessionError::StartupTimeout)??
            .ok_or_else(|| SessionError::InvalidReady("worker disconnected before ready".into()))?;
        let worker_pid = validate_ready(&ready, session_id)?;
        let _ = fs::remove_file(&socket_path);

        let started_at = Utc::now();
        let (state_tx, state_rx) = watch::channel(SessionState::Idle);
        let (execute_tx, execute_rx) = mpsc::channel(128);
        let (control_tx, control_rx) = mpsc::channel(16);
        let (events, _) = broadcast::channel(512);
        let queue_depth = Arc::new(AtomicUsize::new(0));
        let current_operation = Arc::new(StdMutex::new(None));

        let handle = SessionHandle {
            session_id,
            project_id: project.project_id,
            notebook_id,
            environment_generation: project.environment_generation,
            started_at,
            state_rx,
            execute_tx,
            control_tx,
            events: events.clone(),
            queue_depth: queue_depth.clone(),
            current_operation: current_operation.clone(),
        };

        let _ = events.send(SessionEvent::StateChanged {
            session_id,
            state: SessionState::Idle,
        });

        tokio::spawn(run_session_actor(
            SessionActorContext {
                session_id,
                worker_pid,
                state_tx,
                events,
                queue_depth,
                current_operation,
                stderr_tail,
                shutdown_timeout: self.config.shutdown_timeout,
            },
            stream,
            child,
            execute_rx,
            control_rx,
        ));

        Ok(handle)
    }
}

fn is_live_state(state: SessionState) -> bool {
    !matches!(
        state,
        SessionState::Stopped | SessionState::Crashed | SessionState::Error
    )
}

async fn wait_until_terminal(handle: &SessionHandle, maximum: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + maximum;
    while is_live_state(handle.state()) && tokio::time::Instant::now() < deadline {
        sleep(Duration::from_millis(25)).await;
    }
    !is_live_state(handle.state())
}

fn prepare_socket_dir(path: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn validate_ready(frame: &WorkerFrame, expected_session_id: SessionId) -> Result<u32, SessionError> {
    match frame {
        WorkerFrame::Event {
            protocol,
            session_id,
            event,
            payload,
        } if *protocol == WORKER_PROTOCOL_V1
            && session_id == &expected_session_id.to_string()
            && event == worker::event::READY =>
        {
            let pid = payload
                .get("pid")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| SessionError::InvalidReady("ready event is missing worker pid".into()))?;
            Ok(pid)
        }
        other => Err(SessionError::InvalidReady(format!("{other:?}"))),
    }
}

#[derive(Debug)]
struct ExecuteRequest {
    operation_id: OperationId,
    origin: ExecutionOrigin,
    cell_id: Option<String>,
    source: String,
}

#[derive(Debug)]
enum ControlRequest {
    Interrupt,
    Stop,
    MarkEnvironmentStale,
}

struct SessionActorContext {
    session_id: SessionId,
    worker_pid: u32,
    state_tx: watch::Sender<SessionState>,
    events: broadcast::Sender<SessionEvent>,
    queue_depth: Arc<AtomicUsize>,
    current_operation: Arc<StdMutex<Option<OperationId>>>,
    stderr_tail: Arc<Mutex<ByteTail>>,
    shutdown_timeout: Duration,
}

async fn run_session_actor(
    context: SessionActorContext,
    mut stream: UnixStream,
    mut child: Child,
    mut execute_rx: mpsc::Receiver<ExecuteRequest>,
    mut control_rx: mpsc::Receiver<ControlRequest>,
) {
    let mut stale_environment = false;
    loop {
        tokio::select! {
            biased;
            control = control_rx.recv() => {
                match control {
                    Some(ControlRequest::Stop) | None => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        graceful_stop(&context, &mut stream, &mut child).await;
                        return;
                    }
                    Some(ControlRequest::MarkEnvironmentStale) => {
                        stale_environment = true;
                        set_state(&context, SessionState::StaleEnvironment);
                    }
                    Some(ControlRequest::Interrupt) => {}
                }
            }
            request = execute_rx.recv() => {
                let Some(request) = request else {
                    graceful_stop(&context, &mut stream, &mut child).await;
                    return;
                };
                context.queue_depth.fetch_sub(1, Ordering::Relaxed);
                match execute_one(
                    &context,
                    &mut stream,
                    &mut child,
                    &mut control_rx,
                    request,
                    &mut stale_environment,
                ).await {
                    Ok(ExecuteDisposition::Completed) => {}
                    Ok(ExecuteDisposition::Stopped) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        return;
                    }
                    Err(error) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        record_crash(&context, &mut child, error.to_string()).await;
                        return;
                    }
                }
            }
            _ = sleep(Duration::from_millis(250)) => {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        record_exit(&context, status).await;
                        return;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        record_crash(&context, &mut child, error.to_string()).await;
                        return;
                    }
                }
            }
        }
    }
}

enum ExecuteDisposition {
    Completed,
    Stopped,
}

async fn execute_one(
    context: &SessionActorContext,
    stream: &mut UnixStream,
    child: &mut Child,
    control_rx: &mut mpsc::Receiver<ControlRequest>,
    request: ExecuteRequest,
    stale_environment: &mut bool,
) -> Result<ExecuteDisposition, SessionError> {
    *context
        .current_operation
        .lock()
        .expect("operation lock poisoned") = Some(request.operation_id);
    set_state(context, SessionState::Executing);

    let request_id = request.operation_id.to_string();
    let frame = WorkerFrame::Request {
        protocol: WORKER_PROTOCOL_V1,
        id: request_id.clone(),
        session_id: context.session_id.to_string(),
        method: worker::method::EXECUTE.into(),
        payload: json!({
            "operation_id": request.operation_id.to_string(),
            "cell_id": request.cell_id,
            "source": request.source,
            "origin": request.origin,
        }),
    };
    write_worker_frame(stream, &frame).await?;

    let mut interrupted = false;
    loop {
        tokio::select! {
            control = control_rx.recv() => {
                match control {
                    Some(ControlRequest::Interrupt) => {
                        interrupted = true;
                        set_state(context, SessionState::Interrupting);
                        send_sigint(context.worker_pid).await?;
                    }
                    Some(ControlRequest::MarkEnvironmentStale) => {
                        *stale_environment = true;
                    }
                    Some(ControlRequest::Stop) | None => {
                        finish_operation(context, request.operation_id, OperationStatus::Cancelled);
                        let _ = child.start_kill();
                        let _ = child.wait().await;
                        set_state(context, SessionState::Stopped);
                        return Ok(ExecuteDisposition::Stopped);
                    }
                }
            }
            frame = read_worker_frame(stream) => {
                let Some(frame) = frame? else {
                    return Err(SessionError::WorkerDisconnected);
                };
                let _ = context.events.send(SessionEvent::WorkerFrame {
                    session_id: context.session_id,
                    frame: frame.clone(),
                });
                if let WorkerFrame::Response { id, ok, result, .. } = &frame {
                    if id == &request_id {
                        let succeeded = *ok
                            && result
                                .as_ref()
                                .and_then(|value| value.get("status"))
                                .and_then(Value::as_str)
                                == Some("SUCCEEDED");
                        let status = if interrupted {
                            OperationStatus::Interrupted
                        } else if succeeded {
                            OperationStatus::Succeeded
                        } else {
                            OperationStatus::Failed
                        };
                        finish_operation(context, request.operation_id, status);
                        set_state(
                            context,
                            if *stale_environment {
                                SessionState::StaleEnvironment
                            } else {
                                SessionState::Idle
                            },
                        );
                        return Ok(ExecuteDisposition::Completed);
                    }
                }
            }
            _ = sleep(Duration::from_millis(250)) => {
                if let Some(status) = child.try_wait()? {
                    return Err(SessionError::WorkerExited(format_exit_status(status)));
                }
            }
        }
    }
}

fn finish_operation(
    context: &SessionActorContext,
    operation_id: OperationId,
    status: OperationStatus,
) {
    let _ = context.events.send(SessionEvent::OperationFinished {
        session_id: context.session_id,
        operation_id,
        status,
    });
    clear_current_operation(context);
}

fn finish_current_operation(context: &SessionActorContext, status: OperationStatus) {
    let operation_id = *context
        .current_operation
        .lock()
        .expect("operation lock poisoned");
    if let Some(operation_id) = operation_id {
        finish_operation(context, operation_id, status);
    }
}

fn cancel_pending_requests(
    context: &SessionActorContext,
    execute_rx: &mut mpsc::Receiver<ExecuteRequest>,
) {
    execute_rx.close();
    while let Ok(request) = execute_rx.try_recv() {
        let _ = context.events.send(SessionEvent::OperationFinished {
            session_id: context.session_id,
            operation_id: request.operation_id,
            status: OperationStatus::Cancelled,
        });
    }
    context.queue_depth.store(0, Ordering::Relaxed);
}

fn clear_current_operation(context: &SessionActorContext) {
    *context
        .current_operation
        .lock()
        .expect("operation lock poisoned") = None;
}

fn set_state(context: &SessionActorContext, state: SessionState) {
    context.state_tx.send_replace(state);
    let _ = context.events.send(SessionEvent::StateChanged {
        session_id: context.session_id,
        state,
    });
}

async fn send_sigint(pid: u32) -> Result<(), std::io::Error> {
    let status = Command::new("/bin/kill")
        .arg("-INT")
        .arg(pid.to_string())
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "kill -INT {pid} returned {status}"
        )))
    }
}

async fn graceful_stop(
    context: &SessionActorContext,
    stream: &mut UnixStream,
    child: &mut Child,
) {
    set_state(context, SessionState::Stopping);
    let request = WorkerFrame::Request {
        protocol: WORKER_PROTOCOL_V1,
        id: format!("shutdown-{}", context.session_id),
        session_id: context.session_id.to_string(),
        method: worker::method::SHUTDOWN.into(),
        payload: json!({}),
    };
    let _ = write_worker_frame(stream, &request).await;
    match timeout(context.shutdown_timeout, child.wait()).await {
        Ok(Ok(_)) => {}
        _ => {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }
    clear_current_operation(context);
    set_state(context, SessionState::Stopped);
}

async fn record_exit(context: &SessionActorContext, status: ExitStatus) {
    let record = failure_record(
        context,
        status.code(),
        status.signal(),
        classify_exit(status),
        0.8,
    )
    .await;
    finish_current_operation(context, OperationStatus::Failed);
    set_state(context, SessionState::Crashed);
    let _ = context.events.send(SessionEvent::Failure { record });
}

async fn record_crash(context: &SessionActorContext, child: &mut Child, detail: String) {
    let status = child.try_wait().ok().flatten();
    if status.is_none() {
        let _ = child.start_kill();
    }
    let status = match status {
        Some(status) => Some(status),
        None => child.wait().await.ok(),
    };
    let mut record = failure_record(
        context,
        status.as_ref().and_then(ExitStatus::code),
        status.as_ref().and_then(ExitStatusExt::signal),
        status
            .map(classify_exit)
            .unwrap_or(FailureClassification::Unknown),
        0.5,
    )
    .await;
    if !detail.is_empty() {
        if !record.last_stderr.is_empty() {
            record.last_stderr.push_str("\n");
        }
        record.last_stderr.push_str(&detail);
    }
    finish_current_operation(context, OperationStatus::Failed);
    set_state(context, SessionState::Crashed);
    let _ = context.events.send(SessionEvent::Failure { record });
}

async fn failure_record(
    context: &SessionActorContext,
    exit_code: Option<i32>,
    signal: Option<i32>,
    classification: FailureClassification,
    confidence: f32,
) -> FailureRecord {
    let last_stderr = context.stderr_tail.lock().await.text_lossy();
    FailureRecord {
        failure_id: FailureId::new(),
        component: "session-worker".into(),
        session_id: Some(context.session_id),
        operation_id: *context
            .current_operation
            .lock()
            .expect("operation lock poisoned"),
        timestamp: Utc::now(),
        exit_code,
        signal,
        last_stderr,
        classification,
        confidence,
    }
}

fn classify_exit(status: ExitStatus) -> FailureClassification {
    if status.signal().is_some() {
        FailureClassification::ProcessSignal
    } else {
        FailureClassification::Unknown
    }
}

fn format_exit_status(status: ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit code {code}"),
        (_, Some(signal)) => format!("signal {signal}"),
        _ => status.to_string(),
    }
}

struct ByteTail {
    bytes: Vec<u8>,
    maximum: usize,
}

impl ByteTail {
    fn new(maximum: usize) -> Self {
        Self {
            bytes: Vec::new(),
            maximum,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        if self.maximum == 0 {
            return;
        }
        if chunk.len() >= self.maximum {
            self.bytes.clear();
            self.bytes
                .extend_from_slice(&chunk[chunk.len() - self.maximum..]);
            return;
        }
        let overflow = self
            .bytes
            .len()
            .saturating_add(chunk.len())
            .saturating_sub(self.maximum);
        if overflow > 0 {
            self.bytes.drain(..overflow);
        }
        self.bytes.extend_from_slice(chunk);
    }

    fn text_lossy(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

async fn drain_tail<R>(mut reader: R, tail: Arc<Mutex<ByteTail>>)
where
    R: AsyncRead + Unpin,
{
    let mut buffer = [0_u8; 4096];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => tail.lock().await.push(&buffer[..read]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_tail_keeps_only_recent_bytes() {
        let mut tail = ByteTail::new(5);
        tail.push(b"abc");
        tail.push(b"def");
        assert_eq!(tail.text_lossy(), "bcdef");
        tail.push(b"0123456789");
        assert_eq!(tail.text_lossy(), "56789");
    }

    #[tokio::test]
    async fn cancelling_pending_requests_closes_queue_and_finishes_each_operation() {
        let session_id = SessionId::new();
        let (state_tx, _state_rx) = watch::channel(SessionState::Idle);
        let (events, _) = broadcast::channel(16);
        let mut event_rx = events.subscribe();
        let queue_depth = Arc::new(AtomicUsize::new(2));
        let current_operation = Arc::new(StdMutex::new(None));
        let context = SessionActorContext {
            session_id,
            worker_pid: 1,
            state_tx,
            events,
            queue_depth: queue_depth.clone(),
            current_operation,
            stderr_tail: Arc::new(Mutex::new(ByteTail::new(16))),
            shutdown_timeout: Duration::from_millis(10),
        };
        let (execute_tx, mut execute_rx) = mpsc::channel(4);
        let first = OperationId::new();
        let second = OperationId::new();
        for operation_id in [first, second] {
            execute_tx
                .send(ExecuteRequest {
                    operation_id,
                    origin: ExecutionOrigin::Human,
                    cell_id: Some("cell".into()),
                    source: "1 + 1".into(),
                })
                .await
                .unwrap();
        }

        cancel_pending_requests(&context, &mut execute_rx);

        assert_eq!(queue_depth.load(Ordering::Relaxed), 0);
        assert!(execute_tx
            .send(ExecuteRequest {
                operation_id: OperationId::new(),
                origin: ExecutionOrigin::Human,
                cell_id: None,
                source: "2 + 2".into(),
            })
            .await
            .is_err());

        let mut cancelled = Vec::new();
        for _ in 0..2 {
            match event_rx.recv().await.unwrap() {
                SessionEvent::OperationFinished {
                    session_id: actual_session_id,
                    operation_id,
                    status: OperationStatus::Cancelled,
                } => {
                    assert_eq!(actual_session_id, session_id);
                    cancelled.push(operation_id);
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert!(cancelled.contains(&first));
        assert!(cancelled.contains(&second));
    }
}
