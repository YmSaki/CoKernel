use std::{
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
use subtle::ConstantTimeEq;
/// No Debug/Serialize implementation: the value is never an observable service setting.
pub struct Bearer([u8; 64]);
impl Bearer {
    pub fn load_or_create(path: &Path) -> io::Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("CK-MCP-AUTH: Missing private credential directory"))?;
        let metadata = std::fs::symlink_metadata(parent)?;
        if !path.is_absolute()
            || parent.canonicalize()? != parent
            || !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err(io::Error::other(
                "CK-MCP-AUTH: Credential directory must be private to the control identity",
            ));
        }
        let mut options = std::fs::OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW);
        match options.open(path) {
            Ok(mut file) => {
                let mut random = [0u8; 32];
                getrandom::fill(&mut random)
                    .map_err(|_| io::Error::other("CK-MCP-AUTH: OS random source unavailable"))?;
                let mut value = [0u8; 64];
                for (i, byte) in random.iter().enumerate() {
                    value[2 * i] = b"0123456789abcdef"[(byte >> 4) as usize];
                    value[2 * i + 1] = b"0123456789abcdef"[(byte & 15) as usize];
                }
                file.write_all(&value)?;
                file.sync_all()?;
                std::fs::File::open(parent)?.sync_all()?;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            return Err(io::Error::other(
                "CK-MCP-AUTH: Unsafe credential file ownership or permissions",
            ));
        }
        let mut value = Vec::new();
        file.take(65).read_to_end(&mut value)?;
        if value.len() != 64 || !value.iter().all(u8::is_ascii_hexdigit) {
            return Err(io::Error::other("CK-MCP-AUTH: Invalid credential file"));
        }
        let mut bytes = [0u8; 64];
        bytes.copy_from_slice(&value);
        Ok(Self(bytes))
    }
    pub fn accepts(&self, header: Option<&str>) -> bool {
        let Some(value) = header.and_then(|s| s.strip_prefix("Bearer ")) else {
            return false;
        };
        value.as_bytes().ct_eq(&self.0).into()
    }
}
