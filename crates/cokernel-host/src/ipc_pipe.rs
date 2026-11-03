use crate::{ipc, pipe_security, supervisor::Host};
use cokernel_protocol::{
    DEFAULT_MAX_FRAME_BYTES, EventEnvelope, RequestEnvelope, ResponseEnvelope, encode_json_frame,
};
use serde_json::{Value, json};
use std::{
    collections::{HashSet, VecDeque},
    io,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer},
    sync::{OwnedSemaphorePermit, Semaphore, broadcast, mpsc, watch},
    task::JoinHandle,
    time::timeout,
};

const CLIENTS: usize = 16;
const REQUESTS: usize = 32;
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
pub fn default_name() -> io::Result<String> {
    Ok(format!(
        r"\\.\pipe\cokernel-v1-{}",
        pipe_security::current_sid()?
    ))
}
fn valid_name(name: &str) -> bool {
    name.starts_with(r"\\.\pipe\cokernel-v1-")
        && name.len() < 240
        && !name.chars().any(char::is_control)
}

pub struct PipeListener {
    name: String,
    sid: String,
    first: NamedPipeServer,
}
pub struct PipeServer {
    stop: watch::Sender<bool>,
    task: JoinHandle<io::Result<()>>,
}
impl PipeListener {
    pub fn bind() -> io::Result<Self> {
        Self::bind_at(&default_name()?)
    }
    /// Alternate names are for isolated integration tests, not client RPC input.
    pub fn bind_at(name: &str) -> io::Result<Self> {
        if !valid_name(name) {
            return Err(io::Error::other("invalid local CoKernel pipe name"));
        }
        let sid = pipe_security::current_sid()?;
        Ok(Self {
            name: name.into(),
            first: pipe_security::create(name, &sid, true)?,
            sid,
        })
    }
    pub fn serve(self, host: Host) -> PipeServer {
        let (stop, mut stopping) = watch::channel(false);
        let task = tokio::spawn(async move {
            let clients = Arc::new(Semaphore::new(CLIENTS));
            let requests = Arc::new(Semaphore::new(REQUESTS));
            let mut listener = self.first;
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    changed=stopping.changed()=>{if changed.is_err() || *stopping.borrow(){break;}}
                    result=listener.connect()=>{
                        result?;
                        // Keep at least one server instance alive while rotating;
                        // another process must never claim an unowned pipe name.
                        let next=pipe_security::create(&self.name,&self.sid,false)?;
                        let connected=std::mem::replace(&mut listener,next);
                        if let Ok(permit)=clients.clone().try_acquire_owned() {
                            let host=host.clone();let sid=self.sid.clone();let requests=requests.clone();
                            connections.spawn(async move {let _permit=permit;let _=connection(connected,host,sid,requests).await;});
                        }
                    }
                    _=connections.join_next(),if !connections.is_empty()=>{}
                }
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
            Ok(())
        });
        PipeServer { stop, task }
    }
}
impl PipeServer {
    pub async fn shutdown(self) -> io::Result<()> {
        self.stop.send_replace(true);
        self.task.await.map_err(io::Error::other)?
    }
}

