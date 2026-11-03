//! A single owned, bounded, correlated Runtime connection. Never retries RPCs.
use cokernel_protocol::{
    DEFAULT_MAX_FRAME_BYTES, EventEnvelope, RequestEnvelope, ResponseEnvelope,
    methods::BridgeHelloResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{broadcast, mpsc, oneshot, watch},
    time::{Instant, timeout},
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[error("{code}: {message}")]
pub struct BridgeError {
    pub code: &'static str,
    pub message: String,
}
impl BridgeError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    fn io(_: impl std::fmt::Display) -> Self {
        Self::new(
            "CK-HOST-BRIDGE",
            "Runtime transport failed; pending operations may have executed. Refresh state before retrying.",
        )
    }
}
type Result<T> = std::result::Result<T, BridgeError>;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WslConfig {
    pub distribution: String,
    pub linux_user: String,
    pub runtime_path: String,
}
impl WslConfig {
    pub fn command(&self) -> Result<BridgeCommand> {
        let safe_name = |s: &str| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_ .".contains(&c))
                && !s.starts_with('-')
        };
        if !safe_name(&self.distribution)
            || !safe_name(&self.linux_user)
            || !self.runtime_path.starts_with('/')
            || self.runtime_path.len() > 4096
            || self.runtime_path.chars().any(char::is_control)
        {
            return Err(BridgeError::new(
                "CK-HOST-CONFIG",
                "Invalid managed WSL launch configuration",
            ));
        }
        let system = std::env::var_os("SystemRoot")
            .ok_or_else(|| BridgeError::new("CK-HOST-CONFIG", "SystemRoot is unavailable"))?;
        Ok(BridgeCommand {
            program: PathBuf::from(system).join("System32/wsl.exe"),
            args: vec![
                "--distribution".into(),
                self.distribution.clone(),
                "--user".into(),
                self.linux_user.clone(),
                "--exec".into(),
                self.runtime_path.clone(),
                "bridge".into(),
                "--stdio".into(),
            ],
        })
    }
}

/// Trusted launch configuration, never deserialized from a client RPC. Tests
/// can substitute a fake process without changing the production WSL path.
#[derive(Debug, Clone)]
pub struct BridgeCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}
#[derive(Debug, Clone)]
pub struct BridgePolicy {
    pub handshake_timeout: Duration,
    pub request_timeout: Duration,
    pub shutdown_timeout: Duration,
}
impl Default for BridgePolicy {
    fn default() -> Self {
        Self {
            handshake_timeout: Duration::from_secs(20),
            request_timeout: Duration::from_secs(300),
            shutdown_timeout: Duration::from_secs(15),
        }
    }
}

