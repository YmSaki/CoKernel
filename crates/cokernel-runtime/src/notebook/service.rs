use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::Path;

use cokernel_domain::{ImportId, NotebookDocumentRef, NotebookId, Project, ProjectId};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::NotebookOutputs;
use super::storage::{self, Commit, Fingerprint, Location};
use super::{Edit, Notebook, NotebookError};

const MAX_OPEN_DOCUMENTS: usize = 128;
const MAX_OPEN_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentState {
    Clean,
    Dirty,
    ExternalChangeDetected,
    SaveFailed,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentSnapshot {
    #[serde(flatten)]
    pub document: NotebookDocumentRef,
    pub state: DocumentState,
    pub content_hash: String,
    pub saved_content_hash: String,
    pub notebook: Notebook,
}

struct OpenDocument {
    snapshot: DocumentSnapshot,
    disk: Fingerprint,
    bytes: usize,
    source_generations: HashMap<String, u64>,
    execution_sequence: u64,
    applied_executions: HashMap<String, u64>,
}

/// Runtime-only token, never deserialized from a caller's request.
#[derive(Debug, Clone)]
pub(super) struct PreparedCell {
    pub notebook_id: NotebookId,
    pub cell_id: String,
    pub source: String,
    generation: u64,
    sequence: u64,
}

fn cell_generations(notebook: &Notebook, generation: u64) -> HashMap<String, u64> {
    notebook.value()["cells"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|cell| cell["id"].as_str().map(|id| (id.to_owned(), generation)))
        .collect()
}

fn cell_sources(notebook: &Notebook) -> HashMap<&str, (&serde_json::Value, &serde_json::Value)> {
    notebook.value()["cells"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|cell| {
            cell["id"]
                .as_str()
                .map(|id| (id, (&cell["cell_type"], &cell["source"])))
        })
        .collect()
}

struct ProjectRoot {
    path: String,
    directory: File,
}

/// One instance belongs to the Runtime domain service. Its exclusive mutable
/// methods serialize document mutations; Session execution has its own queue.
#[derive(Default)]
pub struct NotebookService {
    roots: HashMap<ProjectId, ProjectRoot>,
    documents: HashMap<NotebookId, OpenDocument>,
    pub(super) imports: HashMap<ImportId, super::import::PendingImport>,
    identities: NotebookIdentities,
}

/// Loaded identity only; never filesystem state, revisions or document contents.
/// Admission can reject a foreign notebook without waiting for storage work.
#[derive(Clone, Default)]
pub(super) struct NotebookIdentities(
    std::sync::Arc<std::sync::RwLock<HashMap<NotebookId, ProjectId>>>,
);
impl NotebookIdentities {
    pub(super) fn owns(&self, project: ProjectId, notebook: NotebookId) -> bool {
        self.0
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&notebook)
            == Some(&project)
    }
}

impl NotebookService {
    pub(super) fn identities(&self) -> NotebookIdentities {
        self.identities.clone()
    }
    /// Compact reconnect snapshot, without copying notebook cells or outputs.
    pub fn open_summaries(&self) -> Vec<serde_json::Value> {
        self.documents.values().map(|document| {
            let s=&document.snapshot;
            serde_json::json!({"document":s.document,"state":s.state,"content_hash":s.content_hash,"saved_content_hash":s.saved_content_hash})
        }).collect()
    }
    /// Flush all Runtime-owned documents, including ones whose Project was
    /// removed from the registry after its Session stopped. Roots remain owned.
    pub fn flush_all(&mut self) -> Vec<cokernel_domain::DomainError> {
        let documents: Vec<_> = self
            .documents
            .values()
            .map(|d| d.snapshot.document.clone())
            .collect();
        let mut errors = Vec::new();
        for document in documents {
            let Some(root) = self.roots.get(&document.project_id) else {
                errors.push(NotebookError::NotFound.domain_error());
                continue;
            };
            let project = Project {
                project_id: document.project_id,
                name: String::new(),
                root_path: root.path.clone(),
                environment_generation: 0,
            };
            if let Err(error) = self.save(&project, document.notebook_id, document.revision) {
                errors.push(error.domain_error());
            }
        }
        errors
    }

