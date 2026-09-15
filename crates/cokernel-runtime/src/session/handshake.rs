const REQUIRED_WORKER_CAPABILITIES: &[&str] = &[
    worker::method::EXECUTE,
    worker::method::INSPECT_VARIABLES,
    worker::method::GET_VARIABLE,
    worker::method::RESET,
    worker::method::SHUTDOWN,
];
const MAX_WORKER_VERSION_CHARS: usize = 256;
const MAX_WORKER_CAPABILITIES: usize = 64;
const MAX_WORKER_CAPABILITY_CHARS: usize = 128;
const MAX_HANDSHAKE_ERROR_CODE_CHARS: usize = 128;
const MAX_HANDSHAKE_ERROR_SUMMARY_CHARS: usize = 512;

async fn perform_worker_handshake(
    stream: &mut tokio::net::UnixStream,
    session_id: SessionId,
    worker_generation: u64,
    expected_heartbeat_interval: Duration,
    startup_timeout: Duration,
) -> Result<(), SessionError> {
    let request_id = format!("handshake-{session_id}-{worker_generation}");
    let request = WorkerFrame::Request {
        protocol: WORKER_PROTOCOL_V1,
        id: request_id.clone(),
        session_id: session_id.to_string(),
        method: worker::method::HANDSHAKE.into(),
        payload: json!({}),
    };
    write_worker_frame(stream, &request).await?;

    let response = timeout(
        startup_timeout,
        read_worker_handshake_response(stream, session_id, &request_id),
    )
    .await
    .map_err(|_| SessionError::StartupTimeout)??;
    validate_worker_handshake(
        &response,
        session_id,
        &request_id,
        expected_heartbeat_interval,
    )?;
    Ok(())
}

async fn read_worker_handshake_response<R>(
    reader: &mut R,
    expected_session_id: SessionId,
    expected_request_id: &str,
) -> Result<WorkerFrame, SessionError>
where
    R: AsyncRead + Unpin,
{
    loop {
        let frame = read_worker_frame(reader)
            .await?
            .ok_or(SessionError::WorkerDisconnected)?;
        validate_worker_frame_identity(expected_session_id, &frame)?;
        match &frame {
            WorkerFrame::Response { id, .. } if id == expected_request_id => return Ok(frame),
            WorkerFrame::Event { event, .. } if event == worker::event::HEARTBEAT => continue,
            other => {
                return Err(WorkerTransportError::Protocol(format!(
                    "unexpected worker frame during handshake: {}",
                    worker_frame_descriptor(other)
                ))
                .into())
            }
        }
    }
}

fn worker_frame_descriptor(frame: &WorkerFrame) -> String {
    match frame {
        WorkerFrame::Request { id, method, .. } => {
            format!("request id={id:?} method={method:?}")
        }
        WorkerFrame::Response { id, ok, .. } => format!("response id={id:?} ok={ok}"),
        WorkerFrame::Event { event, .. } => format!("event {event:?}"),
    }
}

fn bounded_handshake_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut bounded = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    bounded.push('…');
    bounded
}

