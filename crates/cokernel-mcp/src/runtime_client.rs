use cokernel_protocol::{RequestEnvelope, ResponseEnvelope, encode_json_frame};
use serde_json::Value;
use std::{
    io,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
#[derive(Clone)]
pub struct RuntimeClient {
    path: PathBuf,
}
impl RuntimeClient {
    pub fn new(path: PathBuf) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::other(
                "CK-MCP-CONFIG: Runtime socket must be absolute",
            ));
        }
        Ok(Self { path })
    }
    pub async fn hello(&self) -> io::Result<cokernel_protocol::methods::BridgeHelloResult> {
        let response = self
            .call(
                "bridge.hello",
                serde_json::json!({
                    "host_product_version": env!("CARGO_PKG_VERSION"),
                    "supported_protocols": [1]
                }),
            )
            .await?;
        let hello: cokernel_protocol::methods::BridgeHelloResult = response
            .result
            .filter(|_| response.ok)
            .and_then(|value| serde_json::from_value(value).ok())
            .ok_or_else(|| io::Error::other("CK-MCP-RUNTIME: Runtime handshake rejected"))?;
        if hello.selected_protocol != 1 {
            return Err(io::Error::other(
                "CK-MCP-RUNTIME: Incompatible Runtime protocol",
            ));
        }
        Ok(hello)
    }
    pub async fn call(&self, method: &str, params: Value) -> io::Result<ResponseEnvelope> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || validate(&path))
            .await
            .map_err(io::Error::other)??;
        let request = RequestEnvelope {
            protocol: 1,
            request_id: uuid::Uuid::new_v4().to_string(),
            method: method.into(),
            params,
        };
        let frame = encode_json_frame(&request, 1024 * 1024)
            .map_err(|_| io::Error::other("CK-MCP-LIMIT: Runtime request exceeds limit"))?;
        tokio::time::timeout(Duration::from_secs(125), async {
            let mut stream =
                tokio::time::timeout(Duration::from_secs(3), UnixStream::connect(&self.path))
                    .await
                    .map_err(|_| io::Error::other("CK-MCP-RUNTIME: Connection timed out"))??;
            if stream.peer_cred()?.uid() != unsafe { libc::geteuid() } {
                return Err(io::Error::other(
                    "CK-MCP-RUNTIME: Untrusted Runtime identity",
                ));
            }
            stream.write_all(&frame).await?;
            let length = stream.read_u32().await? as usize;
            if length == 0 || length > 8 * 1024 * 1024 {
                return Err(io::Error::other(
                    "CK-MCP-LIMIT: Runtime response exceeds limit",
                ));
            }
            let mut bytes = vec![0; length];
            stream.read_exact(&mut bytes).await?;
            let response: ResponseEnvelope = serde_json::from_slice(&bytes)
                .map_err(|_| io::Error::other("CK-MCP-RUNTIME: Invalid Runtime response"))?;
            if response.protocol != 1
                || response.request_id != request.request_id
                || (response.ok && (response.error.is_some() || response.result.is_none()))
                || (!response.ok && (response.error.is_none() || response.result.is_some()))
            {
                return Err(io::Error::other(
                    "CK-MCP-RUNTIME: Invalid Runtime correlation or response shape",
                ));
            }
            Ok(response)
        })
        .await
        .map_err(|_| {
            io::Error::other(
                "CK-MCP-OUTCOME-UNKNOWN: Reply deadline exceeded; accepted work may continue",
            )
        })?
    }
}
fn validate(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("CK-MCP-CONFIG: Missing socket directory"))?;
    let directory = std::fs::symlink_metadata(parent)?;
    let socket = std::fs::symlink_metadata(path)?;
    if parent.canonicalize()? != parent
        || !directory.is_dir()
        || directory.uid() != unsafe { libc::geteuid() }
        || directory.mode() & 0o077 != 0
        || !socket.file_type().is_socket()
        || socket.uid() != unsafe { libc::geteuid() }
        || socket.mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "CK-MCP-RUNTIME: Unsafe Runtime socket ownership or permissions",
        ));
    }
    Ok(())
}
