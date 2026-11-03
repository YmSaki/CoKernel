//! One Host owner for remote intent, credential mutation and bridge reconciliation.
use crate::{
    bridge::{Bridge, BridgeError},
    remote_intent::IntentStore,
    secrets::{Configuration, TunnelCredentials, Vault},
    state::DesiredState,
    supervisor::{HostStatus, ObservedState},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{RwLock, mpsc, oneshot, watch};
use tokio::time::{Instant, timeout};

type Result<T> = std::result::Result<T, BridgeError>;
fn error(code: &'static str) -> BridgeError {
    BridgeError::new(
        code,
        "Remote Access could not complete the requested change; inspect status before retrying",
    )
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum Phase {
    Loading,
    Disabled,
    WaitingForRuntime,
    Starting,
    Connected,
    Degraded,
    Reconnecting,
    Error,
    Stopping,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Status {
    pub desired_enabled: Option<bool>,
    pub state: Phase,
    pub runtime_generation: Option<u64>,
    pub tunnel_generation: Option<u64>,
    pub consecutive_failures: u32,
    pub error_code: Option<&'static str>,
}
enum Job {
    Configure(TunnelCredentials, oneshot::Sender<Result<Configuration>>),
    Configuration(oneshot::Sender<Result<Configuration>>),
    Clear(oneshot::Sender<Result<Configuration>>),
    Enable(bool, oneshot::Sender<Result<Status>>),
    Restart(oneshot::Sender<Result<Status>>),
    Exit,
}
#[derive(Clone)]
pub(crate) struct Handle {
    queue: mpsc::Sender<Job>,
    status: watch::Receiver<Status>,
    closing: Arc<AtomicBool>,
    done: watch::Receiver<bool>,
}
#[derive(Clone)]
pub(crate) struct Runtime {
    pub status: watch::Receiver<HostStatus>,
    pub bridge: Arc<RwLock<Option<Bridge>>>,
}
struct Binding {
    generation: u64,
    runnable: bool,
    bridge: Bridge,
}
impl Runtime {
    async fn binding(&self) -> Option<Binding> {
        let before = self.status.borrow().clone();
        if matches!(
            before.observed,
            ObservedState::Starting | ObservedState::Stopping
        ) {
            return None;
        }
        let bridge = self.bridge.read().await.clone()?;
        let after = self.status.borrow().clone();
        if before.generation != after.generation
            || before.observed != after.observed
            || bridge.terminal().borrow().is_some()
        {
            return None;
        }
        Some(Binding {
            generation: after.generation,
            runnable: after.desired == DesiredState::Running
                && after.observed == ObservedState::Healthy,
            bridge,
        })
    }
}
impl Binding {
    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let reply = timeout(Duration::from_secs(3), self.bridge.call(method, params))
            .await
            .map_err(|_| error("CK-REMOTE-TIMEOUT"))?
            .map_err(|_| error("CK-REMOTE-BRIDGE"))?;
        // Never forward an arbitrary Runtime error/result from a secret-bearing call.
        if !reply.ok {
            return Err(error("CK-REMOTE-RUNTIME"));
        }
        reply.result.ok_or_else(|| error("CK-REMOTE-PROTOCOL"))
    }
    async fn control(&self, method: &str, params: Value) -> Result<u64> {
        let value = self.call(method, params).await?;
        if value.as_object().is_none_or(|v| v.len() != 2) || value["accepted"] != true {
            return Err(error("CK-REMOTE-PROTOCOL"));
        }
        value["epoch"]
            .as_u64()
            .ok_or_else(|| error("CK-REMOTE-PROTOCOL"))
    }
    async fn observation(&self) -> Result<Observation> {
        let value = self.call("runtime.get_tunnel_status", json!({})).await?;
        let invalid = || error("CK-REMOTE-PROTOCOL");
        if !value.as_object().is_some_and(|v| {
            [
                "state",
                "desired_enabled",
                "applied_epoch",
                "generation",
                "pid",
                "consecutive_failures",
            ]
            .iter()
            .all(|name| v.contains_key(*name))
        }) {
            return Err(invalid());
        }
        let state: Phase = serde_json::from_value(value["state"].clone()).map_err(|_| invalid())?;
        if matches!(state, Phase::Loading | Phase::WaitingForRuntime) {
            return Err(invalid());
        }
        let generation = value["generation"].as_u64().ok_or_else(invalid)?;
        let epoch = value["applied_epoch"].as_u64().ok_or_else(invalid)?;
        let enabled = value["desired_enabled"].as_bool().ok_or_else(invalid)?;
        let pid = if value["pid"].is_null() {
            None
        } else {
            Some(
                value["pid"]
                    .as_u64()
                    .filter(|n| *n > 1 && *n <= u32::MAX as u64)
                    .ok_or_else(invalid)?,
            )
        };
        if (state == Phase::Disabled && pid.is_some())
            || (state == Phase::Connected && (!enabled || pid.is_none()))
        {
            return Err(invalid());
        }
        let failures = value["consecutive_failures"]
            .as_u64()
            .filter(|n| *n <= u32::MAX as u64)
            .ok_or_else(invalid)? as u32;
        Ok(Observation {
            state,
            generation,
            epoch,
            enabled,
            failures,
        })
    }
}

pub(crate) async fn dispatch(
    host: &crate::supervisor::Host,
    id: String,
    method: &str,
    params: Value,
) -> cokernel_protocol::ResponseEnvelope {
    let remote = host.remote_access();
    let result: Result<Value> = async {
        if method == "remote_access.configure" {
            let credentials =
                crate::secrets::parse_credentials(params).map_err(|e| error(e.code()))?;
            return remote
                .configure(credentials)
                .await
                .map(|value| json!(value));
        }
        if params != json!({}) {
            return Err(error("CK-REMOTE-INPUT"));
        }
        match method {
            "remote_access.get_configuration" => {
                remote.configuration().await.map(|value| json!(value))
            }
            "remote_access.clear_configuration" => remote.clear().await.map(|value| json!(value)),
            "remote_access.get_status" => Ok(json!(remote.status())),
            "remote_access.start" => remote.enable(true).await.map(|value| json!(value)),
            "remote_access.stop" => remote.enable(false).await.map(|value| json!(value)),
            "remote_access.restart" => remote.restart().await.map(|value| json!(value)),
            _ => Err(error("CK-REMOTE-INPUT")),
        }
    }
    .await;
    match result {
        Ok(value) => crate::ipc::success(id, value),
        Err(e) => crate::ipc::error(id, e.code, &e.message),
    }
}
struct Observation {
    state: Phase,
    generation: u64,
    epoch: u64,
    enabled: bool,
    failures: u32,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Target {
    runtime: u64,
    revision: u64,
    enabled: bool,
}
struct Applied {
    target: Target,
    epoch: u64,
}
struct Owner {
    vault: Vault,
    store: IntentStore,
    runtime: Runtime,
    enabled: bool,
    intent_error: bool,
    revision: u64,
    applied: Option<Applied>,
    attempt: Option<Target>,
    attempt_failures: u32,
    retry_at: Option<Instant>,
    status: watch::Sender<Status>,
}
impl Handle {
    pub fn start(directory: &Path, vault: Vault, runtime: Runtime) -> Self {
        let store = IntentStore::new(directory);
        let (status_tx, status) = watch::channel(Status {
            desired_enabled: None,
            state: Phase::Loading,
            runtime_generation: None,
            tunnel_generation: None,
            consecutive_failures: 0,
            error_code: None,
        });
        let (queue, mut commands) = mpsc::channel(8);
        let closing = Arc::new(AtomicBool::new(false));
        let closed = closing.clone();
        let (done_tx, done) = watch::channel(false);
        tokio::spawn(async move {
            let initial = store.clone();
            let loaded = tokio::task::spawn_blocking(move || initial.load()).await;
            let mut owner = Owner {
                vault,
                store,
                runtime,
                enabled: matches!(loaded, Ok(Ok(true))),
                intent_error: !matches!(loaded, Ok(Ok(_))),
                revision: 0,
                applied: None,
                attempt: None,
                attempt_failures: 0,
                retry_at: None,
                status: status_tx,
            };
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            let mut exiting = false;
            loop {
                let job = tokio::select! {biased;
                    job=commands.recv()=>job,
                    _=tick.tick(),if !exiting=>{owner.reconcile().await;continue;},
                };
                let Some(job) = job else {
                    break;
                };
                match job {
                    Job::Configure(credentials, reply) => {
                        let result = owner
                            .vault
                            .replace(credentials)
                            .await
                            .map_err(|e| error(e.code()));
                        if result.is_ok() {
                            owner.invalidate();
                        }
                        let _ = reply.send(result);
                    }
                    Job::Configuration(reply) => {
                        let _ = reply.send(
                            owner
                                .vault
                                .configuration()
                                .await
                                .map_err(|e| error(e.code())),
                        );
                    }
                    Job::Clear(reply) => {
                        let result = owner.clear().await;
                        let _ = reply.send(result);
                    }
                    Job::Enable(enabled, reply) => {
                        let result = owner.enable(enabled).await;
                        let _ = reply.send(result);
                    }
                    Job::Restart(reply) => {
                        let result = owner.restart().await;
                        let _ = reply.send(result);
                    }
                    Job::Exit => {
                        exiting = true;
                        commands.close();
                    }
                }
                if !exiting {
                    owner.reconcile().await;
                }
            }
            closed.store(true, Ordering::Release);
            done_tx.send_replace(true);
        });
        Self {
            queue,
            status,
            closing,
            done,
        }
    }
    pub fn subscribe(&self) -> watch::Receiver<Status> {
        self.status.clone()
    }
    pub fn status(&self) -> Status {
        self.status.borrow().clone()
    }
    fn submit(&self, job: Job) -> Result<()> {
        if self.closing.load(Ordering::Acquire) {
            return Err(error("CK-REMOTE-CLOSED"));
        }
        self.queue
            .try_send(job)
            .map_err(|_| error("CK-REMOTE-BUSY"))
    }
    pub async fn configure(&self, c: TunnelCredentials) -> Result<Configuration> {
        let (tx, rx) = oneshot::channel();
        self.submit(Job::Configure(c, tx))?;
        rx.await.map_err(|_| error("CK-REMOTE-CLOSED"))?
    }
    pub async fn configuration(&self) -> Result<Configuration> {
        let (tx, rx) = oneshot::channel();
        self.submit(Job::Configuration(tx))?;
        rx.await.map_err(|_| error("CK-REMOTE-CLOSED"))?
    }
    pub async fn clear(&self) -> Result<Configuration> {
        let (tx, rx) = oneshot::channel();
        self.submit(Job::Clear(tx))?;
        rx.await.map_err(|_| error("CK-REMOTE-CLOSED"))?
    }
    pub async fn enable(&self, enabled: bool) -> Result<Status> {
        let (tx, rx) = oneshot::channel();
        self.submit(Job::Enable(enabled, tx))?;
        rx.await.map_err(|_| error("CK-REMOTE-CLOSED"))?
    }
    pub async fn restart(&self) -> Result<Status> {
        let (tx, rx) = oneshot::channel();
        self.submit(Job::Restart(tx))?;
        rx.await.map_err(|_| error("CK-REMOTE-CLOSED"))?
    }
    pub async fn shutdown(&self) -> Result<()> {
        if !self.closing.swap(true, Ordering::AcqRel) {
            self.queue
                .send(Job::Exit)
                .await
                .map_err(|_| error("CK-REMOTE-CLOSED"))?;
        }
        let mut done = self.done.clone();
        timeout(Duration::from_secs(5), async {
            loop {
                if *done.borrow() {
                    return Ok(());
                }
                done.changed()
                    .await
                    .map_err(|_| error("CK-REMOTE-CLOSED"))?;
            }
        })
        .await
        .map_err(|_| error("CK-REMOTE-CLEANUP"))?
    }
}
impl Owner {
    fn invalidate(&mut self) {
        self.revision = self.revision.saturating_add(1);
        self.applied = None;
        self.attempt = None;
        self.attempt_failures = 0;
        self.retry_at = None;
    }
    fn publish(&self, mut value: Status) {
        value.desired_enabled = Some(self.enabled);
        self.status.send_if_modified(|old| {
            if old == &value {
                false
            } else {
                *old = value;
                true
            }
        });
    }

    fn basic(&self, state: Phase, code: Option<&'static str>) -> Status {
        Status {
            desired_enabled: Some(self.enabled),
            state,
            runtime_generation: None,
            tunnel_generation: None,
            consecutive_failures: self.attempt_failures,
            error_code: code,
        }
    }
    async fn persist(&mut self, enabled: bool) -> Result<()> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.save(enabled))
            .await
            .map_err(|_| error("CK-REMOTE-STATE"))??;
        self.enabled = enabled;
        self.intent_error = false;
        self.invalidate();
        Ok(())
    }
    async fn enable(&mut self, enabled: bool) -> Result<Status> {
        if enabled
            && !self
                .vault
                .configuration()
                .await
                .map_err(|e| error(e.code()))?
                .configured
        {
            return Err(error("CK-REMOTE-NO-CREDENTIAL"));
        }
        if self.enabled != enabled || self.intent_error {
            self.persist(enabled).await?;
        }
        self.reconcile().await;
        Ok(self.status.borrow().clone())
    }
    async fn clear(&mut self) -> Result<Configuration> {
        self.persist(false).await?;
        // Clear is successful only after a live bridge confirms revocation. A
        // stopping/uncertain Runtime retains the Windows credential for retry.
        if let Some(binding) = self.runtime.binding().await {
            let epoch = binding.control("tunnel.stop", json!({})).await?;
            timeout(Duration::from_secs(5), async {
                loop {
                    let observed = binding.observation().await?;
                    if observed.state == Phase::Disabled
                        && !observed.enabled
                        && observed.epoch >= epoch
                    {
                        return Ok(());
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await
            .map_err(|_| error("CK-REMOTE-CLEANUP"))??;
        } else if self.runtime.bridge.read().await.is_some() {
            return Err(error("CK-REMOTE-CLEANUP"));
        }
        self.vault.clear().await.map_err(|e| error(e.code()))
    }
    async fn restart(&mut self) -> Result<Status> {
        if !self.enabled || self.intent_error {
            return Err(error("CK-REMOTE-DISABLED"));
        }
        self.invalidate();
        self.reconcile().await;
        let binding = self
            .runtime
            .binding()
            .await
            .ok_or_else(|| error("CK-REMOTE-BRIDGE"))?;
        if !binding.runnable {
            return Err(error("CK-REMOTE-BRIDGE"));
        }
        let applied = self
            .applied
            .as_mut()
            .filter(|a| {
                a.target.runtime == binding.generation
                    && a.target.revision == self.revision
                    && a.target.enabled
            })
            .ok_or_else(|| error("CK-REMOTE-RUNTIME"))?;
        applied.epoch = binding.control("tunnel.restart", json!({})).await?;
        self.reconcile().await;
        Ok(self.status.borrow().clone())
    }
    async fn reconcile(&mut self) {
        if self.intent_error {
            self.publish(self.basic(Phase::Error, Some("CK-REMOTE-STATE")));
            return;
        }
        let Some(binding) = self.runtime.binding().await else {
            self.applied = None;
            self.attempt = None;
            let owns_bridge = self.runtime.bridge.read().await.is_some();
            self.publish(self.basic(
                if owns_bridge {
                    Phase::Stopping
                } else if self.enabled {
                    Phase::WaitingForRuntime
                } else {
                    Phase::Disabled
                },
                None,
            ));
            return;
        };
        let target = Target {
            runtime: binding.generation,
            revision: self.revision,
            enabled: self.enabled && binding.runnable,
        };
        if self.attempt != Some(target) {
            self.attempt = Some(target);
            self.attempt_failures = 0;
            self.retry_at = None;
        }
        if self.attempt_failures >= 4 || self.retry_at.is_some_and(|t| Instant::now() < t) {
            return;
        }
        let result = self.apply(&binding, target).await;
        match result {
            Ok(mut status) => {
                // A response from an old bridge must not certify the current one.
                if self.runtime.binding().await.is_none_or(|current| {
                    current.generation != binding.generation || current.runnable != binding.runnable
                }) {
                    self.applied = None;
                    return;
                }
                self.attempt_failures = 0;
                self.retry_at = None;
                status.runtime_generation = Some(binding.generation);
                self.publish(status);
            }
            Err(e) => {
                self.attempt_failures += 1;
                self.retry_at = [250, 1000, 4000]
                    .get(self.attempt_failures as usize - 1)
                    .map(|ms| Instant::now() + Duration::from_millis(*ms));
                self.publish(self.basic(
                    if self.attempt_failures >= 4 {
                        Phase::Error
                    } else {
                        Phase::Reconnecting
                    },
                    Some(e.code),
                ));
            }
        }
    }
    async fn apply(&mut self, binding: &Binding, target: Target) -> Result<Status> {
        if !self.applied.as_ref().is_some_and(|a| a.target == target) {
            let epoch = if target.enabled {
                let credentials = match self.vault.load_for_tunnel().await {
                    Ok(Some(credentials)) => credentials,
                    unavailable => {
                        binding.control("tunnel.stop", json!({})).await?;
                        return Err(error(match unavailable {
                            Err(e) => e.code(),
                            _ => "CK-REMOTE-NO-CREDENTIAL",
                        }));
                    }
                };
                let key = std::str::from_utf8(credentials.key_for_tunnel())
                    .map_err(|_| error("CK-SEC-CREDENTIAL-DATA"))?;
                binding
                    .control(
                        "tunnel.start",
                        json!({"tunnel_id":credentials.tunnel_id(),"runtime_key":key}),
                    )
                    .await?
            } else {
                binding.control("tunnel.stop", json!({})).await?
            };
            self.applied = Some(Applied { target, epoch });
        }
        let observed = binding.observation().await?;
        let epoch = self.applied.as_ref().map_or(0, |a| a.epoch);
        let mut status = self.basic(
            if target.enabled {
                Phase::Starting
            } else {
                Phase::Stopping
            },
            None,
        );
        if observed.epoch >= epoch && observed.enabled == target.enabled {
            status.state = observed.state;
            if self.enabled && !target.enabled && status.state == Phase::Disabled {
                status.state = Phase::WaitingForRuntime;
            }
            status.tunnel_generation = Some(observed.generation);
            status.consecutive_failures = observed.failures;
        }
        Ok(status)
    }
}
