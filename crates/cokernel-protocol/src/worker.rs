use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const WORKER_PROTOCOL_V1: u32 = 1;

pub mod method {
    pub const HANDSHAKE: &str = "handshake";
    pub const EXECUTE: &str = "execute";
    pub const INSPECT_VARIABLES: &str = "inspect_variables";
    pub const GET_VARIABLE: &str = "get_variable";
    pub const RESET: &str = "reset";
    pub const SHUTDOWN: &str = "shutdown";
    pub const PING: &str = "ping";
}

pub mod event {
    pub const READY: &str = "ready";
    pub const EXECUTION_STARTED: &str = "execution_started";
    pub const STDOUT: &str = "stdout";
    pub const STDERR: &str = "stderr";
    pub const EXECUTE_RESULT: &str = "execute_result";
    pub const DISPLAY_DATA: &str = "display_data";
    pub const ERROR: &str = "error";
    pub const EXECUTION_FINISHED: &str = "execution_finished";
    pub const HEARTBEAT: &str = "heartbeat";
    pub const WORKER_WARNING: &str = "worker_warning";
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WorkerFrame {
    Request {
        protocol: u32,
        id: String,
        session_id: String,
        method: String,
        #[serde(default)]
        payload: Value,
    },
    Response {
        protocol: u32,
        id: String,
        session_id: String,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<WorkerError>,
    },
    Event {
        protocol: u32,
        session_id: String,
        event: String,
        #[serde(default)]
        payload: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerError {
    pub code: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
}

impl WorkerFrame {
    pub fn protocol(&self) -> u32 {
        match self {
            Self::Request { protocol, .. }
            | Self::Response { protocol, .. }
            | Self::Event { protocol, .. } => *protocol,
        }
    }

    pub fn session_id(&self) -> &str {
        match self {
            Self::Request { session_id, .. }
            | Self::Response { session_id, .. }
            | Self::Event { session_id, .. } => session_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_round_trip_matches_worker_wire_shape() {
        let frame = WorkerFrame::Request {
            protocol: WORKER_PROTOCOL_V1,
            id: "req-1".into(),
            session_id: "session-1".into(),
            method: method::EXECUTE.into(),
            payload: json!({
                "operation_id": "op-1",
                "cell_id": "cell-1",
                "source": "x = 1\nx + 1"
            }),
        };

        let value = serde_json::to_value(&frame).unwrap();
        assert_eq!(value["type"], "request");
        assert_eq!(value["protocol"], WORKER_PROTOCOL_V1);
        assert_eq!(value["method"], method::EXECUTE);
        assert_eq!(value["payload"]["operation_id"], "op-1");

        let decoded: WorkerFrame = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, frame);
    }
}
