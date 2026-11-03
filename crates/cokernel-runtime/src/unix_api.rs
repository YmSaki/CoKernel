//! Trusted control-identity API for MCP. Every request uses the existing RuntimeHandle.
use crate::endpoint::RuntimeHandle;
use cokernel_domain::{DomainError, ExecutionOrigin};
use cokernel_protocol::{RequestEnvelope, ResponseEnvelope, encode_json_frame};
use std::{
    io,
    os::{
        fd::AsRawFd,
        unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{Semaphore, watch},
    task::JoinHandle,
};

const REQUEST_LIMIT: usize = 1024 * 1024;
const RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
pub struct UnixApi {
    stop: watch::Sender<bool>,
    task: JoinHandle<io::Result<()>>,
}
struct SocketGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
    _lock: SocketLock,
}
struct SocketLock(std::fs::File);
impl Drop for SocketLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
async fn claim_socket(path: &Path) -> io::Result<SocketLock> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path.with_extension("lock"))?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
        || unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0
    {
        return Err(io::Error::other(
            "Runtime socket ownership is unavailable or unsafe",
        ));
    }
    let lock = SocketLock(file);
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
        Ok(meta) => {
            if !meta.file_type().is_socket() || meta.uid() != unsafe { libc::geteuid() } {
                return Err(io::Error::other(
                    "Runtime socket path contains an unrelated file",
                ));
            }
            match tokio::time::timeout(Duration::from_secs(1), UnixStream::connect(path)).await {
                Ok(Err(e)) if e.kind() == io::ErrorKind::ConnectionRefused => {}
                _ => {
                    return Err(io::Error::other(
                        "Runtime socket may still have a live listener",
                    ));
                }
            }
            let now = std::fs::symlink_metadata(path)?;
            if now.dev() != meta.dev() || now.ino() != meta.ino() {
                return Err(io::Error::other(
                    "Runtime socket changed during ownership check",
                ));
            }
            std::fs::remove_file(path)?;
        }
    }
    Ok(lock)
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.dev() == self.device && m.ino() == self.inode)
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
pub(crate) fn check_directory(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Runtime socket needs a private parent directory"))?;
    if !path.is_absolute() || parent.canonicalize()? != parent {
        return Err(io::Error::other(
            "Runtime socket requires an absolute non-symlink directory",
        ));
    }
    let meta = std::fs::symlink_metadata(parent)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        return Err(io::Error::other(
            "Runtime socket directory must be private to the control identity",
        ));
    }
    Ok(())
}
impl UnixApi {
    /// The installer creates the private directory; a locked, refused old socket may be recovered.
    pub async fn bind(path: &Path, runtime: RuntimeHandle) -> io::Result<Self> {
        check_directory(path)?;
        let lock = claim_socket(path).await?;
        let listener = UnixListener::bind(path)?;
        let meta = std::fs::symlink_metadata(path)?;
        let guard = SocketGuard {
            path: path.to_owned(),
            device: meta.dev(),
            inode: meta.ino(),
            _lock: lock,
        };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        let (stop, mut stopping) = watch::channel(false);
        let task = tokio::spawn(async move {
            let _guard = guard;
            let capacity = Arc::new(Semaphore::new(8));
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _=stopping.changed()=>break,
                    _=clients.join_next(),if !clients.is_empty()=>{},
                    accepted=listener.accept()=>{
                        let (stream,_)=accepted?;
                        if !stream.peer_cred().is_ok_and(|peer|peer.uid()==unsafe {libc::geteuid()}) {continue;}
                        let Ok(permit)=capacity.clone().try_acquire_owned() else {continue;};
                        let runtime=runtime.clone();
                        clients.spawn(async move {let _permit=permit; let _=connection(stream,runtime).await;});
                    }
                }
            }
            clients.abort_all();
            while clients.join_next().await.is_some() {}
            Ok(())
        });
        Ok(Self { stop, task })
    }
    pub async fn shutdown(self) -> io::Result<()> {
        self.stop.send_replace(true);
        self.task.await.map_err(io::Error::other)?
    }
}
fn permitted(method: &str) -> bool {
    matches!(
        method,
        "bridge.hello"
            | "bridge.ping"
            | "projects.list"
            | "projects.get"
            | "notebooks.list"
            | "notebooks.get"
            | "notebooks.read_chunk"
            | "notebooks.read_cells"
            | "sessions.list"
            | "sessions.get"
            | "sessions.ensure_primary"
            | "sessions.execute_cell"
            | "sessions.interrupt"
            | "sessions.restart"
            | "sessions.stop"
            | "sessions.list_variables"
            | "sessions.get_variable"
            | "operations.get"
            | "runtime.get_linux_metrics"
            | "runtime.get_mcp_status"
    )
}
fn rejected(id: String, code: &str, summary: &str) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol: 1,
        request_id: id,
        ok: false,
        result: None,
        error: Some(DomainError {
            code: code.into(),
            component: "runtime-unix".into(),
            summary: summary.into(),
            action: "Use a supported authenticated MCP domain operation".into(),
            detail: None,
        }),
    }
}
async fn read(stream: &mut UnixStream) -> io::Result<RequestEnvelope> {
    let size = stream.read_u32().await? as usize;
    if size == 0 || size > REQUEST_LIMIT {
        return Err(io::Error::other("Runtime Unix request exceeds limit"));
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| io::Error::other("Invalid Runtime Unix request"))
}
async fn send(stream: &mut UnixStream, response: ResponseEnvelope) -> io::Result<()> {
    let bytes = encode_json_frame(&response, RESPONSE_LIMIT)
        .or_else(|_| {
            encode_json_frame(
                &rejected(
                    response.request_id,
                    "CK-RUN-RESPONSE-LIMIT",
                    "Use a bounded notebook read",
                ),
                RESPONSE_LIMIT,
            )
        })
        .map_err(io::Error::other)?;
    tokio::time::timeout(Duration::from_secs(5), stream.write_all(&bytes))
        .await
        .map_err(io::Error::other)?
}
async fn connection(mut stream: UnixStream, runtime: RuntimeHandle) -> io::Result<()> {
    // One bounded request per connection. A client owns only its reply, never Runtime lifetime.
    let request = tokio::time::timeout(Duration::from_secs(5), read(&mut stream))
        .await
        .map_err(io::Error::other)??;
    let id = request.request_id.clone();
    if id.is_empty()
        || id.len() > 128
        || id.chars().any(char::is_control)
        || request.method.len() > 128
    {
        return send(
            &mut stream,
            rejected(
                String::new(),
                "CK-RUN-BAD-PARAMS",
                "Invalid request identity",
            ),
        )
        .await;
    }
    if !permitted(&request.method) {
        return send(
            &mut stream,
            rejected(id, "CK-RUN-NO-METHOD", "Method is not exposed to MCP"),
        )
        .await;
    }
    let reply = match runtime.submit(request, ExecutionOrigin::Mcp) {
        Err(error) => ResponseEnvelope {
            protocol: 1,
            request_id: id.clone(),
            ok: false,
            result: None,
            error: Some(error),
        },
        Ok(ticket) => match tokio::time::timeout(Duration::from_secs(120), ticket.wait()).await {
            Ok(Ok(reply)) => reply.response,
            Ok(Err(error)) => ResponseEnvelope {
                protocol: 1,
                request_id: id.clone(),
                ok: false,
                result: None,
                error: Some(error),
            },
            Err(_) => rejected(
                id,
                "CK-RUN-OUTCOME-UNKNOWN",
                "Reply deadline exceeded; accepted work is not replayed or cancelled",
            ),
        },
    };
    send(&mut stream, reply).await
}
