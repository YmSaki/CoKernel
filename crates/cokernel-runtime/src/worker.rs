use std::path::Path;
use std::process::Stdio;

use cokernel_domain::SessionId;
use cokernel_protocol::worker::{self, WorkerFrame};
use cokernel_protocol::{
    decode_json_payload, encode_json_frame, FrameError, DEFAULT_MAX_FRAME_BYTES,
};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::Command;

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
            ok, result, error, ..
        } => {
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
            Ok(())
        }
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
    fn worker_response_envelope_is_unambiguous() {
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
    fn worker_events_are_known_v1_object_payloads() {
        let session_id = SessionId::new().to_string();
        let heartbeat = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.clone(),
            event: worker::event::HEARTBEAT.into(),
            payload: json!({"monotonic_ns": 1}),
        };
        assert!(validate_worker_inbound_frame(&heartbeat).is_ok());

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
}
