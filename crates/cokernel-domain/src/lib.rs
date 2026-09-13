use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

domain_id!(ProjectId);
domain_id!(NotebookId);
domain_id!(SessionId);
domain_id!(OperationId);
domain_id!(FailureId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DesiredRuntimeState {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RuntimeState {
    Uninstalled,
    Stopped,
    Starting,
    Healthy,
    Degraded,
    Updating,
    Repairing,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentState {
    Absent,
    Syncing,
    Ready,
    Broken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionState {
    Starting,
    Idle,
    Executing,
    Interrupting,
    StaleEnvironment,
    Stopping,
    Stopped,
    Crashed,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OperationStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Interrupted,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionOrigin {
    Human,
    Mcp,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionKind {
    ExecuteCell,
    ExecuteCodeInternal,
    Interrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailureClassification {
    OomSuspected,
    ProcessSignal,
    PythonException,
    CudaOrNativeFailureSuspected,
    RuntimeRestart,
    ManualTermination,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub project_id: ProjectId,
    pub name: String,
    pub root_path: String,
    pub environment_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookDocumentRef {
    pub notebook_id: NotebookId,
    pub project_id: ProjectId,
    pub relative_path: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSession {
    pub session_id: SessionId,
    pub project_id: ProjectId,
    pub notebook_id: NotebookId,
    pub state: SessionState,
    #[serde(default = "initial_worker_generation")]
    pub worker_generation: u64,
    pub environment_generation: u64,
    pub started_at: DateTime<Utc>,
    pub current_operation_id: Option<OperationId>,
    pub queue_depth: usize,
}

const fn initial_worker_generation() -> u64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionOperation {
    pub operation_id: OperationId,
    pub session_id: SessionId,
    pub origin: ExecutionOrigin,
    pub kind: ExecutionKind,
    pub cell_id: Option<String>,
    pub requested_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: OperationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureRecord {
    pub failure_id: FailureId,
    pub component: String,
    pub session_id: Option<SessionId>,
    pub operation_id: Option<OperationId>,
    pub timestamp: DateTime<Utc>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub last_stderr: String,
    pub classification: FailureClassification,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainError {
    pub code: String,
    pub component: String,
    pub summary: String,
    pub action: String,
    pub detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_ids_round_trip_through_strings() {
        let id = ProjectId::new();
        let parsed = id.to_string().parse::<ProjectId>().unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn initial_worker_generation_is_one() {
        assert_eq!(initial_worker_generation(), 1);
    }
}
