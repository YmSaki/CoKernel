include!("handshake.rs");

#[derive(Clone)]
pub struct SessionSupervisor {
    config: SessionSupervisorConfig,
    sessions: Arc<Mutex<HashMap<SessionId, SessionHandle>>>,
    primary_by_notebook: Arc<Mutex<HashMap<NotebookId, SessionId>>>,
    start_lock: Arc<Mutex<()>>,
}

impl SessionSupervisor {
    pub fn new(config: SessionSupervisorConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            primary_by_notebook: Arc::new(Mutex::new(HashMap::new())),
            start_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn ensure_primary(
        &self,
        project: &Project,
        notebook_id: NotebookId,
    ) -> Result<SessionHandle, SessionError> {
        let _guard = self.start_lock.lock().await;
        if let Some(existing) = self.primary_handle(notebook_id).await {
            match existing.state() {
                SessionState::Stopped => {}
                // Crash recovery is explicit. Returning the crashed/error handle preserves
                // failure evidence and prevents an innocuous ensure call from silently
                // replacing the worker before a user/policy chooses restart.
                _ => return Ok(existing),
            }
        }

        let handle = self
            .start_worker(
                project,
                notebook_id,
                SessionId::new(),
                1,
                Utc::now(),
            )
            .await?;
        self.primary_by_notebook
            .lock()
            .await
            .insert(notebook_id, handle.session_id());
        self.sessions
            .lock()
            .await
            .insert(handle.session_id(), handle.clone());
        Ok(handle)
    }

    pub async fn get(&self, session_id: SessionId) -> Option<SessionHandle> {
        self.sessions.lock().await.get(&session_id).cloned()
    }

    pub async fn list(&self) -> Vec<ExecutionSession> {
        self.sessions
            .lock()
            .await
            .values()
            .map(SessionHandle::snapshot)
            .collect()
    }

    pub async fn restart_primary(
        &self,
        project: &Project,
        notebook_id: NotebookId,
    ) -> Result<SessionHandle, SessionError> {
        let _guard = self.start_lock.lock().await;
        let existing = self.primary_handle(notebook_id).await;

        let (session_id, worker_generation, started_at) = if let Some(existing) = existing {
            if !is_terminal_state(existing.state()) {
                if existing.state() != SessionState::Stopping {
                    existing.stop().await?;
                }
                wait_until_terminal(
                    &existing,
                    self.config.shutdown_timeout.saturating_add(RESTART_STOP_GRACE),
                )
                .await?;
            }
            (
                existing.session_id(),
                existing.snapshot().worker_generation.saturating_add(1),
                existing.started_at,
            )
        } else {
            (SessionId::new(), 1, Utc::now())
        };

        let handle = self
            .start_worker(
                project,
                notebook_id,
                session_id,
                worker_generation,
                started_at,
            )
            .await?;
        self.primary_by_notebook
            .lock()
            .await
            .insert(notebook_id, session_id);
        self.sessions.lock().await.insert(session_id, handle.clone());
        Ok(handle)
    }

    pub async fn mark_project_environment_stale(
        &self,
        project_id: ProjectId,
        current_generation: u64,
    ) {
        let handles = self
            .sessions
            .lock()
            .await
            .values()
            .filter(|handle| {
                handle.project_id == project_id
                    && handle.environment_generation != current_generation
                    && is_live_state(handle.state())
            })
            .cloned()
            .collect::<Vec<_>>();
        for handle in handles {
            let _ = handle.mark_environment_stale().await;
        }
    }

    async fn primary_handle(&self, notebook_id: NotebookId) -> Option<SessionHandle> {
        let session_id = self
            .primary_by_notebook
            .lock()
            .await
            .get(&notebook_id)
            .copied()?;
        self.get(session_id).await
    }

    async fn start_worker(
        &self,
        project: &Project,
        notebook_id: NotebookId,
        session_id: SessionId,
        worker_generation: u64,
        started_at: DateTime<Utc>,
    ) -> Result<SessionHandle, SessionError> {
        prepare_socket_dir(&self.config.socket_dir)?;
        let socket_path = self
            .config
            .socket_dir
            .join(format!("{session_id}-{worker_generation}.sock"));
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;

        let mut command = uv_worker_command(
            &self.config.uv_executable,
            Path::new(&project.root_path),
            &self.config.worker_package,
            &socket_path,
            session_id,
        );
        let mut child = command.spawn()?;
        let stderr_tail = Arc::new(Mutex::new(ByteTail::new(
            self.config.diagnostic_tail_bytes,
        )));
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(drain_tail(
                stdout,
                Arc::new(Mutex::new(ByteTail::new(8 * 1024))),
            ));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(drain_tail(stderr, stderr_tail.clone()));
        }

        let (stream, _) = tokio::select! {
            accepted = listener.accept() => accepted?,
            status = child.wait() => {
                let status = status?;
                let _ = fs::remove_file(&socket_path);
                return Err(SessionError::WorkerExitedDuringStartup(format_exit_status(status)));
            }
            _ = sleep(self.config.startup_timeout) => {
                terminate_child(&mut child).await;
                let _ = fs::remove_file(&socket_path);
                return Err(SessionError::StartupTimeout);
            }
        };

        let mut stream = stream;
        let ready = match timeout(self.config.startup_timeout, read_worker_frame(&mut stream)).await {
            Ok(Ok(Some(frame))) => frame,
            Ok(Ok(None)) => {
                terminate_child(&mut child).await;
                let _ = fs::remove_file(&socket_path);
                return Err(SessionError::InvalidReady(
                    "worker disconnected before ready".into(),
                ));
            }
            Ok(Err(error)) => {
                terminate_child(&mut child).await;
                let _ = fs::remove_file(&socket_path);
                return Err(error.into());
            }
            Err(_) => {
                terminate_child(&mut child).await;
                let _ = fs::remove_file(&socket_path);
                return Err(SessionError::StartupTimeout);
            }
        };
        let ready = match validate_ready(&ready, session_id) {
            Ok(ready) => ready,
            Err(error) => {
                terminate_child(&mut child).await;
                let _ = fs::remove_file(&socket_path);
                return Err(error);
            }
        };
        if self.config.heartbeat_timeout <= ready.heartbeat_interval {
            terminate_child(&mut child).await;
            let _ = fs::remove_file(&socket_path);
            return Err(SessionError::InvalidReady(format!(
                "heartbeat timeout {:?} must exceed worker interval {:?}",
                self.config.heartbeat_timeout, ready.heartbeat_interval
            )));
        }
        if let Err(error) = perform_worker_handshake(
            &mut stream,
            session_id,
            worker_generation,
            ready.heartbeat_interval,
            self.config.startup_timeout,
        )
        .await
        {
            terminate_child(&mut child).await;
            let _ = fs::remove_file(&socket_path);
            return Err(error);
        }
        let _ = fs::remove_file(&socket_path);

        let (reader, writer) = stream.into_split();
        let (worker_frames_tx, worker_frames_rx) = mpsc::channel(WORKER_FRAME_QUEUE_CAPACITY);
        tokio::spawn(read_worker_frames(reader, worker_frames_tx));

        let (state_tx, state_rx) = watch::channel(SessionState::Idle);
        let (execute_tx, execute_rx) = mpsc::channel(EXECUTE_QUEUE_CAPACITY);
        let (inspection_tx, inspection_rx) = mpsc::channel(INSPECTION_QUEUE_CAPACITY);
        let (control_tx, control_rx) = mpsc::channel(CONTROL_QUEUE_CAPACITY);
        let (events, _) = broadcast::channel(EVENT_QUEUE_CAPACITY);
        let current_operation = Arc::new(StdMutex::new(None));
        let current_cell_id = Arc::new(StdMutex::new(None));
        let worker_started_at = Utc::now();

        let handle = SessionHandle {
            session_id,
            project_id: project.project_id,
            notebook_id,
            worker_generation,
            environment_generation: project.environment_generation,
            started_at,
            state_rx,
            execute_tx,
            inspection_tx,
            control_tx,
            events: events.clone(),
            current_operation: current_operation.clone(),
        };

        let _ = events.send(SessionEvent::StateChanged {
            session_id,
            state: SessionState::Idle,
        });

        tokio::spawn(run_session_actor(
            SessionActorContext {
                session_id,
                worker_pid: ready.pid,
                worker_generation,
                worker_started_at,
                environment_generation: project.environment_generation,
                state_tx,
                events,
                current_operation,
                current_cell_id,
                stderr_tail,
                shutdown_timeout: self.config.shutdown_timeout,
                heartbeat_timeout: self.config.heartbeat_timeout,
            },
            writer,
            child,
            execute_rx,
            inspection_rx,
            control_rx,
            worker_frames_rx,
        ));

        Ok(handle)
    }
}

