const FAILURE_CELL_ID_CHARS: usize = 256;

#[derive(Debug)]
struct ExecuteRequest {
    operation_id: OperationId,
    origin: ExecutionOrigin,
    cell_id: Option<String>,
    source: String,
}

#[derive(Debug)]
enum ControlRequest {
    Interrupt,
    Stop,
    MarkEnvironmentStale,
}

struct SessionActorContext {
    session_id: SessionId,
    worker_pid: u32,
    worker_generation: u64,
    worker_started_at: DateTime<Utc>,
    environment_generation: u64,
    state_tx: watch::Sender<SessionState>,
    events: broadcast::Sender<SessionEvent>,
    current_operation: Arc<StdMutex<Option<OperationId>>>,
    current_cell_id: Arc<StdMutex<Option<String>>>,
    stderr_tail: Arc<Mutex<ByteTail>>,
    shutdown_timeout: Duration,
    heartbeat_timeout: Duration,
}

async fn read_worker_frames(
    mut reader: OwnedReadHalf,
    sender: mpsc::Sender<Result<WorkerFrame, SessionError>>,
) {
    loop {
        let inbound = match read_worker_frame(&mut reader).await {
            Ok(Some(frame)) => Ok(frame),
            Ok(None) => Err(SessionError::WorkerDisconnected),
            Err(error) => Err(error.into()),
        };
        let terminal = inbound.is_err();
        if sender.send(inbound).await.is_err() || terminal {
            return;
        }
    }
}

async fn run_session_actor(
    context: SessionActorContext,
    mut writer: OwnedWriteHalf,
    mut child: Child,
    mut execute_rx: mpsc::Receiver<ExecuteRequest>,
    mut inspection_rx: mpsc::Receiver<InspectionRequest>,
    mut control_rx: mpsc::Receiver<ControlRequest>,
    mut worker_frames: mpsc::Receiver<Result<WorkerFrame, SessionError>>,
) {
    remember_session_event_tail(context.worker_pid);
    record_session_evidence(
        context.worker_pid,
        cokernel_domain::SessionEvidenceKind::StateChanged,
        None,
        Some("IDLE".into()),
    );
    remember_oom_baseline(context.worker_pid);
    let mut stale_environment = false;
    let mut last_worker_activity = Instant::now();
    let mut inspection_open = true;
    loop {
        tokio::select! {
            biased;
            control = control_rx.recv() => {
                match control {
                    Some(ControlRequest::Stop) | None => {
                        cancel_pending_work(&context, &mut execute_rx, &mut inspection_rx);
                        graceful_stop(&context, &mut writer, &mut child).await;
                        return;
                    }
                    Some(ControlRequest::MarkEnvironmentStale) => {
                        stale_environment = true;
                        set_state(&context, SessionState::StaleEnvironment);
                    }
                    Some(ControlRequest::Interrupt) => {}
                }
            }
            request = execute_rx.recv() => {
                let Some(request) = request else {
                    cancel_pending_inspections(&mut inspection_rx);
                    graceful_stop(&context, &mut writer, &mut child).await;
                    return;
                };
                match execute_one(
                    &context,
                    &mut writer,
                    &mut child,
                    &mut control_rx,
                    &mut worker_frames,
                    request,
                    &mut stale_environment,
                    &mut last_worker_activity,
                ).await {
                    Ok(ExecuteDisposition::Completed) => {}
                    Ok(ExecuteDisposition::Stopped) => {
                        cancel_pending_work(&context, &mut execute_rx, &mut inspection_rx);
                        return;
                    }
                    Err(error) => {
                        cancel_pending_work(&context, &mut execute_rx, &mut inspection_rx);
                        record_crash(&context, &mut child, error).await;
                        return;
                    }
                }
            }
            inspection = inspection_rx.recv(), if inspection_open => {
                let Some(InspectionRequest { method, payload, response }) = inspection else {
                    inspection_open = false;
                    continue;
                };
                match inspect_one(
                    &context,
                    &mut writer,
                    &mut child,
                    &mut control_rx,
                    &mut worker_frames,
                    method,
                    payload,
                    &mut stale_environment,
                    &mut last_worker_activity,
                ).await {
                    Ok(InspectionDisposition::Completed(result)) => {
                        let _ = response.send(result);
                    }
                    Ok(InspectionDisposition::Stopped) => {
                        let _ = response.send(Err(SessionInspectionError::CommandChannelClosed));
                        cancel_pending_work(&context, &mut execute_rx, &mut inspection_rx);
                        return;
                    }
                    Err(error) => {
                        let _ = response.send(Err(SessionInspectionError::Unavailable(error.to_string())));
                        cancel_pending_work(&context, &mut execute_rx, &mut inspection_rx);
                        record_crash(&context, &mut child, error).await;
                        return;
                    }
                }
            }
            inbound = worker_frames.recv() => {
                match observe_worker_frame(
                    &context,
                    inbound,
                    &mut last_worker_activity,
                    WorkerFrameExpectation::Idle,
                    None,
                    true,
                ) {
                    Ok(_) => {}
                    Err(error) => {
                        cancel_pending_work(&context, &mut execute_rx, &mut inspection_rx);
                        record_crash(&context, &mut child, error).await;
                        return;
                    }
                }
            }
            _ = sleep(HEALTH_POLL_INTERVAL) => {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        cancel_pending_work(&context, &mut execute_rx, &mut inspection_rx);
                        record_exit(&context, status).await;
                        return;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        cancel_pending_work(&context, &mut execute_rx, &mut inspection_rx);
                        record_crash(&context, &mut child, SessionError::Io(error)).await;
                        return;
                    }
                }
                if heartbeat_expired(last_worker_activity, context.heartbeat_timeout) {
                    cancel_pending_work(&context, &mut execute_rx, &mut inspection_rx);
                    record_crash(
                        &context,
                        &mut child,
                        SessionError::WorkerHeartbeatTimeout(context.heartbeat_timeout),
                    ).await;
                    return;
                }
            }
        }
    }
}

