//! Bounded, immutable diagnostics owned by Runtime, independent of event listeners.
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cokernel_domain::{FailureId, FailureRecord, SessionId};
use serde::{Deserialize, Serialize};

pub const MAX_FAILURE_RECORD_BYTES: usize = 1_048_576;
pub const MAX_FAILURE_RECORDS: usize = 64;
const MAX_DIRECTORY_ENTRIES: usize = MAX_FAILURE_RECORDS * 2 + 4;

#[derive(Clone)]
pub struct FailureStore {
    directory: PathBuf,
    mutation: Arc<Mutex<()>>,
}

#[derive(Serialize, Deserialize)]
struct StoredFailure {
    schema_version: u32,
    record: FailureRecord,
}

impl FailureStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            mutation: Arc::new(Mutex::new(())),
        }
    }

    pub fn persist(&self, record: &FailureRecord) -> io::Result<()> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| io::Error::other("failure store lock poisoned"))?;
        let bytes = serde_json::to_vec(&StoredFailure {
            schema_version: 1,
            record: record.clone(),
        })?;
        if bytes.len() > MAX_FAILURE_RECORD_BYTES {
            return Err(invalid("failure record exceeds storage budget"));
        }
        self.prepare_directory()?;
        // Validate existing evidence before retention can remove anything.
        let mut existing = self.read_all()?;
        let destination = self.path(record.failure_id);
        let temporary = self.directory.join(format!("{}.tmp", FailureId::new()));
        let result: io::Result<()> = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            // Atomic create without replacing an immutable record of the same ID.
            fs::hard_link(&temporary, &destination)?;
            existing.sort_by_key(|item| (item.timestamp, item.failure_id.to_string()));
            // Always retain the record just written, even if the wall clock
            // moved backwards since the older failures were recorded.
            let excess = (existing.len() + 1).saturating_sub(MAX_FAILURE_RECORDS);
            for old in existing.iter().take(excess) {
                fs::remove_file(self.path(old.failure_id))?;
            }
            Ok(())
        })();
        let cleanup = fs::remove_file(&temporary);
        result?;
        cleanup?;
        #[cfg(unix)]
        fs::File::open(&self.directory)?.sync_all()?;
        Ok(())
    }

    pub fn get(&self, id: FailureId) -> io::Result<Option<FailureRecord>> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| io::Error::other("failure store lock poisoned"))?;
        if !self.check_directory()? {
            return Ok(None);
        }
        self.read_record(id)
    }

    pub fn for_session(&self, session_id: SessionId) -> io::Result<Vec<FailureRecord>> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| io::Error::other("failure store lock poisoned"))?;
        if !self.check_directory()? {
            return Ok(Vec::new());
        }
        let mut records = self.read_all()?;
        records.retain(|record| record.session_id == Some(session_id));
        records.sort_by_key(|item| (item.timestamp, item.failure_id.to_string()));
        Ok(records)
    }

    fn path(&self, id: FailureId) -> PathBuf {
        self.directory.join(format!("{id}.json"))
    }

    fn check_directory(&self) -> io::Result<bool> {
        match fs::symlink_metadata(&self.directory) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(true),
            Ok(_) => Err(invalid("failure store path must be a real directory")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn prepare_directory(&self) -> io::Result<()> {
        if !self.check_directory()? {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(&self.directory)?;
            }
            #[cfg(not(unix))]
            fs::create_dir_all(&self.directory)?;
        }
        self.check_directory()?;
        Ok(())
    }

    fn read_all(&self) -> io::Result<Vec<FailureRecord>> {
        let mut records = Vec::new();
        for (index, entry) in fs::read_dir(&self.directory)?.enumerate() {
            if index >= MAX_DIRECTORY_ENTRIES {
                return Err(invalid("failure directory exceeds entry budget"));
            }
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|name| name.to_str())
                .and_then(|name| name.parse::<FailureId>().ok())
                .ok_or_else(|| invalid("invalid failure record filename"))?;
            if let Some(record) = self.read_record(id)? {
                records.push(record);
            }
        }
        Ok(records)
    }

    fn read_record(&self, id: FailureId) -> io::Result<Option<FailureRecord>> {
        let path = self.path(id);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid("failure record must be a regular file"));
        }
        if metadata.len() > MAX_FAILURE_RECORD_BYTES as u64 {
            return Err(invalid("stored failure exceeds read budget"));
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(path)?;
        if !file.metadata()?.is_file() {
            return Err(invalid("failure record changed file type"));
        }
        let mut bytes = Vec::new();
        file.take((MAX_FAILURE_RECORD_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_FAILURE_RECORD_BYTES {
            return Err(invalid("stored failure exceeds read budget"));
        }
        let stored: StoredFailure = serde_json::from_slice(&bytes)?;
        if stored.schema_version != 1 || stored.record.failure_id != id {
            return Err(invalid("failure record schema or identity mismatch"));
        }
        Ok(Some(stored.record))
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cokernel_domain::FailureClassification;

    struct TestStore(PathBuf);
    impl TestStore {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("ckfail-{}", FailureId::new())))
        }
        fn open(&self) -> FailureStore {
            FailureStore::new(&self.0)
        }
    }
    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn record(session: SessionId) -> FailureRecord {
        FailureRecord {
            failure_id: FailureId::new(),
            component: "session-worker".into(),
            session_id: Some(session),
            operation_id: None,
            timestamp: Utc::now(),
            exit_code: Some(23),
            signal: None,
            last_stderr: "evidence".into(),
            worker_pid: Some(123),
            worker_generation: Some(1),
            worker_started_at: None,
            environment_generation: Some(1),
            cell_id: Some("cell-1".into()),
            worker_memory_snapshot: None,
            wsl_memory_snapshot: None,
            linux_oom_evidence: None,
            runtime_event_context: None,
            classification: FailureClassification::Unknown,
            confidence: 0.0,
        }
    }

    #[test]
    fn evidence_survives_reopen_and_cannot_be_overwritten() {
        let temp = TestStore::new();
        let record = record(SessionId::new());
        temp.open().persist(&record).unwrap();
        let restored = temp.open().get(record.failure_id).unwrap().unwrap();
        assert_eq!(restored.exit_code, Some(23));
        assert_eq!(restored.cell_id, record.cell_id);
        assert_eq!(
            temp.open().persist(&record).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            temp.open()
                .get(record.failure_id)
                .unwrap()
                .unwrap()
                .last_stderr,
            "evidence"
        );
    }

    #[test]
    fn retention_is_bounded_and_sessions_are_filtered() {
        let temp = TestStore::new();
        let store = temp.open();
        let session = SessionId::new();
        let first = record(session);
        store.persist(&first).unwrap();
        for _ in 0..MAX_FAILURE_RECORDS {
            store.persist(&record(session)).unwrap();
        }
        assert!(store.get(first.failure_id).unwrap().is_none());
        assert_eq!(
            store.for_session(session).unwrap().len(),
            MAX_FAILURE_RECORDS
        );
        assert!(store.for_session(SessionId::new()).unwrap().is_empty());
    }

    #[test]
    fn oversized_and_corrupt_records_fail_explicitly() {
        let temp = TestStore::new();
        let store = temp.open();
        let mut record = record(SessionId::new());
        record.last_stderr = "x".repeat(MAX_FAILURE_RECORD_BYTES);
        assert!(store.persist(&record).is_err());
        assert!(!temp.0.exists());
        record.last_stderr.clear();
        store.persist(&record).unwrap();
        fs::write(store.path(record.failure_id), "broken").unwrap();
        assert!(store.get(record.failure_id).is_err());
        fs::write(
            store.path(record.failure_id),
            vec![b' '; MAX_FAILURE_RECORD_BYTES + 1],
        )
        .unwrap();
        assert!(store.get(record.failure_id).is_err());
    }

    #[test]
    fn missing_store_is_empty_but_invalid_directory_is_an_error() {
        let temp = TestStore::new();
        let store = temp.open();
        assert!(store.for_session(SessionId::new()).unwrap().is_empty());
        fs::write(&temp.0, "not a directory").unwrap();
        assert!(store.persist(&record(SessionId::new())).is_err());
        fs::remove_file(&temp.0).unwrap();
    }

    #[test]
    fn changed_schema_and_record_identity_are_rejected() {
        let temp = TestStore::new();
        let store = temp.open();
        let record = record(SessionId::new());
        store.persist(&record).unwrap();
        let path = store.path(record.failure_id);
        let mut stored: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        stored["schema_version"] = serde_json::json!(2);
        fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
        assert!(store.get(record.failure_id).is_err());
        stored["schema_version"] = serde_json::json!(1);
        stored["record"]["failure_id"] = serde_json::json!(FailureId::new());
        fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
        assert!(store.get(record.failure_id).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_records_are_rejected_and_permissions_are_private() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let temp = TestStore::new();
        let store = temp.open();
        let first = record(SessionId::new());
        store.persist(&first).unwrap();
        assert_eq!(
            fs::metadata(&temp.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(store.path(first.failure_id))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let other = FailureId::new();
        symlink(store.path(first.failure_id), store.path(other)).unwrap();
        assert!(store.get(other).is_err());
    }
}
