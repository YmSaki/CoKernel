use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use cokernel_domain::{OperationId, ProjectId};
use serde::Serialize;
use tokio::task::JoinSet;

use super::*;
use crate::project::ProjectRecord;
use crate::uv::UvRunControl;

const MAX_ACTIVE: usize = 4;
const MAX_RETAINED: usize = 256;

pub(super) type PackageOutcome = (
    OperationId,
    Result<(), DomainError>,
    Result<ProjectRecord, DomainError>,
);
pub struct PackageCompletion(Result<PackageOutcome, tokio::task::JoinError>);

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "SCREAMING_SNAKE_CASE")]
enum PackageResult {
    Running,
    Succeeded {
        project: ProjectRecord,
    },
    Failed {
        error: DomainError,
        reconciliation_error: Option<DomainError>,
    },
}

#[derive(Clone, Serialize)]
struct PackageOperation {
    operation_id: OperationId,
    project_id: ProjectId,
    kind: &'static str,
    method: String,
    result: PackageResult,
    cancellation_requested: bool,
    #[serde(skip)]
    control: UvRunControl,
    #[serde(skip)]
    task_id: Option<tokio::task::Id>,
}

#[derive(Default)]
pub(super) struct PackageOperations {
    records: VecDeque<PackageOperation>,
    pub(super) tasks: JoinSet<PackageOutcome>,
    pub(super) preparing: HashMap<String, Preparation>,
    pub(super) closing: bool,
}

pub(super) struct Preparation {
    pub project: Option<ProjectId>,
    pub root: Option<PathBuf>,
    pub removing: bool,
}
impl PackageOperations {
    pub(super) fn release_preparation(&mut self, request: &str) {
        self.preparing.remove(request);
    }
    pub(super) fn reserve_capacity(&self) -> Result<(), DomainError> {
        if self.closing
            || self.tasks.len() + self.preparing.values().filter(|p| !p.removing).count()
                >= MAX_ACTIVE
        {
            return Err(operation_error(
                "CK-UV-BUSY",
                "Runtime environment operation capacity is full or stopping",
            ));
        }
        Ok(())
    }
    pub(super) fn cancel_all(&self) {
        for record in &self.records {
            if matches!(record.result, PackageResult::Running) {
                record.control.cancel();
            }
        }
    }

    pub(super) fn cancel(
        &mut self,
        project: ProjectId,
        operation: OperationId,
    ) -> Result<Value, DomainError> {
        let record = self.records.iter_mut().find(|r| r.operation_id == operation && r.project_id == project)
            .ok_or_else(|| operation_error("CK-UV-OPERATION-NOT-FOUND", "Package operation was not found for this Project; use Session interrupt for execution"))?;
        let accepted = matches!(record.result, PackageResult::Running);
        if accepted {
            record.control.cancel();
            record.cancellation_requested = true;
        }
        Ok(json!({"operation_id":operation,"accepted":accepted}))
    }
    pub(super) fn busy(&self, project: ProjectId) -> bool {
        self.records
            .iter()
            .any(|r| r.project_id == project && matches!(r.result, PackageResult::Running))
    }

    pub(super) fn active(&self) -> bool {
        !self.tasks.is_empty() || !self.preparing.is_empty()
    }

    pub(super) fn snapshot(&self) -> Value {
        json!(self.records)
    }

    pub(super) fn get(
        &self,
        project: ProjectId,
        operation: OperationId,
    ) -> Option<Result<Value, DomainError>> {
        self.records
            .iter()
            .find(|r| r.operation_id == operation)
            .map(|r| {
                if r.project_id != project {
                    Err(operation_error(
                        "CK-UV-OPERATION-NOT-FOUND",
                        "Package operation was not found for this Project",
                    ))
                } else {
                    to_value(r)
                }
            })
    }
}

