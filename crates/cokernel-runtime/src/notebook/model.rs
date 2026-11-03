use std::collections::HashSet;

use cokernel_domain::{DomainError, NotebookId};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use thiserror::Error;

pub const MAX_NOTEBOOK_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_CELLS: usize = 10_000;
pub const MAX_EDITS: usize = 1_000;

#[derive(Debug, Error)]
pub enum NotebookError {
    #[error("invalid notebook: {0}")]
    Invalid(String),
    #[error("notebook exceeds the configured size/count limit")]
    Limit,
    #[error("unsupported notebook format: {0}")]
    Unsupported(String),
    #[error("notebook revision conflict; current revision is {current}")]
    Conflict { current: u64 },
    #[error("the executed cell changed, was replaced, or has a newer completed execution")]
    StaleExecution,
    #[error("notebook was changed outside CoKernel; reload or save as another file")]
    ExternalChange,
    #[error("notebook or cell not found")]
    NotFound,
    #[error("target notebook already exists")]
    AlreadyExists,
    #[error("notebook path must stay inside its Project without symlinks")]
    UnsafePath,
    #[error("notebook import transaction is invalid: {0}")]
    Import(String),
    #[error("notebook I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("notebook replacement is visible but directory durability was not confirmed: {0}")]
    Durability(std::io::Error),
}