struct Outgoing {
    value: Value,
    _request: Option<OwnedSemaphorePermit>,
}
async fn enqueue(
    tx: &mpsc::Sender<Outgoing>,
    value: Value,
    permit: Option<OwnedSemaphorePermit>,
) -> io::Result<()> {
    timeout(
        WRITE_TIMEOUT,
        tx.send(Outgoing {
            value,
            _request: permit,
        }),
    )
    .await
    .map_err(|_| io::Error::other("IPC client is not draining its bounded output queue"))?
    .map_err(|_| io::Error::other("IPC output is closed"))
}
async fn connection(
    mut pipe: NamedPipeServer,
    host: Host,
    sid: String,
    requests: Arc<Semaphore>,
) -> io::Result<()> {
    let hello: RequestEnvelope = timeout(Duration::from_secs(5), read_json(&mut pipe))
        .await
        .map_err(io::Error::other)??;
    pipe_security::authorize(&pipe, &sid)?;
    if hello.method != "host.hello" {
        return Err(io::Error::other("Host handshake required"));
    }
    let response = ipc::dispatch(&host, hello.clone()).await;
    write_json(&mut pipe, &response).await?;
    if !response.ok {
        return Ok(());
    }
    let mut statuses = host.subscribe();
    let mut runtime_events = host.subscribe_events();
    let mut remote_status = host.remote_access().subscribe();
    let initial = host.status();
    let runtime = if initial.observed == crate::supervisor::ObservedState::Healthy {
        match host.call("runtime.get_snapshot", json!({})).await {
            Ok(response) if response.ok && host.status().generation == initial.generation => {
                response.result
            }
            _ => None,
        }
    } else {
        None
    };
    let snapshot = EventEnvelope {
        protocol: 1,
        event: "host.snapshot".into(),
        sequence: 0,
        payload: json!({"host":host.status(),"runtime":runtime,"runtime_snapshot_required":runtime.is_none(),"remote_access":remote_status.borrow_and_update().clone()}),
    };
    write_json(&mut pipe, &snapshot).await?;
    let (mut reader, mut writer) = tokio::io::split(pipe);
    let (incoming, mut input) = mpsc::channel(1);
    let reader_task = tokio::spawn(async move {
        loop {
            let frame = read_json::<_, RequestEnvelope>(&mut reader).await;
            let failed = frame.is_err();
            if incoming.send(frame).await.is_err() || failed {
                break;
            }
        }
    });
    let (outgoing, mut output) = mpsc::channel::<Outgoing>(8);
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = output.recv().await {
            write_json(&mut writer, &frame.value).await?;
        }
        Ok::<_, io::Error>(())
    });
    // Abort I/O helpers even on early return; submitted domain work has its own owner.
    struct IoTasks(JoinHandle<()>, JoinHandle<io::Result<()>>);
    impl Drop for IoTasks {
        fn drop(&mut self) {
            self.0.abort();
            self.1.abort();
        }
    }
    let io_tasks = IoTasks(reader_task, writer_task);
    let (completed, mut responses) = mpsc::channel::<(ResponseEnvelope, OwnedSemaphorePermit)>(8);
    let mut seen = HashSet::from([hello.request_id]);
    let mut pending = 0usize;
    let mut sequence = 0u64;
    loop {
        tokio::select! {
            received=input.recv()=>{
                let request=match received {Some(Ok(request))=>request,_=>break};
                if let Err(error)=ipc::validate(&request){enqueue(&outgoing,json!(error),None).await?;continue;}
                if seen.len()>=8192 || !seen.insert(request.request_id.clone()) {
                    return Err(io::Error::other("duplicate request ID or connection request limit"));
                }
                let permit=if pending<8 {requests.clone().try_acquire_owned().ok()}else{None};
                let Some(permit)=permit else {enqueue(&outgoing,json!(ipc::error(request.request_id,"CK-HOST-BUSY","IPC request limit reached; wait for accepted requests")),None).await?;continue;};
                pending+=1;let host=host.clone();let completed=completed.clone();
                // Deliberately independent of connection task lifetime. A dropped
                // view cannot cancel an accepted save/execute/control operation.
                tokio::spawn(async move {let response=ipc::dispatch(&host,request).await;let _=completed.send((response,permit)).await;});
            }
            received=responses.recv(),if pending>0=>{
                let Some((response,permit))=received else {break;};pending-=1;
                enqueue(&outgoing,json!(response),Some(permit)).await?;
            }
            changed=statuses.changed()=>{
                if changed.is_err(){break;}
                sequence=sequence.checked_add(1).ok_or_else(||io::Error::other("event sequence exhausted"))?;
                enqueue(&outgoing,json!(EventEnvelope {protocol:1,event:"runtime.state_changed".into(),sequence,payload:json!({"host":statuses.borrow().clone()})}),None).await?;
            }
            received=runtime_events.recv()=>{
                let (event,payload)=match received {
                    Ok(event)=>(event.event,json!({"generation":event.generation,"runtime_sequence":event.runtime_sequence,"data":event.payload})),
                    Err(broadcast::error::RecvError::Lagged(_))=>("events.resync_required".into(),json!({"generation":host.status().generation,"reason":"host_client_event_lag"})),
                    Err(broadcast::error::RecvError::Closed)=>break,
                };
                sequence=sequence.checked_add(1).ok_or_else(||io::Error::other("event sequence exhausted"))?;
                enqueue(&outgoing,json!(EventEnvelope {protocol:1,event,sequence,payload}),None).await?;
            }
            changed=remote_status.changed()=>{
                if changed.is_err(){break;}
                sequence=sequence.checked_add(1).ok_or_else(||io::Error::other("event sequence exhausted"))?;
                let status=remote_status.borrow().clone();
                enqueue(&outgoing,json!(EventEnvelope{protocol:1,event:"remote_access.state_changed".into(),sequence,payload:json!(status)}),None).await?;
            }
            _=tokio::time::sleep(Duration::from_secs(1))=>{if io_tasks.1.is_finished(){break;}}
        }
    }
    drop(io_tasks);
    Ok(())
}

