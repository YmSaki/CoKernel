//! Bounded document projections shared by UI and MCP; never execute user code.
use super::{DocumentSnapshot, NotebookError};
use serde_json::{Value, json};
pub fn cells(
    snapshot: DocumentSnapshot,
    expected: Option<u64>,
    offset: usize,
    limit: usize,
    outputs: bool,
) -> Result<Value, NotebookError> {
    if limit == 0 || limit > 32 || offset > 10_000 {
        return Err(NotebookError::Limit);
    }
    let revision = snapshot.document.revision;
    if expected.is_some_and(|r| r != revision) {
        return Err(NotebookError::Conflict { current: revision });
    }
    let all = snapshot.notebook.value()["cells"]
        .as_array()
        .ok_or(NotebookError::NotFound)?;
    let mut result = Vec::new();
    let mut budget = 64 * 1024;
    for (index, cell) in all.iter().enumerate().skip(offset).take(limit) {
        let source = match &cell["source"] {
            Value::String(s) => s.clone(),
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => String::new(),
        };
        let mut end = source.len().min(8192);
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        let output_bytes =
            serde_json::to_vec(&cell["outputs"]).map_err(|_| NotebookError::Limit)?;
        let output_included = outputs && output_bytes.len() <= 8192;
        let value = json!({"index":index,"cell_id":cell["id"],"cell_type":cell["cell_type"],
            "source":&source[..end],"source_bytes":source.len(),"source_truncated":end<source.len(),
            "execution_count":cell["execution_count"],"outputs":if output_included {cell["outputs"].clone()}else{Value::Null},
            "outputs_included":output_included,"outputs_truncated":outputs && !output_included,
            "output_bytes":output_bytes.len()});
        let bytes = serde_json::to_vec(&value)
            .map_err(|_| NotebookError::Limit)?
            .len();
        if bytes > budget {
            break;
        }
        budget -= bytes;
        result.push(value);
    }
    let next = offset.saturating_add(result.len());
    Ok(
        json!({"document":snapshot.document,"state":snapshot.state,"revision":revision,
        "total_cells":all.len(),"offset":offset,"next_offset":if next<all.len(){Some(next)}else{None},"cells":result}),
    )
}
