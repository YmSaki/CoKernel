use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::{Duration, Instant};

use cokernel_domain::{ImportId, Project, ProjectId};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{DocumentSnapshot, MAX_NOTEBOOK_BYTES, Notebook, NotebookError, NotebookService};

const CHUNK_BYTES: usize = 262_144;
const MAX_IMPORTS: usize = 4;
const MAX_RESERVED_BYTES: u64 = 128 * 1024 * 1024;
const IMPORT_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Serialize)]
pub struct ImportTicket {
    pub import_id: ImportId,
    pub accepted_chunk_size: usize,
    pub expires_in_seconds: u64,
}

pub(super) struct PendingImport {
    project_id: ProjectId,
    root_path: String,
    target_path: String,
    size: u64,
    sha256: String,
    offset: u64,
    digest: Sha256,
    file: File,
    expires: Instant,
}

fn invalid(message: &str) -> NotebookError {
    NotebookError::Import(message.into())
}

impl NotebookService {
    fn expire_imports(&mut self) {
        let now = Instant::now();
        self.imports.retain(|_, import| import.expires > now);
    }

    fn authorize_import(&mut self, project: &Project, id: ImportId) -> Result<(), NotebookError> {
        self.expire_imports();
        self.imports
            .get(&id)
            .filter(|p| p.project_id == project.project_id && p.root_path == project.root_path)
            .map(|_| ())
            .ok_or(NotebookError::NotFound)
    }