fn validate_worker_handshake(
    frame: &WorkerFrame,
    expected_session_id: SessionId,
    expected_request_id: &str,
    expected_heartbeat_interval: Duration,
) -> Result<(), WorkerTransportError> {
    validate_worker_frame_identity(expected_session_id, frame)?;
    let WorkerFrame::Response {
        id,
        ok,
        result,
        error,
        ..
    } = frame
    else {
        return Err(WorkerTransportError::Protocol(
            "worker handshake did not return a response frame".into(),
        ));
    };
    if id != expected_request_id {
        return Err(WorkerTransportError::Protocol(format!(
            "worker handshake response id {id} does not match expected {expected_request_id}"
        )));
    }
    if !*ok {
        let detail = error
            .as_ref()
            .map(|error| {
                format!(
                    "{}: {}",
                    bounded_handshake_text(&error.code, MAX_HANDSHAKE_ERROR_CODE_CHARS),
                    bounded_handshake_text(&error.summary, MAX_HANDSHAKE_ERROR_SUMMARY_CHARS)
                )
            })
            .unwrap_or_else(|| "without structured error".into());
        return Err(WorkerTransportError::Protocol(format!(
            "worker handshake failed {detail}"
        )));
    }
    if error.is_some() {
        return Err(WorkerTransportError::Protocol(
            "successful worker handshake included an error".into(),
        ));
    }
    let result = result.as_ref().ok_or_else(|| {
        WorkerTransportError::Protocol("successful worker handshake omitted result".into())
    })?;

    let protocol = result
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            WorkerTransportError::Protocol("worker handshake omitted protocol version".into())
        })?;
    if protocol != u64::from(WORKER_PROTOCOL_V1) {
        return Err(WorkerTransportError::Protocol(format!(
            "worker handshake protocol {protocol} does not match expected {WORKER_PROTOCOL_V1}"
        )));
    }

    let worker_version = result
        .get("worker_version")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            WorkerTransportError::Protocol("worker handshake omitted worker version".into())
        })?;
    if worker_version.chars().count() > MAX_WORKER_VERSION_CHARS {
        return Err(WorkerTransportError::Protocol(
            "worker handshake worker version exceeds maximum length".into(),
        ));
    }

    let heartbeat_interval_ms = result
        .get("heartbeat_interval_ms")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            WorkerTransportError::Protocol(
                "worker handshake omitted positive heartbeat interval".into(),
            )
        })?;
    if u128::from(heartbeat_interval_ms) != expected_heartbeat_interval.as_millis() {
        return Err(WorkerTransportError::Protocol(format!(
            "worker handshake heartbeat interval {heartbeat_interval_ms}ms does not match ready interval {}ms",
            expected_heartbeat_interval.as_millis()
        )));
    }

    let capabilities = result
        .get("capabilities")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            WorkerTransportError::Protocol("worker handshake omitted capabilities".into())
        })?;
    if capabilities.len() > MAX_WORKER_CAPABILITIES {
        return Err(WorkerTransportError::Protocol(
            "worker handshake capability list exceeds maximum item count".into(),
        ));
    }
    let mut seen_capabilities = std::collections::HashSet::with_capacity(capabilities.len());
    for capability in capabilities {
        let capability = capability
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                WorkerTransportError::Protocol(
                    "worker handshake capabilities must be non-empty strings".into(),
                )
            })?;
        if capability.chars().count() > MAX_WORKER_CAPABILITY_CHARS {
            return Err(WorkerTransportError::Protocol(
                "worker handshake capability exceeds maximum length".into(),
            ));
        }
        if !seen_capabilities.insert(capability) {
            return Err(WorkerTransportError::Protocol(format!(
                "worker handshake capability {capability:?} is duplicated"
            )));
        }
    }
    for required in REQUIRED_WORKER_CAPABILITIES {
        if !seen_capabilities
            .iter()
            .any(|capability| capability == required)
        {
            return Err(WorkerTransportError::Protocol(format!(
                "worker handshake omitted required capability {required}"
            )));
        }
    }

    let output_limits = result
        .get("output_limits")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            WorkerTransportError::Protocol("worker handshake omitted output limits".into())
        })?;
    for field in ["max_event_bytes", "max_operation_bytes", "max_blob_bytes"] {
        output_limits
            .get(field)
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                WorkerTransportError::Protocol(format!(
                    "worker handshake output limit {field} must be positive"
                ))
            })?;
    }
    let max_event_bytes = output_limits
        .get("max_event_bytes")
        .and_then(Value::as_u64)
        .expect("validated output event limit");
    let max_operation_bytes = output_limits
        .get("max_operation_bytes")
        .and_then(Value::as_u64)
        .expect("validated output operation limit");
    let max_blob_bytes = output_limits
        .get("max_blob_bytes")
        .and_then(Value::as_u64)
        .expect("validated output blob limit");
    if max_event_bytes >= cokernel_protocol::DEFAULT_MAX_FRAME_BYTES as u64 {
        return Err(WorkerTransportError::Protocol(format!(
            "worker handshake max_event_bytes {max_event_bytes} must be smaller than frame limit {}",
            cokernel_protocol::DEFAULT_MAX_FRAME_BYTES
        )));
    }
    if max_operation_bytes < max_event_bytes {
        return Err(WorkerTransportError::Protocol(
            "worker handshake max_operation_bytes must be at least max_event_bytes".into(),
        ));
    }
    if max_blob_bytes > max_event_bytes {
        return Err(WorkerTransportError::Protocol(
            "worker handshake max_blob_bytes must not exceed max_event_bytes".into(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod handshake_tests {
    use super::*;
    use tokio::io::duplex;

    fn valid_handshake(
        session_id: SessionId,
        request_id: &str,
        heartbeat_interval_ms: u64,
    ) -> WorkerFrame {
        WorkerFrame::Response {
            protocol: WORKER_PROTOCOL_V1,
            id: request_id.into(),
            session_id: session_id.to_string(),
            ok: true,
            result: Some(json!({
                "protocol": WORKER_PROTOCOL_V1,
                "worker_version": "1.0.0a0",
                "capabilities": [
                    worker::method::EXECUTE,
                    worker::method::INSPECT_VARIABLES,
                    worker::method::GET_VARIABLE,
                    worker::method::RESET,
                    worker::method::SHUTDOWN,
                ],
                "heartbeat_interval_ms": heartbeat_interval_ms,
                "output_limits": {
                    "max_event_bytes": 6 * 1024 * 1024,
                    "max_operation_bytes": 24 * 1024 * 1024,
                    "max_blob_bytes": 4 * 1024 * 1024,
                },
            })),
            error: None,
        }
    }

    #[test]
    fn handshake_requires_capabilities_and_consistent_heartbeat() {
        let session_id = SessionId::new();
        let request_id = "handshake-test";
        let valid = valid_handshake(session_id, request_id, 2_000);
        assert!(validate_worker_handshake(
            &valid,
            session_id,
            request_id,
            Duration::from_secs(2),
        )
        .is_ok());

        let mut missing_capability = valid_handshake(session_id, request_id, 2_000);
        let WorkerFrame::Response { result, .. } = &mut missing_capability else {
            unreachable!();
        };
        result.as_mut().unwrap()["capabilities"] = json!([
            worker::method::EXECUTE,
            worker::method::INSPECT_VARIABLES,
            worker::method::RESET,
            worker::method::SHUTDOWN,
        ]);
        assert!(validate_worker_handshake(
            &missing_capability,
            session_id,
            request_id,
            Duration::from_secs(2),
        )
        .is_err());

        let inconsistent_heartbeat = valid_handshake(session_id, request_id, 1_000);
        assert!(validate_worker_handshake(
            &inconsistent_heartbeat,
            session_id,
            request_id,
            Duration::from_secs(2),
        )
        .is_err());
    }

    #[test]
    fn handshake_rejects_duplicate_capabilities_and_incoherent_output_limits() {
        let session_id = SessionId::new();
        let request_id = "handshake-test";

        let mut duplicate = valid_handshake(session_id, request_id, 2_000);
        let WorkerFrame::Response { result, .. } = &mut duplicate else {
            unreachable!();
        };
        result.as_mut().unwrap()["capabilities"] = json!([
            worker::method::EXECUTE,
            worker::method::EXECUTE,
            worker::method::INSPECT_VARIABLES,
            worker::method::GET_VARIABLE,
            worker::method::RESET,
            worker::method::SHUTDOWN,
        ]);
        assert!(validate_worker_handshake(
            &duplicate,
            session_id,
            request_id,
            Duration::from_secs(2),
        )
        .is_err());

        let mut incoherent = valid_handshake(session_id, request_id, 2_000);
        let WorkerFrame::Response { result, .. } = &mut incoherent else {
            unreachable!();
        };
        result.as_mut().unwrap()["output_limits"] = json!({
            "max_event_bytes": 1024,
            "max_operation_bytes": 512,
            "max_blob_bytes": 2048,
        });
        assert!(validate_worker_handshake(
            &incoherent,
            session_id,
            request_id,
            Duration::from_secs(2),
        )
        .is_err());
    }

    #[test]
    fn handshake_rejects_oversized_worker_metadata() {
        let session_id = SessionId::new();
        let request_id = "handshake-test";
        let mut oversized = valid_handshake(session_id, request_id, 2_000);
        let WorkerFrame::Response { result, .. } = &mut oversized else {
            unreachable!();
        };
        result.as_mut().unwrap()["worker_version"] =
            json!("v".repeat(MAX_WORKER_VERSION_CHARS + 1));
        assert!(validate_worker_handshake(
            &oversized,
            session_id,
            request_id,
            Duration::from_secs(2),
        )
        .is_err());
    }

    #[tokio::test]
    async fn handshake_reader_ignores_heartbeat_before_response() {
        let session_id = SessionId::new();
        let request_id = "handshake-test";
        let (mut writer, mut reader) = duplex(4096);
        let response = valid_handshake(session_id, request_id, 2_000);
        let heartbeat = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.to_string(),
            event: worker::event::HEARTBEAT.into(),
            payload: json!({"monotonic_ns": 1}),
        };
        let task = tokio::spawn(async move {
            write_worker_frame(&mut writer, &heartbeat).await.unwrap();
            write_worker_frame(&mut writer, &response).await.unwrap();
        });

        let actual = read_worker_handshake_response(&mut reader, session_id, request_id)
            .await
            .unwrap();
        task.await.unwrap();
        assert!(matches!(actual, WorkerFrame::Response { .. }));
    }

    #[tokio::test]
    async fn handshake_reader_does_not_echo_unexpected_response_payload() {
        let session_id = SessionId::new();
        let request_id = "handshake-test";
        let (mut writer, mut reader) = duplex(4096);
        let unexpected = WorkerFrame::Response {
            protocol: WORKER_PROTOCOL_V1,
            id: "stale-response".into(),
            session_id: session_id.to_string(),
            ok: true,
            result: Some(json!({"secret": "must-not-enter-diagnostics"})),
            error: None,
        };
        let task = tokio::spawn(async move {
            write_worker_frame(&mut writer, &unexpected).await.unwrap();
        });

        let error = read_worker_handshake_response(&mut reader, session_id, request_id)
            .await
            .unwrap_err();
        task.await.unwrap();
        let detail = error.to_string();
        assert!(detail.contains("stale-response"));
        assert!(!detail.contains("must-not-enter-diagnostics"));
    }
}
