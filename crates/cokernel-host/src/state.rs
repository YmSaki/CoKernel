//! Host-owned desired state; never reinterpret a persisted Stop as Running.
use crate::bridge::BridgeError;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DesiredState {
    Running,
    Stopped,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Persisted {
    schema: u32,
    desired: DesiredState,
}
pub struct StateStore {
    directory: PathBuf,
    _lock: File,
}

#[cfg(unix)]
impl Drop for StateStore {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // A concurrently forked child can briefly hold the same open file
        // description until exec closes CLOEXEC descriptors. Release our lock
        // explicitly before acknowledging Host exit, not on final fd closure.
        // SAFETY: _lock owns this live descriptor; no pointers are retained.
        unsafe {
            libc::flock(self._lock.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn released_host_lock_does_not_wait_for_an_inherited_descriptor() {
        let path = std::env::temp_dir().join(format!("ckhost-lock-{}", uuid::Uuid::new_v4()));
        let (store, _) = StateStore::open(&path).unwrap();
        let inherited = store._lock.try_clone().unwrap();
        drop(store);
        let (next, _) = StateStore::open(&path).unwrap();
        drop(inherited);
        drop(next);
        std::fs::remove_dir_all(path).unwrap();
    }
}
impl StateStore {
    pub fn open(directory: &Path) -> Result<(Self, DesiredState), BridgeError> {
        let fail = |_: std::io::Error| {
            BridgeError::new(
                "CK-HOST-STATE",
                "Cannot open Host state; check permissions and whether another Host is running",
            )
        };
        if !directory.is_absolute() {
            return Err(BridgeError::new(
                "CK-HOST-CONFIG",
                "Host state directory must be absolute",
            ));
        }
        fs::create_dir_all(directory).map_err(fail)?;
        if fs::symlink_metadata(directory)
            .map_err(fail)?
            .file_type()
            .is_symlink()
        {
            return Err(BridgeError::new(
                "CK-HOST-STATE",
                "Host state directory must not be a symlink",
            ));
        }
        let lock_path = directory.join("host.lock");
        reject_symlink(&lock_path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lock = options.open(lock_path).map_err(fail)?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: lock owns a live file descriptor; flock does not retain pointers.
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(fail(std::io::Error::last_os_error()));
            }
        }
        let path = directory.join("desired.json");
        reject_symlink(&path)?;
        let desired = match File::open(&path) {
            Ok(file) => {
                if !file.metadata().map_err(fail)?.is_file() {
                    return Err(BridgeError::new(
                        "CK-HOST-STATE",
                        "Host state is not a regular file",
                    ));
                }
                let mut bytes = Vec::new();
                file.take(4097).read_to_end(&mut bytes).map_err(fail)?;
                let state:Persisted=serde_json::from_slice(&bytes).map_err(|_|BridgeError::new("CK-HOST-STATE","Malformed Host state; repair explicitly instead of guessing the desired state"))?;
                if bytes.len() > 4096 || state.schema != 1 {
                    return Err(BridgeError::new(
                        "CK-HOST-STATE",
                        "Unsupported or oversized Host state",
                    ));
                }
                state.desired
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => DesiredState::Stopped,
            Err(error) => return Err(fail(error)),
        };
        Ok((
            Self {
                directory: directory.into(),
                _lock: lock,
            },
            desired,
        ))
    }
    pub fn save(&self, desired: DesiredState) -> Result<(), BridgeError> {
        let path = self.directory.join("desired.json");
        reject_symlink(&path)?;
        let temp = self
            .directory
            .join(format!("desired-{}.tmp", uuid::Uuid::new_v4()));
        let write = || -> std::io::Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            let bytes = serde_json::to_vec(&Persisted { schema: 1, desired })?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, &path)?;
            #[cfg(unix)]
            File::open(&self.directory)?.sync_all()?;
            Ok(())
        };
        if write().is_err() {
            let _ = fs::remove_file(&temp);
            return Err(BridgeError::new(
                "CK-HOST-STATE",
                "Desired state could not be durably saved; inspect state before retrying",
            ));
        }
        Ok(())
    }
}
fn reject_symlink(path: &Path) -> Result<(), BridgeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => Err(
            BridgeError::new("CK-HOST-STATE", "Host state must be a regular file"),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(BridgeError::new(
            "CK-HOST-STATE",
            "Host state cannot be inspected",
        )),
    }
}
