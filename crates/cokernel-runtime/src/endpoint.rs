//! Transport-independent ownership of Runtime admission and correlated replies.
//!
//! A client owns neither the domain service nor its task. Dropping a reply ticket
//! discards only delivery; accepted domain work stays owned by Runtime.
use std::{collections::HashMap, sync::Arc};

use cokernel_domain::{DomainError, ExecutionOrigin};
use cokernel_protocol::{RequestEnvelope, ResponseEnvelope};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot},
    task::JoinHandle,
};

use crate::{service::RuntimeService, uv::UvRunner};

const MAX_REQUESTS: usize = 32;

fn error(code: &str, summary: &str) -> DomainError {
    DomainError {
        code: code.into(),
        component: "runtime".into(),
        summary: summary.into(),
        action:
            "Refresh Runtime status before retrying; accepted work is not replayed automatically"
                .into(),
        detail: None,
    }
}
fn stopped() -> DomainError {
    error("CK-RUN-STOPPED", "Runtime request endpoint is closed")
}
fn failed_response(id: String, error: DomainError) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol: 1,
        request_id: id,
        ok: false,
        result: None,
        error: Some(error),
    }
}

/// All methods use the same result channel. Only an acknowledged whole-Runtime
/// shutdown asks the transport to close; rejected shutdown remains usable.
pub struct RuntimeReply {
    pub response: ResponseEnvelope,
    pub close_transport: bool,
}
struct Delivered {
    reply: RuntimeReply,
    // Includes replies a client has not consumed yet, not just queued work.
    _permit: OwnedSemaphorePermit,
}
pub struct ResponseTicket {
    response: oneshot::Receiver<Delivered>,
}
impl ResponseTicket {
    pub async fn wait(self) -> Result<RuntimeReply, DomainError> {
        self.response
            .await
            .map(|delivered| delivered.reply)
            .map_err(|_| stopped())
    }
}
struct ReplySlot {
    caller_id: String,
    response: oneshot::Sender<Delivered>,
    permit: OwnedSemaphorePermit,
}
impl ReplySlot {
    fn send(self, mut response: ResponseEnvelope, close_transport: bool) {
        response.request_id = self.caller_id;
        let _ = self.response.send(Delivered {
            reply: RuntimeReply {
                response,
                close_transport,
            },
            _permit: self.permit,
        });
    }
}
struct Submission {
    request: RequestEnvelope,
    origin: ExecutionOrigin,
    slot: ReplySlot,
}

/// Cloneable trusted adapter handle. Authentication chooses `origin` before
/// calling this API; it must never be read from client JSON.
#[derive(Clone)]
pub struct RuntimeHandle {
    requests: mpsc::Sender<Submission>,
    capacity: Arc<Semaphore>,
}
impl RuntimeHandle {
    /// Enqueue synchronously, without borrowing the Runtime while awaiting work.
    /// Duplicate caller IDs on independent connections cannot collide internally.
    pub fn submit(
        &self,
        mut request: RequestEnvelope,
        origin: ExecutionOrigin,
    ) -> Result<ResponseTicket, DomainError> {
        if self.requests.is_closed() {
            return Err(stopped());
        }
        cokernel_protocol::encode_json_frame(&request, cokernel_protocol::DEFAULT_MAX_FRAME_BYTES)
            .map_err(|_| {
                error(
                    "CK-RUN-REQUEST-LIMIT",
                    "Request exceeds the endpoint frame limit",
                )
            })?;
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| error("CK-RUN-BUSY", "Runtime request endpoint capacity is full"))?;
        let caller_id =
            std::mem::replace(&mut request.request_id, uuid::Uuid::new_v4().to_string());
        let (response, receiver) = oneshot::channel();
        let submission = Submission {
            request,
            origin,
            slot: ReplySlot {
                caller_id,
                response,
                permit,
            },
        };
        self.requests.try_send(submission).map_err(|e| match e {
            mpsc::error::TrySendError::Closed(_) => stopped(),
            mpsc::error::TrySendError::Full(_) => {
                error("CK-RUN-BUSY", "Runtime request queue is full")
            }
        })?;
        Ok(ResponseTicket { response: receiver })
    }
    pub async fn call(
        &self,
        request: RequestEnvelope,
        origin: ExecutionOrigin,
    ) -> Result<RuntimeReply, DomainError> {
        self.submit(request, origin)?.wait().await
    }
}

