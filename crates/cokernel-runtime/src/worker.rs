use std::path::Path;
use std::process::Stdio;

use cokernel_domain::SessionId;
use cokernel_protocol::worker::{self, WorkerFrame};
use cokernel_protocol::{
    decode_json_payload, encode_json_frame, FrameError, DEFAULT_MAX_FRAME_BYTES,
};
use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::Command;

const MAX_WORKER_CORRELATION_ID_CHARS: usize = 256;

#[derive(Debug, Error)]
pub enum WorkerTransportError {
    #[error("worker transport I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error("worker protocol violation: {0}")]
    Protocol(String),
}

pub async fn write_worker_frame<W>(
    writer: &mut W,
    frame: &WorkerFrame,
) -> Result<(), WorkerTransportError>
where
    W: AsyncWrite + Unpin,
{
    let bytes = encode_json_frame(frame, DEFAULT_MAX_FRAME_BYTES)?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_worker_frame<R>(
    reader: &mut R,
) -> Result<Option<WorkerFrame>, WorkerTransportError>
where
    R: AsyncRead + Unpin,
{
    let mut prefix = [0_u8; 4];
    match reader.read_exact(&mut prefix).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let size = u32::from_be_bytes(prefix) as usize;
    if size > DEFAULT_MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(size).into());
    }
    let mut payload = vec![0_u8; size];
    reader.read_exact(&mut payload).await?;
    let frame = decode_json_payload(&payload, DEFAULT_MAX_FRAME_BYTES)?;
    validate_worker_inbound_frame(&frame)?;
    Ok(Some(frame))
}

fn validate_worker_inbound_frame(frame: &WorkerFrame) -> Result<(), WorkerTransportError> {
    match frame {
        WorkerFrame::Request { .. } => Err(WorkerTransportError::Protocol(
            "worker must not send request frames to the Runtime".into(),
        )),
        WorkerFrame::Response {
            id,
            ok,
            result,
            error,
            ..
        } => {
            validate_correlation_id("response id", id)?;
            if *ok {
                if error.is_some() {
                    return Err(WorkerTransportError::Protocol(
                        "successful worker response included an error".into(),
                    ));
                }
                if result.is_none() {
                    return Err(WorkerTransportError::Protocol(
                        "successful worker response omitted result".into(),
                    ));
                }
            } else {
                if result.is_some() {
                    return Err(WorkerTransportError::Protocol(
                        "failed worker response included a result".into(),
                    ));
                }
                if error.is_none() {
                    return Err(WorkerTransportError::Protocol(
                        "failed worker response omitted error".into(),
                    ));
                }
            }
            Ok(())
        }
        WorkerFrame::Event { event, payload, .. } => {
            if !known_worker_event(event) {
                return Err(WorkerTransportError::Protocol(format!(
                    "worker sent unknown v1 event {event:?}"
                )));
            }
            if !payload.is_object() {
                return Err(WorkerTransportError::Protocol(format!(
                    "worker event {event:?} payload must be an object"
                )));
            }
            validate_worker_event_payload(event, payload)
        }
    }
}

fn validate_correlation_id(label: &str, value: &str) -> Result<(), WorkerTransportError> {
    if value.is_empty() {
        return Err(WorkerTransportError::Protocol(format!(
            "worker {label} must be non-empty"
        )));
    }
    if value.chars().count() > MAX_WORKER_CORRELATION_ID_CHARS {
        return Err(WorkerTransportError::Protocol(format!(
            "worker {label} exceeds maximum length"
        )));
    }
    Ok(())
}

fn required_string<'a>(
    event: &str,
    payload: &'a Value,
    field: &str,
) -> Result<&'a str, WorkerTransportError> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            WorkerTransportError::Protocol(format!(
                "worker event {event:?} requires non-empty string field {field:?}"
            ))
        })
}

fn required_u64(event: &str, payload: &Value, field: &str) -> Result<u64, WorkerTransportError> {
    payload.get(field).and_then(Value::as_u64).ok_or_else(|| {
        WorkerTransportError::Protocol(format!(
            "worker event {event:?} requires unsigned integer field {field:?}"
        ))
    })
}

fn require_operation_id(event: &str, payload: &Value) -> Result<(), WorkerTransportError> {
    let operation_id = required_string(event, payload, "operation_id")?;
    validate_correlation_id("operation_id", operation_id)
}

fn require_sequence(event: &str, payload: &Value) -> Result<(), WorkerTransportError> {
    if required_u64(event, payload, "sequence")? == 0 {
        return Err(WorkerTransportError::Protocol(format!(
            "worker event {event:?} sequence must be positive"
        )));
    }
    Ok(())
}

