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
    state_tx: watch::Sender<SessionState>,
    events: broadcast::Sender<SessionEvent>,
    current_operation: Arc<StdMutex<Option<OperationId>>>,
    stderr_tail: Arc<Mutex<ByteTail>>,
    shutdown_timeout: Duration,
}

async fn run_session_actor(
    context: SessionActorContext,
    mut stream: UnixStream,
    mut child: Child,
    mut execute_rx: mpsc::Receiver<ExecuteRequest>,
    mut control_rx: mpsc::Receiver<ControlRequest>,
) {
    let mut stale_environment = false;
    loop {
        tokio::select! {
            biased;
            control = control_rx.recv() => {
                match control {
                    Some(ControlRequest::Stop) | None => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        graceful_stop(&context, &mut stream, &mut child).await;
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
                    graceful_stop(&context, &mut stream, &mut child).await;
                    return;
                };
                match execute_one(
                    &context,
                    &mut stream,
                    &mut child,
                    &mut control_rx,
                    request,
                    &mut stale_environment,
                ).await {
                    Ok(ExecuteDisposition::Completed) => {}
                    Ok(ExecuteDisposition::Stopped) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        return;
                    }
                    Err(error) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        record_crash(&context, &mut child, error.to_string()).await;
                        return;
                    }
                }
            }
            _ = sleep(Duration::from_millis(250)) => {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        record_exit(&context, status).await;
                        return;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        cancel_pending_requests(&context, &mut execute_rx);
                        record_crash(&context, &mut child, error.to_string()).await;
                        return;
                    }
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
    stream: &mut UnixStream,
    child: &mut Child,
    control_rx: &mut mpsc::Receiver<ControlRequest>,
    request: ExecuteRequest,
    stale_environment: &mut bool,
) -> Result<ExecuteDisposition, SessionError> {
    *context
        .current_operation
        .lock()
        .expect("operation lock poisoned") = Some(request.operation_id);
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
    write_worker_frame(stream, &frame).await?;

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
                        graceful_stop(context, stream, child).await;
                        return Ok(ExecuteDisposition::Stopped);
                    }
                }
            }
            frame = read_worker_frame(stream) => {
                let Some(frame) = frame? else {
                    return Err(SessionError::WorkerDisconnected);
                };
                let _ = context.events.send(SessionEvent::WorkerFrame {
                    session_id: context.session_id,
                    frame: frame.clone(),
                });
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
            _ = sleep(Duration::from_millis(250)) => {
                if let Some(status) = child.try_wait()? {
                    return Err(SessionError::WorkerExited(format_exit_status(status)));
                }
            }
        }
    }
}

fn finish_operation(
    context: &SessionActorContext,
    operation_id: OperationId,
    status: OperationStatus,
) {
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
}

fn set_state(context: &SessionActorContext, state: SessionState) {
    context.state_tx.send_replace(state);
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