    fn root(&mut self, project: &Project) -> Result<&File, NotebookError> {
        if let Some(root) = self.roots.get(&project.project_id) {
            if root.path != project.root_path {
                return Err(NotebookError::UnsafePath);
            }
        } else {
            if self.roots.len() >= 1024 {
                return Err(NotebookError::Limit);
            }
            self.roots.insert(
                project.project_id,
                ProjectRoot {
                    path: project.root_path.clone(),
                    directory: storage::open_root(Path::new(&project.root_path))?,
                },
            );
        }
        self.roots
            .get(&project.project_id)
            .map(|root| &root.directory)
            .ok_or(NotebookError::NotFound)
    }

    pub(super) fn location(
        &mut self,
        project: &Project,
        path: &str,
    ) -> Result<Location, NotebookError> {
        Location::resolve(self.root(project)?, path)
    }

    fn document(&self, project: ProjectId, id: NotebookId) -> Result<&OpenDocument, NotebookError> {
        self.documents
            .get(&id)
            .filter(|d| d.snapshot.document.project_id == project)
            .ok_or(NotebookError::NotFound)
    }

    fn capacity(&self, bytes: usize, replacing: Option<NotebookId>) -> Result<(), NotebookError> {
        let others: usize = self
            .documents
            .iter()
            .filter(|(id, _)| Some(**id) != replacing)
            .map(|(_, d)| d.bytes)
            .sum();
        if (replacing.is_none() && self.documents.len() >= MAX_OPEN_DOCUMENTS)
            || others.saturating_add(bytes) > MAX_OPEN_BYTES
        {
            return Err(NotebookError::Limit);
        }
        Ok(())
    }

    pub fn list(&mut self, project: &Project) -> Result<Vec<String>, NotebookError> {
        storage::discover(self.root(project)?)
    }

    pub fn create(
        &mut self,
        project: &Project,
        path: &str,
    ) -> Result<DocumentSnapshot, NotebookError> {
        self.install_notebook(project, path, Notebook::default())
    }

    pub(super) fn install_notebook(
        &mut self,
        project: &Project,
        path: &str,
        notebook: Notebook,
    ) -> Result<DocumentSnapshot, NotebookError> {
        // A file removed externally may still have a live document/Session.
        // Do not create a second identity over that unresolved document.
        if self.documents.values().any(|d| {
            d.snapshot.document.project_id == project.project_id
                && d.snapshot.document.relative_path == path
        }) {
            return Err(NotebookError::AlreadyExists);
        }
        let bytes = notebook.to_bytes()?;
        self.capacity(bytes.len(), None)?;
        let location = self.location(project, path)?;
        let commit = location.write(&bytes, None)?;
        let snapshot = self.register(
            project,
            path,
            notebook,
            commit.fingerprint,
            bytes.len(),
            DocumentState::Clean,
        );
        if let Some(error) = commit.sync_error {
            self.documents
                .get_mut(&snapshot.document.notebook_id)
                .ok_or(NotebookError::NotFound)?
                .snapshot
                .state = DocumentState::SaveFailed;
            return Err(NotebookError::Durability(error));
        }
        Ok(snapshot)
    }

