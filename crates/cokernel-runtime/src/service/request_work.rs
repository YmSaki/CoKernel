//! Shared request work: bounded replies, domain reservations and completion ownership.
use super::*;
use cokernel_domain::{ExecutionOrigin, NotebookId, ProjectId, SessionId, SessionState};
use serde::Deserialize;
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
};
use tokio::{
    sync::watch,
    task::{Id, JoinError, JoinSet},
};

const MAX_PENDING_REPLIES: usize = 16;
pub(super) type Outcome = Result<Value, DomainError>;
pub(super) type Work = Pin<Box<dyn Future<Output = Outcome> + Send>>;
pub(super) enum ReplyCompletion<R> {
    Reply(Outcome),
    Project(super::project_rpc::ProjectCompletion<R>),
}
type JoinedReply<R> = Result<(String, ReplyCompletion<R>), JoinError>;
type JoinedPackage = Result<super::package_ops::PackageOutcome, JoinError>;
pub struct BackgroundCompletion<R>(Completion<R>);
enum Completion<R> {
    Request(JoinedReply<R>),
    Package(JoinedPackage),
}

struct Start {
    project: ProjectId,
    leader: Id,
    abort: tokio::task::AbortHandle,
    result: watch::Receiver<Option<Outcome>>,
}
struct Pending {
    request: String,
    notebook: Option<NotebookId>,
    leader: bool,
    document_project: Option<ProjectId>,
    preparation: Option<NotebookId>,
    abort: tokio::task::AbortHandle,
    cancel_on_shutdown: bool,
}