pub async fn read_json<R: AsyncRead + Unpin, T: serde::de::DeserializeOwned>(
    reader: &mut R,
) -> io::Result<T> {
    let length = reader.read_u32().await? as usize;
    if length == 0 || length > DEFAULT_MAX_FRAME_BYTES {
        return Err(io::Error::other("IPC frame exceeds limit"));
    }
    let mut bytes = zeroize::Zeroizing::new(vec![0; length]);
    timeout(Duration::from_secs(10), reader.read_exact(&mut bytes))
        .await
        .map_err(io::Error::other)??;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}
pub async fn write_json<W: AsyncWrite + Unpin, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> io::Result<()> {
    let bytes = zeroize::Zeroizing::new(
        encode_json_frame(value, DEFAULT_MAX_FRAME_BYTES).map_err(io::Error::other)?,
    );
    timeout(WRITE_TIMEOUT, async {
        writer.write_all(&bytes).await?;
        writer.flush().await
    })
    .await
    .map_err(io::Error::other)?
}

/// Sequential RPC helper for local clients and acceptance tests. Stream consumers
/// can use next_event; dropped/overflowed local event history requires a snapshot.
pub struct PipeClient {
    pipe: NamedPipeClient,
    events: VecDeque<EventEnvelope>,
    poisoned: bool,
}
impl PipeClient {
    pub async fn connect(name: &str) -> io::Result<Self> {
        if !valid_name(name) {
            return Err(io::Error::other("invalid local CoKernel pipe name"));
        }
        let pipe = timeout(Duration::from_secs(5), async {
            loop {
                match ClientOptions::new().open(name) {
                    Ok(pipe) => return Ok(pipe),
                    Err(error) if matches!(error.raw_os_error(), Some(2 | 231)) => {
                        tokio::time::sleep(Duration::from_millis(20)).await
                    }
                    Err(error) => return Err(error),
                }
            }
        })
        .await
        .map_err(io::Error::other)??;
        pipe_security::authorize_server(&pipe)?;
        let mut client = Self {
            pipe,
            events: VecDeque::new(),
            poisoned: false,
        };
        let response = client
            .call("host.hello", json!({"supported_protocols":[1]}))
            .await?;
        if !response.ok
            || response
                .result
                .as_ref()
                .is_none_or(|v| v["selected_protocol"] != 1)
        {
            return Err(io::Error::other("Host protocol negotiation failed"));
        }
        Ok(client)
    }
    pub async fn call(&mut self, method: &str, params: Value) -> io::Result<ResponseEnvelope> {
        self.call_params(method, params).await
    }
    /// Borrow secret material only while encoding the private request; do not
    /// create a generic JSON value that callers might log or retain.
    pub async fn configure_remote(
        &mut self,
        credentials: &crate::secrets::TunnelCredentials,
    ) -> io::Result<crate::secrets::Configuration> {
        #[derive(serde::Serialize)]
        struct Input<'a> {
            tunnel_id: &'a str,
            runtime_key: &'a str,
        }
        let invalid = || {
            io::Error::other(
                "CK-REMOTE-SETUP: configuration was not confirmed; inspect configuration before retrying",
            )
        };
        let params = Input {
            tunnel_id: credentials.tunnel_id(),
            runtime_key: std::str::from_utf8(credentials.key_for_tunnel())
                .map_err(|_| invalid())?,
        };
        let response = self
            .call_params("remote_access.configure", params)
            .await
            .map_err(|_| invalid())?;
        configuration_receipt(response, credentials.tunnel_id())
    }
    async fn call_params<T: serde::Serialize>(
        &mut self,
        method: &str,
        params: T,
    ) -> io::Result<ResponseEnvelope> {
        if self.poisoned {
            return Err(io::Error::other(
                "IPC call was cancelled or failed; reconnect before further use",
            ));
        }
        self.poisoned = true;
        let id = uuid::Uuid::new_v4().to_string();
        #[derive(serde::Serialize)]
        struct Request<'a, T> {
            protocol: u32,
            request_id: &'a str,
            method: &'a str,
            params: T,
        }
        let request = Request {
            protocol: 1,
            request_id: &id,
            method,
            params,
        };
        write_json(&mut self.pipe, &request).await?;
        timeout(Duration::from_secs(330), async {
            loop {
                let value: Value = read_json(&mut self.pipe).await?;
                if value.get("event").is_some() {
                    let event: EventEnvelope =
                        serde_json::from_value(value).map_err(io::Error::other)?;
                    if self.events.len() >= 256 {
                        self.events.clear();
                        self.events.push_back(EventEnvelope {
                            protocol: 1,
                            event: "events.resync_required".into(),
                            sequence: event.sequence,
                            payload: json!({"reason":"local_event_buffer"}),
                        });
                    } else {
                        self.events.push_back(event);
                    }
                    continue;
                }
                let response: ResponseEnvelope =
                    serde_json::from_value(value).map_err(io::Error::other)?;
                if response.protocol != 1 || response.request_id != id {
                    return Err(io::Error::other("unexpected IPC response"));
                }
                self.poisoned = false;
                return Ok(response);
            }
        })
        .await
        .map_err(io::Error::other)?
    }
    pub async fn next_event(&mut self) -> io::Result<EventEnvelope> {
        if self.poisoned {
            return Err(io::Error::other(
                "IPC read was cancelled or failed; reconnect before further use",
            ));
        }
        if let Some(event) = self.events.pop_front() {
            return Ok(event);
        }
        self.poisoned = true;
        let result = read_json(&mut self.pipe).await;
        if result.is_ok() {
            self.poisoned = false;
        }
        result
    }
}

