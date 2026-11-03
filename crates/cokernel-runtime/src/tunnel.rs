//! Optional outbound transport; this owner has no authority over notebook workers.
mod health;
mod launch;
mod model;
#[cfg(test)]
mod tests;

use crate::{events::EventHub, mcp_supervisor, owned_child::OwnedChild};
use serde::Serialize;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::watch,
    task::JoinHandle,
    time::{Instant, sleep, timeout},
};
use zeroize::Zeroizing;

/// Deliberately neither Debug nor Serialize. Only the trusted bridge may supply it.
pub struct Credentials {
    id: String,
    key: Zeroizing<String>,
}
impl Credentials {
    pub fn new(id: String, key: String) -> Result<Self, &'static str> {
        let key = Zeroizing::new(key);
        if !(8..=128).contains(&id.len())
            || !id.starts_with("tunnel_")
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || !(16..=2048).contains(&key.len())
            || !key.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err("CK-TUN-CREDENTIAL");
        }
        Ok(Self { id, key })
    }
    pub(crate) fn from_params(mut params: serde_json::Value) -> Result<Self, &'static str> {
        let invalid = "CK-TUN-CREDENTIAL";
        let object = params.as_object_mut().ok_or(invalid)?;
        // Remove the key first so all subsequently rejected string keys are wiped.
        let key = match object.remove("runtime_key") {
            Some(serde_json::Value::String(value)) => Zeroizing::new(value),
            _ => return Err(invalid),
        };
        if object.len() != 1 {
            return Err(invalid);
        }
        let Some(serde_json::Value::String(id)) = object.remove("tunnel_id") else {
            return Err(invalid);
        };
        Self::new(id, key.to_string())
    }
}

#[derive(Clone)]
pub struct Config {
    pub executable: PathBuf,
    pub state_directory: PathBuf,
    pub bearer_file: PathBuf,
}
impl Config {
    pub fn from_env() -> Result<Option<Self>, &'static str> {
        let exe = std::env::var_os("COKERNEL_TUNNEL_EXECUTABLE");
        let directory = std::env::var_os("COKERNEL_TUNNEL_STATE_DIR");
        if exe.is_none() && directory.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            executable: exe.ok_or("CK-TUN-CONFIG")?.into(),
            state_directory: directory.ok_or("CK-TUN-CONFIG")?.into(),
            bearer_file: std::env::var_os("COKERNEL_MCP_BEARER_FILE")
                .ok_or("CK-TUN-CONFIG")?
                .into(),
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum State {
    Disabled,
    Starting,
    Connected,
    Degraded,
    Reconnecting,
    Error,
    Stopping,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Snapshot {
    pub state: State,
    pub desired_enabled: bool,
    pub applied_epoch: u64,
    pub generation: u64,
    pub mcp_generation: Option<u64>,
    pub pid: Option<u32>,
    pub consecutive_failures: u32,
    pub failures: Vec<mcp_supervisor::Failure>,
}
#[derive(Clone, Default)]
struct Desired {
    epoch: u64,
    credentials: Option<Arc<Credentials>>,
}

#[derive(Clone)]
pub struct Monitor {
    state: watch::Sender<Snapshot>,
    desired: watch::Sender<Desired>,
    shutdown: watch::Sender<bool>,
    events: EventHub,
}
impl Default for Monitor {
    fn default() -> Self {
        Self::new(EventHub::default())
    }
}
impl Monitor {
    pub fn new(events: EventHub) -> Self {
        Self {
            state: watch::channel(Snapshot {
                state: State::Disabled,
                desired_enabled: false,
                applied_epoch: 0,
                generation: 0,
                mcp_generation: None,
                pid: None,
                consecutive_failures: 0,
                failures: vec![],
            })
            .0,
            desired: watch::channel(Desired::default()).0,
            shutdown: watch::channel(false).0,
            events,
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.borrow().clone()
    }
    fn publish(&self, state: &Snapshot) {
        if self.state.send_if_modified(|previous| {
            if previous == state {
                false
            } else {
                *previous = state.clone();
                true
            }
        }) {
            if let Ok(value) = serde_json::to_value(state) {
                self.events.publish("tunnel.state_changed", value);
            }
        }
    }
    pub fn failed(&self, code: &'static str) {
        let mut state = self.snapshot();
        state.state = State::Error;
        record(&mut state, code, None);
        self.publish(&state);
    }
    pub fn enable(&self, credentials: Credentials) -> Result<u64, &'static str> {
        if self.desired.receiver_count() == 0 || *self.shutdown.borrow() {
            return Err("CK-TUN-NOT-CONFIGURED");
        }
        let mut epoch = 0;
        self.desired.send_if_modified(|d| {
            if d.credentials.as_ref().is_some_and(|current| {
                current.id == credentials.id && current.key == credentials.key
            }) {
                epoch = d.epoch;
                return false;
            }
            d.epoch = d.epoch.saturating_add(1);
            epoch = d.epoch;
            d.credentials = Some(Arc::new(credentials));
            true
        });
        Ok(epoch)
    }
    pub fn restart(&self) -> Result<u64, &'static str> {
        if self.desired.receiver_count() == 0 || *self.shutdown.borrow() {
            return Err("CK-TUN-NOT-CONFIGURED");
        }
        let mut epoch = None;
        self.desired.send_if_modified(|d| {
            if d.credentials.is_none() {
                return false;
            }
            d.epoch = d.epoch.saturating_add(1);
            epoch = Some(d.epoch);
            true
        });
        epoch.ok_or("CK-TUN-NOT-CONFIGURED")
    }
    /// Nonblocking admission. An accepted disable is independent of its caller.
    pub fn disable(&self) -> u64 {
        let mut epoch = 0;
        self.desired.send_if_modified(|d| {
            if d.credentials.is_none() {
                epoch = d.epoch;
                return false;
            }
            d.epoch = d.epoch.saturating_add(1);
            epoch = d.epoch;
            d.credentials = None;
            true
        });
        epoch
    }
    pub async fn shutdown(&self) -> Result<(), &'static str> {
        self.shutdown.send_replace(true);
        self.disable();
        if self.shutdown.receiver_count() == 0 {
            return if self.snapshot().pid.is_none() {
                Ok(())
            } else {
                Err("CK-TUN-CLEANUP")
            };
        }
        let mut state = self.state.subscribe();
        timeout(Duration::from_secs(3), async {
            loop {
                if state.borrow().state == State::Disabled && state.borrow().pid.is_none() {
                    return Ok(());
                }
                state.changed().await.map_err(|_| "CK-TUN-CLEANUP")?;
            }
        })
        .await
        .map_err(|_| "CK-TUN-CLEANUP")?
    }
}