/// Held by the Runtime daemon, never by an individual Desktop/MCP client.
pub struct RuntimeOwner {
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), DomainError>>>,
    // Keep admission alive even when no transport clients remain.
    _requests: mpsc::Sender<Submission>,
}
impl RuntimeOwner {
    pub fn start<R: UvRunner + 'static>(service: RuntimeService<R>) -> (RuntimeHandle, Self) {
        let (requests, receiver) = mpsc::channel(MAX_REQUESTS);
        let (stop, stopping) = oneshot::channel();
        let handle = RuntimeHandle {
            requests: requests.clone(),
            capacity: Arc::new(Semaphore::new(MAX_REQUESTS)),
        };
        let task = tokio::spawn(run(service, receiver, stopping));
        (
            handle,
            Self {
                stop: Some(stop),
                task: Some(task),
                _requests: requests,
            },
        )
    }
    /// Daemon shutdown/stdio EOF, not client disconnection. Cleanup is joined.
    pub async fn finish(mut self) -> Result<(), DomainError> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        match self.task.take() {
            Some(task) => task
                .await
                .map_err(|_| error("CK-RUN-OWNER", "Runtime owner task failed"))?,
            None => Ok(()),
        }
    }
}
impl Drop for RuntimeOwner {
    fn drop(&mut self) {
        // Do not abort cleanup. The task remains alive long enough to reap children.
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

fn deliver(pending: &mut HashMap<String, ReplySlot>, response: ResponseEnvelope, close: bool) {
    if let Some(slot) = pending.remove(&response.request_id) {
        slot.send(response, close);
    }
}

async fn run<R: UvRunner + 'static>(
    mut service: RuntimeService<R>,
    mut requests: mpsc::Receiver<Submission>,
    mut stopping: oneshot::Receiver<()>,
) -> Result<(), DomainError> {
    let mut pending = HashMap::new();
    let mut shutdown_complete = false;
    loop {
        tokio::select! {
            // Owner termination must not starve behind continuously arriving work.
            biased;
            _ = &mut stopping => break,
            completed = service.next_background_completion() => {
                service.finish_background_completion(completed).await;
            }
            submission = requests.recv() => {
                let Some(submission) = submission else { break; };
                let shutdown = submission.request.protocol == 1
                    && submission.request.method == "bridge.shutdown"
                    && submission.request.params == serde_json::json!({});
                pending.insert(submission.request.request_id.clone(), submission.slot);
                let response = service.admit(submission.request, submission.origin).await;
                for response in service.take_responses() { deliver(&mut pending, response, false); }
                if let Some(response) = response {
                    shutdown_complete = shutdown && response.ok;
                    deliver(&mut pending, response, shutdown_complete);
                }
                if shutdown_complete { break; }
            }
        }
        for response in service.take_responses() {
            deliver(&mut pending, response, false);
        }
    }
    requests.close();
    // Requests not yet admitted must not begin after shutdown.
    while let Some(submission) = requests.recv().await {
        submission
            .slot
            .send(failed_response(String::new(), stopped()), false);
    }
    let cleanup = if shutdown_complete {
        Ok(())
    } else {
        service.begin_owner_shutdown();
        service.cancel_and_drain_requests().await;
        service.cancel_and_settle_packages().await;
        service.shutdown().await
    };
    for response in service.take_responses() {
        deliver(&mut pending, response, false);
    }
    for (_, slot) in pending {
        slot.send(failed_response(String::new(), stopped()), false);
    }
    cleanup
}