impl Drop for PackageOperations {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

pub(super) fn operation_error(code: &str, summary: &str) -> DomainError {
    DomainError { code: code.into(), component: "environment".into(), summary: summary.into(), action: "Inspect the package operation and retry after it completes; do not restart a live Session implicitly".into(), detail: None }
}

impl<R: UvRunner + 'static> RuntimeService<R> {
    pub(super) fn ensure_package_idle(&self, project: ProjectId) -> Result<(), DomainError> {
        let root = PathBuf::from(
            self.projects
                .get(project)
                .map_err(project_error)?
                .project
                .root_path,
        );
        if self.packages.busy(project)
            || self
                .packages
                .preparing
                .values()
                .any(|p| p.project == Some(project) || p.root.as_ref() == Some(&root))
        {
            Err(operation_error(
                "CK-UV-BUSY",
                "Project environment or registry mutation is in progress",
            ))
        } else {
            Ok(())
        }
    }
    pub(super) fn ensure_no_project_removal(&self, project: ProjectId) -> Result<(), DomainError> {
        if self
            .packages
            .preparing
            .values()
            .any(|p| p.removing && p.project == Some(project))
        {
            Err(operation_error(
                "CK-PROJ-BUSY",
                "Project registry removal is in progress",
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn start_environment_plan(
        &mut self,
        method: &str,
        kind: &'static str,
        plan: crate::project::EnvironmentPlan<R>,
    ) -> Result<Value, DomainError> {
        if self.packages.closing {
            return Err(operation_error(
                "CK-RUN-STOPPED",
                "Runtime stopped before environment launch; identity remains available for synchronization",
            ));
        }
        let owner = self.projects.owner()?;
        let crate::project::EnvironmentPlan {
            project_id,
            root,
            runner,
            commands,
        } = plan;
        if self.packages.records.len() >= MAX_RETAINED {
            // Running records are never evicted. Admission bounds ensure a terminal one exists.
            if let Some(index) = self
                .packages
                .records
                .iter()
                .position(|r| !matches!(r.result, PackageResult::Running))
            {
                self.packages.records.remove(index);
            }
        }
        let operation_id = OperationId::new();
        let control = UvRunControl::default();
        let record = PackageOperation {
            operation_id,
            project_id,
            kind,
            method: method.into(),
            result: PackageResult::Running,
            cancellation_requested: false,
            control: control.clone(),
            task_id: None,
        };
        let result = to_value(&record)?;
        self.packages.records.push_back(record);
        if let Some(events) = self.events() {
            events.publish("environment.operation_progress", result.clone());
            if kind == "PROJECT" {
                events.publish(
                    "project.changed",
                    json!({"project_id":project_id,"operation_id":operation_id}),
                );
            }
        }
        let task = self.packages.tasks.spawn(async move {
            let launched = tokio::task::spawn_blocking(move || {
                commands.iter().try_for_each(|invocation| {
                    runner
                        .run_controlled(invocation, &control)
                        .map(|_| ())
                        .map_err(|e| project_error(ProjectError::Uv(e)))
                })
            })
            .await;
            let outcome = launched.unwrap_or_else(|_| {
                Err(operation_error(
                    "CK-UV-TASK",
                    "Package supervisor task failed; environment state is uncertain",
                ))
            });
            let uncertain = outcome.is_err();
            let reconciled = owner
                .call(move |manager| manager.finish_package(project_id, &root, uncertain))
                .await
                .map_err(project_error);
            (operation_id, outcome, reconciled)
        });
        if let Some(record) = self.packages.records.back_mut() {
            record.task_id = Some(task.id());
        }
        Ok(result)
    }

    /// The bridge drives this alongside input reads, so completion never requires polling.
    pub async fn next_package_completion(&mut self) -> PackageCompletion {
        if self.packages.tasks.is_empty() {
            return std::future::pending().await;
        }
        if let Some(joined) = self.packages.tasks.join_next().await {
            return PackageCompletion(joined);
        }
        std::future::pending().await
    }

    pub async fn finish_package_completion(&mut self, completion: PackageCompletion) {
        self.finish_package_task(completion.0).await;
    }

    pub async fn settle_packages(&mut self) {
        while !self.packages.tasks.is_empty() {
            let completion = self.next_package_completion().await;
            self.finish_package_completion(completion).await;
        }
    }

    pub async fn cancel_and_settle_packages(&mut self) {
        self.packages.cancel_all();
        self.settle_packages().await;
    }

    pub(super) async fn drain_package_completions(&mut self) {
        while let Some(joined) = self.packages.tasks.try_join_next() {
            self.finish_package_task(joined).await;
        }
    }

    pub(super) async fn finish_package_task(
        &mut self,
        joined: Result<PackageOutcome, tokio::task::JoinError>,
    ) {
        let (id, outcome, reconciled) = match joined {
            Ok(value) => value,
            Err(error) => {
                let Some(record) = self
                    .packages
                    .records
                    .iter()
                    .find(|r| r.task_id == Some(error.id()))
                else {
                    return;
                };
                record.control.cancel();
                (
                    record.operation_id,
                    Err(operation_error(
                        "CK-UV-TASK",
                        "Package supervisor task failed; environment state is uncertain",
                    )),
                    Err(operation_error(
                        "CK-PROJ-OWNER",
                        "Environment reconciliation did not complete",
                    )),
                )
            }
        };
        let Some(index) = self
            .packages
            .records
            .iter()
            .position(|r| r.operation_id == id)
        else {
            return;
        };
        let record = &self.packages.records[index];
        let project_id = record.project_id;
        if let Some(runtime) = &self.notebooks {
            match &reconciled {
                Ok(record) => {
                    runtime
                        .sessions
                        .mark_project_environment_stale(
                            project_id,
                            record.project.environment_generation,
                        )
                        .await
                }
                Err(_) => {
                    runtime
                        .sessions
                        .mark_project_environment_uncertain(project_id)
                        .await
                }
            }
        }
        let result = match (outcome, reconciled) {
            (Ok(()), Ok(project)) => PackageResult::Succeeded { project },
            (Err(error), other) => PackageResult::Failed {
                error,
                reconciliation_error: other.err(),
            },
            (Ok(()), Err(error)) => PackageResult::Failed {
                error,
                reconciliation_error: None,
            },
        };
        self.packages.records[index].result = result;
        if let Some(events) = self.events() {
            events.publish(
                "environment.operation_progress",
                json!(self.packages.records[index]),
            );
            events.publish(
                "environment.changed",
                json!({"project_id":project_id,"operation_id":id}),
            );
        }
    }
}
