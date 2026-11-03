//! One filesystem owner and a read-only registry projection for Runtime admission.
use super::*;
use std::sync::{Mutex, RwLock};
use tokio::sync::{mpsc, oneshot};

type Job<R> = Box<dyn FnOnce(&mut ProjectManager<R>, &View) + Send>;
#[derive(Clone)]
struct View(Arc<RwLock<Vec<ProjectRecord>>>);
impl View {
    fn publish<R: UvRunner>(&self, manager: &ProjectManager<R>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = manager.list();
    }
    fn uncertain(&self) {
        for record in self.0.write().unwrap_or_else(|e| e.into_inner()).iter_mut() {
            record.environment_reconcile_required = true;
        }
    }
}
pub(crate) struct ProjectOwner<R> {
    jobs: mpsc::Sender<Job<R>>,
    view: View,
    root: PathBuf,
}
impl<R> Clone for ProjectOwner<R> {
    fn clone(&self) -> Self {
        Self {
            jobs: self.jobs.clone(),
            view: self.view.clone(),
            root: self.root.clone(),
        }
    }
}
pub(crate) struct ProjectTicket<T>(oneshot::Receiver<Result<T, ProjectError>>);
impl<T> ProjectTicket<T> {
    pub(crate) async fn completed(self) -> Result<T, ProjectError> {
        self.0.await.map_err(|_| ProjectError::OwnerClosed)?
    }
}
impl<R: UvRunner + 'static> ProjectOwner<R> {
    pub(crate) fn new(manager: ProjectManager<R>) -> Self {
        let root = manager.projects_root.clone();
        let view = View(Arc::new(RwLock::new(manager.list())));
        let published = view.clone();
        let manager = Arc::new(Mutex::new(manager));
        let (sender, mut jobs) = mpsc::channel::<Job<R>>(16);
        tokio::spawn(async move {
            while let Some(job) = jobs.recv().await {
                let manager = manager.clone();
                let view = published.clone();
                let joined = tokio::task::spawn_blocking(move || {
                    let mut manager = manager.lock().unwrap_or_else(|e| e.into_inner());
                    job(&mut manager, &view);
                })
                .await;
                if joined.is_err() {
                    published.uncertain();
                    break;
                }
            }
        });
        Self {
            jobs: sender,
            view,
            root,
        }
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn list(&self) -> Vec<ProjectRecord> {
        self.view
            .0
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub(crate) fn get(&self, id: ProjectId) -> Result<ProjectRecord, ProjectError> {
        self.view
            .0
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|record| record.project.project_id == id)
            .cloned()
            .ok_or_else(|| ProjectError::NotFound(id.to_string()))
    }
    fn job<T: Send + 'static>(
        work: impl FnOnce(&mut ProjectManager<R>) -> Result<T, ProjectError> + Send + 'static,
    ) -> (Job<R>, ProjectTicket<T>) {
        let (sender, receiver) = oneshot::channel();
        (
            Box::new(move |manager, view| {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(manager)));
                let result = match result {
                    Ok(result) => result,
                    Err(panic) => {
                        view.uncertain();
                        let _ = sender.send(Err(ProjectError::OwnerClosed));
                        std::panic::resume_unwind(panic);
                    }
                };
                // Errors can retain an uncertain identity. Publish it before the reply.
                view.publish(manager);
                let _ = sender.send(result);
            }),
            ProjectTicket(receiver),
        )
    }
    pub(crate) fn submit<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut ProjectManager<R>) -> Result<T, ProjectError> + Send + 'static,
    ) -> Result<ProjectTicket<T>, ProjectError> {
        let (job, ticket) = Self::job(work);
        self.jobs.try_send(job).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => ProjectError::OwnerBusy,
            mpsc::error::TrySendError::Closed(_) => ProjectError::OwnerClosed,
        })?;
        Ok(ticket)
    }
    /// Reconciliation belongs to at most four already accepted environment jobs.
    pub(crate) async fn call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut ProjectManager<R>) -> Result<T, ProjectError> + Send + 'static,
    ) -> Result<T, ProjectError> {
        let (job, ticket) = Self::job(work);
        self.jobs
            .send(job)
            .await
            .map_err(|_| ProjectError::OwnerClosed)?;
        ticket.completed().await
    }
}
