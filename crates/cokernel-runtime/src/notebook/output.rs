use cokernel_domain::OperationId;
use cokernel_protocol::worker::{self, WorkerFrame};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Notebook, NotebookError};

const MAX_CAPTURE_BYTES: usize = 32 * 1024 * 1024;
const MAX_CAPTURE_EVENTS: usize = 65_536;

#[derive(Debug, Clone, Serialize)]
pub struct NotebookOutputs {
    outputs: Vec<Value>,
    execution_count: u64,
    execution: Value,
}

#[derive(Debug, Deserialize)]
struct Finished {
    status: String,
    execution_count: u64,
    output_count: usize,
    output_truncated: bool,
    output_omitted_bytes: u64,
    output_truncation_reasons: Vec<String>,
}

fn invalid(message: &str) -> NotebookError {
    NotebookError::Invalid(format!("execution output: {message}"))
}

impl NotebookOutputs {
    /// Convert a complete, ordered operation capture. Missing/gapped events or a
    /// missing final response are errors, never a successful partial notebook.
    pub fn from_frames(
        operation: OperationId,
        frames: &[WorkerFrame],
    ) -> Result<Self, NotebookError> {
        if frames.len() > MAX_CAPTURE_EVENTS {
            return Err(NotebookError::Limit);
        }
        let operation = operation.to_string();
        let mut started = false;
        let mut finished: Option<(Finished, Value)> = None;
        let mut responded = false;
        let mut outputs = Vec::new();
        let mut bytes = 0_usize;
        let mut session: Option<&str> = None;
        for frame in frames {
            if frame.protocol() != worker::WORKER_PROTOCOL_V1 {
                return Err(invalid("protocol mismatch"));
            }
            if session.is_some_and(|id| id != frame.session_id()) {
                return Err(invalid("Session mismatch"));
            }
            session = Some(frame.session_id());
            bytes = bytes.saturating_add(
                serde_json::to_vec(frame)
                    .map_err(|_| invalid("JSON encoding"))?
                    .len(),
            );
            if bytes > MAX_CAPTURE_BYTES {
                return Err(NotebookError::Limit);
            }
            match frame {
                WorkerFrame::Event { event, payload, .. } => {
                    if matches!(
                        event.as_str(),
                        worker::event::HEARTBEAT | worker::event::WORKER_WARNING
                    ) {
                        continue;
                    }
                    if responded || payload["operation_id"].as_str() != Some(&operation) {
                        return Err(invalid("operation mismatch or event after response"));
                    }
                    if event == worker::event::EXECUTION_STARTED {
                        if started {
                            return Err(invalid("duplicate start"));
                        }
                        started = true;
                        continue;
                    }
                    if !started || finished.is_some() {
                        return Err(invalid("event outside execution lifecycle"));
                    }
                    if event == worker::event::EXECUTION_FINISHED {
                        let summary: Finished = serde_json::from_value(payload.clone())
                            .map_err(|_| invalid("finish shape"))?;
                        if summary.output_count != outputs.len()
                            || !matches!(
                                summary.status.as_str(),
                                "SUCCEEDED" | "FAILED" | "INTERRUPTED"
                            )
                        {
                            return Err(invalid("finish count or status"));
                        }
                        if !summary.output_truncated
                            && (summary.output_omitted_bytes != 0
                                || !summary.output_truncation_reasons.is_empty())
                        {
                            return Err(invalid("inconsistent truncation accounting"));
                        }
                        finished = Some((summary, payload.clone()));
                        continue;
                    }
                    if payload["sequence"].as_u64() != Some(outputs.len() as u64 + 1) {
                        return Err(invalid("output sequence gap or duplicate"));
                    }
                    let output = match event.as_str() {
                        worker::event::STDOUT | worker::event::STDERR => {
                            json!({"output_type":"stream","name":event,"text":payload["text"]})
                        }
                        worker::event::DISPLAY_DATA => {
                            json!({"output_type":"display_data","data":payload["data"],"metadata":payload["metadata"]})
                        }
                        worker::event::EXECUTE_RESULT => {
                            json!({"output_type":"execute_result","data":payload["data"],"metadata":payload["metadata"],"execution_count":payload["execution_count"]})
                        }
                        worker::event::ERROR => {
                            json!({"output_type":"error","ename":payload["ename"],"evalue":payload["evalue"],"traceback":payload["traceback"]})
                        }
                        _ => return Err(invalid("unsupported event")),
                    };
                    super::model::validate_output(&output)?;
                    outputs.push(output);
                }
                WorkerFrame::Response {
                    id,
                    ok,
                    result,
                    error,
                    ..
                } => {
                    let Some((_, summary)) = &finished else {
                        return Err(invalid("response before finish"));
                    };
                    if responded || id != &operation || !ok || error.is_some() {
                        return Err(invalid("invalid final response"));
                    }
                    let result = result
                        .as_ref()
                        .ok_or_else(|| invalid("missing response result"))?;
                    for key in [
                        "operation_id",
                        "status",
                        "execution_count",
                        "output_truncated",
                        "output_omitted_bytes",
                        "output_truncation_reasons",
                    ] {
                        if result.get(key) != summary.get(key) {
                            return Err(invalid("finish/response mismatch"));
                        }
                    }
                    responded = true;
                }
                _ => return Err(invalid("unexpected request frame")),
            }
        }
        let (summary, _) = finished.ok_or_else(|| invalid("missing finish"))?;
        if !responded {
            return Err(invalid("missing final response"));
        }
        Ok(Self {
            outputs,
            execution_count: summary.execution_count,
            execution: json!({"operation_id":operation,"status":summary.status,"output_truncated":summary.output_truncated,
                "output_omitted_bytes":summary.output_omitted_bytes,"output_truncation_reasons":summary.output_truncation_reasons}),
        })
    }