enum ExecuteDisposition {
    Completed,
    Stopped,
}

enum InspectionDisposition {
    Completed(Result<Value, SessionInspectionError>),
    Stopped,
}

#[derive(Debug, Clone, Copy)]
enum WorkerFrameExpectation<'a> {
    Idle,
    Execute(&'a str),
    Inspection(&'a str),
}

#[derive(Debug)]
struct ExecuteFinishedSummary {
    status: String,
    execution_count: u64,
    output_count: u64,
    output_truncated: bool,
    output_omitted_bytes: u64,
}

#[derive(Debug)]
struct ExecuteLifecycle {
    started: bool,
    next_sequence: u64,
    finished: Option<ExecuteFinishedSummary>,
}

impl ExecuteLifecycle {
    fn new() -> Self {
        Self {
            started: false,
            next_sequence: 1,
            finished: None,
        }
    }
}

async fn execute_one(
    context: &SessionActorContext,
    writer: &mut OwnedWriteHalf,
    child: &mut Child,
    control_rx: &mut mpsc::Receiver<ControlRequest>,
    worker_frames: &mut mpsc::Receiver<Result<WorkerFrame, SessionError>>,
    request: ExecuteRequest,
    stale_environment: &mut bool,
    last_worker_activity: &mut Instant,
) -> Result<ExecuteDisposition, SessionError> {
    *context
        .current_operation
        .lock()
        .expect("operation lock poisoned") = Some(request.operation_id);
    *context
        .current_cell_id
        .lock()
        .expect("cell lock poisoned") = bounded_failure_cell_id(request.cell_id.as_deref());
    set_state(context, SessionState::Executing);

    let request_id = request.operation_id.to_string();
    let frame = WorkerFrame::Request {
        protocol: WORKER_PROTOCOL_V1,
        id: request_id.clone(),
        session_id: context.session_id.to_string(),
        method: worker::method::EXECUTE.into(),
        payload: json!({
            "operation_id": request.operation_id.to_string(),
            "cell_id": request.cell_id,
            "source": request.source,
            "origin": request.origin,
        }),
    };
    write_worker_frame(writer, &frame).await?;

    let mut interrupted = false;
    let mut lifecycle = ExecuteLifecycle::new();
    loop {
        tokio::select! {
            control = control_rx.recv() => {
                match control {
                    Some(ControlRequest::Interrupt) => {
                        if !interrupted {
                            interrupted = true;
                            set_state(context, SessionState::Interrupting);
                            send_sigint(context.worker_pid).await?;
                        }
                    }
                    Some(ControlRequest::MarkEnvironmentStale) => {
                        *stale_environment = true;
                    }
                    Some(ControlRequest::Stop) | None => {
                        finish_operation(context, request.operation_id, OperationStatus::Cancelled);
                        graceful_stop(context, writer, child).await;
                        return Ok(ExecuteDisposition::Stopped);
                    }
                }
            }
            inbound = worker_frames.recv() => {
                let frame = observe_worker_frame(
                    context,
                    inbound,
                    last_worker_activity,
                    WorkerFrameExpectation::Execute(&request_id),
                    Some(&mut lifecycle),
                    true,
                )?;
                if let WorkerFrame::Response { ok, result, .. } = &frame {
                    let succeeded = *ok
                        && result
                            .as_ref()
                            .and_then(|value| value.get("status"))
                            .and_then(Value::as_str)
                            == Some("SUCCEEDED");
                    let status = if interrupted {
                        OperationStatus::Interrupted
                    } else if succeeded {
                        OperationStatus::Succeeded
                    } else {
                        OperationStatus::Failed
                    };
                    finish_operation(context, request.operation_id, status);
                    set_state(
                        context,
                        if *stale_environment {
                            SessionState::StaleEnvironment
                        } else {
                            SessionState::Idle
                        },
                    );
                    return Ok(ExecuteDisposition::Completed);
                }
            }
            _ = sleep(HEALTH_POLL_INTERVAL) => {
                if let Some(status) = child.try_wait()? {
                    return Err(SessionError::WorkerExited(format_exit_status(status)));
                }
                if heartbeat_expired(*last_worker_activity, context.heartbeat_timeout) {
                    return Err(SessionError::WorkerHeartbeatTimeout(context.heartbeat_timeout));
                }
            }
        }
    }
}

async fn inspect_one(
    context: &SessionActorContext,
    writer: &mut OwnedWriteHalf,
    child: &mut Child,
    control_rx: &mut mpsc::Receiver<ControlRequest>,
    worker_frames: &mut mpsc::Receiver<Result<WorkerFrame, SessionError>>,
    method: &'static str,
    payload: Value,
    stale_environment: &mut bool,
    last_worker_activity: &mut Instant,
) -> Result<InspectionDisposition, SessionError> {
    let request_id = format!("inspect-{}", OperationId::new());
    let frame = WorkerFrame::Request {
        protocol: WORKER_PROTOCOL_V1,
        id: request_id.clone(),
        session_id: context.session_id.to_string(),
        method: method.into(),
        payload,
    };
    write_worker_frame(writer, &frame).await?;

    loop {
        tokio::select! {
            control = control_rx.recv() => {
                match control {
                    Some(ControlRequest::Interrupt) => {}
                    Some(ControlRequest::MarkEnvironmentStale) => {
                        *stale_environment = true;
                        set_state(context, SessionState::StaleEnvironment);
                    }
                    Some(ControlRequest::Stop) | None => {
                        graceful_stop(context, writer, child).await;
                        return Ok(InspectionDisposition::Stopped);
                    }
                }
            }
            inbound = worker_frames.recv() => {
                let frame = observe_worker_frame(
                    context,
                    inbound,
                    last_worker_activity,
                    WorkerFrameExpectation::Inspection(&request_id),
                    None,
                    false,
                )?;
                let WorkerFrame::Response {
                    ok,
                    result,
                    error,
                    ..
                } = &frame else {
                    continue;
                };

                if *ok {
                    if error.is_some() {
                        return Err(WorkerTransportError::Protocol(
                            "successful inspection response included an error".into(),
                        ).into());
                    }
                    let result = result.clone().ok_or_else(|| {
                        WorkerTransportError::Protocol(
                            "successful inspection response omitted result".into(),
                        )
                    })?;
                    return Ok(InspectionDisposition::Completed(Ok(result)));
                }

                if result.is_some() {
                    return Err(WorkerTransportError::Protocol(
                        "failed inspection response included a result".into(),
                    ).into());
                }
                let error = error.as_ref().ok_or_else(|| {
                    WorkerTransportError::Protocol(
                        "failed inspection response omitted error".into(),
                    )
                })?;
                return Ok(InspectionDisposition::Completed(Err(
                    SessionInspectionError::WorkerRequest {
                        code: error.code.clone(),
                        summary: error.summary.clone(),
                    },
                )));
            }
            _ = sleep(HEALTH_POLL_INTERVAL) => {
                if let Some(status) = child.try_wait()? {
                    return Err(SessionError::WorkerExited(format_exit_status(status)));
                }
                if heartbeat_expired(*last_worker_activity, context.heartbeat_timeout) {
                    return Err(SessionError::WorkerHeartbeatTimeout(context.heartbeat_timeout));
                }
            }
        }
    }
}

fn observe_worker_frame(
    context: &SessionActorContext,
    inbound: Option<Result<WorkerFrame, SessionError>>,
    last_worker_activity: &mut Instant,
    expectation: WorkerFrameExpectation<'_>,
    execute_lifecycle: Option<&mut ExecuteLifecycle>,
    publish_event: bool,
) -> Result<WorkerFrame, SessionError> {
    let frame = inbound.ok_or(SessionError::WorkerDisconnected)??;
    validate_worker_frame_identity(context.session_id, &frame)?;
    validate_worker_frame_transaction(expectation, &frame)?;
    if let Some(lifecycle) = execute_lifecycle {
        validate_execute_lifecycle(lifecycle, &frame)?;
    }
    *last_worker_activity = Instant::now();
    if let Some((kind, operation_id, detail)) = session_evidence_from_worker_frame(&frame) {
        record_session_evidence(context.worker_pid, kind, operation_id, detail);
    }
    if publish_event {
        let _ = context.events.send(SessionEvent::WorkerFrame {
            session_id: context.session_id,
            frame: frame.clone(),
        });
    }
    Ok(frame)
}

fn validate_worker_frame_identity(
    expected_session_id: SessionId,
    frame: &WorkerFrame,
) -> Result<(), WorkerTransportError> {
    if frame.protocol() != WORKER_PROTOCOL_V1 {
        return Err(WorkerTransportError::Protocol(format!(
            "worker frame protocol {} does not match expected {}",
            frame.protocol(),
            WORKER_PROTOCOL_V1
        )));
    }
    let expected_session_id = expected_session_id.to_string();
    if frame.session_id() != expected_session_id.as_str() {
        return Err(WorkerTransportError::Protocol(format!(
            "worker frame session_id {} does not match expected {}",
            frame.session_id(),
            expected_session_id
        )));
    }
    Ok(())
}

fn validate_worker_frame_transaction(
    expectation: WorkerFrameExpectation<'_>,
    frame: &WorkerFrame,
) -> Result<(), WorkerTransportError> {
    match expectation {
        WorkerFrameExpectation::Idle => match frame {
            WorkerFrame::Event { event, .. }
                if event == worker::event::HEARTBEAT || event == worker::event::WORKER_WARNING =>
            {
                Ok(())
            }
            other => Err(WorkerTransportError::Protocol(format!(
                "worker sent transaction frame while Session was idle: {other:?}"
            ))),
        },
        WorkerFrameExpectation::Execute(request_id) => match frame {
            WorkerFrame::Event { event, .. }
                if event == worker::event::HEARTBEAT || event == worker::event::WORKER_WARNING =>
            {
                Ok(())
            }
            WorkerFrame::Event { event, payload, .. } if event != worker::event::READY => {
                let operation_id = payload
                    .get("operation_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        WorkerTransportError::Protocol(format!(
                            "worker execute event {event:?} omitted operation_id"
                        ))
                    })?;
                if operation_id == request_id {
                    Ok(())
                } else {
                    Err(WorkerTransportError::Protocol(format!(
                        "worker execute event operation_id {operation_id:?} does not match active request {request_id:?}"
                    )))
                }
            }
            WorkerFrame::Response { id, .. } => {
                if id == request_id {
                    Ok(())
                } else {
                    Err(WorkerTransportError::Protocol(format!(
                        "worker response id {id:?} does not match active execute request {request_id:?}"
                    )))
                }
            }
            other => Err(WorkerTransportError::Protocol(format!(
                "worker sent invalid frame during execute transaction: {other:?}"
            ))),
        },
        WorkerFrameExpectation::Inspection(request_id) => match frame {
            WorkerFrame::Event { event, .. }
                if event == worker::event::HEARTBEAT || event == worker::event::WORKER_WARNING =>
            {
                Ok(())
            }
            WorkerFrame::Response { id, .. } => {
                if id == request_id {
                    Ok(())
                } else {
                    Err(WorkerTransportError::Protocol(format!(
                        "worker response id {id:?} does not match active inspection request {request_id:?}"
                    )))
                }
            }
            other => Err(WorkerTransportError::Protocol(format!(
                "worker sent non-response frame during inspection transaction: {other:?}"
            ))),
        },
    }
}

