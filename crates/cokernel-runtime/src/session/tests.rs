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
            state_tx,
            events,
            current_operation,
            stderr_tail: Arc::new(Mutex::new(ByteTail::new(16))),
            shutdown_timeout: Duration::from_millis(10),
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