    pub fn open(
        &mut self,
        project: &Project,
        path: &str,
    ) -> Result<DocumentSnapshot, NotebookError> {
        storage::validate_path(path)?;
        if let Some(id) = self
            .documents
            .values()
            .find(|d| {
                d.snapshot.document.project_id == project.project_id
                    && d.snapshot.document.relative_path == path
            })
            .map(|d| d.snapshot.document.notebook_id)
        {
            return self.get(project, id);
        }
        let (bytes, disk) = self.location(project, path)?.read()?;
        let notebook = Notebook::parse(&bytes)?;
        let normalized = notebook.to_bytes()?;
        self.capacity(normalized.len(), None)?;
        // Whitespace changes alone do not dirty a document. Legacy ID/version
        // normalization does, and will be persisted by the ordinary save path.
        let original: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| NotebookError::Invalid(e.to_string()))?;
        let state = if notebook.value() == &original {
            DocumentState::Clean
        } else {
            DocumentState::Dirty
        };
        Ok(self.register(project, path, notebook, disk, normalized.len(), state))
    }

    fn register(
        &mut self,
        project: &Project,
        path: &str,
        notebook: Notebook,
        disk: Fingerprint,
        bytes: usize,
        state: DocumentState,
    ) -> DocumentSnapshot {
        let id = NotebookId::new();
        let snapshot = DocumentSnapshot {
            document: NotebookDocumentRef {
                notebook_id: id,
                project_id: project.project_id,
                relative_path: path.into(),
                revision: 1,
            },
            state,
            content_hash: document_hash(&notebook),
            saved_content_hash: disk.hash.clone(),
            notebook,
        };
        self.documents.insert(
            id,
            OpenDocument {
                source_generations: cell_generations(&snapshot.notebook, 1),
                execution_sequence: 0,
                applied_executions: HashMap::new(),
                snapshot: snapshot.clone(),
                disk,
                bytes,
            },
        );
        self.identities
            .0
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, project.project_id);
        snapshot
    }

    fn check_disk(&mut self, project: &Project, id: NotebookId) -> Result<(), NotebookError> {
        let document = self.document(project.project_id, id)?;
        if document.snapshot.state == DocumentState::ExternalChangeDetected {
            return Err(NotebookError::ExternalChange);
        }
        let path = document.snapshot.document.relative_path.clone();
        let expected = document.disk.clone();
        let result = self
            .location(project, &path)
            .and_then(|location| location.read());
        match result {
            Ok((_, disk)) if disk == expected => Ok(()),
            Ok(_)
            | Err(
                NotebookError::NotFound | NotebookError::UnsafePath | NotebookError::ExternalChange,
            ) => {
                self.documents
                    .get_mut(&id)
                    .ok_or(NotebookError::NotFound)?
                    .snapshot
                    .state = DocumentState::ExternalChangeDetected;
                Err(NotebookError::ExternalChange)
            }
            Err(error) => Err(error),
        }
    }

    pub fn get(
        &mut self,
        project: &Project,
        id: NotebookId,
    ) -> Result<DocumentSnapshot, NotebookError> {
        match self.check_disk(project, id) {
            Ok(()) | Err(NotebookError::ExternalChange) => {
                Ok(self.document(project.project_id, id)?.snapshot.clone())
            }
            Err(error) => Err(error),
        }
    }

    fn revision(
        &self,
        project: ProjectId,
        id: NotebookId,
        expected: u64,
    ) -> Result<(), NotebookError> {
        let current = self.document(project, id)?.snapshot.document.revision;
        if current != expected {
            return Err(NotebookError::Conflict { current });
        }
        Ok(())
    }

    pub fn apply_edits(
        &mut self,
        project: &Project,
        id: NotebookId,
        expected: u64,
        edits: &[Edit],
    ) -> Result<DocumentSnapshot, NotebookError> {
        self.revision(project.project_id, id, expected)?;
        self.check_disk(project, id)?;
        let candidate = self
            .document(project.project_id, id)?
            .snapshot
            .notebook
            .edited(edits)?;
        let bytes = candidate.to_bytes()?;
        self.capacity(bytes.len(), Some(id))?;
        let next_revision = expected.checked_add(1).ok_or(NotebookError::Limit)?;
        let document = self.documents.get_mut(&id).ok_or(NotebookError::NotFound)?;
        let mut generations = cell_generations(&candidate, next_revision);
        let before = cell_sources(&document.snapshot.notebook);
        let after = cell_sources(&candidate);
        let touched: HashSet<&str> = edits
            .iter()
            .filter_map(|edit| match edit {
                Edit::SetCellSource { cell_id, .. } | Edit::DeleteCell { cell_id } => {
                    Some(cell_id.as_str())
                }
                Edit::InsertCell { cell, .. } => cell["id"].as_str(),
                _ => None,
            })
            .collect();
        for (cell, generation) in &mut generations {
            if !touched.contains(cell.as_str())
                && before.get(cell.as_str()) == after.get(cell.as_str())
                && let Some(old) = document.source_generations.get(cell)
            {
                *generation = *old;
            }
        }
        document.source_generations = generations;
        document
            .applied_executions
            .retain(|cell, _| document.source_generations.contains_key(cell));
        document.snapshot.notebook = candidate;
        document.snapshot.content_hash = format!("{:x}", Sha256::digest(&bytes));
        document.snapshot.document.revision = next_revision;
        document.snapshot.state = DocumentState::Dirty;
        document.bytes = bytes.len();
        Ok(document.snapshot.clone())
    }

    pub fn apply_outputs(
        &mut self,
        project: &Project,
        id: NotebookId,
        expected: u64,
        cell_id: &str,
        outputs: &NotebookOutputs,
    ) -> Result<DocumentSnapshot, NotebookError> {
        self.revision(project.project_id, id, expected)?;
        self.check_disk(project, id)?;
        let candidate = self
            .document(project.project_id, id)?
            .snapshot
            .notebook
            .with_outputs(cell_id, outputs)?;
        let bytes = candidate.to_bytes()?;
        self.capacity(bytes.len(), Some(id))?;
        let next = expected.checked_add(1).ok_or(NotebookError::Limit)?;
        let document = self.documents.get_mut(&id).ok_or(NotebookError::NotFound)?;
        document.snapshot.notebook = candidate;
        document.snapshot.content_hash = format!("{:x}", Sha256::digest(&bytes));
        document.snapshot.document.revision = next;
        document.snapshot.state = DocumentState::Dirty;
        document.bytes = bytes.len();
        Ok(document.snapshot.clone())
    }

    pub(super) fn prepare_cell(
        &mut self,
        project: &Project,
        id: NotebookId,
        expected: u64,
        cell_id: &str,
    ) -> Result<PreparedCell, NotebookError> {
        self.revision(project.project_id, id, expected)?;
        self.check_disk(project, id)?;
        let document = self.documents.get_mut(&id).ok_or(NotebookError::NotFound)?;
        let (kind, source) = document.snapshot.notebook.cell_source(cell_id)?;
        if kind != super::CellKind::Code {
            return Err(NotebookError::Invalid("only code cells can execute".into()));
        }
        let generation = *document
            .source_generations
            .get(cell_id)
            .ok_or(NotebookError::NotFound)?;
        document.execution_sequence = document
            .execution_sequence
            .checked_add(1)
            .ok_or(NotebookError::Limit)?;
        Ok(PreparedCell {
            notebook_id: id,
            cell_id: cell_id.into(),
            source,
            generation,
            sequence: document.execution_sequence,
        })
    }

    pub(super) fn commit_execution(
        &mut self,
        project: &Project,
        cell: &PreparedCell,
        outputs: &NotebookOutputs,
    ) -> Result<DocumentSnapshot, NotebookError> {
        let document = self.document(project.project_id, cell.notebook_id)?;
        if document.source_generations.get(&cell.cell_id) != Some(&cell.generation)
            || document
                .snapshot
                .notebook
                .cell_source(&cell.cell_id)
                .ok()
                .map(|(_, s)| s)
                .as_ref()
                != Some(&cell.source)
            || document
                .applied_executions
                .get(&cell.cell_id)
                .is_some_and(|sequence| *sequence >= cell.sequence)
        {
            return Err(NotebookError::StaleExecution);
        }
        let revision = document.snapshot.document.revision;
        let changed =
            self.apply_outputs(project, cell.notebook_id, revision, &cell.cell_id, outputs)?;
        self.documents
            .get_mut(&cell.notebook_id)
            .ok_or(NotebookError::NotFound)?
            .applied_executions
            .insert(cell.cell_id.clone(), cell.sequence);
        self.save(project, cell.notebook_id, changed.document.revision)
    }

    pub fn close(
        &mut self,
        project: &Project,
        id: NotebookId,
        expected: u64,
    ) -> Result<DocumentSnapshot, NotebookError> {
        // Closing a view flushes edits but retains identity and live Session
        // association. A failed flush is returned for explicit UI reconciliation.
        self.save(project, id, expected)
    }

    pub fn save(
        &mut self,
        project: &Project,
        id: NotebookId,
        expected: u64,
    ) -> Result<DocumentSnapshot, NotebookError> {
        self.save_with(project, id, expected, |location, bytes, disk| {
            location.write(bytes, Some(disk))
        })
    }

    fn save_with(
        &mut self,
        project: &Project,
        id: NotebookId,
        expected: u64,
        writer: impl FnOnce(&Location, &[u8], &Fingerprint) -> Result<Commit, NotebookError>,
    ) -> Result<DocumentSnapshot, NotebookError> {
        self.revision(project.project_id, id, expected)?;
        self.check_disk(project, id)?;
        let document = self.document(project.project_id, id)?;
        if document.snapshot.state == DocumentState::Clean {
            return Ok(document.snapshot.clone());
        }
        let bytes = document.snapshot.notebook.to_bytes()?;
        let path = document.snapshot.document.relative_path.clone();
        let disk = document.disk.clone();
        let result = self
            .location(project, &path)
            .and_then(|location| writer(&location, &bytes, &disk));
        let document = self.documents.get_mut(&id).ok_or(NotebookError::NotFound)?;
        match result {
            Ok(commit) => {
                document.snapshot.saved_content_hash = commit.fingerprint.hash.clone();
                document.disk = commit.fingerprint;
                if let Some(error) = commit.sync_error {
                    document.snapshot.state = DocumentState::SaveFailed;
                    return Err(NotebookError::Durability(error));
                }
                document.snapshot.state = DocumentState::Clean;
                Ok(document.snapshot.clone())
            }
            Err(error) => {
                document.snapshot.state = if matches!(error, NotebookError::ExternalChange) {
                    DocumentState::ExternalChangeDetected
                } else {
                    DocumentState::SaveFailed
                };
                Err(error)
            }
        }
    }

    /// Explicit reconciliation. Calling this discards unsaved document edits;
    /// the caller must present that consequence. It never touches Session memory.
    pub fn reload(
        &mut self,
        project: &Project,
        id: NotebookId,
        expected: u64,
    ) -> Result<DocumentSnapshot, NotebookError> {
        self.revision(project.project_id, id, expected)?;
        let path = self
            .document(project.project_id, id)?
            .snapshot
            .document
            .relative_path
            .clone();
        let (bytes, disk) = self.location(project, &path)?.read()?;
        let notebook = Notebook::parse(&bytes)?;
        let size = notebook.to_bytes()?.len();
        self.capacity(size, Some(id))?;
        let next_revision = expected.checked_add(1).ok_or(NotebookError::Limit)?;
        let original: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| NotebookError::Invalid(e.to_string()))?;
        let state = if notebook.value() == &original {
            DocumentState::Clean
        } else {
            DocumentState::Dirty
        };
        let document = self.documents.get_mut(&id).ok_or(NotebookError::NotFound)?;
        document.snapshot.content_hash = document_hash(&notebook);
        document.source_generations = cell_generations(&notebook, next_revision);
        document.applied_executions.clear();
        document.snapshot.notebook = notebook;
        document.snapshot.document.revision = next_revision;
        document.snapshot.saved_content_hash = disk.hash.clone();
        document.snapshot.state = state;
        document.disk = disk;
        document.bytes = size;
        Ok(document.snapshot.clone())
    }
}