pub(super) struct RequestWork<R> {
    tasks: JoinSet<(String, ReplyCompletion<R>)>,
    pending: HashMap<Id, Pending>,
    starts: HashMap<NotebookId, Start>,
    ready: VecDeque<ResponseEnvelope>,
}
impl<R> Default for RequestWork<R> {
    fn default() -> Self {
        Self {
            tasks: JoinSet::new(),
            pending: HashMap::new(),
            starts: HashMap::new(),
            ready: VecDeque::new(),
        }
    }
}
impl<R> RequestWork<R> {
    pub(super) fn snapshot(&self) -> Value {
        json!({"pending_requests":self.pending.len(),"starting_notebooks":self.starts.iter().map(|(notebook, start)| json!({"project_id":start.project,"notebook_id":notebook})).collect::<Vec<_>>()})
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotebookParams {
    project_id: ProjectId,
    notebook_id: NotebookId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectionParams {
    project_id: ProjectId,
    session_id: SessionId,
    #[serde(default)]
    name: Option<String>,
}

fn failure(code: &str, summary: &str) -> DomainError {
    DomainError {
        code: code.into(),
        component: "session".into(),
        summary: summary.into(),
        action: "Refresh Session status and retry when the pending operation has completed".into(),
        detail: None,
    }
}
fn session_error(error: impl std::fmt::Display) -> DomainError {
    failure("CK-SESS-REQUEST", &error.to_string())
}
fn response(request: String, result: Outcome) -> ResponseEnvelope {
    match result {
        Ok(value) => success_response(request, value),
        Err(error) => error_response(request, error),
    }
}
async fn follow_start(mut result: watch::Receiver<Option<Outcome>>) -> Outcome {
    loop {
        if let Some(value) = result.borrow().clone() {
            return value;
        }
        result.changed().await.map_err(|_| {
            failure(
                "CK-SESS-START-CANCELLED",
                "Pending Session startup was cancelled",
            )
        })?;
    }
}

impl<R: UvRunner + 'static> RuntimeService<R> {
    pub(super) fn ensure_no_starting_sessions(
        &self,
        project: ProjectId,
    ) -> Result<(), DomainError> {
        if self.requests.starts.values().any(|s| s.project == project) {
            Err(failure(
                "CK-SESS-STARTING",
                "Project has a Session startup or restart in progress",
            ))
        } else if self
            .requests
            .pending
            .values()
            .any(|p| p.preparation.is_some() && p.document_project == Some(project))
        {
            Err(failure(
                "CK-SESS-PREPARING",
                "Project has an execution being prepared",
            ))
        } else {
            Ok(())
        }
    }
    pub(super) fn ensure_no_document_requests(
        &self,
        project: ProjectId,
    ) -> Result<(), DomainError> {
        if self
            .requests
            .pending
            .values()
            .any(|p| p.document_project == Some(project))
        {
            Err(failure(
                "CK-NB-BUSY",
                "Project has accepted document work in progress",
            ))
        } else {
            Ok(())
        }
    }
    pub(super) fn ensure_preparation_idle(&self, notebook: NotebookId) -> Result<(), DomainError> {
        if self
            .requests
            .pending
            .values()
            .any(|p| p.preparation == Some(notebook))
        {
            Err(failure(
                "CK-SESS-PREPARING",
                "Notebook execution admission is in progress",
            ))
        } else {
            Ok(())
        }
    }
    pub(super) fn ensure_lifecycle_idle(&self, notebook: NotebookId) -> Result<(), DomainError> {
        if self.requests.starts.contains_key(&notebook) {
            Err(failure(
                "CK-SESS-STARTING",
                "Notebook Session lifecycle operation is in progress",
            ))
        } else {
            Ok(())
        }
    }

    /// Domain routing returns immediate replies or registers owned request work.
    /// Transport adapters use RuntimeHandle and never drive these collections.
    pub async fn admit(
        &mut self,
        request: RequestEnvelope,
        origin: ExecutionOrigin,
    ) -> Option<ResponseEnvelope> {
        self.drain_reply_completions();
        self.drain_package_completions().await;
        if request.protocol == PROTOCOL_V1 && self.notebooks.is_some() {
            let id = request.request_id.clone();
            let result = if matches!(
                request.method.as_str(),
                "projects.create"
                    | "projects.register"
                    | "packages.add"
                    | "packages.remove"
                    | "packages.sync"
                    | "environment.get_status"
                    | "projects.remove_from_registry"
            ) {
                self.admit_project(request).await
            } else if request.method.starts_with("notebooks.") {
                self.admit_document(request)
            } else if request.method == "runtime.get_snapshot" {
                self.admit_snapshot(request).await
            } else if request.method == "sessions.execute_cell" {
                self.admit_execution(request, origin).await
            } else if matches!(
                request.method.as_str(),
                "sessions.ensure_primary"
                    | "sessions.restart"
                    | "sessions.get_variable"
                    | "sessions.list_variables"
            ) {
                self.admit_session(request).await
            } else {
                return Some(self.dispatch_admitted(request, origin).await);
            };
            match result {
                Ok(()) => None,
                Err(error) => Some(error_response(id, error)),
            }
        } else {
            Some(self.dispatch_admitted(request, origin).await)
        }
    }

    pub(super) fn reserve_response(&self, id: &str) -> Result<(), DomainError> {
        if self.requests.pending.len() + self.requests.ready.len() >= MAX_PENDING_REPLIES {
            return Err(failure(
                "CK-RUN-BUSY",
                "Runtime response capacity is full; control/status remain available",
            ));
        }
        if self.requests.pending.values().any(|p| p.request == id)
            || self.requests.ready.iter().any(|r| r.request_id == id)
        {
            return Err(failure(
                "CK-RUN-REQUEST-ID",
                "Request ID is already pending",
            ));
        }
        Ok(())
    }
    pub(super) fn schedule_reply(
        &mut self,
        id: String,
        work: Work,
        project: Option<ProjectId>,
        preparation: Option<NotebookId>,
        cancel_on_shutdown: bool,
    ) {
        let response_id = id.clone();
        let task = self
            .requests
            .tasks
            .spawn(async move { (response_id, ReplyCompletion::Reply(work.await)) });
        self.requests.pending.insert(
            task.id(),
            Pending {
                request: id,
                notebook: None,
                leader: false,
                document_project: project,
                preparation,
                abort: task,
                cancel_on_shutdown,
            },
        );
    }
    pub(super) fn schedule_project(
        &mut self,
        id: String,
        work: impl Future<Output = super::project_rpc::ProjectCompletion<R>> + Send + 'static,
        project: Option<ProjectId>,
    ) {
        let response_id = id.clone();
        let task = self
            .requests
            .tasks
            .spawn(async move { (response_id, ReplyCompletion::Project(work.await)) });
        self.requests.pending.insert(
            task.id(),
            Pending {
                request: id,
                notebook: None,
                leader: false,
                document_project: project,
                preparation: None,
                abort: task,
                cancel_on_shutdown: false,
            },
        );
    }
    async fn admit_session(&mut self, request: RequestEnvelope) -> Result<(), DomainError> {
        self.reserve_response(&request.request_id)?;
        let runtime = self
            .notebooks
            .as_ref()
            .ok_or_else(|| failure("CK-RUN-NOT-READY", "Notebook Runtime is unavailable"))?
            .clone();
        let mut start = None;
        let work: Work = if matches!(
            request.method.as_str(),
            "sessions.ensure_primary" | "sessions.restart"
        ) {
            let p: NotebookParams = parse_params(request.params)?;
            let record = self.projects.get(p.project_id).map_err(project_error)?;
            if !runtime.owns_notebook(p.project_id, p.notebook_id) {
                return Err(failure(
                    "CK-NB-NOT-FOUND",
                    "Notebook was not found for this Project",
                ));
            }
            self.ensure_preparation_idle(p.notebook_id)?;
            if let Some(existing) = self.requests.starts.get(&p.notebook_id) {
                if existing.project != p.project_id {
                    return Err(failure(
                        "CK-SESS-OWNERSHIP",
                        "Notebook belongs to another Project",
                    ));
                }
                if request.method == "sessions.restart" {
                    return Err(failure(
                        "CK-SESS-STARTING",
                        "A lifecycle operation is already pending",
                    ));
                }
                Box::pin(follow_start(existing.result.clone()))
            } else {
                let existing = runtime.sessions.primary_handle(p.notebook_id).await;
                if existing
                    .as_ref()
                    .is_some_and(|h| h.snapshot().project_id != p.project_id)
                {
                    return Err(failure(
                        "CK-NB-NOT-FOUND",
                        "Primary Session was not found for this Project",
                    ));
                }
                if request.method == "sessions.ensure_primary"
                    && existing
                        .as_ref()
                        .is_some_and(|h| h.snapshot().state != SessionState::Stopped)
                {
                    let snapshot = existing.map(|h| h.snapshot());
                    let project = record.project.clone();
                    let validation = runtime
                        .document_queue
                        .submit(move |docs| docs.get(&project, p.notebook_id))
                        .map_err(|e| e.domain_error())?;
                    Box::pin(async move {
                        validation.completed().await.map_err(|e| e.domain_error())?;
                        to_value(snapshot.ok_or_else(|| {
                            failure("CK-SESS-REQUEST", "Primary Session disappeared")
                        })?)
                    })
                } else {
                    self.ensure_package_idle(p.project_id)?;
                    if record.environment_reconcile_required {
                        return Err(failure(
                            "CK-UV-NEEDS-SYNC",
                            "Synchronize the uncertain Project environment before starting a Session",
                        ));
                    }
                    let (sender, receiver) = watch::channel(None);
                    let project = record.project.clone();
                    let validation = runtime
                        .document_queue
                        .submit(move |docs| docs.get(&project, p.notebook_id))
                        .map_err(|e| e.domain_error())?;
                    start = Some((p.project_id, p.notebook_id, receiver));
                    Box::pin(async move {
                        if let Err(error) = validation.completed().await {
                            let result = Err(error.domain_error());
                            sender.send_replace(Some(result.clone()));
                            return result;
                        }
                        let result = if request.method == "sessions.restart" {
                            runtime
                                .sessions
                                .restart_primary(&record.project, p.notebook_id)
                                .await
                        } else {
                            runtime
                                .sessions
                                .ensure_primary(&record.project, p.notebook_id)
                                .await
                        };
                        let result = result
                            .map_err(session_error)
                            .and_then(|h| to_value(h.snapshot()));
                        sender.send_replace(Some(result.clone()));
                        result
                    })
                }
            }
        } else {
            let has_name = request.params.get("name").is_some();
            let p: InspectionParams = parse_params(request.params)?;
            if request.method == "sessions.list_variables" && has_name {
                return Err(failure(
                    "CK-RUN-BAD-PARAMS",
                    "Variable listing takes no name",
                ));
            }
            if (request.method == "sessions.get_variable") != p.name.is_some() {
                return Err(failure(
                    "CK-RUN-BAD-PARAMS",
                    "Variable lookup requires a name; variable listing takes no name",
                ));
            }
            self.projects.get(p.project_id).map_err(project_error)?;
            let session = runtime
                .sessions
                .get(p.session_id)
                .await
                .ok_or_else(|| failure("CK-NB-NOT-FOUND", "Session was not found"))?;
            if session.snapshot().project_id != p.project_id {
                return Err(failure(
                    "CK-NB-NOT-FOUND",
                    "Session was not found for this Project",
                ));
            }
            self.ensure_lifecycle_idle(session.snapshot().notebook_id)?;
            Box::pin(async move {
                match p.name {
                    Some(name) => {
                        to_value(session.get_variable(name).await.map_err(session_error)?)
                    }
                    None => to_value(session.list_variables().await.map_err(session_error)?),
                }
            })
        };
        let id = request.request_id;
        let response_id = id.clone();
        let task = self
            .requests
            .tasks
            .spawn(async move { (response_id, ReplyCompletion::Reply(work.await)) });
        let notebook = start.as_ref().map(|(_, n, _)| *n);
        self.requests.pending.insert(
            task.id(),
            Pending {
                request: id,
                notebook,
                leader: start.is_some(),
                document_project: None,
                preparation: None,
                abort: task.clone(),
                cancel_on_shutdown: true,
            },
        );
        if let Some((project, notebook, result)) = start {
            self.requests.starts.insert(
                notebook,
                Start {
                    project,
                    leader: task.id(),
                    abort: task,
                    result,
                },
            );
            if let Some(events) = self.events() {
                events.publish(
                    "session.starting",
                    json!({"project_id":project,"notebook_id":notebook}),
                );
            }
        }
        Ok(())
    }

    pub async fn next_background_completion(&mut self) -> BackgroundCompletion<R> {
        tokio::select! {
            value = self.packages.tasks.join_next(), if !self.packages.tasks.is_empty() => {
                if let Some(value) = value { return BackgroundCompletion(Completion::Package(value)); }
            }
            value = self.requests.tasks.join_next(), if !self.requests.tasks.is_empty() => {
                if let Some(value) = value { return BackgroundCompletion(Completion::Request(value)); }
            }
            else => {}
        }
        std::future::pending().await
    }
    pub async fn finish_background_completion(&mut self, completion: BackgroundCompletion<R>) {
        match completion.0 {
            Completion::Package(value) => self.finish_package_task(value).await,
            Completion::Request(value) => self.finish_reply_task(value),
        }
    }
    fn finish_reply_task(&mut self, joined: JoinedReply<R>) {
        let (task, result) = match joined {
            Ok((request, result)) => {
                let Some(id) = self
                    .requests
                    .pending
                    .iter()
                    .find_map(|(id, p)| (p.request == request).then_some(*id))
                else {
                    return;
                };
                (id, result)
            }
            Err(error) => (
                error.id(),
                ReplyCompletion::Reply(Err(failure(
                    "CK-SESS-REQUEST-CANCELLED",
                    "Pending Session request was cancelled or failed",
                ))),
            ),
        };
        if let Some(pending) = self.requests.pending.remove(&task) {
            if pending.leader
                && let Some(notebook) = pending.notebook
            {
                self.requests.starts.remove(&notebook);
            }
            let result = match result {
                ReplyCompletion::Reply(result) => Some(result),
                ReplyCompletion::Project(completion) => {
                    self.finish_project_request(&pending.request, completion)
                }
            };
            if let Some(result) = result {
                self.packages.release_preparation(&pending.request);
                self.requests
                    .ready
                    .push_back(response(pending.request, result));
            }
        }
    }
    pub(super) fn drain_reply_completions(&mut self) {
        while let Some(joined) = self.requests.tasks.try_join_next() {
            self.finish_reply_task(joined);
        }
    }
    pub fn take_responses(&mut self) -> Vec<ResponseEnvelope> {
        self.drain_reply_completions();
        self.requests.ready.drain(..).collect()
    }
    pub(super) async fn cancel_notebook_start(&mut self, notebook: NotebookId) {
        for pending in self.requests.pending.values() {
            if pending.preparation == Some(notebook) {
                pending.abort.abort();
            }
        }
        while self
            .requests
            .pending
            .values()
            .any(|p| p.preparation == Some(notebook))
        {
            if let Some(joined) = self.requests.tasks.join_next().await {
                self.finish_reply_task(joined);
            } else {
                break;
            }
        }
        if let Some(start) = self.requests.starts.get(&notebook) {
            let leader = start.leader;
            start.abort.abort();
            while self.requests.pending.contains_key(&leader) {
                if let Some(joined) = self.requests.tasks.join_next().await {
                    self.finish_reply_task(joined);
                } else {
                    break;
                }
            }
        }
    }
    pub async fn cancel_and_drain_requests(&mut self) {
        for pending in self.requests.pending.values() {
            if pending.cancel_on_shutdown {
                pending.abort.abort();
            }
        }
        while let Some(joined) = self.requests.tasks.join_next().await {
            self.finish_reply_task(joined);
        }
    }
}