fn validate_worker_event_payload(event: &str, payload: &Value) -> Result<(), WorkerTransportError> {
    match event {
        worker::event::READY => {
            let pid = required_u64(event, payload, "pid")?;
            if pid == 0 || pid > u32::MAX as u64 {
                return Err(WorkerTransportError::Protocol(
                    "worker ready pid must fit a positive u32".into(),
                ));
            }
            if required_u64(event, payload, "heartbeat_interval_ms")? == 0 {
                return Err(WorkerTransportError::Protocol(
                    "worker ready heartbeat_interval_ms must be positive".into(),
                ));
            }
            required_string(event, payload, "worker_version")?;
            required_string(event, payload, "python_version")?;
            required_string(event, payload, "ipython_version")?;
            Ok(())
        }
        worker::event::HEARTBEAT => {
            if required_u64(event, payload, "monotonic_ns")? == 0 {
                return Err(WorkerTransportError::Protocol(
                    "worker heartbeat monotonic_ns must be positive".into(),
                ));
            }
            Ok(())
        }
        worker::event::EXECUTION_STARTED => require_operation_id(event, payload),
        worker::event::STDOUT | worker::event::STDERR => {
            require_operation_id(event, payload)?;
            require_sequence(event, payload)?;
            payload.get("text").and_then(Value::as_str).ok_or_else(|| {
                WorkerTransportError::Protocol(format!(
                    "worker event {event:?} requires string field \"text\""
                ))
            })?;
            Ok(())
        }
        worker::event::EXECUTE_RESULT | worker::event::DISPLAY_DATA => {
            require_operation_id(event, payload)?;
            require_sequence(event, payload)?;
            if !payload.get("data").is_some_and(Value::is_object) {
                return Err(WorkerTransportError::Protocol(format!(
                    "worker event {event:?} requires object field \"data\""
                )));
            }
            if !payload.get("metadata").is_some_and(Value::is_object) {
                return Err(WorkerTransportError::Protocol(format!(
                    "worker event {event:?} requires object field \"metadata\""
                )));
            }
            Ok(())
        }
        worker::event::ERROR => {
            require_operation_id(event, payload)?;
            require_sequence(event, payload)?;
            required_string(event, payload, "ename")?;
            payload.get("evalue").and_then(Value::as_str).ok_or_else(|| {
                WorkerTransportError::Protocol(
                    "worker error event requires string field \"evalue\"".into(),
                )
            })?;
            let traceback = payload.get("traceback").and_then(Value::as_array).ok_or_else(|| {
                WorkerTransportError::Protocol(
                    "worker error event requires array field \"traceback\"".into(),
                )
            })?;
            if traceback.iter().any(|line| !line.is_string()) {
                return Err(WorkerTransportError::Protocol(
                    "worker error traceback entries must be strings".into(),
                ));
            }
            Ok(())
        }
        worker::event::EXECUTION_FINISHED => {
            require_operation_id(event, payload)?;
            match required_string(event, payload, "status")? {
                "SUCCEEDED" | "FAILED" => {}
                other => {
                    return Err(WorkerTransportError::Protocol(format!(
                        "worker execution_finished status {other:?} is invalid"
                    )));
                }
            }
            required_u64(event, payload, "output_count")?;
            if payload.get("output_truncated").and_then(Value::as_bool).is_none() {
                return Err(WorkerTransportError::Protocol(
                    "worker execution_finished requires boolean field \"output_truncated\"".into(),
                ));
            }
            required_u64(event, payload, "output_omitted_bytes")?;
            let reasons = payload
                .get("output_truncation_reasons")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    WorkerTransportError::Protocol(
                        "worker execution_finished requires array field \"output_truncation_reasons\""
                            .into(),
                    )
                })?;
            if reasons.iter().any(|reason| !reason.is_string()) {
                return Err(WorkerTransportError::Protocol(
                    "worker output_truncation_reasons entries must be strings".into(),
                ));
            }
            Ok(())
        }
        worker::event::WORKER_WARNING => Ok(()),
        _ => Err(WorkerTransportError::Protocol(format!(
            "worker sent unknown v1 event {event:?}"
        ))),
    }
}

fn known_worker_event(event: &str) -> bool {
    matches!(
        event,
        worker::event::READY
            | worker::event::EXECUTION_STARTED
            | worker::event::STDOUT
            | worker::event::STDERR
            | worker::event::EXECUTE_RESULT
            | worker::event::DISPLAY_DATA
            | worker::event::ERROR
            | worker::event::EXECUTION_FINISHED
            | worker::event::HEARTBEAT
            | worker::event::WORKER_WARNING
    )
}

