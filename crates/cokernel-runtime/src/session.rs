use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use cokernel_domain::{
    ExecutionOrigin, ExecutionSession, FailureClassification, FailureId, FailureRecord, NotebookId,
    OperationId, OperationStatus, Project, ProjectId, SessionId, SessionState,
};
use cokernel_protocol::worker::{self, WorkerFrame, WORKER_PROTOCOL_V1};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::{unix::OwnedReadHalf, unix::OwnedWriteHalf, UnixListener};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, oneshot, watch, Mutex};
use tokio::time::{sleep, timeout};

use crate::worker::{read_worker_frame, uv_worker_command, write_worker_frame, WorkerTransportError};

const EXECUTE_QUEUE_CAPACITY: usize = 128;
const INSPECTION_QUEUE_CAPACITY: usize = 32;
const CONTROL_QUEUE_CAPACITY: usize = 16;
const EVENT_QUEUE_CAPACITY: usize = 512;
const WORKER_FRAME_QUEUE_CAPACITY: usize = 512;
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(250);
const RESTART_STOP_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct SessionSupervisorConfig {
    pub uv_executable: PathBuf,
    pub worker_package: PathBuf,
    pub socket_dir: PathBuf,
    pub startup_timeout: Duration,
    pub shutdown_timeout: Duration,
    pub heartbeat_timeout: Duration,
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
            heartbeat_timeout: Duration::from_secs(10),
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
    #[error("worker heartbeat timed out after {0:?}")]
    WorkerHeartbeatTimeout(Duration),
    #[error("worker did not send a valid ready event: {0}")]
    InvalidReady(String),
    #[error("session command channel is closed")]
    CommandChannelClosed,
    #[error("session {0} did not stop before restart timeout")]
    StopTimeout(SessionId),
}

#[derive(Debug, Error)]
pub enum SessionInspectionError {
    #[error("session inspection channel is closed")]
    CommandChannelClosed,
    #[error("worker inspection is unavailable: {0}")]
    Unavailable(String),
    #[error("worker rejected inspection request ({code}): {summary}")]
    WorkerRequest { code: String, summary: String },
    #[error("worker inspection response is invalid: {0}")]
    InvalidResponse(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionVariableSummary {
    pub name: String,
    pub type_module: String,
    pub type_name: String,
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionVariableValue {
    pub name: String,
    pub type_module: String,
    pub type_name: String,
    pub supported: bool,
    pub value: Option<Value>,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionVariableListResult {
    variables: Vec<SessionVariableSummary>,
}

struct InspectionRequest {
    method: &'static str,
    payload: Value,
    response: oneshot::Sender<Result<Value, SessionInspectionError>>,
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
    worker_generation: u64,
    environment_generation: u64,
    started_at: DateTime<Utc>,
    state_rx: watch::Receiver<SessionState>,
    execute_tx: mpsc::Sender<ExecuteRequest>,
    inspection_tx: mpsc::Sender<InspectionRequest>,
    control_tx: mpsc::Sender<ControlRequest>,
    events: broadcast::Sender<SessionEvent>,
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
            worker_generation: self.worker_generation,
            environment_generation: self.environment_generation,
            started_at: self.started_at,
            current_operation_id: *self
                .current_operation
                .lock()
                .expect("operation lock poisoned"),
            queue_depth: queue_depth(&self.execute_tx),
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
        let request = ExecuteRequest {
            operation_id,
            origin,
            cell_id: Some(cell_id.into()),
            source: source.into(),
        };
        let permit = self
            .execute_tx
            .reserve()
            .await
            .map_err(|_| SessionError::CommandChannelClosed)?;
        let _ = self.events.send(SessionEvent::OperationQueued {
            session_id: self.session_id,
            operation_id,
            origin,
        });
        permit.send(request);
        Ok(operation_id)
    }

    pub async fn list_variables(
        &self,
    ) -> Result<Vec<SessionVariableSummary>, SessionInspectionError> {
        let result = self
            .inspect_worker(worker::method::INSPECT_VARIABLES, json!({}))
            .await?;
        let response: SessionVariableListResult = serde_json::from_value(result)
            .map_err(|error| SessionInspectionError::InvalidResponse(error.to_string()))?;
        Ok(response.variables)
    }

    pub async fn get_variable(
        &self,
        name: impl Into<String>,
    ) -> Result<SessionVariableValue, SessionInspectionError> {
        let name = name.into();
        let result = self
            .inspect_worker(
                worker::method::GET_VARIABLE,
                json!({ "name": name.clone() }),
            )
            .await?;
        let value: SessionVariableValue = serde_json::from_value(result)
            .map_err(|error| SessionInspectionError::InvalidResponse(error.to_string()))?;
        if value.name != name {
            return Err(SessionInspectionError::InvalidResponse(format!(
                "worker returned variable {:?} for requested {:?}",
                value.name, name
            )));
        }
        Ok(value)
    }

    async fn inspect_worker(
        &self,
        method: &'static str,
        payload: Value,
    ) -> Result<Value, SessionInspectionError> {
        let (response, receiver) = oneshot::channel();
        self.inspection_tx
            .send(InspectionRequest {
                method,
                payload,
                response,
            })
            .await
            .map_err(|_| SessionInspectionError::CommandChannelClosed)?;
        receiver
            .await
            .map_err(|_| SessionInspectionError::CommandChannelClosed)?
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

fn queue_depth(sender: &mpsc::Sender<ExecuteRequest>) -> usize {
    if sender.is_closed() {
        0
    } else {
        sender.max_capacity().saturating_sub(sender.capacity())
    }
}

include!("session/supervisor.rs");
include!("session/actor.rs");
include!("session/lifecycle.rs");

#[cfg(test)]
include!("session/tests.rs");
