use std::collections::{HashMap, HashSet};
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
use cokernel_protocol::worker::{self, WORKER_PROTOCOL_V1, WorkerFrame};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::{UnixListener, unix::OwnedReadHalf, unix::OwnedWriteHalf};
use tokio::process::Child;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot, watch};
use tokio::time::{sleep, timeout};

use crate::failure_store::FailureStore;
use crate::worker::{
    WorkerTransportError, read_worker_frame, uv_worker_command, write_worker_frame,
};

const EXECUTE_QUEUE_CAPACITY: usize = 128;
const INSPECTION_QUEUE_CAPACITY: usize = 32;
const EVENT_QUEUE_CAPACITY: usize = 512;
const WORKER_FRAME_QUEUE_CAPACITY: usize = 512;
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(250);
const RESTART_STOP_GRACE: Duration = Duration::from_millis(500);
const MAX_INSPECTION_LIST_ITEMS: usize = 512;
const MAX_INSPECTION_RESPONSE_BYTES: usize = 1_048_576;
const MAX_INSPECTION_METADATA_CHARS: usize = 65_536;

#[derive(Debug, Clone)]
pub struct SessionSupervisorConfig {
    pub workload_identity: crate::workload_identity::WorkloadIdentity,
    pub uv_executable: PathBuf,
    pub worker_package: PathBuf,
    pub socket_dir: PathBuf,
    pub failure_dir: PathBuf,
    pub startup_timeout: Duration,
    pub shutdown_timeout: Duration,
    pub heartbeat_timeout: Duration,
    pub diagnostic_tail_bytes: usize,
}

