use std::io;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, ServerOptions};
use tokio::time::sleep;

fn unique_pipe_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(r"\\.\pipe\cokernel-v1-spike-{}-{nanos}", std::process::id())
}

async fn open_client_with_retry(pipe_name: &str) -> io::Result<NamedPipeClient> {
    let mut last_error = None;
    for _ in 0..100 {
        match ClientOptions::new().open(pipe_name) {
            Ok(client) => return Ok(client),
            Err(error) => {
                last_error = Some(error);
                sleep(Duration::from_millis(10)).await;
            }
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("named-pipe client open failed")))
}

async fn write_frame<W>(writer: &mut W, payload: &[u8]) -> io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let length = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "payload too large"))?;
    writer.write_all(&length.to_be_bytes()).await?;
    writer.write_all(payload).await?;
    writer.flush().await
}

async fn read_frame<R>(reader: &mut R) -> io::Result<Vec<u8>>
where
    R: AsyncReadExt + Unpin,
{
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length).await?;
    let length = u32::from_be_bytes(length) as usize;
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    Ok(payload)
}

/// Phase-0 transport proof for the Desktop/Host local IPC choice.
///
/// This is intentionally not the final Host server. It proves that the Tokio
/// runtime used by CoKernel can provide a framed request/response exchange over
/// a Windows named pipe. Phase 5 adds ACLs, long-lived accept loops, RPC
/// dispatch, cancellation, and event fan-out.
pub async fn prove_round_trip(payload: &[u8]) -> io::Result<Vec<u8>> {
    let pipe_name = unique_pipe_name();
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_name)?;

    let server_task = tokio::spawn(async move {
        server.connect().await?;
        let request = read_frame(&mut server).await?;
        write_frame(&mut server, &request).await?;
        Ok::<(), io::Error>(())
    });

    let mut client = open_client_with_retry(&pipe_name).await?;
    write_frame(&mut client, payload).await?;
    let response = read_frame(&mut client).await?;

    server_task
        .await
        .map_err(|error| io::Error::other(format!("named-pipe server task failed: {error}")))??;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn named_pipe_round_trip() {
        let response = prove_round_trip(b"cokernel-named-pipe").await.unwrap();
        assert_eq!(response, b"cokernel-named-pipe");
    }
}