fn configuration_receipt(
    response: ResponseEnvelope,
    expected_id: &str,
) -> io::Result<crate::secrets::Configuration> {
    let invalid = || {
        io::Error::other(
            "CK-REMOTE-SETUP: configuration was not confirmed; inspect configuration before retrying",
        )
    };
    if !response.ok {
        return Err(invalid());
    }
    let configuration: crate::secrets::Configuration =
        serde_json::from_value(response.result.ok_or_else(invalid)?).map_err(|_| invalid())?;
    if !configuration.configured || configuration.tunnel_id.as_deref() != Some(expected_id) {
        return Err(invalid());
    }
    Ok(configuration)
}

#[cfg(test)]
mod setup_tests {
    use super::*;
    #[test]
    fn unconfirmed_configuration_never_echoes_private_response_fields() {
        let sentinel = "synthetic-private-response-never-print";
        for value in [
            json!({"configured":true,"tunnel_id":"tunnel_test","runtime_key":sentinel}),
            json!({"configured":true,"tunnel_id":sentinel}),
            json!({"configured":false,"tunnel_id":"tunnel_test"}),
            json!({"configured":sentinel,"tunnel_id":"tunnel_test"}),
            json!(null),
        ] {
            let response = crate::ipc::success("test".into(), value);
            let error = configuration_receipt(response, "tunnel_test").unwrap_err();
            assert!(!error.to_string().contains(sentinel));
        }
        let response = crate::ipc::error("test".into(), sentinel, sentinel);
        assert!(
            !configuration_receipt(response, "tunnel_test")
                .unwrap_err()
                .to_string()
                .contains(sentinel)
        );
    }
}
