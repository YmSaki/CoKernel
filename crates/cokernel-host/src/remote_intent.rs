//! Durable user intent only. Credentials and observed connection state never enter this file.
use crate::bridge::BridgeError;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Persisted {
    schema: u32,
    enabled: bool,
}
#[derive(Clone)]
pub(crate) struct IntentStore {
    directory: PathBuf,
}
impl IntentStore {
    pub fn new(directory: &Path) -> Self {
        Self {
            directory: directory.into(),
        }
    }
    pub fn load(&self) -> Result<bool, BridgeError> {
        let path = self.directory.join("remote-access.json");
        check(&path)?;
        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(error()),
        };
        if !file.metadata().map_err(|_| error())?.is_file() {
            return Err(error());
        }
        let mut bytes = Vec::new();
        file.take(1025)
            .read_to_end(&mut bytes)
            .map_err(|_| error())?;
        if bytes.len() > 1024 {
            return Err(error());
        }
        let value: Persisted = serde_json::from_slice(&bytes).map_err(|_| error())?;
        if value.schema != 1 {
            return Err(error());
        }
        Ok(value.enabled)
    }
    pub fn save(&self, enabled: bool) -> Result<(), BridgeError> {
        let path = self.directory.join("remote-access.json");
        check(&path)?;
        let temporary = self
            .directory
            .join(format!("remote-access-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(&serde_json::to_vec(&Persisted { schema: 1, enabled })?)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &path)?;
            #[cfg(unix)]
            File::open(&self.directory)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
            return Err(error());
        }
        Ok(())
    }
}
fn check(path: &Path) -> Result<(), BridgeError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(error()),
    }
}
fn error() -> BridgeError {
    BridgeError::new(
        "CK-REMOTE-STATE",
        "Remote Access intent is unavailable or malformed; explicitly disable or repair it",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durable_intent_is_bounded_and_never_inferred_from_configuration() {
        let path = std::env::temp_dir().join(format!("ckremote-intent-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        let store = IntentStore::new(&path);
        assert!(!store.load().unwrap());
        store.save(true).unwrap();
        assert!(IntentStore::new(&path).load().unwrap());
        store.save(false).unwrap();
        assert!(!store.load().unwrap());
        for bytes in [
            b"{}".to_vec(),
            b"{\"schema\":1,\"enabled\":true,\"runtime_key\":\"secret\"}".to_vec(),
            b"{\"schema\":2,\"enabled\":true}".to_vec(),
            vec![b' '; 1025],
        ] {
            fs::write(path.join("remote-access.json"), bytes).unwrap();
            assert!(store.load().is_err());
        }
        store.save(false).unwrap();
        assert!(!store.load().unwrap());
        fs::remove_dir_all(path).unwrap();
    }
}