    pub fn begin_import(
        &mut self,
        project: &Project,
        path: &str,
        size: u64,
        sha256: &str,
    ) -> Result<ImportTicket, NotebookError> {
        self.expire_imports();
        if size == 0
            || size > MAX_NOTEBOOK_BYTES as u64
            || self.imports.len() >= MAX_IMPORTS
            || self
                .imports
                .values()
                .map(|p| p.size)
                .sum::<u64>()
                .saturating_add(size)
                > MAX_RESERVED_BYTES
        {
            return Err(NotebookError::Limit);
        }
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("sha256 must be 64 hexadecimal characters"));
        }
        if self
            .imports
            .values()
            .any(|p| p.project_id == project.project_id && p.target_path == path)
        {
            return Err(NotebookError::AlreadyExists);
        }
        let location = self.location(project, path)?;
        location.assert_absent()?;
        let file = location.import_staging()?;
        let id = ImportId::new();
        self.imports.insert(
            id,
            PendingImport {
                project_id: project.project_id,
                root_path: project.root_path.clone(),
                target_path: path.into(),
                size,
                sha256: sha256.to_ascii_lowercase(),
                offset: 0,
                digest: Sha256::new(),
                file,
                expires: Instant::now() + IMPORT_TTL,
            },
        );
        Ok(ImportTicket {
            import_id: id,
            accepted_chunk_size: CHUNK_BYTES,
            expires_in_seconds: IMPORT_TTL.as_secs(),
        })
    }

    /// Transport decodes a bounded base64 chunk before this domain call. An
    /// invalid chunk terminates its transaction; unrelated IDs are not touched.
    pub fn import_chunk(
        &mut self,
        project: &Project,
        id: ImportId,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, NotebookError> {
        self.authorize_import(project, id)?;
        let mut import = self.imports.remove(&id).ok_or(NotebookError::NotFound)?;
        if bytes.is_empty()
            || bytes.len() > CHUNK_BYTES
            || offset != import.offset
            || offset
                .checked_add(bytes.len() as u64)
                .is_none_or(|end| end > import.size)
        {
            return Err(invalid(
                "chunk offset, size or total length mismatch; transaction cancelled",
            ));
        }
        import.file.write_all(bytes)?;
        import.digest.update(bytes);
        import.offset += bytes.len() as u64;
        let accepted = import.offset;
        self.imports.insert(id, import);
        Ok(accepted)
    }

    pub fn cancel_import(&mut self, project: &Project, id: ImportId) -> Result<(), NotebookError> {
        self.authorize_import(project, id)?;
        self.imports.remove(&id);
        Ok(())
    }

    pub fn finish_import(
        &mut self,
        project: &Project,
        id: ImportId,
    ) -> Result<DocumentSnapshot, NotebookError> {
        self.authorize_import(project, id)?;
        let mut import = self.imports.remove(&id).ok_or(NotebookError::NotFound)?;
        if import.offset != import.size {
            return Err(invalid("incomplete upload; transaction cancelled"));
        }
        let actual = format!("{:x}", import.digest.finalize());
        if actual != import.sha256 {
            return Err(invalid("sha256 mismatch; transaction cancelled"));
        }
        import.file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        import
            .file
            .take(MAX_NOTEBOOK_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 != import.size
            || format!("{:x}", Sha256::digest(&bytes)) != import.sha256
        {
            return Err(invalid("staging content changed; transaction cancelled"));
        }
        let notebook = Notebook::parse(&bytes)?;
        // Re-resolve the target now: parent symlinks or collisions introduced
        // during upload must not be followed or overwritten.
        self.install_notebook(project, &import.target_path, notebook)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cokernel_domain::NotebookId;

    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> (Directory, Project, NotebookService, Vec<u8>) {
        let directory =
            Directory(std::env::temp_dir().join(format!("ckimp-{}", NotebookId::new())));
        std::fs::create_dir(&directory.0).unwrap();
        let project = Project {
            project_id: ProjectId::new(),
            name: "import".into(),
            root_path: directory.0.to_str().unwrap().into(),
            environment_generation: 1,
        };
        (
            directory,
            project,
            NotebookService::default(),
            include_bytes!("../../../../tests/fixtures/notebooks/compatibility.ipynb").to_vec(),
        )
    }
    fn begin(service: &mut NotebookService, project: &Project, bytes: &[u8]) -> ImportId {
        service
            .begin_import(
                project,
                "import.ipynb",
                bytes.len() as u64,
                &format!("{:x}", Sha256::digest(bytes)),
            )
            .unwrap()
            .import_id
    }

    #[test]
    fn ordered_chunks_preserve_notebook_and_publish_only_at_finish() {
        let (dir, project, mut service, bytes) = fixture();
        let id = begin(&mut service, &project, &bytes);
        for (index, chunk) in bytes.chunks(123).enumerate() {
            service
                .import_chunk(&project, id, (index * 123) as u64, chunk)
                .unwrap();
            assert!(!dir.0.join("import.ipynb").exists());
        }
        let snapshot = service.finish_import(&project, id).unwrap();
        assert_eq!(snapshot.notebook, Notebook::parse(&bytes).unwrap());
        assert_eq!(
            Notebook::parse(&std::fs::read(dir.0.join("import.ipynb")).unwrap()).unwrap(),
            snapshot.notebook
        );
        assert!(service.imports.is_empty());
    }

    #[test]
    fn invalid_chunks_hashes_and_incomplete_uploads_cancel_without_files() {
        let (dir, project, mut service, bytes) = fixture();
        for (offset, chunk) in [
            (1, vec![1]),
            (0, vec![]),
            (0, vec![1; CHUNK_BYTES + 1]),
            (0, vec![1; bytes.len() + 1]),
        ] {
            let id = begin(&mut service, &project, &bytes);
            assert!(service.import_chunk(&project, id, offset, &chunk).is_err());
            assert!(service.imports.is_empty());
        }
        let id = begin(&mut service, &project, &bytes);
        service.import_chunk(&project, id, 0, &bytes).unwrap();
        assert!(service.import_chunk(&project, id, 0, &bytes[..1]).is_err());
        let id = begin(&mut service, &project, &bytes);
        assert!(service.finish_import(&project, id).is_err());
        let id = begin(&mut service, &project, &bytes);
        let wrong = vec![b' '; bytes.len()];
        service.import_chunk(&project, id, 0, &wrong).unwrap();
        assert!(service.finish_import(&project, id).is_err());
        let id = begin(&mut service, &project, b"malformed");
        service.import_chunk(&project, id, 0, b"malformed").unwrap();
        assert!(service.finish_import(&project, id).is_err());
        assert!(service.imports.is_empty());
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
    }

    #[test]
    fn owner_expiry_cancellation_limits_and_collision_are_enforced() {
        let (dir, project, mut service, bytes) = fixture();
        let id = begin(&mut service, &project, &bytes);
        let mut other = project.clone();
        other.project_id = ProjectId::new();
        assert!(matches!(
            service.cancel_import(&other, id),
            Err(NotebookError::NotFound)
        ));
        assert!(service.imports.contains_key(&id));
        service.imports.get_mut(&id).unwrap().expires = Instant::now();
        assert!(matches!(
            service.finish_import(&project, id),
            Err(NotebookError::NotFound)
        ));
        assert!(service.imports.is_empty());
        let id = begin(&mut service, &project, &bytes);
        service.cancel_import(&project, id).unwrap();
        for index in 0..MAX_IMPORTS {
            service
                .begin_import(&project, &format!("{index}.ipynb"), 1, &"0".repeat(64))
                .unwrap();
        }
        assert!(matches!(
            service.begin_import(&project, "extra.ipynb", 1, &"0".repeat(64)),
            Err(NotebookError::Limit)
        ));
        service.imports.clear();
        let id = begin(&mut service, &project, &bytes);
        service.import_chunk(&project, id, 0, &bytes).unwrap();
        std::fs::write(dir.0.join("import.ipynb"), b"external").unwrap();
        assert!(matches!(
            service.finish_import(&project, id),
            Err(NotebookError::AlreadyExists)
        ));
        assert_eq!(
            std::fs::read(dir.0.join("import.ipynb")).unwrap(),
            b"external"
        );
    }

    #[test]
    fn path_escape_and_parent_swap_during_upload_are_rejected() {
        let (dir, project, mut service, bytes) = fixture();
        let (outside, _, _, _) = fixture();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        for path in ["../escape.ipynb", "/escape.ipynb", "C:/escape.ipynb"] {
            assert!(matches!(
                service.begin_import(&project, path, bytes.len() as u64, &hash),
                Err(NotebookError::UnsafePath)
            ));
        }
        std::fs::create_dir(dir.0.join("target")).unwrap();
        let id = service
            .begin_import(&project, "target/a.ipynb", bytes.len() as u64, &hash)
            .unwrap()
            .import_id;
        service.import_chunk(&project, id, 0, &bytes).unwrap();
        std::fs::rename(dir.0.join("target"), dir.0.join("old-target")).unwrap();
        std::os::unix::fs::symlink(&outside.0, dir.0.join("target")).unwrap();
        assert!(matches!(
            service.finish_import(&project, id),
            Err(NotebookError::UnsafePath)
        ));
        assert!(!outside.0.join("a.ipynb").exists());
        assert_eq!(
            std::fs::read_dir(dir.0.join("old-target")).unwrap().count(),
            0
        );
    }

    #[test]
    fn declared_size_hash_and_reserved_storage_are_bounded() {
        let (_dir, project, mut service, bytes) = fixture();
        for size in [0, MAX_NOTEBOOK_BYTES as u64 + 1, u64::MAX] {
            assert!(matches!(
                service.begin_import(&project, "a.ipynb", size, &"0".repeat(64)),
                Err(NotebookError::Limit)
            ));
        }
        for hash in [String::new(), "abcd".into(), "g".repeat(64)] {
            assert!(matches!(
                service.begin_import(&project, "a.ipynb", 1, &hash),
                Err(NotebookError::Import(_))
            ));
        }
        let id = begin(&mut service, &project, &bytes);
        assert!(matches!(
            service.begin_import(&project, "import.ipynb", 1, &"0".repeat(64)),
            Err(NotebookError::AlreadyExists)
        ));
        service.cancel_import(&project, id).unwrap();
        for name in ["a.ipynb", "b.ipynb"] {
            service
                .begin_import(&project, name, MAX_NOTEBOOK_BYTES as u64, &"0".repeat(64))
                .unwrap();
        }
        assert!(matches!(
            service.begin_import(&project, "c.ipynb", 1, &"0".repeat(64)),
            Err(NotebookError::Limit)
        ));
    }
}
