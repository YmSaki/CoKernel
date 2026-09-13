async fn graceful_stop(
    context: &SessionActorContext,
    writer: &mut OwnedWriteHalf,
    child: &mut Child,
) {
    set_state(context, SessionState::Stopping);
    let request = WorkerFrame::Request {
        protocol: WORKER_PROTOCOL_V1,
        id: format!("shutdown-{}", context.session_id),
        session_id: context.session_id.to_string(),
        method: worker::method::SHUTDOWN.into(),
        payload: json!({}),
    };
    let _ = write_worker_frame(writer, &request).await;
    match timeout(context.shutdown_timeout, child.wait()).await {
        Ok(Ok(_)) => {}
        _ => {
            let _ = send_signal(context.worker_pid, "TERM").await;
            match timeout(Duration::from_millis(250), child.wait()).await {
                Ok(Ok(_)) => {}
                _ => {
                    let _ = terminate_child(child).await;
                }
            }
        }
    }
    clear_current_operation(context);
    set_state(context, SessionState::Stopped);
}

async fn terminate_child(child: &mut Child) -> Option<ExitStatus> {
    let _ = child.start_kill();
    child.wait().await.ok()
}

async fn record_exit(context: &SessionActorContext, status: ExitStatus) {
    let record = failure_record(
        context,
        status.code(),
        status.signal(),
        classify_exit(status),
        0.8,
    )
    .await;
    finish_current_operation(context, OperationStatus::Failed);
    set_state(context, SessionState::Crashed);
    let _ = context.events.send(SessionEvent::Failure { record });
}

async fn record_crash(context: &SessionActorContext, child: &mut Child, detail: String) {
    let observed_status = child.try_wait().ok().flatten();
    let supervisor_terminated = observed_status.is_none();
    if supervisor_terminated {
        let _ = send_signal(context.worker_pid, "TERM").await;
    }
    let status = match observed_status {
        Some(status) => Some(status),
        None => match timeout(Duration::from_millis(250), child.wait()).await {
            Ok(Ok(status)) => Some(status),
            _ => terminate_child(child).await,
        },
    };
    let (classification, confidence) = crash_classification(status.as_ref(), supervisor_terminated);
    let mut record = failure_record(
        context,
        status.as_ref().and_then(ExitStatus::code),
        status.as_ref().and_then(ExitStatusExt::signal),
        classification,
        confidence,
    )
    .await;
    if !detail.is_empty() {
        if !record.last_stderr.is_empty() {
            record.last_stderr.push('\n');
        }
        record.last_stderr.push_str(&detail);
    }
    finish_current_operation(context, OperationStatus::Failed);
    set_state(context, SessionState::Crashed);
    let _ = context.events.send(SessionEvent::Failure { record });
}

async fn failure_record(
    context: &SessionActorContext,
    exit_code: Option<i32>,
    signal: Option<i32>,
    classification: FailureClassification,
    confidence: f32,
) -> FailureRecord {
    let last_stderr = context.stderr_tail.lock().await.text_lossy();
    FailureRecord {
        failure_id: FailureId::new(),
        component: "session-worker".into(),
        session_id: Some(context.session_id),
        operation_id: *context
            .current_operation
            .lock()
            .expect("operation lock poisoned"),
        timestamp: Utc::now(),
        exit_code,
        signal,
        last_stderr,
        classification,
        confidence,
    }
}

fn crash_classification(
    status: Option<&ExitStatus>,
    supervisor_terminated: bool,
) -> (FailureClassification, f32) {
    if supervisor_terminated {
        // A signal sent by the Supervisor is recovery action, not root-cause evidence.
        return (FailureClassification::Unknown, 0.25);
    }
    match status {
        Some(status) => (classify_exit_ref(status), 0.5),
        None => (FailureClassification::Unknown, 0.25),
    }
}

fn classify_exit(status: ExitStatus) -> FailureClassification {
    classify_exit_ref(&status)
}

fn classify_exit_ref(status: &ExitStatus) -> FailureClassification {
    if status.signal().is_some() {
        FailureClassification::ProcessSignal
    } else {
        FailureClassification::Unknown
    }
}

fn format_exit_status(status: ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit code {code}"),
        (_, Some(signal)) => format!("signal {signal}"),
        _ => status.to_string(),
    }
}

struct ByteTail {
    bytes: Vec<u8>,
    maximum: usize,
}

impl ByteTail {
    fn new(maximum: usize) -> Self {
        Self {
            bytes: Vec::new(),
            maximum,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        if self.maximum == 0 {
            return;
        }
        if chunk.len() >= self.maximum {
            self.bytes.clear();
            self.bytes
                .extend_from_slice(&chunk[chunk.len() - self.maximum..]);
            return;
        }
        let overflow = self
            .bytes
            .len()
            .saturating_add(chunk.len())
            .saturating_sub(self.maximum);
        if overflow > 0 {
            self.bytes.drain(..overflow);
        }
        self.bytes.extend_from_slice(chunk);
    }

    fn text_lossy(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

async fn drain_tail<R>(mut reader: R, tail: Arc<Mutex<ByteTail>>)
where
    R: AsyncRead + Unpin,
{
    let mut buffer = [0_u8; 4096];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => tail.lock().await.push(&buffer[..read]),
        }
    }
}
