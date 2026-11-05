//! Owned Project storage with admission-time reservations and typed completion.
use super::package_ops::{Preparation, operation_error};
use super::*;
use crate::project::{EnvironmentPlan, ProjectRecord};
use cokernel_domain::{ProjectId, SessionState};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateProjectParams {
    name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterProjectParams {
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectIdParams {
    project_id: ProjectId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageMutationParams {
    project_id: ProjectId,
    packages: Vec<String>,
}

pub(super) enum ProjectCompletion<R> {
    Resolved {
        result: Result<(PathBuf, Option<ProjectRecord>), DomainError>,
    },
    Prepared {
        method: String,
        kind: &'static str,
        result: Result<EnvironmentPlan<R>, DomainError>,
    },
    Removed {
        project: ProjectId,
        result: Result<ProjectRecord, DomainError>,
    },
}

impl<R: UvRunner + 'static> RuntimeService<R> {
    pub(super) async fn admit_project(
        &mut self,
        request: RequestEnvelope,
    ) -> Result<(), DomainError> {
        self.reserve_response(&request.request_id)?;
        let id = request.request_id.clone();
        let result = self.admit_project_inner(request).await;
        if result.is_err() {
            self.packages.release_preparation(&id);
        }
        result
    }
    async fn admit_project_inner(&mut self, request: RequestEnvelope) -> Result<(), DomainError> {
        let owner = self.projects.owner()?;
        let id = request.request_id;
        let method = request.method;
        match method.as_str() {
            "environment.get_status" => {
                let p: ProjectIdParams = parse_params(request.params)?;
                self.projects.get(p.project_id).map_err(project_error)?;
                let ticket = owner
                    .submit(move |manager| manager.environment_status(p.project_id))
                    .map_err(project_error)?;
                self.schedule_reply(
                    id,
                    Box::pin(
                        async move { to_value(ticket.completed().await.map_err(project_error)?) },
                    ),
                    Some(p.project_id),
                    None,
                    true,
                );
            }
            "projects.remove_from_registry" => {
                let p: ProjectIdParams = parse_params(request.params)?;
                self.ensure_package_idle(p.project_id)?;
                self.ensure_no_starting_sessions(p.project_id)?;
                self.ensure_no_document_requests(p.project_id)?;
                if let Some(runtime) = &self.notebooks
                    && runtime.sessions.list().await.iter().any(|s| {
                        s.project_id == p.project_id
                            && !matches!(s.state, SessionState::Stopped | SessionState::Crashed)
                    })
                {
                    return Err(DomainError { code:"CK-PROJ-IN-USE".into(), component:"project".into(), summary:"Project still has live Sessions".into(), action:"Stop its Sessions explicitly before removing the Project from the registry".into(), detail:None });
                }
                self.packages.preparing.insert(
                    id.clone(),
                    Preparation {
                        project: Some(p.project_id),
                        root: None,
                        removing: true,
                    },
                );
                let ticket = owner
                    .submit(move |manager| manager.remove_from_registry(p.project_id))
                    .map_err(project_error)?;
                self.schedule_project(
                    id,
                    async move {
                        ProjectCompletion::Removed {
                            project: p.project_id,
                            result: ticket.completed().await.map_err(project_error),
                        }
                    },
                    Some(p.project_id),
                );
            }
            "projects.create" => {
                let p: CreateProjectParams = parse_params(request.params)?;
                crate::project::validate_project_name(&p.name).map_err(project_error)?;
                self.packages.reserve_capacity()?;
                let root = owner.root().join(&p.name);
                if self
                    .projects
                    .list()
                    .iter()
                    .any(|record| Path::new(&record.project.root_path) == root)
                {
                    return Err(project_error(ProjectError::AlreadyExists(
                        root.display().to_string(),
                    )));
                }
                self.ensure_root_idle(&root, &id)?;
                self.packages.preparing.insert(
                    id.clone(),
                    Preparation {
                        project: None,
                        root: Some(root),
                        removing: false,
                    },
                );
                let ticket = owner
                    .submit(move |manager| manager.prepare_project(Some(&p.name), None))
                    .map_err(project_error)?;
                self.schedule_project(
                    id,
                    async move {
                        ProjectCompletion::Prepared {
                            method,
                            kind: "PROJECT",
                            result: ticket.completed().await.map_err(project_error),
                        }
                    },
                    None,
                );
            }
            "projects.register" => {
                let p: RegisterProjectParams = parse_params(request.params)?;
                self.packages.reserve_capacity()?;
                self.packages.preparing.insert(
                    id.clone(),
                    Preparation {
                        project: None,
                        root: None,
                        removing: false,
                    },
                );
                let ticket = owner
                    .submit(move |manager| manager.resolve_registration(Path::new(&p.path)))
                    .map_err(project_error)?;
                self.schedule_project(
                    id,
                    async move {
                        ProjectCompletion::Resolved {
                            result: ticket.completed().await.map_err(project_error),
                        }
                    },
                    None,
                );
            }
            _ => {
                let (project, packages) = if method == "packages.sync" {
                    let p: ProjectIdParams = parse_params(request.params)?;
                    (p.project_id, Vec::new())
                } else {
                    let p: PackageMutationParams = parse_params(request.params)?;
                    (p.project_id, p.packages)
                };
                self.ensure_package_idle(project)?;
                self.ensure_no_starting_sessions(project)?;
                self.packages.reserve_capacity()?;
                let root = PathBuf::from(
                    self.projects
                        .get(project)
                        .map_err(project_error)?
                        .project
                        .root_path,
                );
                self.packages.preparing.insert(
                    id.clone(),
                    Preparation {
                        project: Some(project),
                        root: Some(root),
                        removing: false,
                    },
                );
                let requested_method = method.clone();
                let ticket = owner
                    .submit(move |manager| {
                        manager.prepare_package(project, &requested_method, &packages)
                    })
                    .map_err(project_error)?;
                self.schedule_project(
                    id,
                    async move {
                        ProjectCompletion::Prepared {
                            method,
                            kind: "PACKAGE",
                            result: ticket.completed().await.map_err(project_error),
                        }
                    },
                    Some(project),
                );
            }
        }
        Ok(())
    }
    fn ensure_root_idle(&self, root: &Path, request: &str) -> Result<(), DomainError> {
        if self
            .packages
            .preparing
            .iter()
            .any(|(id, p)| id != request && p.root.as_deref() == Some(root))
        {
            return Err(operation_error(
                "CK-UV-BUSY",
                "Project path has an environment operation pending",
            ));
        }
        for record in self.projects.list() {
            if Path::new(&record.project.root_path) == root {
                if self.packages.busy(record.project.project_id)
                    || self.packages.preparing.iter().any(|(id, p)| {
                        id != request && p.project == Some(record.project.project_id)
                    })
                {
                    return Err(operation_error(
                        "CK-UV-BUSY",
                        "Project environment or registry mutation is in progress",
                    ));
                }
                self.ensure_no_starting_sessions(record.project.project_id)?;
            }
        }
        Ok(())
    }
    pub(super) fn finish_project_request(
        &mut self,
        id: &str,
        completion: ProjectCompletion<R>,
    ) -> Option<request_work::Outcome> {
        match completion {
            ProjectCompletion::Resolved { result } => {
                let prepared = (|| -> Result<(), DomainError> {
                    let (root, existing) = result?;
                    if self.packages.closing {
                        return Err(operation_error(
                            "CK-RUN-STOPPED",
                            "Runtime stopped before Project preparation",
                        ));
                    }
                    self.ensure_root_idle(&root, id)?;
                    let project = existing.map(|r| r.project.project_id);
                    self.packages.preparing.insert(
                        id.into(),
                        Preparation {
                            project,
                            root: Some(root.clone()),
                            removing: false,
                        },
                    );
                    let ticket = self
                        .projects
                        .owner()?
                        .submit(move |manager| manager.prepare_project(None, Some(&root)))
                        .map_err(project_error)?;
                    self.schedule_project(
                        id.into(),
                        async move {
                            ProjectCompletion::Prepared {
                                method: "projects.register".into(),
                                kind: "PROJECT",
                                result: ticket.completed().await.map_err(project_error),
                            }
                        },
                        project,
                    );
                    Ok(())
                })();
                match prepared {
                    Ok(()) => None,
                    Err(error) => Some(Err(error)),
                }
            }
            ProjectCompletion::Prepared {
                method,
                kind,
                result,
            } => {
                let result =
                    result.and_then(|plan| self.start_environment_plan(&method, kind, plan));
                if result.is_err()
                    && let Some(events) = self.events()
                {
                    events.publish("project.refresh_required", json!({"method":method}));
                }
                Some(result)
            }
            ProjectCompletion::Removed { project, result } => {
                if let Some(events) = self.events() {
                    events.publish(
                        if result.is_ok() {
                            "project.changed"
                        } else {
                            "project.refresh_required"
                        },
                        json!({"method":"projects.remove_from_registry","project_id":project}),
                    );
                }
                Some(result.and_then(to_value))
            }
        }
    }
    pub(crate) fn begin_owner_shutdown(&mut self) {
        self.packages.closing = true;
    }
}