fn validate_execute_lifecycle(
    lifecycle: &mut ExecuteLifecycle,
    frame: &WorkerFrame,
) -> Result<(), WorkerTransportError> {
    match frame {
        WorkerFrame::Event { event, .. }
            if event == worker::event::HEARTBEAT || event == worker::event::WORKER_WARNING =>
        {
            Ok(())
        }
        WorkerFrame::Event { event, .. } if event == worker::event::EXECUTION_STARTED => {
            if lifecycle.started || lifecycle.finished.is_some() {
                return Err(WorkerTransportError::Protocol(
                    "worker emitted execution_started more than once for one execute request".into(),
                ));
            }
            lifecycle.started = true;
            Ok(())
        }
        WorkerFrame::Event { event, payload, .. }
            if matches!(
                event.as_str(),
                worker::event::STDOUT
                    | worker::event::STDERR
                    | worker::event::EXECUTE_RESULT
                    | worker::event::DISPLAY_DATA
                    | worker::event::ERROR
            ) =>
        {
            if !lifecycle.started {
                return Err(WorkerTransportError::Protocol(format!(
                    "worker emitted execute output {event:?} before execution_started"
                )));
            }
            if lifecycle.finished.is_some() {
                return Err(WorkerTransportError::Protocol(format!(
                    "worker emitted execute output {event:?} after execution_finished"
                )));
            }
            let sequence = payload
                .get("sequence")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    WorkerTransportError::Protocol(format!(
                        "worker execute output {event:?} omitted a valid sequence"
                    ))
                })?;
            if sequence != lifecycle.next_sequence {
                return Err(WorkerTransportError::Protocol(format!(
                    "worker execute output sequence {sequence} does not match expected {}",
                    lifecycle.next_sequence
                )));
            }
            lifecycle.next_sequence = lifecycle.next_sequence.checked_add(1).ok_or_else(|| {
                WorkerTransportError::Protocol("worker execute output sequence overflowed u64".into())
            })?;
            Ok(())
        }
        WorkerFrame::Event { event, payload, .. }
            if event == worker::event::EXECUTION_FINISHED =>
        {
            if !lifecycle.started {
                return Err(WorkerTransportError::Protocol(
                    "worker emitted execution_finished before execution_started".into(),
                ));
            }
            if lifecycle.finished.is_some() {
                return Err(WorkerTransportError::Protocol(
                    "worker emitted execution_finished more than once for one execute request".into(),
                ));
            }
            let output_count = payload
                .get("output_count")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    WorkerTransportError::Protocol(
                        "worker execution_finished omitted output_count".into(),
                    )
                })?;
            let expected_output_count = lifecycle.next_sequence.saturating_sub(1);
            if output_count != expected_output_count {
                return Err(WorkerTransportError::Protocol(format!(
                    "worker execution_finished output_count {output_count} does not match observed {expected_output_count}"
                )));
            }
            let status = payload
                .get("status")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    WorkerTransportError::Protocol(
                        "worker execution_finished omitted status".into(),
                    )
                })?
                .to_owned();
            let execution_count = payload
                .get("execution_count")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    WorkerTransportError::Protocol(
                        "worker execution_finished omitted execution_count".into(),
                    )
                })?;
            let output_truncated = payload
                .get("output_truncated")
                .and_then(Value::as_bool)
                .ok_or_else(|| {
                    WorkerTransportError::Protocol(
                        "worker execution_finished omitted output_truncated".into(),
                    )
                })?;
            let output_omitted_bytes = payload
                .get("output_omitted_bytes")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    WorkerTransportError::Protocol(
                        "worker execution_finished omitted output_omitted_bytes".into(),
                    )
                })?;
            lifecycle.finished = Some(ExecuteFinishedSummary {
                status,
                execution_count,
                output_count,
                output_truncated,
                output_omitted_bytes,
            });
            Ok(())
        }
        WorkerFrame::Response {
            ok,
            result,
            error,
            ..
        } => {
            if !lifecycle.started {
                return Err(WorkerTransportError::Protocol(
                    "worker returned execute response before execution_started".into(),
                ));
            }
            let finished = lifecycle.finished.as_ref().ok_or_else(|| {
                WorkerTransportError::Protocol(
                    "worker returned execute response before execution_finished".into(),
                )
            })?;
            if !*ok || error.is_some() {
                return Err(WorkerTransportError::Protocol(
                    "started execute request must finish with a successful response envelope".into(),
                ));
            }
            let result = result.as_ref().and_then(Value::as_object).ok_or_else(|| {
                WorkerTransportError::Protocol(
                    "worker execute response result must be an object".into(),
                )
            })?;
            let status = result.get("status").and_then(Value::as_str).ok_or_else(|| {
                WorkerTransportError::Protocol("worker execute response omitted status".into())
            })?;
            if status != finished.status {
                return Err(WorkerTransportError::Protocol(format!(
                    "worker execute response status {status:?} does not match execution_finished {:?}",
                    finished.status
                )));
            }
            let execution_count = result
                .get("execution_count")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    WorkerTransportError::Protocol(
                        "worker execute response omitted execution_count".into(),
                    )
                })?;
            if execution_count != finished.execution_count {
                return Err(WorkerTransportError::Protocol(format!(
                    "worker execute response execution_count {execution_count} does not match execution_finished {}",
                    finished.execution_count
                )));
            }
            let output_truncated = result
                .get("output_truncated")
                .and_then(Value::as_bool)
                .ok_or_else(|| {
                    WorkerTransportError::Protocol(
                        "worker execute response omitted output_truncated".into(),
                    )
                })?;
            if output_truncated != finished.output_truncated {
                return Err(WorkerTransportError::Protocol(
                    "worker execute response output_truncated does not match execution_finished"
                        .into(),
                ));
            }
            let output_omitted_bytes = result
                .get("output_omitted_bytes")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    WorkerTransportError::Protocol(
                        "worker execute response omitted output_omitted_bytes".into(),
                    )
                })?;
            if output_omitted_bytes != finished.output_omitted_bytes {
                return Err(WorkerTransportError::Protocol(format!(
                    "worker execute response output_omitted_bytes {output_omitted_bytes} does not match execution_finished {}",
                    finished.output_omitted_bytes
                )));
            }
            let operation_id = result
                .get("operation_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    WorkerTransportError::Protocol(
                        "worker execute response omitted operation_id".into(),
                    )
                })?;
            if operation_id.is_empty() {
                return Err(WorkerTransportError::Protocol(
                    "worker execute response operation_id must be non-empty".into(),
                ));
            }
            if finished.output_count != lifecycle.next_sequence.saturating_sub(1) {
                return Err(WorkerTransportError::Protocol(
                    "worker execute lifecycle output count changed after execution_finished".into(),
                ));
            }
            Ok(())
        }
        other => Err(WorkerTransportError::Protocol(format!(
            "worker emitted invalid frame in execute lifecycle: {other:?}"
        ))),
    }
}

