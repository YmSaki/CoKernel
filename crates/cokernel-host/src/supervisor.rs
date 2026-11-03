use crate::{
    bridge::{Bridge, BridgeCommand, BridgeError, BridgePolicy},
    state::{DesiredState, StateStore},
};
use cokernel_protocol::ResponseEnvelope;
use serde::Serialize;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    sync::{RwLock, broadcast, mpsc, oneshot, watch},
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObservedState {
    Stopped,
    Starting,
    Healthy,
    Stopping,
    Error,
    Degraded,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostStatus {
    pub desired: DesiredState,
    pub observed: ObservedState,
    pub generation: u64,
    pub consecutive_failures: u32,
    pub last_error: Option<BridgeError>,
    pub runtime_version: Option<String>,
    pub runtime_capabilities: Vec<String>,
}
#[derive(Debug, Clone)]
pub struct HostRuntimeEvent {
    pub generation: u64,
    pub runtime_sequence: Option<u64>,
    pub event: String,
    pub payload: Value,
}
struct EventForwarder(Option<tokio::task::JoinHandle<()>>);
impl EventForwarder {
    fn clear(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}
impl Drop for EventForwarder {
    fn drop(&mut self) {
        self.clear();
    }
}
#[derive(Debug, Clone)]
pub struct RecoveryPolicy {
    pub delays: Vec<Duration>,
    pub stable_after: Duration,
}
impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            delays: vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8),
            ],
            stable_after: Duration::from_secs(60),
        }
    }
}
enum Control {
    Set(DesiredState, oneshot::Sender<Result<(), BridgeError>>),
    Exit(oneshot::Sender<Result<(), BridgeError>>),
}
#[derive(Clone)]
pub struct Host {
    #[cfg(windows)]
    remote: crate::remote_access::Handle,
    resources: Arc<crate::resources::HostResources>,
    #[cfg(windows)]
    autostart: Option<crate::autostart::AutostartTarget>,
    control: mpsc::Sender<Control>,
    status: watch::Receiver<HostStatus>,
    bridge: Arc<RwLock<Option<Bridge>>>,
    events: broadcast::Sender<HostRuntimeEvent>,
}
impl Host {
    #[cfg(windows)]
    pub(crate) fn remote_access(&self) -> &crate::remote_access::Handle {
        &self.remote
    }
    #[cfg(windows)]
    pub fn with_autostart(mut self, target: crate::autostart::AutostartTarget) -> Self {
        self.autostart = Some(target);
        self
    }
    #[cfg(windows)]
    pub(crate) fn autostart_target(&self) -> Option<crate::autostart::AutostartTarget> {
        self.autostart.clone()
    }
    pub fn start(
        directory: &Path,
        command: BridgeCommand,
        bridge_policy: BridgePolicy,
        recovery: RecoveryPolicy,
    ) -> Result<Self, BridgeError> {
        if recovery.delays.len() > 10 || recovery.delays.iter().any(|d| d.is_zero()) {
            return Err(BridgeError::new(
                "CK-HOST-CONFIG",
                "Recovery policy must have bounded, nonzero delays",
            ));
        }
        let (store, desired) = StateStore::open(directory)?;
        #[cfg(windows)]
        let secrets = crate::secrets::Vault::start(
            crate::secrets::WindowsSecretStore::for_state(directory)
                .map_err(|error| BridgeError::new(error.code(), error.to_string()))?,
        );
        let initial = HostStatus {
            desired,
            observed: ObservedState::Stopped,
            generation: 0,
            consecutive_failures: 0,
            last_error: None,
            runtime_version: None,
            runtime_capabilities: Vec::new(),
        };
        let (status_tx, status) = watch::channel(initial.clone());
        let (control, mut commands) = mpsc::channel(8);
        let bridge = Arc::new(RwLock::new(None::<Bridge>));
        let shared = bridge.clone();
        let events = broadcast::channel(256).0;
        let observations = events.clone();
        #[cfg(windows)]
        let remote = crate::remote_access::Handle::start(
            directory,
            secrets,
            crate::remote_access::Runtime {
                status: status.clone(),
                bridge: bridge.clone(),
            },
        );
        #[cfg(windows)]
        let remote_owner = remote.clone();
        tokio::spawn(async move {
            let mut forwarder = EventForwarder(None);
            let mut state = initial;
            let mut next_start = (desired == DesiredState::Running).then(Instant::now);
            let mut started = Instant::now();
            let mut active: Option<Bridge> = None;
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            loop {
                status_tx.send_if_modified(|last| {
                    if *last == state {
                        false
                    } else {
                        *last = state.clone();
                        true
                    }
                });
                tokio::select! {
                    biased;
                    command=commands.recv()=>{
                        match command {
                            Some(Control::Set(desired,reply))=>{
                                if let Err(error)=store.save(desired) {let _=reply.send(Err(error));continue;}
                                state.desired=desired;
                                if desired==DesiredState::Stopped {
                                    next_start=None;state.observed=ObservedState::Stopping;status_tx.send_replace(state.clone());
                                    let stopped=if let Some(current)=&active {current.stop().await} else {Ok(())};
                                    if let Err(error)=stopped {state.last_error=Some(error.clone());state.observed=ObservedState::Error;status_tx.send_replace(state.clone());let _=reply.send(Err(error));continue;}
                                    active=None;forwarder.clear();*shared.write().await=None;state.runtime_version=None;state.runtime_capabilities.clear();
                                    state.observed=ObservedState::Stopped;
                                } else if active.is_none() {state.consecutive_failures=0;state.last_error=None;next_start=Some(Instant::now());} else {state.observed=ObservedState::Healthy;}
                                status_tx.send_replace(state.clone());let _=reply.send(Ok(()));
                            }
                            request=>{
                                // Host exit preserves desired state for sign-in/relaunch.
                                state.observed=ObservedState::Stopping;status_tx.send_replace(state.clone());
                                let stopped=if let Some(current)=&active {current.stop().await} else {Ok(())};
                                if let Err(error)=&stopped {
                                    state.last_error=Some(error.clone());state.observed=ObservedState::Error;status_tx.send_replace(state.clone());
                                    if error.code=="CK-HOST-SHUTDOWN-REFUSED" && let Some(Control::Exit(reply))=request {let _=reply.send(Err(error.clone()));continue;}
                                } else {state.observed=ObservedState::Stopped;}
                                *shared.write().await=None;
                                state.runtime_version=None;state.runtime_capabilities.clear();status_tx.send_replace(state.clone());
                                #[cfg(windows)]
                                if let Err(error)=remote_owner.shutdown().await {
                                    state.last_error=Some(error.clone());state.observed=ObservedState::Error;status_tx.send_replace(state.clone());
                                    if let Some(Control::Exit(reply))=request {let _=reply.send(Err(error));}
                                    continue;
                                }
                                drop(store);
                                if let Some(Control::Exit(reply))=request {let _=reply.send(stopped);}
                                break;
                            }
                        }
                    }
                    _=tick.tick()=>{
                        if let Some(current)=&active {
                            let terminal=current.terminal().borrow().clone();
                            if let Some(error)=terminal {
                                active=None;forwarder.clear();*shared.write().await=None;state.runtime_version=None;state.runtime_capabilities.clear();
                                recover(&mut state,&mut next_start,error,&recovery);
                            } else if started.elapsed()>=recovery.stable_after {state.consecutive_failures=0;}
                        }
                        if active.is_none() && next_start.is_some_and(|time|time<=Instant::now()) && state.desired==DesiredState::Running {
                            next_start=None;state.observed=ObservedState::Starting;status_tx.send_replace(state.clone());
                            // Bounded handshake; Stop is processed after the attempt.
                            match Bridge::connect(&command,bridge_policy.clone()).await {
                                Ok(current)=>{
                                    state.generation=state.generation.saturating_add(1);state.runtime_version=Some(current.hello.runtime_product_version.clone());state.runtime_capabilities=current.hello.capabilities.clone();state.observed=ObservedState::Healthy;
                                    let mut incoming=current.subscribe();
                                    let observations=observations.clone();let generation=state.generation;
                                    forwarder.clear();
                                    forwarder.0=Some(tokio::spawn(async move {
                                        loop {
                                            let event=match incoming.recv().await {
                                                Ok(event)=>HostRuntimeEvent {generation,runtime_sequence:Some(event.sequence),event:event.event,payload:event.payload},
                                                Err(broadcast::error::RecvError::Lagged(_))=>HostRuntimeEvent {generation,runtime_sequence:None,event:"events.resync_required".into(),payload:json!({"reason":"host_bridge_event_lag"})},
                                                Err(broadcast::error::RecvError::Closed)=>break,
                                            };
                                            let _=observations.send(event);
                                        }
                                    }));
                                    *shared.write().await=Some(current.clone());active=Some(current);started=Instant::now();
                                }
                                Err(error)=>recover(&mut state,&mut next_start,error,&recovery),
                            }
                        }
                    }
                }
            }
        });
        Ok(Self {
            #[cfg(windows)]
            remote,
            resources: Arc::new(crate::resources::HostResources::new(
                directory.to_path_buf(),
            )),
            #[cfg(windows)]
            autostart: None,
            control,
            status,
            bridge,
            events,
        })
    }
    pub fn status(&self) -> HostStatus {
        self.status.borrow().clone()
    }
    pub async fn resources(&self) -> Value {
        self.resources.snapshot(self).await
    }
    pub fn subscribe(&self) -> watch::Receiver<HostStatus> {
        self.status.clone()
    }
    pub fn subscribe_events(&self) -> broadcast::Receiver<HostRuntimeEvent> {
        self.events.subscribe()
    }
    pub async fn set_desired(&self, desired: DesiredState) -> Result<(), BridgeError> {
        let (reply, receive) = oneshot::channel();
        self.control
            .try_send(Control::Set(desired, reply))
            .map_err(|_| {
                BridgeError::new("CK-HOST-BUSY", "Host control queue is unavailable or full")
            })?;
        receive
            .await
            .map_err(|_| BridgeError::new("CK-HOST-CLOSED", "Host exited"))?
    }
    pub async fn call(
        &self,
        method: impl Into<String>,
        params: Value,
    ) -> Result<ResponseEnvelope, BridgeError> {
        if self.status().observed == ObservedState::Stopping {
            return Err(BridgeError::new(
                "CK-HOST-STOPPING",
                "Runtime stop is in progress",
            ));
        }
        let current = self.bridge.read().await.clone().ok_or_else(|| {
            BridgeError::new(
                "CK-HOST-NOT-RUNNING",
                "Runtime is not healthy; inspect Host status",
            )
        })?;
        current.call(method, params).await
    }
    pub async fn shutdown(&self) -> Result<(), BridgeError> {
        self.resources.shutdown().await?;
        let (reply, receive) = oneshot::channel();
        self.control
            .send(Control::Exit(reply))
            .await
            .map_err(|_| BridgeError::new("CK-HOST-CLOSED", "Host already exited"))?;
        receive
            .await
            .map_err(|_| BridgeError::new("CK-HOST-CLOSED", "Host shutdown acknowledgement lost"))?
    }
}
fn recover(
    state: &mut HostStatus,
    next: &mut Option<Instant>,
    error: BridgeError,
    policy: &RecoveryPolicy,
) {
    state.consecutive_failures = state.consecutive_failures.saturating_add(1);
    let retry = error.code != "CK-HOST-VERSION" && state.desired == DesiredState::Running;
    *next = None;
    state.last_error = Some(error);
    state.observed = ObservedState::Error;
    if let Some(delay) = policy
        .delays
        .get(state.consecutive_failures.saturating_sub(1) as usize)
        .filter(|_| retry)
    {
        *next = Some(Instant::now() + *delay);
        state.observed = ObservedState::Degraded;
    }
}
