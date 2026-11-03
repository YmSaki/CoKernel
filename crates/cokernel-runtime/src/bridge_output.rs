//! One writer owns complete framed responses and observations; no interleaving.
use crate::events::EventHub;
use cokernel_protocol::{DEFAULT_MAX_FRAME_BYTES, EventEnvelope, encode_json_frame};
use serde_json::json;
use std::{io, time::Duration};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::{broadcast, mpsc, oneshot},
    task::JoinHandle,
};

struct Response {
    bytes: Vec<u8>,
    written: oneshot::Sender<io::Result<()>>,
}
pub struct BridgeOutput {
    responses: mpsc::Sender<Response>,
    task: JoinHandle<()>,
}
impl Drop for BridgeOutput {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn write<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) -> io::Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        writer.write_all(bytes).await?;
        writer.flush().await
    })
    .await
    .map_err(io::Error::other)?
}
impl BridgeOutput {
    pub fn start<W: AsyncWrite + Unpin + Send + 'static>(mut writer: W, hub: EventHub) -> Self {
        let (responses, mut pending) = mpsc::channel::<Response>(8);
        let (mut events, _) = hub.subscribe();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    response=pending.recv()=>{
                        let Some(response)=response else {break;};
                        let result=write(&mut writer,&response.bytes).await;
                        let failed=result.is_err();let _=response.written.send(result);
                        if failed {break;}
                    }
                    received=events.recv()=>{
                        let event=match received {
                            Ok(event)=>event,
                            Err(broadcast::error::RecvError::Lagged(_))=>{
                                // Drop the retained old suffix atomically with its
                                // watermark, otherwise this marker could precede
                                // events carrying lower sequence numbers.
                                let (fresh,sequence)=hub.subscribe();events=fresh;
                                EventEnvelope{protocol:1,event:"events.resync_required".into(),sequence,payload:json!({"reason":"runtime_event_lag"})}
                            }
                            Err(broadcast::error::RecvError::Closed)=>break,
                        };
                        let Ok(bytes)=encode_json_frame(&event,DEFAULT_MAX_FRAME_BYTES) else {break;};
                        if write(&mut writer,&bytes).await.is_err() {break;}
                    }
                }
            }
        });
        Self { responses, task }
    }
    pub async fn send(&self, bytes: Vec<u8>) -> io::Result<()> {
        let (written, complete) = oneshot::channel();
        self.responses
            .send(Response { bytes, written })
            .await
            .map_err(|_| io::Error::other("Runtime output closed"))?;
        complete
            .await
            .map_err(|_| io::Error::other("Runtime response write not acknowledged"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, duplex};
    #[tokio::test]
    async fn bounded_writer_preserves_frames_and_makes_lag_explicit() {
        let hub = EventHub::default();
        let (write, mut read) = duplex(32);
        let output = BridgeOutput::start(write, hub.clone());
        for _ in 0..300 {
            hub.publish("session.output", json!({"data":"x".repeat(100)}));
        }
        let (gap_seen, gap_ready) = oneshot::channel();
        let receiver = tokio::spawn(async move {
            let mut gap_seen = Some(gap_seen);
            let mut last = 0;
            let mut gap = false;
            loop {
                let size = read.read_u32().await.unwrap();
                let mut bytes = vec![0; size as usize];
                read.read_exact(&mut bytes).await.unwrap();
                let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                if value.get("request_id").is_some() {
                    assert!(gap);
                    break;
                }
                let sequence = value["sequence"].as_u64().unwrap();
                assert!(sequence > last);
                last = sequence;
                gap |= value["event"] == "events.resync_required";
                if gap && let Some(ready) = gap_seen.take() {
                    let _ = ready.send(());
                }
            }
        });
        // Let the selected event frame complete before requesting an RPC reply.
        tokio::time::timeout(Duration::from_secs(2), gap_ready)
            .await
            .unwrap()
            .unwrap();
        output
            .send(
                encode_json_frame(
                    &json!({"protocol":1,"request_id":"r","ok":true,"result":{}}),
                    DEFAULT_MAX_FRAME_BYTES,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        receiver.await.unwrap();
    }
}