fn session_evidence_from_worker_frame(
    frame: &WorkerFrame,
) -> Option<(
    cokernel_domain::SessionEvidenceKind,
    Option<OperationId>,
    Option<String>,
)> {
    match frame {
        WorkerFrame::Event { event, payload, .. } => {
            let kind = match event.as_str() {
                worker::event::EXECUTION_STARTED => {
                    cokernel_domain::SessionEvidenceKind::WorkerExecutionStarted
                }
                worker::event::EXECUTION_FINISHED => {
                    cokernel_domain::SessionEvidenceKind::WorkerExecutionFinished
                }
                worker::event::WORKER_WARNING => cokernel_domain::SessionEvidenceKind::WorkerWarning,
                worker::event::ERROR => cokernel_domain::SessionEvidenceKind::WorkerError,
                _ => return None,
            };
            let operation_id = payload
                .get("operation_id")
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<OperationId>().ok());
            let detail = match event.as_str() {
                worker::event::EXECUTION_FINISHED => payload
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                worker::event::ERROR => payload
                    .get("ename")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                _ => Some(event.clone()),
            };
            Some((kind, operation_id, detail))
        }
        WorkerFrame::Response { id, ok, .. } => Some((
            cokernel_domain::SessionEvidenceKind::WorkerResponse,
            id.parse::<OperationId>().ok(),
            Some(if *ok { "ok" } else { "error" }.into()),
        )),
        WorkerFrame::Request { .. } => None,
    }
}

