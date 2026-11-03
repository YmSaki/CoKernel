use super::*;
use std::sync::{Arc, Mutex};

fn credential() -> TunnelCredentials {
    TunnelCredentials::new(
        "tunnel_0123456789abcdef0123456789abcdef".into(),
        b"test-only-runtime-key-12345".to_vec(),
    )
    .unwrap()
}
#[test]
fn bounded_versioned_blob_roundtrip_and_rejection_do_not_echo_secret() {
    let value = credential();
    let encoded = value.encode();
    let decoded = TunnelCredentials::decode(&encoded).unwrap();
    assert_eq!(decoded.key_for_tunnel(), value.key_for_tunnel());
    assert_eq!(decoded.tunnel_id(), value.tunnel_id());
    for bad in [
        vec![],
        b"CKTC\x02\0\0secret".to_vec(),
        vec![b'x'; 2561],
        b"CKTC\x01\xff\xff".to_vec(),
    ] {
        assert!(matches!(
            TunnelCredentials::decode(&bad),
            Err(SecretError::Corrupt)
        ));
    }
    for id in ["", "https://server/path", "tunnel_x\n", "tunnel_../other"] {
        assert!(matches!(
            TunnelCredentials::new(id.into(), b"test-only-runtime-key-12345".to_vec()),
            Err(SecretError::Invalid)
        ));
    }
    for key in [
        vec![],
        vec![b'a'; 2049],
        b"test-secret\nother".to_vec(),
        vec![0; 16],
    ] {
        assert!(matches!(
            TunnelCredentials::new(value.tunnel_id().into(), key),
            Err(SecretError::Invalid)
        ));
    }
    let status = serde_json::to_string(&value.status()).unwrap();
    assert!(!status.contains("test-only-runtime-key"));
    assert!(!format!("{:?}", SecretError::Invalid).contains("test-only"));
}

struct FakeStore {
    bytes: Arc<Mutex<Option<Zeroizing<Vec<u8>>>>>,
    fail: bool,
}
impl SecretStore for FakeStore {
    fn load(&mut self) -> Result<Option<TunnelCredentials>> {
        self.bytes
            .lock()
            .unwrap()
            .as_ref()
            .map(|b| TunnelCredentials::decode(b))
            .transpose()
    }
    fn replace(&mut self, value: &TunnelCredentials) -> Result<()> {
        if self.fail {
            return Err(SecretError::Unavailable);
        }
        *self.bytes.lock().unwrap() = Some(value.encode());
        Ok(())
    }
    fn clear(&mut self) -> Result<()> {
        if self.fail {
            return Err(SecretError::Unavailable);
        }
        *self.bytes.lock().unwrap() = None;
        Ok(())
    }
}
#[tokio::test]
async fn failed_backend_mutation_preserves_stored_configuration() {
    let bytes = Arc::new(Mutex::new(Some(credential().encode())));
    let vault = Vault::start(FakeStore { bytes, fail: true });
    assert!(matches!(
        vault.replace(credential()).await,
        Err(SecretError::Unavailable)
    ));
    assert!(matches!(vault.clear().await, Err(SecretError::Unavailable)));
    assert!(vault.configuration().await.unwrap().configured);
    assert_eq!(
        vault
            .load_for_tunnel()
            .await
            .unwrap()
            .unwrap()
            .key_for_tunnel(),
        credential().key_for_tunnel()
    );
}
#[tokio::test]
async fn bounded_fifo_owner_retains_accepted_write_after_reply_disconnection() {
    struct Blocking {
        inner: FakeStore,
        started: Option<oneshot::Sender<()>>,
        release: std::sync::mpsc::Receiver<()>,
    }
    impl SecretStore for Blocking {
        fn load(&mut self) -> Result<Option<TunnelCredentials>> {
            self.inner.load()
        }
        fn replace(&mut self, c: &TunnelCredentials) -> Result<()> {
            if let Some(started) = self.started.take() {
                let _ = started.send(());
                self.release
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
            }
            self.inner.replace(c)
        }
        fn clear(&mut self) -> Result<()> {
            self.inner.clear()
        }
    }
    let bytes = Arc::new(Mutex::new(None));
    let (started, waiting) = oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let vault = Vault::start(Blocking {
        inner: FakeStore {
            bytes: bytes.clone(),
            fail: false,
        },
        started: Some(started),
        release: blocked,
    });
    let (reply, receive) = oneshot::channel();
    vault.submit(Job::Replace(credential(), reply)).unwrap();
    waiting.await.unwrap();
    drop(receive);
    let mut replies = vec![];
    for _ in 0..8 {
        let (reply, receive) = oneshot::channel();
        vault.submit(Job::Status(reply)).unwrap();
        replies.push(receive);
    }
    let (reply, _) = oneshot::channel();
    assert!(matches!(
        vault.submit(Job::Clear(reply)),
        Err(SecretError::Busy)
    ));
    release.send(()).unwrap();
    for receive in replies {
        assert!(receive.await.unwrap().unwrap().configured);
    }
    assert!(bytes.lock().unwrap().is_some());
    assert!(!vault.clear().await.unwrap().configured);
    assert!(vault.load_for_tunnel().await.unwrap().is_none());
}

#[cfg(windows)]
#[test]
fn actual_windows_credential_roundtrip_replacement_isolation_and_clear() {
    struct Fixture {
        directory: std::path::PathBuf,
        store: WindowsSecretStore,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!("ck-vault-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&directory).unwrap();
            let store = WindowsSecretStore::for_state(&directory).unwrap();
            Self { directory, store }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.store.clear();
            let _ = std::fs::remove_dir(&self.directory);
        }
    }
    let mut one = Fixture::new();
    let mut other = Fixture::new();
    assert!(one.store.load().unwrap().is_none());
    one.store.replace(&credential()).unwrap();
    let mut reopened = WindowsSecretStore::for_state(&one.directory).unwrap();
    assert_eq!(
        reopened.load().unwrap().unwrap().key_for_tunnel(),
        credential().key_for_tunnel()
    );
    assert!(other.store.load().unwrap().is_none());
    let changed = TunnelCredentials::new(
        credential().tunnel_id().into(),
        b"test-only-replacement-key".to_vec(),
    )
    .unwrap();
    reopened.replace(&changed).unwrap();
    assert_eq!(
        one.store.load().unwrap().unwrap().key_for_tunnel(),
        changed.key_for_tunnel()
    );
    one.store.clear().unwrap();
    one.store.clear().unwrap();
    assert!(reopened.load().unwrap().is_none());
    assert_eq!(
        std::fs::read_dir(&one.directory).unwrap().count(),
        0,
        "secret created in state files"
    );
}