pub struct Supervisor {
    monitor: Monitor,
    task: JoinHandle<()>,
}
impl Supervisor {
    pub fn start(config: Config, monitor: Monitor, mcp: mcp_supervisor::Monitor) -> Self {
        let desired = monitor.desired.subscribe();
        let stopping = monitor.shutdown.subscribe();
        let observer = monitor.clone();
        let task = tokio::spawn(run(config, observer, desired, stopping, mcp.subscribe()));
        Self { monitor, task }
    }
    pub async fn shutdown(mut self) -> Result<(), &'static str> {
        self.monitor.shutdown().await?;
        (&mut self.task).await.map_err(|_| "CK-TUN-CLEANUP")
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.monitor.shutdown.send_replace(true);
        self.monitor.disable();
    }
}

fn record(state: &mut Snapshot, code: &'static str, exit: Option<std::process::ExitStatus>) {
    use std::os::unix::process::ExitStatusExt;
    if state.failures.len() == 8 {
        state.failures.remove(0);
    }
    state.failures.push(mcp_supervisor::Failure {
        code,
        timestamp: chrono::Utc::now().to_rfc3339(),
        exit_code: exit.as_ref().and_then(|s| s.code()),
        signal: exit.and_then(|s| s.signal()),
    });
}
struct Live {
    child: OwnedChild,
    prepared: launch::Prepared,
    identity: model::Identity,
    connection: model::Connection,
    started: Instant,
    last_good: Instant,
    stable_since: Option<Instant>,
    polling: health::PollFreshness,
    failure: Option<&'static str>,
}
impl Live {
    fn observe(&mut self, observation: std::io::Result<health::Observation>) {
        let now = Instant::now();
        let connected = observation.is_ok_and(|value| self.polling.connected(value, now));
        if connected {
            self.last_good = now;
            self.stable_since.get_or_insert(now);
            self.connection = model::Connection::Connected;
        } else {
            self.stable_since = None;
            if self.connection != model::Connection::Starting
                || self.started.elapsed() >= Duration::from_secs(60)
            {
                self.connection = model::Connection::Degraded;
            }
        }
        if self.last_good.elapsed() >= Duration::from_secs(120) {
            self.failure = Some("CK-TUN-HEALTH");
        }
    }
}

fn target(intent: &Desired, mcp: &mcp_supervisor::Snapshot) -> Option<model::Identity> {
    if intent.credentials.is_some() && mcp.state == mcp_supervisor::State::Ready {
        mcp.address.map(|address| model::Identity {
            epoch: intent.epoch,
            mcp_generation: mcp.generation,
            address,
        })
    } else {
        None
    }
}

