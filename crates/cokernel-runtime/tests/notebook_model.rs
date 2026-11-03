use cokernel_runtime::notebook::{CellKind, Edit, MAX_NOTEBOOK_BYTES, Notebook, NotebookError};
use serde_json::{Value, json};

const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/notebooks/compatibility.ipynb");

#[test]
fn round_trip_keeps_metadata_attachments_sources_and_all_outputs() {
    let notebook = Notebook::parse(FIXTURE).unwrap();
    let original: Value = serde_json::from_slice(FIXTURE).unwrap();
    assert_eq!(notebook.value(), &original);
    let reparsed = Notebook::parse(&notebook.to_bytes().unwrap()).unwrap();
    assert_eq!(reparsed, notebook);
    assert_eq!(
        notebook.cell_source("compute").unwrap(),
        (CellKind::Code, "x = 123\nx + 1".into())
    );
}

#[test]
fn legacy_ids_are_generated_once_and_existing_ids_preserved() {
    let value = json!({"nbformat":4,"nbformat_minor":0,"metadata":{},"cells":[
        {"cell_type":"markdown","metadata":{},"source":"legacy"},
        {"id":"keep_me","cell_type":"raw","metadata":{},"source":[]}
    ]});
    let notebook = Notebook::from_value(value).unwrap();
    assert_eq!(notebook.value()["nbformat_minor"], 5);
    assert_eq!(notebook.value()["cells"][1]["id"], "keep_me");
    assert!(notebook.value()["cells"][0]["id"].as_str().unwrap().len() <= 64);
    assert_eq!(
        Notebook::parse(&notebook.to_bytes().unwrap()).unwrap(),
        notebook
    );
}

#[test]
fn malformed_shape_versions_cells_outputs_and_ids_are_rejected() {
    let original = Notebook::parse(FIXTURE).unwrap().value().clone();
    for (pointer, replacement) in [
        ("/nbformat", json!(3)),
        ("/nbformat_minor", json!(6)),
        ("/metadata", json!([])),
        ("/cells", json!({})),
        ("/cells/0/id", json!("bad/id")),
        ("/cells/1/id", json!("intro")),
        ("/cells/0/source", json!([1])),
        ("/cells/1/execution_count", json!(-1)),
        ("/cells/1/outputs/0/name", json!("other")),
        ("/cells/1/outputs/2/data/image~1png", json!({})),
        ("/cells/1/outputs/3/execution_count", Value::Null),
        ("/cells/1/outputs/4/traceback", json!("bad")),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            Notebook::from_value(value).is_err(),
            "accepted malformed {pointer}"
        );
    }
}

#[test]
fn malformed_json_duplicate_keys_missing_ids_and_bounds_are_rejected() {
    for bytes in [
        b"{".as_slice(),
        br#"{"nbformat":4,"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#,
        br#"{"nbformat":4,"nbformat_minor":5,"metadata":{"x":1,"x":2},"cells":[]}"#,
    ] {
        assert!(Notebook::parse(bytes).is_err());
    }
    let mut value = Notebook::parse(FIXTURE).unwrap().value().clone();
    value["cells"][0].as_object_mut().unwrap().remove("id");
    assert!(Notebook::from_value(value).is_err());
    assert!(matches!(
        Notebook::parse(&vec![b' '; MAX_NOTEBOOK_BYTES + 1]),
        Err(NotebookError::Limit)
    ));
}

#[test]
fn edit_batch_is_atomic_and_retains_untouched_json() {
    let notebook = Notebook::parse(FIXTURE).unwrap();
    let first = Edit::SetCellSource {
        cell_id: "compute".into(),
        source: "x = 456".into(),
    };
    let edited = notebook.edited(std::slice::from_ref(&first)).unwrap();
    assert_eq!(edited.value()["metadata"], notebook.value()["metadata"]);
    assert_eq!(edited.value()["cells"][0], notebook.value()["cells"][0]);
    assert_eq!(
        edited.value()["cells"][1]["outputs"],
        notebook.value()["cells"][1]["outputs"]
    );
    assert!(
        notebook
            .edited(&[
                first,
                Edit::DeleteCell {
                    cell_id: "missing".into()
                }
            ])
            .is_err()
    );
    assert_eq!(notebook.cell_source("compute").unwrap().1, "x = 123\nx + 1");
}

#[test]
fn insert_move_delete_preserve_stable_ids_and_validate_indices() {
    let notebook = Notebook::default().edited(&[
        Edit::InsertCell { index: 0, cell: json!({"cell_type":"code","metadata":{},"source":"","execution_count":null,"outputs":[]}) },
        Edit::InsertCell { index: 1, cell: json!({"id":"end","cell_type":"markdown","metadata":{},"source":"end"}) },
    ]).unwrap();
    let id = notebook.value()["cells"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let moved = notebook
        .edited(&[Edit::MoveCell {
            cell_id: id.clone(),
            index: 1,
        }])
        .unwrap();
    assert_eq!(moved.value()["cells"][1]["id"], id);
    let deleted = moved
        .edited(&[Edit::DeleteCell {
            cell_id: "end".into(),
        }])
        .unwrap();
    assert_eq!(deleted.value()["cells"].as_array().unwrap().len(), 1);
    assert!(
        deleted
            .edited(&[Edit::MoveCell {
                cell_id: id,
                index: 2
            }])
            .is_err()
    );
}

#[test]
fn large_output_round_trips_and_count_limits_are_enforced() {
    let mut value = Notebook::parse(FIXTURE).unwrap().value().clone();
    value["cells"][1]["outputs"][0]["text"] = json!("x".repeat(2 * 1024 * 1024));
    let notebook = Notebook::from_value(value.clone()).unwrap();
    assert_eq!(
        Notebook::parse(&notebook.to_bytes().unwrap())
            .unwrap()
            .value(),
        &value
    );
    let cell = value["cells"][0].clone();
    value["cells"] = json!(vec![cell; 10_001]);
    assert!(matches!(
        Notebook::from_value(value),
        Err(NotebookError::Limit)
    ));
    let edits = vec![
        Edit::DeleteCell {
            cell_id: "intro".into()
        };
        1_001
    ];
    assert!(matches!(notebook.edited(&edits), Err(NotebookError::Limit)));
}
