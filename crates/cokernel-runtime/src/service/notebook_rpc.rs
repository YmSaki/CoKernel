use super::*;
use cokernel_domain::{ExecutionOrigin, OperationId, ProjectId, SessionId, SessionState};
use serde::Deserialize;

use crate::notebook::{NotebookError, NotebookRuntime};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationParams {
    project_id: ProjectId,
    operation_id: OperationId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionParams {
    project_id: ProjectId,
    session_id: SessionId,
}
fn notebook_error(error: NotebookError) -> DomainError {
    error.domain_error()
}
fn session_error(error: impl std::fmt::Display) -> DomainError {
    DomainError {
        code: "CK-SESS-REQUEST".into(),
        component: "session".into(),
        summary: error.to_string(),
        action: "Inspect Session status and retry only when appropriate".into(),
        detail: None,
    }
}

impl<R: UvRunner + 'static> RuntimeService<R> {
    pub fn set_mcp_monitor(&mut self, monitor: crate::mcp_supervisor::Monitor) {
        self.mcp = monitor;
    }
    pub fn set_tunnel_monitor(&mut self, monitor: crate::tunnel::Monitor) {
        self.tunnel = monitor;
    }
    pub fn events(&self) -> Option<crate::events::EventHub> {
        self.notebooks
            .as_ref()
            .map(|runtime| runtime.sessions.events())
    }
    pub async fn shutdown(&mut self) -> Result<(), DomainError> {
        tokio::time::timeout(std::time::Duration::from_secs(12), self.shutdown_inner())
            .await
            .map_err(|_| DomainError {
                code: "CK-RUN-SHUTDOWN".into(),
                component: "runtime".into(),
                summary: "Runtime shutdown exceeded its response budget; cleanup remains owned"
                    .into(),
                action: "Inspect Session and document state and retry shutdown".into(),
                detail: None,
            })?
    }
    async fn shutdown_inner(&mut self) -> Result<(), DomainError> {
        if self.packages.active() {
            return Err(super::package_ops::operation_error(
                "CK-UV-BUSY",
                "Wait for accepted package operations before stopping Runtime",
            ));
        }
        if tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.cancel_and_drain_requests(),
        )
        .await
        .is_err()
        {
            return Err(DomainError {
                code: "CK-RUN-SHUTDOWN".into(),
                component: "runtime".into(),
                summary: "Accepted document work has not finished; Runtime remains available"
                    .into(),
                action: "Wait for document storage to recover and retry shutdown".into(),
                detail: None,
            });
        }
        if let Some(runtime) = &self.notebooks {
            runtime.shutdown().await?;
        }
        if let Some(metrics) = self.metrics.get() {
            metrics.shutdown().await.map_err(|summary| DomainError {
                code: "CK-RUN-SHUTDOWN".into(),
                component: "metrics".into(),
                summary: summary.into(),
                action: "Wait for resource collector cleanup and retry".into(),
                detail: None,
            })?;
        }
        self.tunnel.shutdown().await.map_err(|code| DomainError {
            code: code.into(),
            component: "tunnel".into(),
            summary: "Tunnel cleanup remains pending".into(),
            action: "Wait for remote access cleanup and retry Runtime shutdown".into(),
            detail: None,
        })?;
        self.mcp.shutdown().await.map_err(|summary| DomainError {
            code: "CK-MCP-CLEANUP".into(),
            component: "mcp".into(),
            summary: summary.into(),
            action: "Wait for MCP cleanup and retry Runtime shutdown".into(),
            detail: None,
        })
    }
    pub fn with_notebooks(projects: ProjectManager<R>, notebooks: NotebookRuntime) -> Self {
        Self {
            metrics_root: projects.projects_root().to_path_buf(),
            metrics: std::sync::OnceLock::new(),
            projects: ProjectAccess::owned(projects),
            notebooks: Some(notebooks),
            packages: super::package_ops::PackageOperations::default(),
            requests: super::request_work::RequestWork::default(),
            mcp: crate::mcp_supervisor::Monitor::default(),
            tunnel: crate::tunnel::Monitor::default(),
        }
    }

    /// Called only by domain admission. Transports use RuntimeHandle.
    pub(super) async fn dispatch_admitted(
        &mut self,
        request: RequestEnvelope,
        origin: ExecutionOrigin,
    ) -> ResponseEnvelope {
        if request.method.starts_with("tunnel.") && origin != ExecutionOrigin::Human {
            return error_response(
                request.request_id,
                DomainError {
                    code: "CK-TUN-FORBIDDEN".into(),
                    component: "tunnel".into(),
                    summary: "Remote access control requires the Host bridge".into(),
                    action: "Use Desktop Remote Access settings".into(),
                    detail: None,
                },
            );
        }
        if request.protocol != PROTOCOL_V1 || self.notebooks.is_none() {
            return self.dispatch(request);
        }
        self.drain_reply_completions();
        self.drain_package_completions().await;
        let name = request.method.as_str();
        if name == "runtime.get_diagnostics" {
            if request.params != json!({}) {
                return error_response(
                    request.request_id,
                    DomainError {
                        code: "CK-DIAG-INPUT".into(),
                        component: "diagnostics".into(),
                        summary: "Diagnostics takes empty parameters".into(),
                        action: "Use empty parameters".into(),
                        detail: None,
                    },
                );
            }
            let sessions = self
                .notebooks
                .as_ref()
                .expect("checked above")
                .sessions
                .diagnostics()
                .await;
            let result = crate::diagnostics::RuntimeDiagnostics::capture(
                sessions,
                self.mcp.snapshot(),
                self.tunnel.snapshot(),
            )
            .into_value();
            return match result {
                Ok(value) => success_response(request.request_id, value),
                Err(code) => error_response(
                    request.request_id,
                    DomainError {
                        code: code.into(),
                        component: "diagnostics".into(),
                        summary: "Diagnostic snapshot unavailable".into(),
                        action: "Retry after checking Runtime status".into(),
                        detail: None,
                    },
                ),
            };
        }
        if name == methods::name::BRIDGE_SHUTDOWN {
            if request.params != json!({}) {
                return error_response(
                    request.request_id,
                    DomainError {
                        code: "CK-RUN-BAD-PARAMS".into(),
                        component: "runtime".into(),
                        summary: "Shutdown requires empty parameters".into(),
                        action: "Use the Host runtime stop action".into(),
                        detail: None,
                    },
                );
            }
            return match self.shutdown().await {
                Ok(()) => success_response(request.request_id, json!({"stopped":true})),
                Err(error) => error_response(request.request_id, error),
            };
        }
        if name.starts_with("sessions.")
            || name == methods::name::OPERATIONS_GET
            || name == methods::name::OPERATIONS_CANCEL
        {
            let result = self.notebook_method(name, request.params, origin).await;
            let response = match result {
                Ok(value) => success_response(request.request_id.clone(), value),
                Err(error) => error_response(request.request_id.clone(), error),
            };
            if cokernel_protocol::encode_json_frame(
                &response,
                cokernel_protocol::DEFAULT_MAX_FRAME_BYTES,
            )
            .is_err()
            {
                return error_response(
                    request.request_id,
                    DomainError {
                        code: "CK-NB-LIMIT".into(),
                        component: "notebook".into(),
                        summary: "Response exceeds the bridge frame limit".into(),
                        action: "Use notebooks.read_chunk with the current document revision"
                            .into(),
                        detail: None,
                    },
                );
            }
            return response;
        }
        let method = request.method.clone();
        let event_project = request.params.get("project_id").cloned();
        let mut response = self.dispatch(request);
        if response.ok && method == methods::name::BRIDGE_HELLO {
            if let Some(capabilities) = response
                .result
                .as_mut()
                .and_then(|v| v["capabilities"].as_array_mut())
            {
                capabilities.extend([
                    json!("notebooks"),
                    json!("sessions"),
                    json!("notebook-import"),
                    json!("operation-status"),
                    json!("runtime-events"),
                    json!("runtime-snapshot"),
                    json!("sanitized-diagnostics"),
                    json!("async-packages"),
                    json!("async-projects"),
                    json!("concurrent-session-rpc"),
                    json!("owned-document-io"),
                    json!("linux-metrics"),
                ]);
            }
        }
        if let Some(events) = self.events() {
            if response.ok
                && matches!(
                    method.as_str(),
                    methods::name::PROJECTS_CREATE
                        | methods::name::PROJECTS_REGISTER
                        | methods::name::PROJECTS_REMOVE_FROM_REGISTRY
                )
            {
                events.publish(
                    "project.changed",
                    json!({"project_id":event_project,"method":method,"result":response.result}),
                );
            }
        }
        response
    }

    async fn notebook_method(
        &mut self,
        method: &str,
        params: Value,
        _origin: ExecutionOrigin,
    ) -> Result<Value, DomainError> {
        let runtime = self
            .notebooks
            .as_ref()
            .ok_or_else(|| notebook_error(NotebookError::NotFound))?
            .clone();
        match method {
            "operations.get" => {
                let p: OperationParams = parse_params(params)?;
                self.projects.get(p.project_id).map_err(project_error)?;
                if let Some(result) = self.packages.get(p.project_id, p.operation_id) {
                    return result;
                }
                to_value(runtime.operation(p.project_id, p.operation_id).await?)
            }
            "operations.cancel" => {
                let p: OperationParams = parse_params(params)?;
                self.projects.get(p.project_id).map_err(project_error)?;
                let result = self.packages.cancel(p.project_id, p.operation_id)?;
                if let Some(events) = self.events() {
                    events.publish("environment.operation_cancel_requested", json!({"project_id":p.project_id,"operation_id":p.operation_id,"accepted":result["accepted"]}));
                }
                Ok(result)
            }
            "sessions.list" => {
                let p: ProjectIdParams = parse_params(params)?;
                self.projects.get(p.project_id).map_err(project_error)?;
                to_value(
                    runtime
                        .sessions
                        .list()
                        .await
                        .into_iter()
                        .filter(|s| s.project_id == p.project_id)
                        .collect::<Vec<_>>(),
                )
            }
            "sessions.get" | "sessions.interrupt" | "sessions.stop" => {
                let p: SessionParams = parse_params(params)?;
                let session = owned_session(&runtime, p.project_id, p.session_id).await?;
                match method {
                    "sessions.interrupt" => {
                        session.interrupt().await.map_err(session_error)?;
                        Ok(json!({"accepted":true}))
                    }
                    "sessions.stop" => {
                        self.cancel_notebook_start(session.snapshot().notebook_id)
                            .await;
                        let current = owned_session(&runtime, p.project_id, p.session_id).await?;
                        if !matches!(
                            current.snapshot().state,
                            SessionState::Stopped | SessionState::Crashed | SessionState::Stopping
                        ) {
                            current.stop().await.map_err(session_error)?;
                        }
                        Ok(json!({"accepted":true}))
                    }
                    _ => to_value(session.snapshot()),
                }
            }
            _ => Err(DomainError {
                code: "CK-RUN-NO-METHOD".into(),
                component: "runtime".into(),
                summary: "Unknown notebook/Session method".into(),
                action: "Use a supported domain method".into(),
                detail: None,
            }),
        }
    }
}

async fn owned_session(
    runtime: &NotebookRuntime,
    project: ProjectId,
    id: SessionId,
) -> Result<crate::session::SessionHandle, DomainError> {
    let session = runtime
        .sessions
        .get(id)
        .await
        .ok_or_else(|| notebook_error(NotebookError::NotFound))?;
    if session.snapshot().project_id != project {
        return Err(notebook_error(NotebookError::NotFound));
    }
    Ok(session)
}
