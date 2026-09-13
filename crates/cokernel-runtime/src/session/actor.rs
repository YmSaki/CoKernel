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
    loop {
        tokio::select! {
            biased;
            control = control_rx.recv() => {
                match control {
                    Some(ControlRequest::Stop) | None => {
                        cancel_pending_requests(&context, &mut execute_rx);
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
                        cancel_pending_requests(&context, &mut execute_rx);
                        return;
                    }
                    Err(error) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        record_crash(&context, &mut child, error).await;
                        return;
                    }
                }
            }
            inbound = worker_frames.recv() => {
                match observe_worker_frame(&context, inbound, &mut last_worker_activity) {
                    Ok(_) => {}
                    Err(error) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        record_crash(&context, &mut child, error).await;
                        return;
                    }
                }
            }
            _ = sleep(HEALTH_POLL_INTERVAL) => {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        record_exit(&context, status).await;
                        return;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        record_crash(&context, &mut child, SessionError::Io(error)).await;
                        return;
                    }
                }
                if heartbeat_expired(last_worker_activity, context.heartbeat_timeout) {
                    cancel_pending_requests(&context, &mut execute_rx);
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
        .expect("cell lock poisoned") = request.cell_id.clone();
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
                let frame = observe_worker_frame(context, inbound, last_worker_activity)?;
                if let WorkerFrame::Response { id, ok, result, .. } = &frame {
                    if id == &request_id {
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
) -> Result<WorkerFrame, SessionError> {
    let frame = inbound.ok_or(SessionError::WorkerDisconnected)??;
    *last_worker_activity = Instant::now();
    if let Some((kind, operation_id, detail)) = session_evidence_from_worker_frame(&frame) {
        record_session_evidence(context.worker_pid, kind, operation_id, detail);
    }
    let _ = context.events.send(SessionEvent::WorkerFrame {
        session_id: context.session_id,
        frame: frame.clone(),
    });
    Ok(frame)
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