fn bounded_failure_cell_id(value: Option<&str>) -> Option<String> {
    value.map(|value| {
        if value.chars().count() <= FAILURE_CELL_ID_CHARS {
            return value.to_owned();
        }
        let mut truncated = value
            .chars()
            .take(FAILURE_CELL_ID_CHARS.saturating_sub(1))
            .collect::<String>();
        truncated.push('…');
        truncated
    })
}

fn heartbeat_expired(last_worker_activity: Instant, heartbeat_timeout: Duration) -> bool {
    last_worker_activity.elapsed() >= heartbeat_timeout
}

fn finish_operation(
    context: &SessionActorContext,
    operation_id: OperationId,
    status: OperationStatus,
) {
    record_session_evidence(
        context.worker_pid,
        cokernel_domain::SessionEvidenceKind::OperationFinished,
        Some(operation_id),
        Some(format!("{status:?}")),
    );
    let _ = context.events.send(SessionEvent::OperationFinished {
        session_id: context.session_id,
        operation_id,
        status,
    });
    clear_current_operation(context);
}

fn finish_current_operation(context: &SessionActorContext, status: OperationStatus) {
    let operation_id = *context
        .current_operation
        .lock()
        .expect("operation lock poisoned");
    if let Some(operation_id) = operation_id {
        finish_operation(context, operation_id, status);
    }
}

