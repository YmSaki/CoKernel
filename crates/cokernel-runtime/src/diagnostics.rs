//! Shareable Runtime diagnostics are a typed projection, never a raw state/log
//! dump followed by regex masking. Credentials and workload text are not inputs.
use chrono::{DateTime, Utc};
use cokernel_domain::{FailureClassification, FailureId, FailureTrigger, SessionId, SessionState};
use serde::Serialize;

use crate::{mcp_supervisor, session::SessionFailure, tunnel};

pub const SESSION_LIMIT: usize = 128;
pub const BYTE_LIMIT: usize = 128 * 1024;

#[derive(Serialize)]
pub struct RuntimeDiagnostics {
    schema: u8,
    product_version: &'static str,
    captured_at: DateTime<Utc>,
    scope: &'static str,
    exclusions: &'static [&'static str],
    pub sessions: SessionDiagnostics,
    mcp: McpDiagnostics,
    tunnel: TunnelDiagnostics,
}

impl RuntimeDiagnostics {
    pub fn capture(
        sessions: SessionDiagnostics,
        mcp: mcp_supervisor::Snapshot,
        tunnel: tunnel::Snapshot,
    ) -> Self {
        Self {
            schema: 1,
            product_version: env!("CARGO_PKG_VERSION"),
            captured_at: Utc::now(),
            scope: "CURRENT_RUNTIME_OBSERVATIONS",
            exclusions: &[
                "credentials",
                "environment",
                "paths_and_names",
                "notebook_content",
                "variable_values",
                "raw_logs_and_tracebacks",
                "historical_failure_store",
                "host_and_installer_state",
                "resource_metrics",
            ],
            sessions,
            mcp: McpDiagnostics {
                state: mcp.state,
                generation: mcp.generation,
                process_present: mcp.pid.is_some(),
                consecutive_failures: mcp.consecutive_failures,
                failures: mcp
                    .failures
                    .iter()
                    .rev()
                    .take(8)
                    .map(ServiceFailure::from)
                    .collect(),
            },
            tunnel: TunnelDiagnostics {
                state: tunnel.state,
                desired_enabled: tunnel.desired_enabled,
                generation: tunnel.generation,
                mcp_generation: tunnel.mcp_generation,
                process_present: tunnel.pid.is_some(),
                consecutive_failures: tunnel.consecutive_failures,
                failures: tunnel
                    .failures
                    .iter()
                    .rev()
                    .take(8)
                    .map(ServiceFailure::from)
                    .collect(),
            },
        }
    }

    pub fn into_value(self) -> Result<serde_json::Value, &'static str> {
        let bytes = serde_json::to_vec(&self).map_err(|_| "CK-DIAG-ENCODING")?;
        if bytes.len() > BYTE_LIMIT {
            return Err("CK-DIAG-LIMIT");
        }
        serde_json::from_slice(&bytes).map_err(|_| "CK-DIAG-ENCODING")
    }
}

#[derive(Serialize)]
pub struct SessionDiagnostics {
    pub managed_identity_configured: bool,
    pub total: usize,
    pub omitted: usize,
    pub items: Vec<SessionDiagnostic>,
}

#[derive(Serialize)]
pub struct SessionDiagnostic {
    pub session_id: SessionId,
    pub state: SessionState,
    pub worker_generation: u64,
    pub environment_generation: u64,
    pub last_failure: Option<FailureDiagnostic>,
}

#[derive(Serialize)]
pub struct FailureDiagnostic {
    failure_id: FailureId,
    timestamp: DateTime<Utc>,
    classification: FailureClassification,
    trigger: Option<FailureTrigger>,
    exit_code: Option<i32>,
    signal: Option<i32>,
    persisted: bool,
}

impl From<&SessionFailure> for FailureDiagnostic {
    fn from(failure: &SessionFailure) -> Self {
        let record = &failure.record;
        Self {
            failure_id: record.failure_id,
            timestamp: record.timestamp,
            classification: record.classification,
            trigger: record.runtime_event_context.as_ref().map(|c| c.trigger),
            exit_code: record.exit_code,
            signal: record.signal,
            persisted: failure.persistence_error.is_none(),
        }
    }
}