#[derive(Debug, Clone, Copy)]
struct WorkerReady {
    pid: u32,
    heartbeat_interval: Duration,
}

fn is_terminal_state(state: SessionState) -> bool {
    matches!(
        state,
        SessionState::Stopped | SessionState::Crashed | SessionState::Error
    )
}

fn is_live_state(state: SessionState) -> bool {
    !is_terminal_state(state)
}

async fn wait_until_terminal(
    handle: &SessionHandle,
    maximum: Duration,
) -> Result<(), SessionError> {
    timeout(maximum, async {
        loop {
            if is_terminal_state(handle.state()) {
                return;
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| SessionError::StopTimeout(handle.session_id()))?;
    Ok(())
}

fn prepare_socket_dir(path: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn validate_ready(
    frame: &WorkerFrame,
    expected_session_id: SessionId,
) -> Result<WorkerReady, SessionError> {
    match frame {
        WorkerFrame::Event {
            protocol,
            session_id,
            event,
            payload,
        } if *protocol == WORKER_PROTOCOL_V1
            && session_id == &expected_session_id.to_string()
            && event == worker::event::READY =>
        {
            let pid = payload
                .get("pid")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| SessionError::InvalidReady("ready event is missing worker pid".into()))?;
            let heartbeat_interval_ms = payload
                .get("heartbeat_interval_ms")
                .and_then(Value::as_u64)
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    SessionError::InvalidReady(
                        "ready event is missing a positive heartbeat interval".into(),
                    )
                })?;
            Ok(WorkerReady {
                pid,
                heartbeat_interval: Duration::from_millis(heartbeat_interval_ms),
            })
        }
        other => Err(SessionError::InvalidReady(format!("{other:?}"))),
    }
}
