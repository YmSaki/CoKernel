use super::*;
use crate::project::ProjectRecord;

/// Read-only access exposed to domain admission and component callers.
pub struct ProjectAccess<R>(Access<R>);
enum Access<R> {
    Direct(ProjectManager<R>),
    #[cfg(target_os = "linux")]
    Owned(crate::project::owner::ProjectOwner<R>),
}
impl<R: UvRunner + 'static> ProjectAccess<R> {
    pub(super) fn direct(manager: ProjectManager<R>) -> Self {
        Self(Access::Direct(manager))
    }
    #[cfg(target_os = "linux")]
    pub(super) fn owned(manager: ProjectManager<R>) -> Self {
        Self(Access::Owned(crate::project::owner::ProjectOwner::new(
            manager,
        )))
    }
    pub fn list(&self) -> Vec<ProjectRecord> {
        match &self.0 {
            Access::Direct(manager) => manager.list(),
            #[cfg(target_os = "linux")]
            Access::Owned(owner) => owner.list(),
        }
    }
    pub fn get(&self, id: cokernel_domain::ProjectId) -> Result<ProjectRecord, ProjectError> {
        match &self.0 {
            Access::Direct(manager) => manager.get(id),
            #[cfg(target_os = "linux")]
            Access::Owned(owner) => owner.get(id),
        }
    }
    pub(super) fn synchronous(&mut self) -> Result<&mut ProjectManager<R>, DomainError> {
        match &mut self.0 {
            Access::Direct(manager) => Ok(manager),
            #[cfg(target_os = "linux")]
            Access::Owned(_) => Err(DomainError {
                code: "CK-RUN-OWNED-OPERATION".into(),
                component: "runtime".into(),
                summary: "Project storage requires owned admission".into(),
                action: "Use the shared Runtime request endpoint".into(),
                detail: None,
            }),
        }
    }
    #[cfg(target_os = "linux")]
    pub(super) fn owner(&self) -> Result<crate::project::owner::ProjectOwner<R>, DomainError> {
        match &self.0 {
            Access::Owned(owner) => Ok(owner.clone()),
            Access::Direct(_) => Err(project_error(ProjectError::OwnerClosed)),
        }
    }
}