async fn run(
    config: Config,
    monitor: Monitor,
    mut desired: watch::Receiver<Desired>,
    mut stopping: watch::Receiver<bool>,
    mut mcp: watch::Receiver<mcp_supervisor::Snapshot>,
) {
    use model::{Decision, Recovery};
    let mut state = monitor.snapshot();
    let mut last_epoch = 0;
    let mut recovery = Recovery::default();
    let mut live: Option<Live> = None;
    loop {
        let intent = desired.borrow_and_update().clone();
        let shutdown = *stopping.borrow();
        let upstream = mcp.borrow_and_update().clone();
        if intent.epoch != last_epoch {
            recovery = Recovery::default();
            last_epoch = intent.epoch;
        }
        let enabled = intent.credentials.is_some() && !shutdown;
        let wanted = target(&intent, &upstream);
        // A failed child must be reaped even if the desired incarnation is unchanged.
        let decision = if live.as_ref().is_some_and(|l| l.failure.is_some()) {
            Decision::Stop
        } else {
            model::decide(
                wanted,
                live.as_ref().map(|l| l.identity),
                shutdown,
                &recovery,
                Instant::now(),
            )
        };
        state.state = model::display(decision, enabled, live.as_ref().map(|l| l.connection));
        state.desired_enabled = enabled;
        state.applied_epoch = live.as_ref().map_or(intent.epoch, |l| l.identity.epoch);
        state.pid = live.as_ref().map(|l| l.child.pid as u32);
        state.mcp_generation = live.as_ref().map(|l| l.identity.mcp_generation);
        state.consecutive_failures = recovery.failures;
        monitor.publish(&state);
        match decision {
            Decision::Finish => break,
            Decision::Stop => {
                if let Some(mut owned) = live.take() {
                    let status = cleanup(&mut owned.child, &mut state, &monitor).await;
                    if let Some(code) = owned.failure {
                        if owned.identity.epoch == last_epoch {
                            recovery.failed(
                                Instant::now(),
                                owned
                                    .stable_since
                                    .is_some_and(|t| t.elapsed() >= Duration::from_secs(60)),
                            );
                        }
                        record(&mut state, code, Some(status));
                    }
                    // Reconciliation cannot launch a replacement until this owner
                    // and its child group have completed cleanup.
                }
            }
            Decision::Launch(identity) => {
                let owned_config = config.clone();
                let prepared =
                    tokio::task::spawn_blocking(move || launch::prepare(&owned_config)).await;
                // Re-read desired facts after asynchronous preparation. A result
                // belongs only to the incarnation for which it was prepared.
                let latest = desired.borrow().clone();
                let latest_mcp = mcp.borrow().clone();
                let current_target = target(&latest, &latest_mcp);
                if model::decide(
                    current_target,
                    None,
                    *stopping.borrow(),
                    &recovery,
                    Instant::now(),
                ) != Decision::Launch(identity)
                {
                    continue;
                }
                state.generation = state.generation.saturating_add(1);
                let mut prepared = match prepared {
                    Ok(Ok(value)) => value,
                    _ => {
                        record(&mut state, "CK-TUN-CONFIG", None);
                        recovery.failed(Instant::now(), false);
                        continue;
                    }
                };
                let Some(credentials) = latest.credentials else {
                    continue;
                };
                let launched = launch::launch(&config, &prepared, &credentials, identity.address);
                prepared.clear_bearer();
                let child = match launched {
                    Ok(child) => child,
                    Err(_) => {
                        record(&mut state, "CK-TUN-LAUNCH", None);
                        recovery.failed(Instant::now(), false);
                        continue;
                    }
                };
                let now = Instant::now();
                live = Some(Live {
                    child,
                    prepared,
                    identity,
                    connection: model::Connection::Starting,
                    started: now,
                    last_good: now,
                    stable_since: None,
                    polling: health::PollFreshness::default(),
                    failure: None,
                });
            }
            Decision::Observe => {
                if let Some(owned) = &mut live {
                    tokio::select! {biased;
                        _=stopping.changed()=>{}, _=desired.changed()=>{}, _=mcp.changed()=>{},
                        _=owned.child.exited()=>owned.failure=Some("CK-TUN-EXIT"),
                        _=sleep(Duration::from_millis(500))=>owned.observe(health::probe(&owned.prepared.socket).await),
                    }
                }
            }
            Decision::Backoff => {
                if let Some(deadline) = recovery.retry_at {
                    tokio::select! {biased;
                        _=stopping.changed()=>{}, _=desired.changed()=>{}, _=mcp.changed()=>{},
                        _=tokio::time::sleep_until(deadline)=>{},
                    }
                }
            }
            Decision::Wait | Decision::Exhausted => {
                tokio::select! {biased; _=stopping.changed()=>{}, _=desired.changed()=>{}, _=mcp.changed()=>{} }
            }
        }
    }
}

async fn cleanup(
    child: &mut OwnedChild,
    state: &mut Snapshot,
    monitor: &Monitor,
) -> std::process::ExitStatus {
    loop {
        match child.cleanup().await {
            Ok(status) => return status,
            Err(_) => {
                record(state, "CK-TUN-CLEANUP", None);
                monitor.publish(state);
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
}