struct Call {
    method: String,
    params: Value,
    reply: oneshot::Sender<Result<ResponseEnvelope>>,
}
#[derive(Clone)]
pub struct Bridge {
    calls: mpsc::Sender<Call>,
    _owner: watch::Sender<bool>,
    terminal: watch::Receiver<Option<BridgeError>>,
    events: broadcast::Sender<EventEnvelope>,
    pub hello: BridgeHelloResult,
}
impl Bridge {
    pub async fn connect(spec: &BridgeCommand, policy: BridgePolicy) -> Result<Self> {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // No WSLENV, tokens or arbitrary Host environment reach the launch.
        for name in [
            "SystemRoot",
            "WINDIR",
            "USERPROFILE",
            "LOCALAPPDATA",
            "TEMP",
            "TMP",
            "HOME",
            "PATH",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let mut child = command.spawn().map_err(BridgeError::io)?;
        let mut input = child.stdin.take().ok_or_else(|| BridgeError::io("stdin"))?;
        let mut output = child
            .stdout
            .take()
            .ok_or_else(|| BridgeError::io("stdout"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| BridgeError::io("stderr"))?;
        // Drain without retaining/echoing potentially secret-bearing subprocess text.
        let stderr_task = tokio::spawn(async move {
            let mut bytes = [0u8; 4096];
            while matches!(stderr.read(&mut bytes).await,Ok(n) if n>0) {}
        });
        let id = Uuid::new_v4().to_string();
        let request = RequestEnvelope {
            protocol: 1,
            request_id: id.clone(),
            method: "bridge.hello".into(),
            params: json!({"host_product_version":env!("CARGO_PKG_VERSION"),"supported_protocols":[1]}),
        };
        let mut last_sequence = None;
        let handshake = timeout(policy.handshake_timeout, async {
            let bytes = cokernel_protocol::encode_json_frame(&request, DEFAULT_MAX_FRAME_BYTES)
                .map_err(BridgeError::io)?;
            input.write_all(&bytes).await.map_err(BridgeError::io)?;
            input.flush().await.map_err(BridgeError::io)?;
            let mut startup_events = 0;
            let response: ResponseEnvelope = loop {
                let value = read_value(&mut output).await?;
                if value.get("event").is_some() && value.get("request_id").is_none() {
                    startup_events += 1;
                    if startup_events > 32 {
                        return Err(BridgeError::new(
                            "CK-HOST-LIMIT",
                            "Too many Runtime events before handshake",
                        ));
                    }
                    decode_event(value, &mut last_sequence)?;
                    // No subscribers exist yet. The initial Runtime snapshot is
                    // authoritative; retain the sequence watermark, not payloads.
                    continue;
                }
                break serde_json::from_value(value).map_err(BridgeError::io)?;
            };
            validate_response(&response).map_err(|_| {
                BridgeError::new(
                    "CK-HOST-VERSION",
                    "Malformed or unsupported Runtime handshake envelope",
                )
            })?;
            if response.request_id != id || !response.ok {
                return Err(BridgeError::new(
                    "CK-HOST-VERSION",
                    "Runtime handshake rejected; update/repair Host and Runtime together",
                ));
            }
            let hello: BridgeHelloResult =
                serde_json::from_value(response.result.unwrap_or(Value::Null)).map_err(|_| {
                    BridgeError::new("CK-HOST-VERSION", "Malformed Runtime handshake")
                })?;
            if hello.selected_protocol != 1
                || hello.runtime_product_version.is_empty()
                || hello.runtime_product_version.len() > 128
                || hello.capabilities.len() > 128
                || hello.capabilities.iter().any(|s| s.len() > 128)
            {
                return Err(BridgeError::new(
                    "CK-HOST-VERSION",
                    "Unsupported or malformed Runtime handshake",
                ));
            }
            Ok(hello)
        })
        .await
        .unwrap_or_else(|_| {
            Err(BridgeError::new(
                "CK-HOST-HANDSHAKE-TIMEOUT",
                "Runtime handshake timed out",
            ))
        });
        let hello = match handshake {
            Ok(h) => h,
            Err(error) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                stderr_task.abort();
                return Err(error);
            }
        };
        let (calls, mut rx) = mpsc::channel::<Call>(32);
        let (stop, mut stopping) = watch::channel(false);
        let (done, terminal) = watch::channel(None);
        let (events, _) = broadcast::channel(256);
        let fanout = events.clone();
        let (incoming, mut frames) = mpsc::channel(8);
        let reader = tokio::spawn(async move {
            loop {
                let value = read_value(&mut output).await;
                let failed = value.is_err();
                if incoming.send(value).await.is_err() || failed {
                    break;
                }
            }
        });
        tokio::spawn(async move {
            let mut pending: HashMap<
                String,
                (Instant, oneshot::Sender<Result<ResponseEnvelope>>, bool),
            > = HashMap::new();
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            let mut graceful = false;
            let mut shutdown_sent = false;
            let mut exited_at = None;
            let error = loop {
                tokio::select! {
                    changed=stopping.changed()=>{ if changed.is_err() || *stopping.borrow() { graceful=true; break BridgeError::new("CK-HOST-STOPPED","Runtime connection stopped"); } }
                    frame=frames.recv()=>{
                        let value=match frame { Some(Ok(value))=>value, _=>break BridgeError::io("read") };
                        if value.get("event").is_some() && value.get("request_id").is_none() {
                            let event=match decode_event(value,&mut last_sequence) {Ok(event)=>event,Err(error)=>break error};
                            let _=fanout.send(event);
                        } else {
                            let response=match serde_json::from_value::<ResponseEnvelope>(value) {Ok(response)=>response,Err(_)=>break BridgeError::io("response")};
                            if validate_response(&response).is_err() {break BridgeError::io("response shape");}
                            let Some((_,reply,shutdown))=pending.remove(&response.request_id) else {break BridgeError::io("unexpected response ID");};
                            let accepted=shutdown && response.ok;
                            let _=reply.send(Ok(response));
                            if accepted {graceful=true;break BridgeError::new("CK-HOST-STOPPED","Runtime confirmed shutdown");}
                        }
                    }
                    call=rx.recv(), if pending.len()<32=>{
                        let Some(call)=call else {graceful=true; break BridgeError::new("CK-HOST-STOPPED","Host released the connection");};
                        let id=Uuid::new_v4().to_string();
                        let mut request=RequestEnvelope {protocol:1,request_id:id.clone(),method:call.method,params:call.params};
                        let encoded=cokernel_protocol::encode_json_frame(&request,DEFAULT_MAX_FRAME_BYTES);
                        if request.method=="tunnel.start" {
                            use zeroize::Zeroize;
                            if let Some(Value::String(key))=request.params.get_mut("runtime_key") {key.zeroize();}
                        }
                        let bytes=match encoded {Ok(bytes)=>zeroize::Zeroizing::new(bytes),Err(_)=>{let _=call.reply.send(Err(BridgeError::new("CK-HOST-LIMIT","Request exceeds frame limit")));continue;}};
                        let shutdown=request.method=="bridge.shutdown";
                        shutdown_sent|=shutdown;
                        let deadline=if shutdown {policy.shutdown_timeout} else {policy.request_timeout};
                        pending.insert(id,(Instant::now()+deadline,call.reply,shutdown));
                        let write=async { input.write_all(&bytes).await?; input.flush().await };
                        if !matches!(timeout(Duration::from_secs(10),write).await,Ok(Ok(()))) {break BridgeError::io("write");}
                    }
                    _=tick.tick()=>{
                        if pending.values().any(|(deadline,_,_)|*deadline<=Instant::now()) { break BridgeError::new("CK-HOST-TIMEOUT","Runtime request timed out; it may have executed. The connection is being closed. No request is replayed."); }
                        // A process may exit just after writing its final reply.
                        // Give the reader a bounded opportunity to drain it; an
                        // inherited stdout handle must not conceal exit forever.
                        match child.try_wait() {Ok(None)=>{},Ok(Some(_))=>{exited_at.get_or_insert_with(Instant::now);},Err(_)=>break BridgeError::io("exit")}
                        if exited_at.is_some_and(|time|time.elapsed()>=Duration::from_secs(1)) {break BridgeError::io("exit without stream close");}
                    }
                }
            };
            rx.close();
            for (_, (_, reply, _)) in pending {
                let _ = reply.send(Err(error.clone()));
            }
            while let Ok(call) = rx.try_recv() {
                let _ = call.reply.send(Err(error.clone()));
            }
            if graceful && !shutdown_sent {
                let shutdown = RequestEnvelope {
                    protocol: 1,
                    request_id: Uuid::new_v4().to_string(),
                    method: "bridge.shutdown".into(),
                    params: json!({}),
                };
                if let Ok(bytes) =
                    cokernel_protocol::encode_json_frame(&shutdown, DEFAULT_MAX_FRAME_BYTES)
                {
                    let _ = timeout(Duration::from_secs(2), input.write_all(&bytes)).await;
                }
            }
            drop(input); // EOF also requests Runtime Session cleanup.
            // Continue draining replies so a busy Runtime cannot block on stdout.
            let drain = tokio::spawn(async move { while frames.recv().await.is_some() {} });
            let exit = timeout(policy.shutdown_timeout, child.wait()).await;
            let clean_exit = matches!(&exit,Ok(Ok(status)) if status.success());
            if !matches!(exit, Ok(Ok(_))) {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
            let error = if graceful && !clean_exit {
                BridgeError::new(
                    "CK-HOST-SHUTDOWN",
                    "Runtime did not confirm clean shutdown; inspect save/Session state before restarting",
                )
            } else {
                error
            };
            reader.abort();
            drain.abort();
            stderr_task.abort();
            done.send_replace(Some(error));
        });
        Ok(Self {
            calls,
            _owner: stop,
            terminal,
            events,
            hello,
        })
    }
    pub async fn call(&self, method: impl Into<String>, params: Value) -> Result<ResponseEnvelope> {
        let method = method.into();
        if method.is_empty()
            || method.len() > 128
            || matches!(method.as_str(), "bridge.shutdown" | "bridge.hello")
        {
            return Err(BridgeError::new(
                "CK-HOST-METHOD",
                "Reserved or invalid Runtime method",
            ));
        }
        self.request(method, params).await
    }
    async fn request(&self, method: String, params: Value) -> Result<ResponseEnvelope> {
        let (reply, receive) = oneshot::channel();
        self.calls
            .try_send(Call {
                method,
                params,
                reply,
            })
            .map_err(|_| {
                BridgeError::new(
                    "CK-HOST-BUSY",
                    "Runtime connection is unavailable or its request queue is full",
                )
            })?;
        receive.await.map_err(BridgeError::io)?
    }
    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.events.subscribe()
    }
    pub fn terminal(&self) -> watch::Receiver<Option<BridgeError>> {
        self.terminal.clone()
    }
    pub async fn stop(&self) -> Result<()> {
        if let Some(error) = self.terminal.borrow().clone() {
            return if error.code == "CK-HOST-STOPPED" {
                Ok(())
            } else {
                Err(error)
            };
        }
        let response = self.request("bridge.shutdown".into(), json!({})).await;
        if matches!(&response,Ok(response) if !response.ok) {
            return Err(BridgeError::new(
                "CK-HOST-SHUTDOWN-REFUSED",
                "Runtime could not save/stop safely. The connection and unsaved documents remain available; resolve conflicts and retry Stop.",
            ));
        }
        let mut done = self.terminal.clone();
        while done.borrow().is_none() {
            if done.changed().await.is_err() {
                break;
            }
        }
        if response.is_err() {
            return Err(BridgeError::new(
                "CK-HOST-SHUTDOWN",
                "Runtime shutdown transport failed; inspect document and Session state before restarting",
            ));
        }
        match done.borrow().clone() {
            Some(error) if error.code == "CK-HOST-STOPPED" => Ok(()),
            Some(error) => Err(error),
            None => Err(BridgeError::io("bridge owner exited")),
        }
    }
}
fn decode_event(value: Value, last_sequence: &mut Option<u64>) -> Result<EventEnvelope> {
    let event: EventEnvelope = serde_json::from_value(value).map_err(BridgeError::io)?;
    if event.protocol != 1
        || event.event.is_empty()
        || event.event.len() > 128
        || last_sequence.is_some_and(|last| event.sequence <= last)
    {
        return Err(BridgeError::io("event order"));
    }
    *last_sequence = Some(event.sequence);
    Ok(event)
}

fn validate_response(response: &ResponseEnvelope) -> Result<()> {
    if response.protocol != 1
        || response.request_id.len() > 128
        || response.ok != (response.error.is_none())
        || response.ok != (response.result.is_some())
    {
        return Err(BridgeError::io("response"));
    }
    Ok(())
}
async fn read_value(reader: &mut (impl AsyncRead + Unpin)) -> Result<Value> {
    let length = reader.read_u32().await.map_err(BridgeError::io)? as usize;
    if length == 0 || length > DEFAULT_MAX_FRAME_BYTES {
        return Err(BridgeError::new(
            "CK-HOST-FRAME",
            "Invalid Runtime frame length",
        ));
    }
    let mut payload = vec![0; length];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(BridgeError::io)?;
    serde_json::from_slice(&payload).map_err(BridgeError::io)
}
