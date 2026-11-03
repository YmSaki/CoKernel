#![cfg(target_os = "linux")]

use cokernel_domain::{NotebookId, Project, ProjectId};
use cokernel_runtime::notebook::{DocumentState, Edit, Notebook, NotebookError, NotebookService};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

struct Workspace {
    path: PathBuf,
    project: Project,
}
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ckdoc-{}", NotebookId::new()));
        fs::create_dir(&path).unwrap();
        let project = Project {
            project_id: ProjectId::new(),
            name: "test".into(),
            root_path: path.to_str().unwrap().into(),
            environment_generation: 1,
        };
        Self { path, project }
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn insert() -> Edit {
    Edit::InsertCell {
        index: 0,
        cell: json!({"id":"code","cell_type":"code","source":"x=123","metadata":{},"execution_count":null,"outputs":[]}),
    }
}
fn source(value: &str) -> Edit {
    Edit::SetCellSource {
        cell_id: "code".into(),
        source: value.into(),
    }
}

#[test]
fn cell_view_bounds_revision_output_and_unicode_without_changing_document() {
    use cokernel_runtime::notebook::view;
    let w = Workspace::new();
    let mut service = NotebookService::default();
    let created = service.create(&w.project, "view.ipynb").unwrap();
    let id = created.document.notebook_id;
    let edits: Vec<_>=(0..24).map(|i|Edit::InsertCell{index:i,cell:json!({"id":format!("c{i}"),"cell_type":"code","source":"変数\n".repeat(4000),"metadata":{},"execution_count":1,"outputs":[{"output_type":"display_data","data":{"image/png":"x".repeat(100_000)},"metadata":{}}]})}).collect();
    let snapshot = service.apply_edits(&w.project, id, 1, &edits).unwrap();
    let original = snapshot.notebook.to_bytes().unwrap();
    let page = view::cells(snapshot.clone(), Some(2), 0, 32, true).unwrap();
    assert!(serde_json::to_vec(&page).unwrap().len() < 70 * 1024);
    assert_eq!(page["total_cells"], 24);
    assert_eq!(page["cells"][0]["source_truncated"], true);
    assert_eq!(page["cells"][0]["outputs_truncated"], true);
    assert!(page["cells"][0]["outputs"].is_null());
    let next = page["next_offset"].as_u64().unwrap() as usize;
    assert!(next > 0 && next < 24);
    let second = view::cells(snapshot.clone(), Some(2), next, 1, false).unwrap();
    assert_eq!(second["cells"][0]["index"], next);
    assert_eq!(second["cells"][0]["outputs_truncated"], false);
    assert!(matches!(
        view::cells(snapshot.clone(), Some(1), 0, 1, false),
        Err(NotebookError::Conflict { .. })
    ));
    assert!(view::cells(snapshot.clone(), None, 0, 33, false).is_err());
    assert_eq!(
        service
            .get(&w.project, id)
            .unwrap()
            .notebook
            .to_bytes()
            .unwrap(),
        original
    );
}

