use std::path::Path;
use std::process::Stdio;

use cokernel_domain::SessionId;
use cokernel_protocol::worker::WorkerFrame;
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
    Ok(Some(decode_json_payload(
        &payload,
        DEFAULT_MAX_FRAME_BYTES,
    )?))
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
    use cokernel_protocol::worker::{method, WORKER_PROTOCOL_V1};
    use serde_json::json;
    use tokio::io::duplex;

    #[tokio::test]
    async fn worker_frame_round_trip_uses_shared_length_prefix_codec() {
        let (mut left, mut right) = duplex(4096);
        let session_id = SessionId::new().to_string();
        let expected = WorkerFrame::Request {
            protocol: WORKER_PROTOCOL_V1,
            id: "req-1".into(),
            session_id,
            method: method::PING.into(),
            payload: json!({}),
        };
        let writer_frame = expected.clone();
        let writer = tokio::spawn(async move {
            write_worker_frame(&mut left, &writer_frame).await.unwrap();
        });
        let actual = read_worker_frame(&mut right).await.unwrap().unwrap();
        writer.await.unwrap();
        assert_eq!(actual, expected);
    }
}