#[derive(Serialize)]
struct McpDiagnostics {
    state: mcp_supervisor::State,
    generation: u64,
    process_present: bool,
    consecutive_failures: u32,
    failures: Vec<ServiceFailure>,
}
#[derive(Serialize)]
struct TunnelDiagnostics {
    state: tunnel::State,
    desired_enabled: bool,
    generation: u64,
    mcp_generation: Option<u64>,
    process_present: bool,
    consecutive_failures: u32,
    failures: Vec<ServiceFailure>,
}

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ServiceFailureKind {
    Configuration,
    Launch,
    Readiness,
    Health,
    Exit,
    Cleanup,
    Output,
    Unknown,
}
#[derive(Serialize)]
struct ServiceFailure {
    kind: ServiceFailureKind,
    timestamp: Option<DateTime<Utc>>,
    exit_code: Option<i32>,
    signal: Option<i32>,
}
impl From<&mcp_supervisor::Failure> for ServiceFailure {
    fn from(failure: &mcp_supervisor::Failure) -> Self {
        use ServiceFailureKind::*;
        let kind = match failure.code {
            "CK-MCP-CONFIG" | "CK-MCP-API" | "CK-TUN-CONFIG" => Configuration,
            "CK-MCP-LAUNCH" | "CK-TUN-LAUNCH" => Launch,
            "CK-MCP-READINESS" => Readiness,
            "CK-MCP-HEALTH" | "CK-TUN-HEALTH" => Health,
            "CK-MCP-EXIT" | "CK-TUN-EXIT" => Exit,
            "CK-MCP-CLEANUP" | "CK-TUN-CLEANUP" => Cleanup,
            "CK-MCP-OUTPUT" => Output,
            _ => Unknown,
        };
        Self {
            kind,
            timestamp: DateTime::parse_from_rfc3339(&failure.timestamp)
                .ok()
                .map(|time| time.with_timezone(&Utc)),
            exit_code: failure.exit_code,
            signal: failure.signal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cokernel_domain::{FailureRecord, RuntimeFailureContext};

    #[test]
    fn projection_omits_every_free_text_failure_field_including_unknown_codes() {
        const SECRET: &str = "synthetic-provider-key-never-export";
        use base64::{Engine, engine::general_purpose::STANDARD};
        let variants = [
            SECRET.to_owned(),
            STANDARD.encode(SECRET),
            SECRET
                .bytes()
                .map(|b| format!("%{b:02X}"))
                .collect::<String>(),
            SECRET
                .chars()
                .map(|c| format!("\\u{:04x}", c as u32))
                .collect::<String>(),
        ];
        let corpus = variants.join("\n");
        let record: FailureRecord = serde_json::from_value(serde_json::json!({
            "failure_id":FailureId::new(), "component":SECRET, "session_id":SessionId::new(),
            "operation_id":null, "timestamp":Utc::now(), "exit_code":29, "signal":null,
            "last_stderr":format!("{corpus}\nBearer {SECRET}\nhttps://user:{SECRET}@example.invalid\n{}", "界".repeat(2000)),
            "cell_id":SECRET, "classification":"UNKNOWN", "confidence":0.5,
            "runtime_event_context":RuntimeFailureContext {
                trigger: FailureTrigger::WorkerDisconnected, detail:Some(SECRET.into()),
                session_state: SessionState::Crashed, recent_session_events:vec![],
            }
        })).unwrap();
        let failure = SessionFailure {
            record,
            persistence_error: Some(SECRET.into()),
        };
        let sessions = SessionDiagnostics {
            managed_identity_configured: false,
            total: 1,
            omitted: 0,
            items: vec![SessionDiagnostic {
                session_id: SessionId::new(),
                state: SessionState::Crashed,
                worker_generation: 2,
                environment_generation: 3,
                last_failure: Some((&failure).into()),
            }],
        };
        let mut mcp = mcp_supervisor::Monitor::default().snapshot();
        mcp.failures = vec![
            mcp_supervisor::Failure {
                code: SECRET,
                timestamp: SECRET.into(),
                exit_code: Some(17),
                signal: None
            };
            20
        ];
        let value =
            RuntimeDiagnostics::capture(sessions, mcp, tunnel::Monitor::default().snapshot())
                .into_value()
                .unwrap();
        let encoded = value.to_string();
        assert!(!encoded.contains(SECRET) && !encoded.contains("example.invalid"));
        for variant in variants {
            assert!(!encoded.contains(&variant));
        }
        assert_eq!(
            value["sessions"]["items"][0]["last_failure"]["persisted"],
            false
        );
        assert_eq!(
            value["sessions"]["items"][0]["last_failure"]["exit_code"],
            29
        );
        assert_eq!(value["mcp"]["failures"].as_array().unwrap().len(), 8);
        assert_eq!(value["mcp"]["failures"][0]["kind"], "UNKNOWN");
        assert!(value["mcp"]["failures"][0]["timestamp"].is_null());
    }
}