impl SessionSupervisorConfig {
    pub fn development(worker_package: impl Into<PathBuf>, socket_dir: impl Into<PathBuf>) -> Self {
        let socket_dir = socket_dir.into();
        Self {
            workload_identity: Default::default(),
            uv_executable: PathBuf::from("uv"),
            worker_package: worker_package.into(),
            failure_dir: socket_dir.join("failures"),
            socket_dir,
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
    #[error("Session does not belong to the requested Project")]
    OwnershipMismatch,
}

#[derive(Debug, Error)]
pub enum SessionInspectionError {
    #[error("session inspection channel is closed")]
    CommandChannelClosed,
    #[error("session inspection request is invalid: {0}")]
    InvalidRequest(String),
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

fn invalid_inspection_response(message: impl Into<String>) -> SessionInspectionError {
    SessionInspectionError::InvalidResponse(message.into())
}

fn validate_inspection_response_size(
    label: &str,
    result: &Value,
) -> Result<(), SessionInspectionError> {
    let bytes = serde_json::to_vec(result).map_err(|error| {
        invalid_inspection_response(format!("{label} is not JSON encodable: {error}"))
    })?;
    if bytes.len() > MAX_INSPECTION_RESPONSE_BYTES {
        return Err(invalid_inspection_response(format!(
            "{label} exceeds maximum serialized response size"
        )));
    }
    Ok(())
}

fn validate_inspection_metadata(
    label: &str,
    field: &str,
    value: &str,
) -> Result<(), SessionInspectionError> {
    if value.is_empty() {
        return Err(invalid_inspection_response(format!(
            "{label} field {field:?} must be non-empty"
        )));
    }
    if value.chars().count() > MAX_INSPECTION_METADATA_CHARS {
        return Err(invalid_inspection_response(format!(
            "{label} field {field:?} exceeds maximum length"
        )));
    }
    Ok(())
}

fn validate_variable_list_result(
    result: Value,
) -> Result<Vec<SessionVariableSummary>, SessionInspectionError> {
    validate_inspection_response_size("list_variables result", &result)?;
    let response: SessionVariableListResult = serde_json::from_value(result)
        .map_err(|error| invalid_inspection_response(error.to_string()))?;
    if response.variables.len() > MAX_INSPECTION_LIST_ITEMS {
        return Err(invalid_inspection_response(
            "list_variables result exceeds maximum item count",
        ));
    }

    let mut names = HashSet::with_capacity(response.variables.len());
    for variable in &response.variables {
        validate_inspection_metadata("list_variables result", "name", &variable.name)?;
        validate_inspection_metadata(
            "list_variables result",
            "type_module",
            &variable.type_module,
        )?;
        validate_inspection_metadata("list_variables result", "type_name", &variable.type_name)?;
        if !names.insert(variable.name.clone()) {
            return Err(invalid_inspection_response(format!(
                "list_variables returned duplicate variable {:?}",
                variable.name
            )));
        }
    }
    Ok(response.variables)
}

fn validate_variable_value_result(
    result: Value,
    requested_name: &str,
) -> Result<SessionVariableValue, SessionInspectionError> {
    validate_inspection_response_size("get_variable result", &result)?;
    let object = result
        .as_object()
        .ok_or_else(|| invalid_inspection_response("get_variable result must be an object"))?;
    for field in [
        "name",
        "type_module",
        "type_name",
        "supported",
        "value",
        "reason",
    ] {
        if !object.contains_key(field) {
            return Err(invalid_inspection_response(format!(
                "get_variable result omitted field {field:?}"
            )));
        }
    }

    let raw_value_is_null = object.get("value").is_some_and(Value::is_null);
    let value: SessionVariableValue = serde_json::from_value(result)
        .map_err(|error| invalid_inspection_response(error.to_string()))?;
    if value.name != requested_name {
        return Err(invalid_inspection_response(format!(
            "worker returned variable {:?} for requested {:?}",
            value.name, requested_name
        )));
    }
    validate_inspection_metadata("get_variable result", "name", &value.name)?;
    validate_inspection_metadata("get_variable result", "type_module", &value.type_module)?;
    validate_inspection_metadata("get_variable result", "type_name", &value.type_name)?;

    if value.supported {
        if value.reason.is_some() {
            return Err(invalid_inspection_response(
                "supported get_variable result must not include a reason",
            ));
        }
    } else {
        if !raw_value_is_null {
            return Err(invalid_inspection_response(
                "unsupported get_variable result must carry null value",
            ));
        }
        let reason = value
            .reason
            .as_deref()
            .filter(|reason| !reason.is_empty())
            .ok_or_else(|| {
                invalid_inspection_response(
                    "unsupported get_variable result requires a non-empty reason",
                )
            })?;
        if reason.chars().count() > MAX_INSPECTION_METADATA_CHARS {
            return Err(invalid_inspection_response(
                "get_variable result reason exceeds maximum length",
            ));
        }
    }

    Ok(value)
}

fn validate_inspection_request_frame(
    session_id: SessionId,
    method: &'static str,
    payload: &Value,
) -> Result<(), SessionInspectionError> {
    let frame = WorkerFrame::Request {
        protocol: WORKER_PROTOCOL_V1,
        id: format!("inspect-{}", OperationId::new()),
        session_id: session_id.to_string(),
        method: method.into(),
        payload: payload.clone(),
    };
    cokernel_protocol::encode_json_frame(&frame, cokernel_protocol::DEFAULT_MAX_FRAME_BYTES)
        .map(|_| ())
        .map_err(|error| SessionInspectionError::InvalidRequest(error.to_string()))
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
        record: Box<FailureRecord>,
    },
    FailurePersistenceFailed {
        session_id: SessionId,
        failure_id: FailureId,
        error: String,
    },
}

#[derive(Clone)]
struct SessionEventBus {
    local: broadcast::Sender<SessionEvent>,
    global: crate::events::EventHub,
    project_id: ProjectId,
    notebook_id: NotebookId,
}
impl SessionEventBus {
    fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.local.subscribe()
    }
    fn send(&self, event: SessionEvent) -> Option<usize> {
        let observation = match &event {
            SessionEvent::StateChanged { session_id, state } => Some((
                "session.state_changed",
                json!({"session_id":session_id,"state":state}),
            )),
            SessionEvent::OperationQueued {
                session_id,
                operation_id,
                origin,
            } => Some((
                "session.execution_queued",
                json!({"session_id":session_id,"operation_id":operation_id,"origin":origin}),
            )),
            SessionEvent::OperationFinished {
                session_id,
                operation_id,
                status,
            } => Some((
                "session.execution_finished",
                json!({"session_id":session_id,"operation_id":operation_id,"status":status}),
            )),
            SessionEvent::Failure { record } => Some((
                "session.failure",
                json!({"session_id":record.session_id,"failure_id":record.failure_id,"classification":record.classification}),
            )),
            SessionEvent::FailurePersistenceFailed {
                session_id,
                failure_id,
                ..
            } => Some((
                "session.failure_persistence_failed",
                json!({"session_id":session_id,"failure_id":failure_id}),
            )),
            SessionEvent::WorkerFrame {
                session_id,
                frame: WorkerFrame::Event { event, payload, .. },
            } => match event.as_str() {
                worker::event::EXECUTION_STARTED => Some((
                    "session.execution_started",
                    json!({"session_id":session_id,"data":payload}),
                )),
                worker::event::STDOUT
                | worker::event::STDERR
                | worker::event::EXECUTE_RESULT
                | worker::event::DISPLAY_DATA
                | worker::event::ERROR
                | worker::event::WORKER_WARNING => Some((
                    "session.output",
                    json!({"session_id":session_id,"kind":event,"data":payload}),
                )),
                _ => None,
            },
            _ => None,
        };
        if let Some((name, mut payload)) = observation {
            payload["project_id"] = json!(self.project_id);
            payload["notebook_id"] = json!(self.notebook_id);
            self.global.publish(name, payload);
        }
        self.local.send(event).ok()
    }
}

#[derive(Debug, Clone)]
pub struct SessionFailure {
    pub record: FailureRecord,
    /// None means the durable write completed; Some means only live evidence is available.
    pub persistence_error: Option<String>,
}

#[derive(Debug)]
pub struct ExecutionCapture {
    pub operation_id: OperationId,
    pub status: OperationStatus,
    /// Validated ordered frames. Overflow is explicit; a partial capture is
    /// never handed to notebook persistence as if it were complete.
    pub frames: Result<Vec<WorkerFrame>, String>,
}

pub struct ExecutionReceipt {
    pub operation_id: OperationId,
    completion: oneshot::Receiver<ExecutionCapture>,
}

impl ExecutionReceipt {
    pub async fn completed(self) -> Result<ExecutionCapture, SessionError> {
        self.completion
            .await
            .map_err(|_| SessionError::CommandChannelClosed)
    }
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
    control_tx: control::ControlSender,
    events: SessionEventBus,
    current_operation: Arc<StdMutex<Option<OperationId>>>,
    last_failure: watch::Receiver<Option<SessionFailure>>,
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

