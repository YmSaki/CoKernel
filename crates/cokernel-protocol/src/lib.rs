use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const PROTOCOL_V1: u32 = 1;
pub const DEFAULT_MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub protocol: u32,
    pub request_id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub protocol: u32,
    pub request_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<cokernel_domain::DomainError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub protocol: u32,
    pub event: String,
    pub sequence: u64,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("frame length {0} exceeds configured maximum")]
    TooLarge(usize),
    #[error("payload is not valid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
}

pub fn encode_json_frame<T: Serialize>(value: &T, max_bytes: usize) -> Result<Vec<u8>, FrameError> {
    let payload = serde_json::to_vec(value)?;
    if payload.len() > max_bytes {
        return Err(FrameError::TooLarge(payload.len()));
    }
    let mut out = Vec::with_capacity(payload.len() + 4);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

pub fn decode_json_payload<T: for<'de> Deserialize<'de>>(
    payload: &[u8],
    max_bytes: usize,
) -> Result<T, FrameError> {
    if payload.len() > max_bytes {
        return Err(FrameError::TooLarge(payload.len()));
    }
    Ok(serde_json::from_slice(payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_prefix_is_big_endian_payload_length() {
        let req = RequestEnvelope {
            protocol: PROTOCOL_V1,
            request_id: "1".into(),
            method: "bridge.ping".into(),
            params: Value::Null,
        };
        let frame = encode_json_frame(&req, DEFAULT_MAX_FRAME_BYTES).unwrap();
        let len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(len, frame.len() - 4);
    }
}
