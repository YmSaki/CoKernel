mod tests {
    use super::*;

    #[test]
    fn byte_tail_keeps_only_recent_bytes() {
        let mut tail = ByteTail::new(5);
        tail.push(b"abc");
        tail.push(b"def");
        assert_eq!(tail.text_lossy(), "bcdef");
        tail.push(b"0123456789");
        assert_eq!(tail.text_lossy(), "56789");
    }

    #[test]
    fn heartbeat_expiry_is_fail_closed() {
        let timeout = Duration::from_secs(10);
        let now = Instant::now();
        assert!(!heartbeat_expired(now, timeout));
        let expired = now.checked_sub(Duration::from_secs(11)).unwrap();
        assert!(heartbeat_expired(expired, timeout));
    }

    #[test]
    fn ready_event_requires_positive_heartbeat_contract() {
        let session_id = SessionId::new();
        let valid = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.to_string(),
            event: worker::event::READY.into(),
            payload: json!({
                "pid": 1234,
                "heartbeat_interval_ms": 2000,
            }),
        };
        let ready = validate_ready(&valid, session_id).unwrap();
        assert_eq!(ready.pid, 1234);
        assert_eq!(ready.heartbeat_interval, Duration::from_secs(2));

        let missing_heartbeat = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.to_string(),
            event: worker::event::READY.into(),
            payload: json!({"pid": 1234}),
        };
        assert!(validate_ready(&missing_heartbeat, session_id).is_err());
    }

    #[test]
    fn worker_frame_identity_is_strict_after_ready() {
        let session_id = SessionId::new();
        let valid = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.to_string(),
            event: worker::event::HEARTBEAT.into(),
            payload: json!({"monotonic_ns": 1}),
        };
        assert!(validate_worker_frame_identity(session_id, &valid).is_ok());

        let wrong_protocol = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1 + 1,
            session_id: session_id.to_string(),
            event: worker::event::HEARTBEAT.into(),
            payload: json!({"monotonic_ns": 1}),
        };
        assert!(validate_worker_frame_identity(session_id, &wrong_protocol).is_err());

        let wrong_session = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: SessionId::new().to_string(),
            event: worker::event::HEARTBEAT.into(),
            payload: json!({"monotonic_ns": 1}),
        };
        assert!(validate_worker_frame_identity(session_id, &wrong_session).is_err());
    }

    #[test]
    fn supervisor_termination_is_not_misclassified_as_process_signal() {
        let (classification, confidence) = crash_classification(None, true);
        assert_eq!(classification, FailureClassification::Unknown);
        assert_eq!(confidence, 0.25);
    }

    #[test]
    fn worker_frame_evidence_excludes_heartbeat_and_output_content() {
        let operation_id = OperationId::new();
        let heartbeat = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: SessionId::new().to_string(),
            event: worker::event::HEARTBEAT.into(),
            payload: json!({"monotonic_ms": 10}),
        };
        assert!(session_evidence_from_worker_frame(&heartbeat).is_none());

        let stdout = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: SessionId::new().to_string(),
            event: worker::event::STDOUT.into(),
            payload: json!({
                "operation_id": operation_id.to_string(),
                "text": "secret-output-must-not-enter-failure-tail",
            }),
        };
        assert!(session_evidence_from_worker_frame(&stdout).is_none());

        let error = WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: SessionId::new().to_string(),
            event: worker::event::ERROR.into(),
            payload: json!({
                "operation_id": operation_id.to_string(),
                "ename": "ValueError",
                "evalue": "secret-error-value",
            }),
        };
        let (kind, actual_operation, detail) =
            session_evidence_from_worker_frame(&error).expect("error evidence");
        assert_eq!(kind, cokernel_domain::SessionEvidenceKind::WorkerError);
        assert_eq!(actual_operation, Some(operation_id));
        assert_eq!(detail.as_deref(), Some("ValueError"));
    }

    #[test]
    fn failure_cell_id_is_unicode_safe_and_bounded() {
        assert_eq!(bounded_failure_cell_id(None), None);
        assert_eq!(
            bounded_failure_cell_id(Some("cell-1")).as_deref(),
            Some("cell-1")
        );

        let oversized = "界".repeat(FAILURE_CELL_ID_CHARS + 10);
        let bounded = bounded_failure_cell_id(Some(&oversized)).expect("bounded cell id");
        assert_eq!(bounded.chars().count(), FAILURE_CELL_ID_CHARS);
        assert!(bounded.ends_with('…'));
    }

    #[tokio::test]
    async fn failure_record_captures_worker_identity_and_cell_context() {
        let session_id = SessionId::new();
        let operation_id = OperationId::new();
        let worker_started_at = Utc::now();
        let (state_tx, _state_rx) = watch::channel(SessionState::Executing);
        let (events, _) = broadcast::channel(4);
        let context = SessionActorContext {
            session_id,
            worker_pid: 4242,
            worker_generation: 7,
            worker_started_at,
            environment_generation: 11,
            state_tx,
            events,
            current_operation: Arc::new(StdMutex::new(Some(operation_id))),
            current_cell_id: Arc::new(StdMutex::new(Some("cell-alpha".into()))),
            stderr_tail: Arc::new(Mutex::new(ByteTail::new(16))),
            shutdown_timeout: Duration::from_millis(10),
            heartbeat_timeout: Duration::from_secs(10),
        };

        let record = failure_record(
            &context,
            Some(23),
            None,
            FailureClassification::Unknown,
            0.25,
            FailureEvidenceSnapshot::default(),
        )
        .await;

        assert_eq!(record.session_id, Some(session_id));
        assert_eq!(record.operation_id, Some(operation_id));
        assert_eq!(record.worker_pid, Some(4242));
        assert_eq!(record.worker_generation, Some(7));
        assert_eq!(record.worker_started_at, Some(worker_started_at));
        assert_eq!(record.environment_generation, Some(11));
        assert_eq!(record.cell_id.as_deref(), Some("cell-alpha"));
    }

    #[tokio::test]
    async fn cancelling_pending_requests_closes_queue_and_finishes_each_operation() {
        let session_id = SessionId::new();
        let (state_tx, _state_rx) = watch::channel(SessionState::Idle);
        let (events, _) = broadcast::channel(16);
        let mut event_rx = events.subscribe();
        let current_operation = Arc::new(StdMutex::new(None));
        let context = SessionActorContext {
            session_id,
            worker_pid: 1,
            worker_generation: 1,
            worker_started_at: Utc::now(),
            environment_generation: 1,
            state_tx,
            events,
            current_operation,
            current_cell_id: Arc::new(StdMutex::new(None)),
            stderr_tail: Arc::new(Mutex::new(ByteTail::new(16))),
            shutdown_timeout: Duration::from_millis(10),
            heartbeat_timeout: Duration::from_secs(10),
        };
        let (execute_tx, mut execute_rx) = mpsc::channel(4);
        let first = OperationId::new();
        let second = OperationId::new();
        for operation_id in [first, second] {
            execute_tx
                .send(ExecuteRequest {
                    operation_id,
                    origin: ExecutionOrigin::Human,
                    cell_id: Some("cell".into()),
                    source: "1 + 1".into(),
                })
                .await
                .unwrap();
        }
        assert_eq!(queue_depth(&execute_tx), 2);

        cancel_pending_requests(&context, &mut execute_rx);

        assert_eq!(queue_depth(&execute_tx), 0);
        assert!(execute_tx
            .send(ExecuteRequest {
                operation_id: OperationId::new(),
                origin: ExecutionOrigin::Human,
                cell_id: None,
                source: "2 + 2".into(),
            })
            .await
            .is_err());

        let mut cancelled = Vec::new();
        for _ in 0..2 {
            match event_rx.recv().await.unwrap() {
                SessionEvent::OperationFinished {
                    session_id: actual_session_id,
                    operation_id,
                    status: OperationStatus::Cancelled,
                } => {
                    assert_eq!(actual_session_id, session_id);
                    cancelled.push(operation_id);
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert!(cancelled.contains(&first));
        assert!(cancelled.contains(&second));
    }
}