    pub fn outputs(&self) -> &[Value] {
        &self.outputs
    }
}

impl Notebook {
    pub fn with_outputs(
        &self,
        cell_id: &str,
        result: &NotebookOutputs,
    ) -> Result<Self, NotebookError> {
        let mut value = self.value().clone();
        let cell = value["cells"]
            .as_array_mut()
            .and_then(|cells| {
                cells
                    .iter_mut()
                    .find(|cell| cell["id"].as_str() == Some(cell_id))
            })
            .ok_or(NotebookError::NotFound)?;
        if cell["cell_type"] != "code" {
            return Err(invalid("target is not a code cell"));
        }
        cell["outputs"] = json!(result.outputs);
        cell["execution_count"] = json!(result.execution_count);
        let metadata = cell["metadata"]
            .as_object_mut()
            .ok_or_else(|| invalid("cell metadata"))?;
        let product = metadata
            .entry("cokernel")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| invalid("reserved cokernel metadata is not an object"))?;
        product.insert("execution".into(), result.execution.clone());
        Notebook::from_value(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn capture(operation: OperationId, output: Value) -> Vec<WorkerFrame> {
        let event = |name: &str, payload: Value| WorkerFrame::Event {
            protocol: 1,
            session_id: "session".into(),
            event: name.into(),
            payload,
        };
        let id = operation.to_string();
        let mut output = output;
        output["operation_id"] = json!(id);
        output["sequence"] = json!(1);
        let summary = json!({"operation_id":id,"status":"SUCCEEDED","execution_count":1,"output_count":1,"output_truncated":false,"output_omitted_bytes":0,"output_truncation_reasons":[]});
        vec![
            event("execution_started", json!({"operation_id":id})),
            event("stdout", output),
            event("execution_finished", summary.clone()),
            WorkerFrame::Response {
                protocol: 1,
                session_id: "session".into(),
                id,
                ok: true,
                result: Some(summary),
                error: None,
            },
        ]
    }

    #[test]
    fn complete_outputs_replace_only_target_output_and_owned_metadata() {
        let op = OperationId::new();
        let outputs =
            NotebookOutputs::from_frames(op, &capture(op, json!({"text":"hello\n"}))).unwrap();
        let original = Notebook::parse(include_bytes!(
            "../../../../tests/fixtures/notebooks/compatibility.ipynb"
        ))
        .unwrap();
        let saved = original.with_outputs("compute", &outputs).unwrap();
        assert_eq!(
            saved.value()["cells"][1]["outputs"],
            json!([{"output_type":"stream","name":"stdout","text":"hello\n"}])
        );
        assert_eq!(saved.value()["cells"][0], original.value()["cells"][0]);
        assert_eq!(
            saved.value()["cells"][1]["metadata"]["tags"],
            original.value()["cells"][1]["metadata"]["tags"]
        );
        assert_eq!(
            saved.value()["cells"][1]["metadata"]["cokernel"]["execution"]["operation_id"],
            op.to_string()
        );
    }

    #[test]
    fn capture_gaps_mismatch_and_missing_terminal_are_rejected() {
        let op = OperationId::new();
        let frames = capture(op, json!({"text":"ok"}));
        for removed in 0..4 {
            let mut bad = frames.clone();
            bad.remove(removed);
            assert!(NotebookOutputs::from_frames(op, &bad).is_err());
        }
        assert!(NotebookOutputs::from_frames(OperationId::new(), &frames).is_err());
        let mut bad = frames;
        if let WorkerFrame::Event { payload, .. } = &mut bad[1] {
            payload["sequence"] = json!(2);
        }
        assert!(NotebookOutputs::from_frames(op, &bad).is_err());
    }

    #[test]
    fn rich_mime_errors_counts_and_truncation_remain_standard_outputs() {
        let op = OperationId::new();
        for (name, payload) in [
            ("stderr", json!({"text":"warning"})),
            (
                "display_data",
                json!({"data":{"text/html":"<b>ok</b>","image/png":"eA==","image/svg+xml":"<svg/>","application/json":{"a":1}},"metadata":{"vendor":true}}),
            ),
            (
                "execute_result",
                json!({"data":{"text/plain":"124"},"metadata":{},"execution_count":1}),
            ),
            (
                "error",
                json!({"ename":"ValueError","evalue":"bad","traceback":["trace"]}),
            ),
        ] {
            let mut frames = capture(op, payload);
            if let WorkerFrame::Event { event, .. } = &mut frames[1] {
                *event = name.into();
            }
            let result = NotebookOutputs::from_frames(op, &frames).unwrap();
            super::super::model::validate_output(&result.outputs[0]).unwrap();
        }
        let mut frames = capture(op, json!({"text":"partial"}));
        for frame in &mut frames {
            let summary = match frame {
                WorkerFrame::Event { event, payload, .. } if event == "execution_finished" => {
                    Some(payload)
                }
                WorkerFrame::Response {
                    result: Some(result),
                    ..
                } => Some(result),
                _ => None,
            };
            if let Some(summary) = summary {
                summary["output_truncated"] = json!(true);
                summary["output_omitted_bytes"] = json!(100);
                summary["output_truncation_reasons"] = json!(["operation_bytes"]);
            }
        }
        let result = NotebookOutputs::from_frames(op, &frames).unwrap();
        assert_eq!(result.execution["output_truncated"], true);
        assert_eq!(result.execution["output_omitted_bytes"], 100);
    }
}