fn cancel_pending_requests(
    context: &SessionActorContext,
    execute_rx: &mut mpsc::Receiver<ExecuteRequest>,
) {
    execute_rx.close();
    while let Ok(request) = execute_rx.try_recv() {
        record_session_evidence(
            context.worker_pid,
            cokernel_domain::SessionEvidenceKind::OperationFinished,
            Some(request.operation_id),
            Some("Cancelled".into()),
        );
        let _ = context.events.send(SessionEvent::OperationFinished {
            session_id: context.session_id,
            operation_id: request.operation_id,
            status: OperationStatus::Cancelled,
        });
    }
}

fn cancel_pending_inspections(inspection_rx: &mut mpsc::Receiver<InspectionRequest>) {
    inspection_rx.close();
    while let Ok(request) = inspection_rx.try_recv() {
        let _ = request
            .response
            .send(Err(SessionInspectionError::CommandChannelClosed));
    }
}

fn cancel_pending_work(
    context: &SessionActorContext,
    execute_rx: &mut mpsc::Receiver<ExecuteRequest>,
    inspection_rx: &mut mpsc::Receiver<InspectionRequest>,
) {
    cancel_pending_requests(context, execute_rx);
    cancel_pending_inspections(inspection_rx);
}

fn clear_current_operation(context: &SessionActorContext) {
    *context
        .current_operation
        .lock()
        .expect("operation lock poisoned") = None;
    *context
        .current_cell_id
        .lock()
        .expect("cell lock poisoned") = None;
}

