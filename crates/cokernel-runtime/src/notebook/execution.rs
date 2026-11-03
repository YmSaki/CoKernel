use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::{future::Future, pin::Pin};

use cokernel_domain::{
    DomainError, ExecutionOrigin, NotebookId, OperationId, OperationStatus, Project, ProjectId,
    SessionId,
};
use serde::Serialize;
use tokio::sync::{Mutex, Semaphore, watch};

use super::{NotebookError, NotebookOutputs, NotebookService};
use crate::session::{SessionError, SessionHandle, SessionSupervisor};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentExecutionState {
    Pending,
    Persisted {
        execution_status: OperationStatus,
        revision: u64,
    },
    NotPersisted {
        execution_status: Option<OperationStatus>,
        error: DomainError,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentOperation {
    pub operation_id: OperationId,
    pub project_id: ProjectId,
    pub notebook_id: NotebookId,
    pub session_id: SessionId,
    pub result: DocumentExecutionState,
}

struct OperationRecord {
    operation: DocumentOperation,
    state: watch::Receiver<DocumentExecutionState>,
}

pub struct PreparedExecution {
    work: Pin<Box<dyn Future<Output = Result<DocumentOperation, DomainError>> + Send>>,
}

#[derive(Default)]
struct ExecutionLane {
    queue: std::sync::Mutex<VecDeque<uuid::Uuid>>,
    changed: tokio::sync::Notify,
}
struct ExecutionTurn {
    lane: Arc<ExecutionLane>,
    id: uuid::Uuid,
}
impl ExecutionTurn {
    async fn ready(&self) {
        loop {
            let changed = self.lane.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self
                .lane
                .queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .front()
                == Some(&self.id)
            {
                return;
            }
            changed.await;
        }
    }
}
impl Drop for ExecutionTurn {
    fn drop(&mut self) {
        // Removing a cancelled middle turn cannot release a later turn ahead
        // of the still-active head. No detached cancellation-chain tasks.
        self.lane
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|id| *id != self.id);
        self.lane.changed.notify_waiters();
    }
}
impl PreparedExecution {
    pub async fn completed(self) -> Result<DocumentOperation, DomainError> {
        self.work.await
    }
}

/// Shared by Human and MCP domain adapters, not a separate execution engine.
#[derive(Clone)]
pub struct NotebookRuntime {
    pub documents: Arc<Mutex<NotebookService>>,
    pub document_queue: super::DocumentQueue,
    identities: super::service::NotebookIdentities,
    pub sessions: SessionSupervisor,
    execution_order: Arc<std::sync::Mutex<HashMap<NotebookId, std::sync::Weak<ExecutionLane>>>>,
    operations: Arc<Mutex<VecDeque<OperationRecord>>>,
    pending: Arc<Semaphore>,
}

impl NotebookRuntime {
    pub async fn shutdown(&self) -> Result<(), DomainError> {
        let failures = self.sessions.stop_all().await;
        // Accepted persistence tasks must finish before the final document flush.
        let settled = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.pending.clone().acquire_many_owned(32),
        )
        .await;
        let documents = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.document_queue
                .call(|documents| Ok(documents.flush_all())),
        )
        .await
        .map_err(|_| DomainError {
            code: "CK-RUN-SHUTDOWN".into(),
            component: "runtime".into(),
            summary: "Document flush is still pending; Runtime retains its document owner".into(),
            action: "Wait for document storage to recover and retry shutdown".into(),
            detail: None,
        })?
        .map_err(|e| e.domain_error())?;
        if !failures.is_empty() || !documents.is_empty() || !matches!(settled, Ok(Ok(_))) {
            return Err(DomainError {
                code: "CK-RUN-SHUTDOWN".into(),
                component: "runtime".into(),
                summary:
                    "Runtime shutdown could not confirm all Session cleanup and document saves"
                        .into(),
                action: "Inspect document files and Session failure evidence before restarting"
                    .into(),
                detail: Some(format!(
                    "session_errors={}, document_errors={}",
                    failures.len(),
                    documents.len()
                )),
            });
        }
        Ok(())
    }

    pub fn new(documents: NotebookService, sessions: SessionSupervisor) -> Self {
        let identities = documents.identities();
        let documents = Arc::new(Mutex::new(documents));
        let document_queue = super::DocumentQueue::new(documents.clone());
        Self {
            documents,
            document_queue,
            identities,
            sessions,
            execution_order: Arc::new(std::sync::Mutex::new(HashMap::new())),
            operations: Arc::new(Mutex::new(VecDeque::new())),
            pending: Arc::new(Semaphore::new(32)),
        }
    }
    pub(crate) fn owns_notebook(&self, project: ProjectId, notebook: NotebookId) -> bool {
        self.identities.owns(project, notebook)
    }

    pub async fn ensure_primary(
        &self,
        project: &Project,
        notebook: NotebookId,
    ) -> Result<SessionHandle, DomainError> {
        let checked_project = project.clone();
        self.document_queue
            .call(move |docs| docs.get(&checked_project, notebook))
            .await
            .map_err(|e| e.domain_error())?;
        self.sessions
            .ensure_primary(project, notebook)
            .await
            .map_err(session_error)
    }

    /// The public boundary accepts identity/revision only; source comes from the
    /// document service. The internal receipt task survives client cancellation.
    pub async fn execute_cell(
        &self,
        project: &Project,
        session_id: SessionId,
        notebook: NotebookId,
        cell_id: &str,
        revision: u64,
        origin: ExecutionOrigin,
    ) -> Result<DocumentOperation, DomainError> {
        self.prepare_execution(project, session_id, notebook, cell_id, revision, origin)
            .await?
            .completed()
            .await
    }

    pub async fn prepare_execution(
        &self,
        project: &Project,
        session_id: SessionId,
        notebook: NotebookId,
        cell_id: &str,
        revision: u64,
        origin: ExecutionOrigin,
    ) -> Result<PreparedExecution, DomainError> {
        let permit = self
            .pending
            .clone()
            .try_acquire_owned()
            .map_err(|_| NotebookError::Limit.domain_error())?;
        let session = self
            .sessions
            .get(session_id)
            .await
            .ok_or_else(|| NotebookError::NotFound.domain_error())?;
        let owner = session.snapshot();
        if owner.project_id != project.project_id || owner.notebook_id != notebook {
            return Err(NotebookError::NotFound.domain_error());
        }
        let project = project.clone();
        let preparation_project = project.clone();
        let cell_id = cell_id.to_owned();
        let prepared = self
            .document_queue
            .submit(move |documents| {
                documents.prepare_cell(&preparation_project, notebook, revision, &cell_id)
            })
            .map_err(|e| e.domain_error())?;
        // Allocate the FIFO turn at admission, before spawned tasks can race.
        let turn = {
            let mut order = self
                .execution_order
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            order.retain(|_, value| value.strong_count() > 0);
            let lane = order
                .get(&notebook)
                .and_then(std::sync::Weak::upgrade)
                .unwrap_or_default();
            order.insert(notebook, Arc::downgrade(&lane));
            let id = uuid::Uuid::new_v4();
            lane.queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push_back(id);
            ExecutionTurn { lane, id }
        };
        let runtime = self.clone();
        Ok(PreparedExecution {
            work: Box::pin(async move {
                let turn = turn;
                turn.ready().await;
                let cell = prepared.completed().await.map_err(|e| e.domain_error())?;
                let mut operations = runtime.operations.lock().await;
                let receipt = session
                    .execute_cell_captured(origin, cell.cell_id.clone(), cell.source.clone())
                    .await
                    .map_err(session_error)?;
                // No await between queue acceptance and spawning its persistence owner.
                // Registration uses the lock acquired before submission below.
                let operation_id = receipt.operation_id;
                let (state, receiver) = watch::channel(DocumentExecutionState::Pending);
                let operation = DocumentOperation {
                    operation_id,
                    project_id: project.project_id,
                    notebook_id: notebook,
                    session_id,
                    result: DocumentExecutionState::Pending,
                };
                let runtime_documents = runtime.document_queue.clone();
                let events = runtime.sessions.events();
                let project = project.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let result = match receipt.completed().await {
                        Ok(capture) => {
                            let execution_status = capture.status;
                            let outputs = capture
                                .frames
                                .map_err(|message| NotebookError::Invalid(message).domain_error())
                                .and_then(|frames| {
                                    NotebookOutputs::from_frames(operation_id, &frames)
                                        .map_err(|e| e.domain_error())
                                });
                            match outputs {
                                Ok(outputs) => match runtime_documents
                                    .call({
                                        let project = project.clone();
                                        move |documents| {
                                            documents.commit_execution(&project, &cell, &outputs)
                                        }
                                    })
                                    .await
                                {
                                    Ok(saved) => DocumentExecutionState::Persisted {
                                        execution_status,
                                        revision: saved.document.revision,
                                    },
                                    Err(error) => DocumentExecutionState::NotPersisted {
                                        execution_status: Some(execution_status),
                                        error: error.domain_error(),
                                    },
                                },
                                Err(error) => DocumentExecutionState::NotPersisted {
                                    execution_status: Some(execution_status),
                                    error,
                                },
                            }
                        }
                        Err(error) => DocumentExecutionState::NotPersisted {
                            execution_status: None,
                            error: session_error(error),
                        },
                    };
                    state.send_replace(result.clone());
                    events.publish("notebook.execution_completed",serde_json::json!({"project_id":project.project_id,"notebook_id":notebook,"session_id":session_id,"operation_id":operation_id,"result":result}));
                });
                // Retain every pending operation and the newest completed records, up to
                // 128. Data-heavy output lives in documents, not duplicated in this table.
                while operations.len() >= 128 {
                    if let Some(index) = operations.iter().position(|record| {
                        !matches!(*record.state.borrow(), DocumentExecutionState::Pending)
                    }) {
                        operations.remove(index);
                    } else {
                        break;
                    }
                }
                operations.push_back(OperationRecord {
                    operation: operation.clone(),
                    state: receiver,
                });
                Ok(operation)
            }),
        })
    }

    pub async fn operation(
        &self,
        project: ProjectId,
        id: OperationId,
    ) -> Result<DocumentOperation, DomainError> {
        let operations = self.operations.lock().await;
        let record = operations
            .iter()
            .find(|r| r.operation.operation_id == id && r.operation.project_id == project)
            .ok_or_else(|| NotebookError::NotFound.domain_error())?;
        let mut operation = record.operation.clone();
        operation.result = record.state.borrow().clone();
        Ok(operation)
    }
}

fn session_error(error: SessionError) -> DomainError {
    DomainError {
        code: "CK-SESS-EXECUTE".into(),
        component: "session".into(),
        summary: error.to_string(),
        action: "Inspect Session state and failure evidence; restart explicitly when needed".into(),
        detail: None,
    }
}
