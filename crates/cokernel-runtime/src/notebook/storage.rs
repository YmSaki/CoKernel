//! Linux descriptor-relative I/O: no symlink traversal and no path re-resolution
//! between checking a parent directory and installing a document into it.
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

use cokernel_domain::NotebookId;
use sha2::{Digest, Sha256};

use super::{MAX_NOTEBOOK_BYTES, NotebookError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Fingerprint {
    pub hash: String,
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

fn identity(metadata: &std::fs::Metadata, hash: String) -> Fingerprint {
    Fingerprint {
        hash,
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.len(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    }
}

pub(super) struct Location {
    parent: File,
    name: CString,
}

pub(super) struct Commit {
    pub fingerprint: Fingerprint,
    pub sync_error: Option<io::Error>,
}

pub(super) fn open_root(path: &Path) -> Result<File, NotebookError> {
    Ok(std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?)
}

pub(super) fn validate_path(path: &str) -> Result<Vec<&str>, NotebookError> {
    let parts: Vec<_> = path.split('/').collect();
    if path.len() > 4096
        || parts.len() > 32
        || path.contains(['\\', ':', '\0'])
        || !path.ends_with(".ipynb")
        || parts
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == ".." || p.len() > 255)
    {
        return Err(NotebookError::UnsafePath);
    }
    Ok(parts)
}

pub(super) fn discover(root: &File) -> Result<Vec<String>, NotebookError> {
    fn walk(
        directory: &File,
        prefix: &str,
        depth: usize,
        seen: &mut usize,
        found: &mut Vec<String>,
    ) -> Result<(), NotebookError> {
        if depth > 32 {
            return Err(NotebookError::Limit);
        }
        // /proc resolves our still-owned descriptor, not a caller-supplied path.
        for entry in std::fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))? {
            let entry = entry?;
            *seen += 1;
            if *seen > 100_000 {
                return Err(NotebookError::Limit);
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let kind = entry.file_type()?;
            let relative = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            if kind.is_dir() && !matches!(name, ".venv" | ".git" | ".cokernel") {
                let child = open_at(
                    directory.as_raw_fd(),
                    &cstring(name)?,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )?;
                walk(&child, &relative, depth + 1, seen, found)?;
            } else if kind.is_file() && name.ends_with(".ipynb") {
                validate_path(&relative)?;
                found.push(relative);
                if found.len() > super::model::MAX_CELLS {
                    return Err(NotebookError::Limit);
                }
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    walk(root, "", 0, &mut 0, &mut found)?;
    found.sort();
    Ok(found)
}

impl Location {
    pub fn assert_absent(&self) -> Result<(), NotebookError> {
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: valid directory FD, NUL-terminated name, writable stat buffer.
        let result = unsafe {
            libc::fstatat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            return Err(NotebookError::AlreadyExists);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOENT) {
            Ok(())
        } else {
            Err(map_io(error))
        }
    }

    pub fn import_staging(&self) -> Result<File, NotebookError> {
        // Anonymous same-filesystem storage: close/process exit removes it,
        // including incomplete imports after a Runtime crash.
        open_at(
            self.parent.as_raw_fd(),
            &cstring(".")?,
            libc::O_RDWR | libc::O_TMPFILE,
            0o600,
        )
    }

    pub fn resolve(root: &File, path: &str) -> Result<Self, NotebookError> {
        let parts = validate_path(path)?;
        let mut parent = root.try_clone()?;
        for part in &parts[..parts.len() - 1] {
            parent = open_at(
                parent.as_raw_fd(),
                &cstring(part)?,
                libc::O_RDONLY | libc::O_DIRECTORY,
                0,
            )?;
        }
        Ok(Self {
            parent,
            name: cstring(parts[parts.len() - 1])?,
        })
    }

    pub fn read(&self) -> Result<(Vec<u8>, Fingerprint), NotebookError> {
        let mut file = open_at(
            self.parent.as_raw_fd(),
            &self.name,
            libc::O_RDONLY | libc::O_NONBLOCK,
            0,
        )?;
        let before = file.metadata()?;
        if !before.is_file() || before.nlink() != 1 {
            return Err(NotebookError::UnsafePath);
        }
        if before.len() > MAX_NOTEBOOK_BYTES as u64 {
            return Err(NotebookError::Limit);
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_NOTEBOOK_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_NOTEBOOK_BYTES {
            return Err(NotebookError::Limit);
        }
        let after = file.metadata()?;
        if identity(&before, String::new()) != identity(&after, String::new()) {
            return Err(NotebookError::ExternalChange);
        }
        let fingerprint = identity(&after, format!("{:x}", Sha256::digest(&bytes)));
        Ok((bytes, fingerprint))
    }

    pub fn write(
        &self,
        bytes: &[u8],
        expected: Option<&Fingerprint>,
    ) -> Result<Commit, NotebookError> {
        self.write_with(
            bytes,
            expected,
            |file, data| file.write_all(data),
            || Ok(()),
        )
    }

    fn write_with(
        &self,
        bytes: &[u8],
        expected: Option<&Fingerprint>,
        write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
        before_commit: impl FnOnce() -> io::Result<()>,
    ) -> Result<Commit, NotebookError> {
        if bytes.len() > MAX_NOTEBOOK_BYTES {
            return Err(NotebookError::Limit);
        }
        let temporary = cstring(&format!(".cokernel-{}.tmp", NotebookId::new()))?;
        let mut file = open_at(
            self.parent.as_raw_fd(),
            &temporary,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        let cleanup = Temporary {
            directory: &self.parent,
            name: &temporary,
        };
        write(&mut file, bytes)?;
        // Managed Projects share a setgid workload group. Keep staging private
        // while writing, then make the complete document accessible to that
        // group before atomic installation. Control-private state is elsewhere.
        if self.parent.metadata()?.mode() & 0o2000 != 0 {
            file.set_permissions(std::fs::Permissions::from_mode(0o660))?;
        }
        file.sync_all()?;
        before_commit()?;
        if let Some(expected) = expected {
            match self.read() {
                Ok((_, current)) if &current == expected => {}
                Ok(_) | Err(NotebookError::NotFound | NotebookError::UnsafePath) => {
                    return Err(NotebookError::ExternalChange);
                }
                Err(error) => return Err(error),
            }
        }
        let flags = if expected.is_none() {
            libc::RENAME_NOREPLACE
        } else {
            0
        };
        // SAFETY: both C strings live for this call and parent is an owned open FD.
        let result = unsafe {
            libc::renameat2(
                self.parent.as_raw_fd(),
                temporary.as_ptr(),
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                flags,
            )
        };
        if result != 0 {
            return Err(map_io(io::Error::last_os_error()));
        }
        drop(cleanup);
        // Read identity from the file we installed, not from a pathname an
        // external writer could already have replaced. A later save detects it.
        let fingerprint = identity(&file.metadata()?, format!("{:x}", Sha256::digest(bytes)));
        let sync_error = self.parent.sync_all().err();
        Ok(Commit {
            fingerprint,
            sync_error,
        })
    }
}

struct Temporary<'a> {
    directory: &'a File,
    name: &'a CString,
}
impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        // SAFETY: the live parent FD and private generated name designate only
        // our temporary directory entry; after rename this is harmless ENOENT.
        unsafe {
            libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0);
        }
    }
}

fn cstring(text: &str) -> Result<CString, NotebookError> {
    CString::new(text).map_err(|_| NotebookError::UnsafePath)
}

fn open_at(parent: RawFd, name: &CString, flags: i32, mode: u32) -> Result<File, NotebookError> {
    // SAFETY: name is NUL-terminated. A successful new FD is owned exactly once.
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode as libc::mode_t,
        )
    };
    if fd < 0 {
        return Err(map_io(io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn map_io(error: io::Error) -> NotebookError {
    match error.raw_os_error() {
        Some(libc::ELOOP | libc::ENOTDIR) => NotebookError::UnsafePath,
        Some(libc::ENOENT) => NotebookError::NotFound,
        Some(libc::EEXIST) => NotebookError::AlreadyExists,
        _ => NotebookError::Io(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("cknb-{}", NotebookId::new()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn location(&self) -> Location {
            Location::resolve(&open_root(&self.0).unwrap(), "a.ipynb").unwrap()
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn failed_partial_write_preserves_original_and_cleans_temporary() {
        let root = Directory::new();
        let location = root.location();
        let original = location.write(b"original", None).unwrap();
        let result = location.write_with(
            b"replacement",
            Some(&original.fingerprint),
            |f, b| {
                f.write_all(&b[..3])?;
                Err(io::Error::other("injected write failure"))
            },
            || Ok(()),
        );
        assert!(result.is_err());
        assert_eq!(location.read().unwrap().0, b"original");
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
    }

    #[test]
    fn external_change_during_staging_and_create_collision_are_preserved() {
        let root = Directory::new();
        let location = root.location();
        let original = location.write(b"original", None).unwrap();
        let result = location.write_with(
            b"replacement",
            Some(&original.fingerprint),
            |f, b| f.write_all(b),
            || fs::write(root.0.join("a.ipynb"), b"external"),
        );
        assert!(matches!(result, Err(NotebookError::ExternalChange)));
        assert!(matches!(
            location.write(b"overwrite", None),
            Err(NotebookError::AlreadyExists)
        ));
        assert_eq!(location.read().unwrap().0, b"external");
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
    }

    #[test]
    fn traversal_symlinks_hardlinks_and_fifo_are_rejected() {
        let root = Directory::new();
        let outside = Directory::new();
        let fd = open_root(&root.0).unwrap();
        for path in [
            "../a.ipynb",
            "/a.ipynb",
            "C:/a.ipynb",
            "a\\b.ipynb",
            "./a.ipynb",
            "a//b.ipynb",
            "a/../b.ipynb",
            "a.txt",
            "a\0.ipynb",
        ] {
            assert!(
                matches!(Location::resolve(&fd, path), Err(NotebookError::UnsafePath)),
                "{path:?}"
            );
        }
        fs::write(outside.0.join("a.ipynb"), b"outside").unwrap();
        std::os::unix::fs::symlink(&outside.0, root.0.join("link")).unwrap();
        assert!(matches!(
            Location::resolve(&fd, "link/a.ipynb"),
            Err(NotebookError::UnsafePath)
        ));
        std::os::unix::fs::symlink(outside.0.join("a.ipynb"), root.0.join("a.ipynb")).unwrap();
        assert!(matches!(
            root.location().read(),
            Err(NotebookError::UnsafePath)
        ));
        assert!(root.location().write(b"bad", None).is_err());
        fs::remove_file(root.0.join("a.ipynb")).unwrap();
        fs::hard_link(outside.0.join("a.ipynb"), root.0.join("a.ipynb")).unwrap();
        assert!(matches!(
            root.location().read(),
            Err(NotebookError::UnsafePath)
        ));
        fs::remove_file(root.0.join("a.ipynb")).unwrap();
        let name = cstring(root.0.join("a.ipynb").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(matches!(
            root.location().read(),
            Err(NotebookError::UnsafePath)
        ));
        assert_eq!(fs::read(outside.0.join("a.ipynb")).unwrap(), b"outside");
    }
}