#[test]
fn create_open_list_edit_save_round_trip_and_identity() {
    let w = Workspace::new();
    let mut service = NotebookService::default();
    let created = service.create(&w.project, "a.ipynb").unwrap();
    let id = created.document.notebook_id;
    assert_eq!(created.document.revision, 1);
    assert_eq!(created.state, DocumentState::Clean);
    assert_eq!(
        service
            .open(&w.project, "a.ipynb")
            .unwrap()
            .document
            .notebook_id,
        id
    );
    let edited = service.apply_edits(&w.project, id, 1, &[insert()]).unwrap();
    assert_eq!(edited.state, DocumentState::Dirty);
    assert_eq!(edited.document.revision, 2);
    assert_ne!(edited.content_hash, edited.saved_content_hash);
    assert!(
        Notebook::parse(&fs::read(w.path.join("a.ipynb")).unwrap())
            .unwrap()
            .value()["cells"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let saved = service.save(&w.project, id, 2).unwrap();
    assert_eq!(saved.state, DocumentState::Clean);
    assert_eq!(saved.document.revision, 2);
    assert_eq!(saved.content_hash, saved.saved_content_hash);
    let reopened = NotebookService::default()
        .open(&w.project, "a.ipynb")
        .unwrap();
    assert_eq!(reopened.notebook.cell_source("code").unwrap().1, "x=123");
    fs::create_dir(w.path.join("nested")).unwrap();
    fs::write(
        w.path.join("nested/imported.ipynb"),
        Notebook::default().to_bytes().unwrap(),
    )
    .unwrap();
    assert_eq!(
        service.list(&w.project).unwrap(),
        ["a.ipynb", "nested/imported.ipynb"]
    );
    assert!(matches!(
        service.create(&w.project, "a.ipynb"),
        Err(NotebookError::AlreadyExists)
    ));
}

#[test]
fn two_callers_cannot_overwrite_same_revision_or_partially_apply_invalid_batch() {
    let w = Workspace::new();
    let mut service = NotebookService::default();
    let id = service
        .create(&w.project, "a.ipynb")
        .unwrap()
        .document
        .notebook_id;
    service.apply_edits(&w.project, id, 1, &[insert()]).unwrap();
    service
        .apply_edits(&w.project, id, 2, &[source("human=1")])
        .unwrap();
    assert!(matches!(
        service.apply_edits(&w.project, id, 2, &[source("ai=2")]),
        Err(NotebookError::Conflict { current: 3 })
    ));
    assert!(matches!(
        service.save(&w.project, id, 2),
        Err(NotebookError::Conflict { current: 3 })
    ));
    assert!(
        service
            .apply_edits(
                &w.project,
                id,
                3,
                &[
                    source("wrong"),
                    Edit::DeleteCell {
                        cell_id: "missing".into()
                    }
                ]
            )
            .is_err()
    );
    let current = service.get(&w.project, id).unwrap();
    assert_eq!(current.document.revision, 3);
    assert_eq!(current.notebook.cell_source("code").unwrap().1, "human=1");
}

#[test]
fn external_change_blocks_save_preserves_dirty_edits_and_requires_explicit_reload() {
    let w = Workspace::new();
    let mut service = NotebookService::default();
    let id = service
        .create(&w.project, "a.ipynb")
        .unwrap()
        .document
        .notebook_id;
    service.apply_edits(&w.project, id, 1, &[insert()]).unwrap();
    let external = Notebook::default()
        .edited(&[Edit::SetNotebookMetadata {
            metadata: json!({"external":"keep"}).as_object().unwrap().clone(),
        }])
        .unwrap()
        .to_bytes()
        .unwrap();
    fs::write(w.path.join("a.ipynb"), &external).unwrap();
    assert!(matches!(
        service.save(&w.project, id, 2),
        Err(NotebookError::ExternalChange)
    ));
    let current = service.get(&w.project, id).unwrap();
    assert_eq!(current.state, DocumentState::ExternalChangeDetected);
    assert_eq!(current.notebook.cell_source("code").unwrap().1, "x=123");
    assert_eq!(fs::read(w.path.join("a.ipynb")).unwrap(), external);
    assert!(matches!(
        service.apply_edits(&w.project, id, 2, &[source("overwrite")]),
        Err(NotebookError::ExternalChange)
    ));
    let reloaded = service.reload(&w.project, id, 2).unwrap();
    assert_eq!(reloaded.document.revision, 3);
    assert_eq!(reloaded.state, DocumentState::Clean);
    assert_eq!(reloaded.notebook.value()["metadata"]["external"], "keep");
}

#[test]
fn identical_bytes_replaced_by_another_inode_are_still_an_external_change() {
    let w = Workspace::new();
    let mut service = NotebookService::default();
    let id = service
        .create(&w.project, "a.ipynb")
        .unwrap()
        .document
        .notebook_id;
    fs::copy(w.path.join("a.ipynb"), w.path.join("replacement")).unwrap();
    fs::rename(w.path.join("replacement"), w.path.join("a.ipynb")).unwrap();
    assert_eq!(
        service.get(&w.project, id).unwrap().state,
        DocumentState::ExternalChangeDetected
    );
}

#[test]
fn project_ownership_and_symlink_replacement_do_not_expose_or_overwrite_other_project() {
    let a = Workspace::new();
    let b = Workspace::new();
    let mut service = NotebookService::default();
    let id = service
        .create(&a.project, "a.ipynb")
        .unwrap()
        .document
        .notebook_id;
    assert!(matches!(
        service.get(&b.project, id),
        Err(NotebookError::NotFound)
    ));
    assert!(matches!(
        service.apply_edits(&b.project, id, 1, &[insert()]),
        Err(NotebookError::NotFound)
    ));
    service.apply_edits(&a.project, id, 1, &[insert()]).unwrap();
    fs::write(b.path.join("private.ipynb"), b"private").unwrap();
    fs::remove_file(a.path.join("a.ipynb")).unwrap();
    std::os::unix::fs::symlink(b.path.join("private.ipynb"), a.path.join("a.ipynb")).unwrap();
    assert!(matches!(
        service.save(&a.project, id, 2),
        Err(NotebookError::ExternalChange)
    ));
    assert_eq!(fs::read(b.path.join("private.ipynb")).unwrap(), b"private");
    std::os::unix::fs::symlink(&b.path, a.path.join("escape")).unwrap();
    assert!(service.create(&a.project, "escape/new.ipynb").is_err());
    assert!(!b.path.join("new.ipynb").exists());
    assert!(service.list(&a.project).unwrap().is_empty());
}

#[test]
fn legacy_open_marks_dirty_without_rewriting_or_choosing_imported_environment() {
    let w = Workspace::new();
    let mut service = NotebookService::default();
    let original = br#"{"nbformat":4,"nbformat_minor":0,"metadata":{"kernelspec":{"name":"alien","display_name":"Alien"}},"cells":[{"cell_type":"code","source":"x=1","metadata":{},"execution_count":null,"outputs":[]}]}"#;
    fs::write(w.path.join("legacy.ipynb"), original).unwrap();
    let snapshot = service.open(&w.project, "legacy.ipynb").unwrap();
    assert_eq!(snapshot.state, DocumentState::Dirty);
    assert_eq!(
        snapshot.notebook.value()["metadata"]["kernelspec"]["name"],
        "alien"
    );
    assert_eq!(fs::read(w.path.join("legacy.ipynb")).unwrap(), original);
    service
        .save(&w.project, snapshot.document.notebook_id, 1)
        .unwrap();
    let saved = Notebook::parse(&fs::read(w.path.join("legacy.ipynb")).unwrap()).unwrap();
    assert!(saved.value()["cells"][0]["id"].is_string());
    assert_eq!(w.project.environment_generation, 1);
}
