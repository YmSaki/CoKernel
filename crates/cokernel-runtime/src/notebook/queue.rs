//! FIFO ownership of document filesystem work, off the Runtime admission task.
use super::{NotebookError, NotebookService};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};

type Job = Box<dyn FnOnce(&mut NotebookService) + Send>;

#[derive(Clone)]
pub struct DocumentQueue(mpsc::Sender<Job>);
pub struct DocumentTicket<T>(oneshot::Receiver<Result<T, NotebookError>>);
impl<T> DocumentTicket<T> {
    pub async fn completed(self) -> Result<T, NotebookError> {
        self.0.await.map_err(|_| {
            NotebookError::Invalid("Document owner stopped before returning a result".into())
        })?
    }
}

impl DocumentQueue {
    pub(super) fn new(documents: Arc<Mutex<NotebookService>>) -> Self {
        let (sender, mut jobs) = mpsc::channel::<Job>(32);
        tokio::spawn(async move {
            while let Some(job) = jobs.recv().await {
                let documents = documents.clone();
                // Cancellation of a caller must not abort an accepted file write.
                // Join every job before taking the next, including failure paths.
                let _ =
                    tokio::task::spawn_blocking(move || job(&mut documents.blocking_lock())).await;
            }
        });
        Self(sender)
    }
    fn job<T: Send + 'static>(
        work: impl FnOnce(&mut NotebookService) -> Result<T, NotebookError> + Send + 'static,
    ) -> (Job, DocumentTicket<T>) {
        let (sender, receiver) = oneshot::channel();
        (
            Box::new(move |documents| {
                let _ = sender.send(work(documents));
            }),
            DocumentTicket(receiver),
        )
    }
    /// Public admission never waits for filesystem work or queue capacity.
    pub fn submit<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut NotebookService) -> Result<T, NotebookError> + Send + 'static,
    ) -> Result<DocumentTicket<T>, NotebookError> {
        let (job, ticket) = Self::job(work);
        self.0.try_send(job).map_err(|_| NotebookError::Limit)?;
        Ok(ticket)
    }
    /// Already-owned persistence/shutdown work can wait for a queue slot. Its
    /// caller is bounded by accepted execution capacity, not an unbounded producer.
    pub async fn call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut NotebookService) -> Result<T, NotebookError> + Send + 'static,
    ) -> Result<T, NotebookError> {
        let (job, ticket) = Self::job(work);
        self.0
            .send(job)
            .await
            .map_err(|_| NotebookError::Invalid("Document owner is closed".into()))?;
        ticket.completed().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cokernel_domain::{Project, ProjectId};

    #[tokio::test]
    async fn capacity_rejection_and_dropped_reply_do_not_erase_accepted_document_jobs() {
        let root = std::env::temp_dir().join(format!("ckdocq-{}", ProjectId::new()));
        std::fs::create_dir(&root).unwrap();
        let project = Project {
            project_id: ProjectId::new(),
            name: "queue".into(),
            root_path: root.to_str().unwrap().into(),
            environment_generation: 1,
        };
        let queue = DocumentQueue::new(Arc::new(Mutex::new(NotebookService::default())));
        let mut tickets = Vec::new();
        // No await: the owner cannot consume slots until all 32 are reserved.
        for i in 0..32 {
            let project = project.clone();
            tickets.push(
                queue
                    .submit(move |docs| docs.create(&project, &format!("{i}.ipynb")))
                    .unwrap(),
            );
        }
        drop(tickets.remove(0));
        let extra = project.clone();
        assert!(
            queue
                .submit(move |docs| docs.create(&extra, "rejected.ipynb"))
                .is_err()
        );
        for ticket in tickets {
            ticket.completed().await.unwrap();
        }
        assert!(root.join("0.ipynb").is_file());
        assert!(!root.join("rejected.ipynb").exists());
        // Fault injection affects one caller, not the owner's remaining jobs.
        let failed = queue
            .submit::<()>(|_| panic!("document job fault injection"))
            .unwrap();
        let after = queue
            .submit(move |docs| docs.create(&project, "after.ipynb"))
            .unwrap();
        assert!(failed.completed().await.is_err());
        after.completed().await.unwrap();
        assert!(root.join("after.ipynb").is_file());
        drop(queue);
        std::fs::remove_dir_all(root).unwrap();
    }
}