fn document_hash(notebook: &Notebook) -> String {
    // Streaming avoids another allocation and cannot exceed an already validated
    // document's bound. sha2's digest input uses the same compact JSON + newline.
    struct HashWriter(Sha256);
    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(Sha256::new());
    // Notebook only contains JSON Values; this writer never returns an I/O error.
    let _ = serde_json::to_writer(&mut writer, notebook);
    writer.0.update(b"\n");
    format!("{:x}", writer.0.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> (Directory, Project, NotebookService, NotebookId) {
        let directory =
            Directory(std::env::temp_dir().join(format!("cksave-{}", NotebookId::new())));
        std::fs::create_dir(&directory.0).unwrap();
        let project = Project {
            project_id: ProjectId::new(),
            name: "save".into(),
            root_path: directory.0.to_str().unwrap().into(),
            environment_generation: 1,
        };
        let mut service = NotebookService::default();
        let id = service
            .create(&project, "a.ipynb")
            .unwrap()
            .document
            .notebook_id;
        service
            .apply_edits(
                &project,
                id,
                1,
                &[Edit::SetNotebookMetadata {
                    metadata: json!({"unsaved":123}).as_object().unwrap().clone(),
                }],
            )
            .unwrap();
        (directory, project, service, id)
    }

    #[test]
    fn older_completion_cannot_replace_a_newer_result_for_the_same_source() {
        use cokernel_domain::OperationId;
        use cokernel_protocol::worker::WorkerFrame;
        let (_directory, project, mut service, id) = fixture();
        service.apply_edits(&project, id, 2, &[Edit::InsertCell {
            index: 0,
            cell: json!({"id":"a","cell_type":"code","source":"123","metadata":{},"execution_count":null,"outputs":[]}),
        }]).unwrap();
        let older = service.prepare_cell(&project, id, 3, "a").unwrap();
        let newer = service.prepare_cell(&project, id, 3, "a").unwrap();
        let op = OperationId::new();
        let summary = json!({"operation_id":op,"status":"SUCCEEDED","execution_count":2,"output_count":0,"output_truncated":false,"output_omitted_bytes":0,"output_truncation_reasons":[]});
        let event = |name: &str, payload| WorkerFrame::Event {
            protocol: 1,
            session_id: "session".into(),
            event: name.into(),
            payload,
        };
        let outputs = NotebookOutputs::from_frames(
            op,
            &[
                event("execution_started", json!({"operation_id":op})),
                event("execution_finished", summary.clone()),
                WorkerFrame::Response {
                    protocol: 1,
                    session_id: "session".into(),
                    id: op.to_string(),
                    ok: true,
                    result: Some(summary),
                    error: None,
                },
            ],
        )
        .unwrap();
        let saved = service
            .commit_execution(&project, &newer, &outputs)
            .unwrap();
        assert!(matches!(
            service.commit_execution(&project, &older, &outputs),
            Err(NotebookError::StaleExecution)
        ));
        assert!(matches!(
            service.commit_execution(&project, &newer, &outputs),
            Err(NotebookError::StaleExecution)
        ));
        let current = service.get(&project, id).unwrap();
        assert_eq!(current.document.revision, saved.document.revision);
        assert_eq!(current.saved_content_hash, saved.saved_content_hash);
        assert_eq!(current.notebook.value()["cells"][0]["execution_count"], 2);
    }

    #[test]
    fn io_failure_keeps_edits_dirty_and_retry_saves_same_revision() {
        let (directory, project, mut service, id) = fixture();
        let original = std::fs::read(directory.0.join("a.ipynb")).unwrap();
        let result = service.save_with(&project, id, 2, |_, _, _| {
            Err(std::io::Error::other("injected disk failure").into())
        });
        assert!(matches!(result, Err(NotebookError::Io(_))));
        let failed = service.get(&project, id).unwrap();
        assert_eq!(failed.state, DocumentState::SaveFailed);
        assert_eq!(failed.document.revision, 2);
        assert_eq!(failed.notebook.value()["metadata"]["unsaved"], 123);
        assert_eq!(
            std::fs::read(directory.0.join("a.ipynb")).unwrap(),
            original
        );
        assert_eq!(
            service.save(&project, id, 2).unwrap().state,
            DocumentState::Clean
        );
    }

    #[test]
    fn directory_sync_failure_is_visible_and_retry_uses_installed_identity() {
        let (directory, project, mut service, id) = fixture();
        let result = service.save_with(&project, id, 2, |location, bytes, disk| {
            let mut commit = location.write(bytes, Some(disk))?;
            commit.sync_error = Some(std::io::Error::other("injected directory sync failure"));
            Ok(commit)
        });
        assert!(matches!(result, Err(NotebookError::Durability(_))));
        let failed = service.get(&project, id).unwrap();
        assert_eq!(failed.state, DocumentState::SaveFailed);
        assert_eq!(failed.content_hash, failed.saved_content_hash);
        assert_eq!(
            Notebook::parse(&std::fs::read(directory.0.join("a.ipynb")).unwrap())
                .unwrap()
                .value()["metadata"]["unsaved"],
            123
        );
        assert_eq!(
            service.save(&project, id, 2).unwrap().state,
            DocumentState::Clean
        );
    }
}
