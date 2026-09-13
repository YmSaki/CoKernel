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
    pub environment_generation: u64,
    pub started_at: DateTime<Utc>,
    pub current_operation_id: Option<OperationId>,
    pub queue_depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainError {
    pub code: String,
    pub component: String,
    pub summary: String,
    pub action: String,
    pub detail: Option<String>,
}