fn set_state(context: &SessionActorContext, state: SessionState) {
    context.state_tx.send_replace(state);
    record_session_evidence(
        context.worker_pid,
        cokernel_domain::SessionEvidenceKind::StateChanged,
        None,
        Some(format!("{state:?}")),
    );
    let _ = context.events.send(SessionEvent::StateChanged {
        session_id: context.session_id,
        state,
    });
}

async fn send_sigint(pid: u32) -> Result<(), std::io::Error> {
    send_signal(pid, "INT").await
}

async fn send_signal(pid: u32, signal: &str) -> Result<(), std::io::Error> {
    let status = Command::new("/bin/kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "kill -{signal} {pid} returned {status}"
        )))
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;

    fn heartbeat(session_id: SessionId) -> WorkerFrame {
        WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.to_string(),
            event: worker::event::HEARTBEAT.into(),
            payload: json!({"monotonic_ns": 1}),
        }
    }

    fn execute_event(session_id: SessionId, operation_id: &str) -> WorkerFrame {
        WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.to_string(),
            event: worker::event::STDOUT.into(),
            payload: json!({
                "operation_id": operation_id,
                "sequence": 1,
                "text": "ok",
            }),
        }
    }

    fn response(session_id: SessionId, request_id: &str) -> WorkerFrame {
        WorkerFrame::Response {
            protocol: WORKER_PROTOCOL_V1,
            id: request_id.into(),
            session_id: session_id.to_string(),
            ok: true,
            result: Some(json!({"status": "SUCCEEDED"})),
            error: None,
        }
    }

    fn lifecycle_event(
        session_id: SessionId,
        operation_id: &str,
        event: &str,
        payload: Value,
    ) -> WorkerFrame {
        let mut payload = payload.as_object().cloned().unwrap_or_default();
        payload.insert("operation_id".into(), json!(operation_id));
        WorkerFrame::Event {
            protocol: WORKER_PROTOCOL_V1,
            session_id: session_id.to_string(),
            event: event.into(),
            payload: Value::Object(payload),
        }
    }

    fn lifecycle_response(
        session_id: SessionId,
        operation_id: &str,
        status: &str,
        execution_count: u64,
        output_truncated: bool,
        output_omitted_bytes: u64,
    ) -> WorkerFrame {
        WorkerFrame::Response {
            protocol: WORKER_PROTOCOL_V1,
            id: operation_id.into(),
            session_id: session_id.to_string(),
            ok: true,
            result: Some(json!({
                "operation_id": operation_id,
                "status": status,
                "execution_count": execution_count,
                "output_truncated": output_truncated,
                "output_omitted_bytes": output_omitted_bytes,
            })),
            error: None,
        }
    }

    #[test]
    fn idle_accepts_only_background_worker_events() {
        let session_id = SessionId::new();
        assert!(validate_worker_frame_transaction(
            WorkerFrameExpectation::Idle,
            &heartbeat(session_id),
        )
        .is_ok());

        let stale = response(session_id, "stale-request");
        assert!(validate_worker_frame_transaction(WorkerFrameExpectation::Idle, &stale).is_err());
    }

    #[test]
    fn execute_frames_must_match_active_operation() {
        let session_id = SessionId::new();
        let active = OperationId::new().to_string();
        let other = OperationId::new().to_string();

        assert!(validate_worker_frame_transaction(
            WorkerFrameExpectation::Execute(&active),
            &execute_event(session_id, &active),
        )
        .is_ok());
        assert!(validate_worker_frame_transaction(
            WorkerFrameExpectation::Execute(&active),
            &response(session_id, &active),
        )
        .is_ok());
        assert!(validate_worker_frame_transaction(
            WorkerFrameExpectation::Execute(&active),
            &execute_event(session_id, &other),
        )
        .is_err());
        assert!(validate_worker_frame_transaction(
            WorkerFrameExpectation::Execute(&active),
            &response(session_id, &other),
        )
        .is_err());
    }

    #[test]
    fn inspection_accepts_only_matching_response_or_background_events() {
        let session_id = SessionId::new();
        let active = "inspect-active";

        assert!(validate_worker_frame_transaction(
            WorkerFrameExpectation::Inspection(active),
            &heartbeat(session_id),
        )
        .is_ok());
        assert!(validate_worker_frame_transaction(
            WorkerFrameExpectation::Inspection(active),
            &response(session_id, active),
        )
        .is_ok());
        assert!(validate_worker_frame_transaction(
            WorkerFrameExpectation::Inspection(active),
            &response(session_id, "inspect-stale"),
        )
        .is_err());
        assert!(validate_worker_frame_transaction(
            WorkerFrameExpectation::Inspection(active),
            &execute_event(session_id, active),
        )
        .is_err());
    }

    #[test]
    fn execute_lifecycle_requires_ordered_outputs_and_matching_terminal_response() {
        let session_id = SessionId::new();
        let operation_id = OperationId::new().to_string();
        let mut lifecycle = ExecuteLifecycle::new();

        let started = lifecycle_event(
            session_id,
            &operation_id,
            worker::event::EXECUTION_STARTED,
            json!({}),
        );
        assert!(validate_execute_lifecycle(&mut lifecycle, &started).is_ok());

        for sequence in 1..=2 {
            let output = lifecycle_event(
                session_id,
                &operation_id,
                worker::event::STDOUT,
                json!({"sequence": sequence, "text": "ok"}),
            );
            assert!(validate_execute_lifecycle(&mut lifecycle, &output).is_ok());
        }

        let finished = lifecycle_event(
            session_id,
            &operation_id,
            worker::event::EXECUTION_FINISHED,
            json!({
                "status": "SUCCEEDED",
                "execution_count": 7,
                "output_count": 2,
                "output_truncated": false,
                "output_omitted_bytes": 0,
                "output_truncation_reasons": [],
            }),
        );
        assert!(validate_execute_lifecycle(&mut lifecycle, &finished).is_ok());

        let response = lifecycle_response(
            session_id,
            &operation_id,
            "SUCCEEDED",
            7,
            false,
            0,
        );
        assert!(validate_execute_lifecycle(&mut lifecycle, &response).is_ok());
    }

    #[test]
    fn execute_lifecycle_rejects_output_before_start_sequence_gaps_and_bad_output_count() {
        let session_id = SessionId::new();
        let operation_id = OperationId::new().to_string();
        let output = lifecycle_event(
            session_id,
            &operation_id,
            worker::event::STDOUT,
            json!({"sequence": 1, "text": "early"}),
        );
        assert!(validate_execute_lifecycle(&mut ExecuteLifecycle::new(), &output).is_err());

        let mut lifecycle = ExecuteLifecycle::new();
        let started = lifecycle_event(
            session_id,
            &operation_id,
            worker::event::EXECUTION_STARTED,
            json!({}),
        );
        validate_execute_lifecycle(&mut lifecycle, &started).unwrap();
        let gap = lifecycle_event(
            session_id,
            &operation_id,
            worker::event::STDOUT,
            json!({"sequence": 2, "text": "gap"}),
        );
        assert!(validate_execute_lifecycle(&mut lifecycle, &gap).is_err());

        let mut lifecycle = ExecuteLifecycle::new();
        validate_execute_lifecycle(&mut lifecycle, &started).unwrap();
        let output = lifecycle_event(
            session_id,
            &operation_id,
            worker::event::STDOUT,
            json!({"sequence": 1, "text": "ok"}),
        );
        validate_execute_lifecycle(&mut lifecycle, &output).unwrap();
        let bad_finished = lifecycle_event(
            session_id,
            &operation_id,
            worker::event::EXECUTION_FINISHED,
            json!({
                "status": "SUCCEEDED",
                "execution_count": 1,
                "output_count": 0,
                "output_truncated": false,
                "output_omitted_bytes": 0,
                "output_truncation_reasons": [],
            }),
        );
        assert!(validate_execute_lifecycle(&mut lifecycle, &bad_finished).is_err());
    }

    #[test]
    fn execute_lifecycle_rejects_response_before_finished_and_terminal_mismatch() {
        let session_id = SessionId::new();
        let operation_id = OperationId::new().to_string();
        let started = lifecycle_event(
            session_id,
            &operation_id,
            worker::event::EXECUTION_STARTED,
            json!({}),
        );
        let mut lifecycle = ExecuteLifecycle::new();
        validate_execute_lifecycle(&mut lifecycle, &started).unwrap();
        let response = lifecycle_response(
            session_id,
            &operation_id,
            "SUCCEEDED",
            1,
            false,
            0,
        );
        assert!(validate_execute_lifecycle(&mut lifecycle, &response).is_err());

        let finished = lifecycle_event(
            session_id,
            &operation_id,
            worker::event::EXECUTION_FINISHED,
            json!({
                "status": "FAILED",
                "execution_count": 1,
                "output_count": 0,
                "output_truncated": false,
                "output_omitted_bytes": 0,
                "output_truncation_reasons": [],
            }),
        );
        validate_execute_lifecycle(&mut lifecycle, &finished).unwrap();
        assert!(validate_execute_lifecycle(&mut lifecycle, &response).is_err());
    }
}
