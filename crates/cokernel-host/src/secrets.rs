//! Host-owned tunnel credentials. No public read RPC returns credential bytes.
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use zeroize::Zeroizing;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::WindowsSecretStore;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum SecretError {
    #[error("Tunnel ID or runtime key is invalid or exceeds limits")]
    Invalid,
    #[error("Stored tunnel credential is malformed or unsupported; replace or clear it explicitly")]
    Corrupt,
    #[error("Windows credential storage is unavailable for the current user")]
    Unavailable,
    #[error("Credential queue is full; refresh configuration before retrying")]
    Busy,
    #[error("Credential owner is closed; accepted writes may have completed")]
    Closed,
}
impl SecretError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Invalid => "CK-SEC-CREDENTIAL-INPUT",
            Self::Corrupt => "CK-SEC-CREDENTIAL-DATA",
            Self::Unavailable => "CK-SEC-CREDENTIAL-STORE",
            Self::Busy => "CK-SEC-CREDENTIAL-BUSY",
            Self::Closed => "CK-SEC-CREDENTIAL-CLOSED",
        }
    }
}
pub type Result<T> = std::result::Result<T, SecretError>;

// Intentionally not Clone, Debug or Serialize. Only explicit tunnel handoff may
// borrow the key. Owned application buffers are wiped when dropped.
pub struct TunnelCredentials {
    tunnel_id: String,
    runtime_key: Zeroizing<Vec<u8>>,
}
impl TunnelCredentials {
    pub fn new(tunnel_id: String, runtime_key: Vec<u8>) -> Result<Self> {
        let runtime_key = Zeroizing::new(runtime_key);
        if !(8..=128).contains(&tunnel_id.len())
            || !tunnel_id.starts_with("tunnel_")
            || !tunnel_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            || !(16..=2048).contains(&runtime_key.len())
            || !runtime_key.iter().all(|c| c.is_ascii_graphic())
        {
            return Err(SecretError::Invalid);
        }
        Ok(Self {
            tunnel_id,
            runtime_key,
        })
    }
    pub fn tunnel_id(&self) -> &str {
        &self.tunnel_id
    }
    /// Internal product integration API, never exposed as a domain/MCP response.
    pub fn key_for_tunnel(&self) -> &[u8] {
        &self.runtime_key
    }
    #[cfg(any(windows, test))]
    fn encode(&self) -> Zeroizing<Vec<u8>> {
        let mut bytes = Zeroizing::new(Vec::with_capacity(
            7 + self.tunnel_id.len() + self.runtime_key.len(),
        ));
        bytes.extend_from_slice(b"CKTC\x01");
        bytes.extend_from_slice(&(self.tunnel_id.len() as u16).to_be_bytes());
        bytes.extend_from_slice(self.tunnel_id.as_bytes());
        bytes.extend_from_slice(&self.runtime_key);
        bytes
    }
    #[cfg(any(windows, test))]
    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 7 || bytes.len() > 2560 || &bytes[..5] != b"CKTC\x01" {
            return Err(SecretError::Corrupt);
        }
        let length = u16::from_be_bytes([bytes[5], bytes[6]]) as usize;
        if length > 128 || bytes.len() < 7 + length {
            return Err(SecretError::Corrupt);
        }
        let id = std::str::from_utf8(&bytes[7..7 + length]).map_err(|_| SecretError::Corrupt)?;
        Self::new(id.into(), bytes[7 + length..].to_vec()).map_err(|_| SecretError::Corrupt)
    }
    fn status(&self) -> Configuration {
        Configuration {
            configured: true,
            tunnel_id: Some(self.tunnel_id.clone()),
        }
    }
}
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub configured: bool,
    pub tunnel_id: Option<String>,
}

/// Synchronous OS backend, driven only by the owned blocking queue below.
pub trait SecretStore: Send + 'static {
    fn load(&mut self) -> Result<Option<TunnelCredentials>>;
    fn replace(&mut self, credentials: &TunnelCredentials) -> Result<()>;
    fn clear(&mut self) -> Result<()>;
}
enum Job {
    Status(oneshot::Sender<Result<Configuration>>),
    Replace(TunnelCredentials, oneshot::Sender<Result<Configuration>>),
    Clear(oneshot::Sender<Result<Configuration>>),
    Load(oneshot::Sender<Result<Option<TunnelCredentials>>>),
}
#[derive(Clone)]
pub struct Vault {
    queue: mpsc::Sender<Job>,
}
impl Vault {
    pub fn start(mut store: impl SecretStore) -> Self {
        let (queue, mut commands) = mpsc::channel(8);
        // Accepted work belongs to the owner, even when a UI drops its reply.
        // Closing the last sender drains already accepted operations before exit.
        tokio::task::spawn_blocking(move || {
            while let Some(job) = commands.blocking_recv() {
                match job {
                    Job::Status(reply) => {
                        let result = store
                            .load()
                            .map(|c| c.map_or_else(Configuration::default, |c| c.status()));
                        let _ = reply.send(result);
                    }
                    Job::Replace(credentials, reply) => {
                        let result = store.replace(&credentials).map(|_| credentials.status());
                        drop(credentials);
                        let _ = reply.send(result);
                    }
                    Job::Clear(reply) => {
                        let _ = reply.send(store.clear().map(|_| Configuration::default()));
                    }
                    Job::Load(reply) => {
                        let _ = reply.send(store.load());
                    }
                }
            }
        });
        Self { queue }
    }
    fn submit(&self, job: Job) -> Result<()> {
        self.queue.try_send(job).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => SecretError::Busy,
            mpsc::error::TrySendError::Closed(_) => SecretError::Closed,
        })
    }
    pub async fn configuration(&self) -> Result<Configuration> {
        let (reply, receive) = oneshot::channel();
        self.submit(Job::Status(reply))?;
        receive.await.map_err(|_| SecretError::Closed)?
    }
    pub async fn replace(&self, credentials: TunnelCredentials) -> Result<Configuration> {
        let (reply, receive) = oneshot::channel();
        self.submit(Job::Replace(credentials, reply))?;
        receive.await.map_err(|_| SecretError::Closed)?
    }
    pub async fn clear(&self) -> Result<Configuration> {
        let (reply, receive) = oneshot::channel();
        self.submit(Job::Clear(reply))?;
        receive.await.map_err(|_| SecretError::Closed)?
    }
    pub async fn load_for_tunnel(&self) -> Result<Option<TunnelCredentials>> {
        let (reply, receive) = oneshot::channel();
        self.submit(Job::Load(reply))?;
        receive.await.map_err(|_| SecretError::Closed)?
    }
}

#[cfg(windows)]
pub(crate) fn parse_credentials(mut params: serde_json::Value) -> Result<TunnelCredentials> {
    let object = params.as_object_mut().ok_or(SecretError::Invalid)?;
    let key = match object.remove("runtime_key") {
        Some(serde_json::Value::String(key)) => Zeroizing::new(key.into_bytes()),
        _ => return Err(SecretError::Invalid),
    };
    if object.len() != 1 {
        return Err(SecretError::Invalid);
    }
    let Some(serde_json::Value::String(tunnel_id)) = object.remove("tunnel_id") else {
        return Err(SecretError::Invalid);
    };
    TunnelCredentials::new(tunnel_id, key.to_vec())
}