    pub fn last_failure(&self) -> Option<SessionFailure> {
        self.last_failure.borrow().clone()
    }

    pub async fn execute_cell(
        &self,
        origin: ExecutionOrigin,
        cell_id: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<OperationId, SessionError> {
        self.queue_execution(origin, cell_id.into(), source.into(), None)
            .await
    }

    pub async fn execute_cell_captured(
        &self,
        origin: ExecutionOrigin,
        cell_id: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<ExecutionReceipt, SessionError> {
        let (completion, receiver) = oneshot::channel();
        let operation_id = self
            .queue_execution(origin, cell_id.into(), source.into(), Some(completion))
            .await?;
        Ok(ExecutionReceipt {
            operation_id,
            completion: receiver,
        })
    }

    async fn queue_execution(
        &self,
        origin: ExecutionOrigin,
        cell_id: String,
        source: String,
        completion: Option<oneshot::Sender<ExecutionCapture>>,
    ) -> Result<OperationId, SessionError> {
        let operation_id = OperationId::new();
        let request = ExecuteRequest {
            operation_id,
            origin,
            cell_id: Some(cell_id),
            source,
            completion,
        };
        validate_execute_request_frame(self.session_id, &request)?;
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
        validate_variable_list_result(result)
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
        validate_variable_value_result(result, &name)
    }

    async fn inspect_worker(
        &self,
        method: &'static str,
        payload: Value,
    ) -> Result<Value, SessionInspectionError> {
        validate_inspection_request_frame(self.session_id, method, &payload)?;
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
        self.control_tx.send(ControlRequest::Interrupt)
    }

    pub async fn stop(&self) -> Result<(), SessionError> {
        self.control_tx.send(ControlRequest::Stop)
    }

    pub async fn mark_environment_stale(&self) -> Result<(), SessionError> {
        self.control_tx.send(ControlRequest::MarkEnvironmentStale)
    }
}

fn queue_depth(sender: &mpsc::Sender<ExecuteRequest>) -> usize {
    if sender.is_closed() {
        0
    } else {
        sender.max_capacity().saturating_sub(sender.capacity())
    }
}

fn validate_execute_request_frame(
    session_id: SessionId,
    request: &ExecuteRequest,
) -> Result<(), SessionError> {
    let frame = WorkerFrame::Request {
        protocol: WORKER_PROTOCOL_V1,
        id: request.operation_id.to_string(),
        session_id: session_id.to_string(),
        method: worker::method::EXECUTE.into(),
        payload: json!({
            "operation_id": request.operation_id.to_string(),
            "cell_id": request.cell_id,
            "source": request.source,
            "origin": request.origin,
        }),
    };
    cokernel_protocol::encode_json_frame(&frame, cokernel_protocol::DEFAULT_MAX_FRAME_BYTES)
        .map(|_| ())
        .map_err(WorkerTransportError::from)
        .map_err(SessionError::from)
}

#[cfg(test)]
mod request_frame_tests {
    use super::*;

    #[test]
    fn execute_request_preflight_accepts_small_frame() {
        let request = ExecuteRequest {
            operation_id: OperationId::new(),
            origin: ExecutionOrigin::Human,
            cell_id: Some("cell-1".into()),
            source: "x = 1\nx + 1".into(),
            completion: None,
        };
        assert!(validate_execute_request_frame(SessionId::new(), &request).is_ok());
    }

    #[test]
    fn execute_request_preflight_rejects_oversized_frame() {
        let request = ExecuteRequest {
            operation_id: OperationId::new(),
            origin: ExecutionOrigin::Human,
            cell_id: Some("cell-oversized".into()),
            source: "x".repeat(cokernel_protocol::DEFAULT_MAX_FRAME_BYTES),
            completion: None,
        };
        let error = validate_execute_request_frame(SessionId::new(), &request).unwrap_err();
        assert!(matches!(
            error,
            SessionError::WorkerTransport(WorkerTransportError::Frame(
                cokernel_protocol::FrameError::TooLarge(_)
            ))
        ));
    }

    #[test]
    fn inspection_request_preflight_accepts_small_frame() {
        assert!(
            validate_inspection_request_frame(
                SessionId::new(),
                worker::method::GET_VARIABLE,
                &json!({"name": "x"}),
            )
            .is_ok()
        );
    }

    #[test]
    fn inspection_request_preflight_rejects_oversized_frame() {
        let error = validate_inspection_request_frame(
            SessionId::new(),
            worker::method::GET_VARIABLE,
            &json!({"name": "x".repeat(cokernel_protocol::DEFAULT_MAX_FRAME_BYTES)}),
        )
        .unwrap_err();
        assert!(matches!(error, SessionInspectionError::InvalidRequest(_)));
    }
}

mod control;
include!("session/supervisor.rs");
include!("session/process_group.rs");
include!("session/actor.rs");
include!("session/lifecycle.rs");

#[cfg(test)]
include!("session/tests.rs");

#[cfg(test)]
include!("session/inspection_contract_tests.rs");

#[cfg(test)]
mod observation_tests {
    use super::*;
    #[test]
    fn global_stream_carries_domain_identity_but_not_private_worker_responses_or_heartbeats() {
        let global = crate::events::EventHub::default();
        let (mut observations, _) = global.subscribe();
        let bus = SessionEventBus {
            local: broadcast::channel(8).0,
            global,
            project_id: ProjectId::new(),
            notebook_id: NotebookId::new(),
        };
        let session_id = SessionId::new();
        let mut local = bus.subscribe();
        let _ = bus.send(SessionEvent::WorkerFrame {
            session_id,
            frame: WorkerFrame::Response {
                protocol: 1,
                id: "inspection".into(),
                session_id: session_id.to_string(),
                ok: true,
                result: Some(json!({"private_variable":"sent-only-to-requester"})),
                error: None,
            },
        });
        assert!(local.try_recv().is_ok());
        assert!(matches!(
            observations.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        let _ = bus.send(SessionEvent::WorkerFrame {
            session_id,
            frame: WorkerFrame::Event {
                protocol: 1,
                session_id: session_id.to_string(),
                event: worker::event::HEARTBEAT.into(),
                payload: json!({}),
            },
        });
        assert!(matches!(
            observations.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        let _ = bus.send(SessionEvent::StateChanged {
            session_id,
            state: SessionState::Executing,
        });
        let event = observations.try_recv().unwrap();
        assert_eq!(event.event, "session.state_changed");
        assert_eq!(event.payload["project_id"], json!(bus.project_id));
        assert_eq!(event.payload["notebook_id"], json!(bus.notebook_id));
        assert_eq!(event.payload["session_id"], json!(session_id));
    }
}