pub fn uv_worker_command(
    uv_executable: &Path,
    project_root: &Path,
    worker_package: &Path,
    socket_path: &Path,
    session_id: SessionId,
) -> Command {
    let mut command = Command::new(uv_executable);
    command
        .arg("run")
        .arg("--project")
        .arg(project_root)
        .arg("--with-editable")
        .arg(worker_package)
        .arg("--")
        .arg("cokernel-worker")
        .arg("--socket")
        .arg(socket_path)
        .arg("--session-id")
        .arg(session_id.to_string())
        .env("UV_NO_PROGRESS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use cokernel_protocol::worker::{WorkerError, WORKER_PROTOCOL_V1};
    use serde_json::json;
    use tokio::io::duplex;

    #[tokio::test]
    async fn worker_frame_round_trip_uses_shared_length_prefix_codec() {
        let (mut left, mut right) = duplex(4096);
        let session_id = SessionId::new().to_string();
        let expected = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id,
            event: worker::event::HEARTBEAT.into(),
            payload: json!({"monotonic_ns": 1}),
        };
        let writer_frame = expected.clone();
        let writer = tokio::spawn(async move {
            write_worker_frame(&mut left, &writer_frame).await.unwrap();
        });
        let actual = read_worker_frame(&mut right).await.unwrap().unwrap();
        writer.await.unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn worker_cannot_send_runtime_request_frames() {
        let frame = WorkerFrame::Request {
            protocol: WORKER_PROTOCOL_V1,
            id: "req-1".into(),
            session_id: SessionId::new().to_string(),
            method: worker::method::PING.into(),
            payload: json!({}),
        };
        assert!(matches!(
            validate_worker_inbound_frame(&frame),
            Err(WorkerTransportError::Protocol(message))
                if message.contains("must not send request frames")
        ));
    }

    #[test]
    fn worker_response_envelope_is_unambiguous_and_correlated() {
        let session_id = SessionId::new().to_string();
        let successful = WorkerFrame::Response {
            protocol: WORKER_PROTOCOL_V1,
            id: "ok".into(),
            session_id: session_id.clone(),
            ok: true,
            result: Some(json!({"pong": true})),
            error: None,
        };
        assert!(validate_worker_inbound_frame(&successful).is_ok());

        let blank_id = WorkerFrame::Response {
            protocol: WORKER_PROTOCOL_V1,
            id: "".into(),
            session_id: session_id.clone(),
            ok: true,
            result: Some(json!({})),
            error: None,
        };
        assert!(validate_worker_inbound_frame(&blank_id).is_err());

        let success_with_error = WorkerFrame::Response {
            protocol: WORKER_PROTOCOL_V1,
            id: "bad-success".into(),
            session_id: session_id.clone(),
            ok: true,
            result: Some(json!({})),
            error: Some(WorkerError {
                code: "CK-TEST".into(),
                summary: "unexpected".into(),
                error_type: None,
            }),
        };
        assert!(validate_worker_inbound_frame(&success_with_error).is_err());

        let failed_without_error = WorkerFrame::Response {
            protocol: WORKER_PROTOCOL_V1,
            id: "bad-failure".into(),
            session_id,
            ok: false,
            result: None,
            error: None,
        };
        assert!(validate_worker_inbound_frame(&failed_without_error).is_err());
    }

    #[test]
    fn worker_events_require_liveness_and_operation_correlation_fields() {
        let session_id = SessionId::new().to_string();
        let heartbeat = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.clone(),
            event: worker::event::HEARTBEAT.into(),
            payload: json!({"monotonic_ns": 1}),
        };
        assert!(validate_worker_inbound_frame(&heartbeat).is_ok());

        let malformed_heartbeat = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.clone(),
            event: worker::event::HEARTBEAT.into(),
            payload: json!({}),
        };
        assert!(validate_worker_inbound_frame(&malformed_heartbeat).is_err());

        let stdout_missing_operation = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.clone(),
            event: worker::event::STDOUT.into(),
            payload: json!({"sequence": 1, "text": "hello"}),
        };
        assert!(validate_worker_inbound_frame(&stdout_missing_operation).is_err());

        let stdout_zero_sequence = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.clone(),
            event: worker::event::STDOUT.into(),
            payload: json!({"operation_id": "op-1", "sequence": 0, "text": "hello"}),
        };
        assert!(validate_worker_inbound_frame(&stdout_zero_sequence).is_err());

        let unknown = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.clone(),
            event: "future-or-corrupt-event".into(),
            payload: json!({}),
        };
        assert!(validate_worker_inbound_frame(&unknown).is_err());

        let scalar_payload = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id,
            event: worker::event::HEARTBEAT.into(),
            payload: json!(1),
        };
        assert!(validate_worker_inbound_frame(&scalar_payload).is_err());
    }

    #[test]
    fn worker_execution_finished_contract_is_strict() {
        let session_id = SessionId::new().to_string();
        let valid = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.clone(),
            event: worker::event::EXECUTION_FINISHED.into(),
            payload: json!({
                "operation_id": "op-1",
                "status": "FAILED",
                "execution_count": 1,
                "output_count": 2,
                "output_truncated": true,
                "output_omitted_bytes": 10,
                "output_truncation_reasons": ["invalid_json"],
            }),
        };
        assert!(validate_worker_inbound_frame(&valid).is_ok());

        let invalid_status = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id,
            event: worker::event::EXECUTION_FINISHED.into(),
            payload: json!({
                "operation_id": "op-1",
                "status": "MAYBE",
                "output_count": 0,
                "output_truncated": false,
                "output_omitted_bytes": 0,
                "output_truncation_reasons": [],
            }),
        };
        assert!(validate_worker_inbound_frame(&invalid_status).is_err());
    }
}