impl NotebookError {
    pub fn domain_error(&self) -> DomainError {
        let suffix = match self {
            Self::Invalid(_) => "INVALID",
            Self::Limit => "LIMIT",
            Self::Unsupported(_) => "UNSUPPORTED",
            Self::Conflict { .. } => "CONFLICT",
            Self::StaleExecution => "STALE-EXECUTION",
            Self::ExternalChange => "EXTERNAL-CHANGE",
            Self::NotFound => "NOT-FOUND",
            Self::AlreadyExists => "EXISTS",
            Self::UnsafePath => "PATH",
            Self::Import(_) => "IMPORT",
            Self::Io(_) | Self::Durability(_) => "SAVE",
        };
        DomainError {
            code: format!("CK-NB-{suffix}"),
            component: "notebook".into(),
            summary: self.to_string(),
            action:
                "Inspect document status; reload or save as a new file when there is a conflict"
                    .into(),
            detail: match self {
                Self::Conflict { current } => {
                    Some(json!({"current_revision": current}).to_string())
                }
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Code,
    Markdown,
    Raw,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    SetCellSource {
        cell_id: String,
        source: String,
    },
    InsertCell {
        index: usize,
        cell: Value,
    },
    DeleteCell {
        cell_id: String,
    },
    MoveCell {
        cell_id: String,
        index: usize,
    },
    SetNotebookMetadata {
        metadata: Map<String, Value>,
    },
    SetCellMetadata {
        cell_id: String,
        metadata: Map<String, Value>,
    },
}

/// Keep the original JSON shape, including string-list sources and opaque
/// extension fields. All construction and mutation validates the whole candidate.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Notebook(Value);

impl Default for Notebook {
    fn default() -> Self {
        Self(json!({"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}))
    }
}

impl Notebook {
    pub fn parse(bytes: &[u8]) -> Result<Self, NotebookError> {
        if bytes.len() > MAX_NOTEBOOK_BYTES {
            return Err(NotebookError::Limit);
        }
        let value: UniqueValue = serde_json::from_slice(bytes)
            .map_err(|error| NotebookError::Invalid(error.to_string()))?;
        Self::from_value(value.0)
    }

    pub fn from_value(mut value: Value) -> Result<Self, NotebookError> {
        validate(&value)?;
        // v4.0..4.4 did not require cell IDs. Normalize to 4.5 once, without
        // changing any existing valid ID or opaque metadata.
        if value["nbformat_minor"]
            .as_u64()
            .is_some_and(|minor| minor < 5)
        {
            let mut used: HashSet<String> = value["cells"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|cell| cell["id"].as_str().map(str::to_owned))
                .collect();
            if let Some(cells) = value["cells"].as_array_mut() {
                for cell in cells {
                    if cell.get("id").is_none() {
                        let mut id = NotebookId::new().to_string();
                        while !used.insert(id.clone()) {
                            id = NotebookId::new().to_string();
                        }
                        cell["id"] = Value::String(id);
                    }
                }
            }
            value["nbformat_minor"] = json!(5);
        }
        let notebook = Self(value);
        notebook.to_bytes()?;
        Ok(notebook)
    }

    pub fn value(&self) -> &Value {
        &self.0
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, NotebookError> {
        let mut bytes = serde_json::to_vec(&self.0)
            .map_err(|error| NotebookError::Invalid(error.to_string()))?;
        bytes.push(b'\n');
        if bytes.len() > MAX_NOTEBOOK_BYTES {
            return Err(NotebookError::Limit);
        }
        Ok(bytes)
    }

    pub fn cell_source(&self, id: &str) -> Result<(CellKind, String), NotebookError> {
        let cell = &self.0["cells"][self.cell_index(id)?];
        let kind = match cell["cell_type"].as_str() {
            Some("code") => CellKind::Code,
            Some("markdown") => CellKind::Markdown,
            Some("raw") => CellKind::Raw,
            _ => return Err(NotebookError::Unsupported("cell type".into())),
        };
        Ok((kind, text(&cell["source"])?))
    }

    fn cell_index(&self, id: &str) -> Result<usize, NotebookError> {
        self.0["cells"]
            .as_array()
            .and_then(|cells| {
                cells
                    .iter()
                    .position(|cell| cell["id"].as_str() == Some(id))
            })
            .ok_or(NotebookError::NotFound)
    }

    pub fn edited(&self, edits: &[Edit]) -> Result<Self, NotebookError> {
        if edits.is_empty() || edits.len() > MAX_EDITS {
            return Err(NotebookError::Limit);
        }
        let mut candidate = self.clone();
        for edit in edits {
            match edit {
                Edit::SetCellSource { cell_id, source } => {
                    let index = candidate.cell_index(cell_id)?;
                    candidate.0["cells"][index]["source"] = json!(source);
                }
                Edit::InsertCell { index, cell } => {
                    let cells = candidate.0["cells"]
                        .as_array_mut()
                        .ok_or_else(|| invalid("cells must be an array"))?;
                    if *index > cells.len() {
                        return Err(invalid("insert index out of bounds"));
                    }
                    let mut cell = cell.clone();
                    if cell.is_object() && cell.get("id").is_none() {
                        cell["id"] = json!(NotebookId::new().to_string());
                    }
                    cells.insert(*index, cell);
                }
                Edit::DeleteCell { cell_id } => {
                    let index = candidate.cell_index(cell_id)?;
                    candidate.0["cells"]
                        .as_array_mut()
                        .ok_or_else(|| invalid("cells"))?
                        .remove(index);
                }
                Edit::MoveCell { cell_id, index } => {
                    let old = candidate.cell_index(cell_id)?;
                    let cells = candidate.0["cells"]
                        .as_array_mut()
                        .ok_or_else(|| invalid("cells"))?;
                    if *index >= cells.len() {
                        return Err(invalid("move index out of bounds"));
                    }
                    let cell = cells.remove(old);
                    cells.insert(*index, cell);
                }
                Edit::SetNotebookMetadata { metadata } => candidate.0["metadata"] = json!(metadata),
                Edit::SetCellMetadata { cell_id, metadata } => {
                    let index = candidate.cell_index(cell_id)?;
                    candidate.0["cells"][index]["metadata"] = json!(metadata);
                }
            }
            // Prevent transiently oversized or malformed intermediate documents.
            validate(&candidate.0)?;
            candidate.to_bytes()?;
        }
        candidate.to_bytes()?;
        Ok(candidate)
    }
}

// serde_json::Value normally keeps the last duplicate object key. Reject that
// ambiguity when importing documents instead of silently discarding content.
struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(json!(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(json!(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(json!(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| UniqueValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("non-finite number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(json!(v)))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(v)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some((key, value)) = map.next_entry::<String, UniqueValue>()? {
                    if values.insert(key, value.0).is_some() {
                        return Err(serde::de::Error::custom("duplicate JSON object key"));
                    }
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

fn invalid(message: &str) -> NotebookError {
    NotebookError::Invalid(message.into())
}

fn text(value: &Value) -> Result<String, NotebookError> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Array(lines) if lines.iter().all(Value::is_string) => {
            Ok(lines.iter().filter_map(Value::as_str).collect())
        }
        _ => Err(invalid(
            "multiline text must be a string or array of strings",
        )),
    }
}

fn validate(value: &Value) -> Result<(), NotebookError> {
    if value["nbformat"].as_u64() != Some(4) {
        return Err(NotebookError::Unsupported("requires nbformat 4".into()));
    }
    let minor = value["nbformat_minor"]
        .as_u64()
        .ok_or_else(|| invalid("nbformat_minor"))?;
    if minor > 5 {
        return Err(NotebookError::Unsupported(format!("nbformat 4.{minor}")));
    }
    object(&value["metadata"], "notebook metadata")?;
    let cells = value["cells"]
        .as_array()
        .ok_or_else(|| invalid("cells must be an array"))?;
    if cells.len() > MAX_CELLS {
        return Err(NotebookError::Limit);
    }
    let mut ids = HashSet::new();
    for cell in cells {
        object(cell, "cell")?;
        if let Some(id) = cell.get("id") {
            let id = id
                .as_str()
                .ok_or_else(|| invalid("cell id must be a string"))?;
            if id.is_empty()
                || id.len() > 64
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                || !ids.insert(id)
            {
                return Err(invalid("cell IDs must be valid and unique"));
            }
        } else if minor >= 5 {
            return Err(invalid("cell id required in nbformat 4.5"));
        }
        object(&cell["metadata"], "cell metadata")?;
        text(&cell["source"])?;
        match cell["cell_type"].as_str() {
            Some("code") => {
                count(cell.get("execution_count"), true)?;
                if cell.get("attachments").is_some() {
                    return Err(invalid("code cell attachments"));
                }
                let outputs = cell["outputs"]
                    .as_array()
                    .ok_or_else(|| invalid("outputs"))?;
                for output in outputs {
                    validate_output(output)?;
                }
            }
            Some("markdown" | "raw") => {
                if cell.get("outputs").is_some() || cell.get("execution_count").is_some() {
                    return Err(invalid("non-code execution fields"));
                }
                if let Some(attachments) = cell.get("attachments") {
                    for bundle in object(attachments, "attachments")?.values() {
                        mime(bundle)?;
                    }
                }
            }
            _ => return Err(NotebookError::Unsupported("cell type".into())),
        }
    }
    Ok(())
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, NotebookError> {
    value.as_object().ok_or_else(|| invalid(label))
}

fn count(value: Option<&Value>, nullable: bool) -> Result<(), NotebookError> {
    match value {
        Some(Value::Null) if nullable => Ok(()),
        Some(v) if v.as_u64().is_some() => Ok(()),
        _ => Err(invalid("invalid or missing execution_count")),
    }
}

fn mime(value: &Value) -> Result<(), NotebookError> {
    for (kind, data) in object(value, "MIME bundle")? {
        if kind != "application/json" && !kind.ends_with("+json") {
            text(data)?;
        }
    }
    Ok(())
}

pub(super) fn validate_output(output: &Value) -> Result<(), NotebookError> {
    object(output, "output")?;
    match output["output_type"].as_str() {
        Some("stream") => {
            if !matches!(output["name"].as_str(), Some("stdout" | "stderr")) {
                return Err(invalid("stream name"));
            }
            text(&output["text"])?;
        }
        Some("display_data" | "execute_result") => {
            mime(&output["data"])?;
            object(&output["metadata"], "output metadata")?;
            if output["output_type"] == "execute_result" {
                count(output.get("execution_count"), false)?;
            }
        }
        Some("error") => {
            if !output["ename"].is_string()
                || !output["evalue"].is_string()
                || !output["traceback"]
                    .as_array()
                    .is_some_and(|v| v.iter().all(Value::is_string))
            {
                return Err(invalid("error output"));
            }
        }
        _ => return Err(NotebookError::Unsupported("output type".into())),
    }
    Ok(())
}
