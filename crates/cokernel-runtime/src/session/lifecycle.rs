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
    forget_oom_baseline(context.worker_pid);
    clear_current_operation(context);
    set_state(context, SessionState::Stopped);
    forget_session_event_tail(context.worker_pid);
}

async fn terminate_child(child: &mut Child) -> Option<ExitStatus> {
    let _ = child.start_kill();
    child.wait().await.ok()
}

async fn record_exit(context: &SessionActorContext, status: ExitStatus) {
    let runtime_event_context = cokernel_domain::RuntimeFailureContext {
        trigger: cokernel_domain::FailureTrigger::ProcessExit,
        detail: Some(format_exit_status_ref(&status)),
        session_state: observed_session_state(context),
        recent_session_events: take_session_event_tail(context.worker_pid),
    };
    let evidence = collect_failure_evidence(context.worker_pid, Some(runtime_event_context));
    let (classification, confidence) = apply_oom_classification(
        classify_exit_ref(&status),
        0.8,
        evidence.linux_oom_evidence.as_ref(),
    );
    let record = failure_record(
        context,
        status.code(),
        status.signal(),
        classification,
        confidence,
        evidence,
    )
    .await;
    finish_current_operation(context, OperationStatus::Failed);
    set_state(context, SessionState::Crashed);
    let _ = context.events.send(SessionEvent::Failure { record });
}

async fn record_crash(context: &SessionActorContext, child: &mut Child, error: SessionError) {
    let detail = error.to_string();
    let runtime_event_context = cokernel_domain::RuntimeFailureContext {
        trigger: failure_trigger(&error),
        detail: Some(detail.clone()),
        session_state: observed_session_state(context),
        recent_session_events: take_session_event_tail(context.worker_pid),
    };
    // Capture /proc/cgroup evidence before recovery termination whenever the worker is still alive.
    // This is intentionally best-effort and bounded; failure evidence collection must never block
    // worker cleanup or manufacture a root cause.
    let evidence = collect_failure_evidence(context.worker_pid, Some(runtime_event_context));
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
    let (base_classification, base_confidence) =
        crash_classification(status.as_ref(), supervisor_terminated);
    let (classification, confidence) = if supervisor_terminated {
        // If the worker was still alive, the Supervisor's recovery signal is the terminal action.
        // A cgroup OOM delta from another process must not be attributed to this worker.
        (base_classification, base_confidence)
    } else {
        apply_oom_classification(
            base_classification,
            base_confidence,
            evidence.linux_oom_evidence.as_ref(),
        )
    };
    let mut record = failure_record(
        context,
        status.as_ref().and_then(ExitStatus::code),
        status.as_ref().and_then(ExitStatusExt::signal),
        classification,
        confidence,
        evidence,
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
    evidence: FailureEvidenceSnapshot,
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
        worker_pid: Some(context.worker_pid),
        worker_memory_snapshot: evidence.worker_memory_snapshot,
        wsl_memory_snapshot: evidence.wsl_memory_snapshot,
        linux_oom_evidence: evidence.linux_oom_evidence,
        runtime_event_context: evidence.runtime_event_context,
        classification,
        confidence,
    }
}

#[derive(Debug, Default)]
struct FailureEvidenceSnapshot {
    worker_memory_snapshot: Option<cokernel_domain::ProcessMemorySnapshot>,
    wsl_memory_snapshot: Option<cokernel_domain::SystemMemorySnapshot>,
    linux_oom_evidence: Option<cokernel_domain::LinuxOomEvidence>,
    runtime_event_context: Option<cokernel_domain::RuntimeFailureContext>,
}

const RECENT_SESSION_EVENT_CAPACITY: usize = 64;
const RECENT_SESSION_EVENT_DETAIL_CHARS: usize = 160;

fn session_event_tails(
) -> &'static StdMutex<HashMap<u32, std::collections::VecDeque<cokernel_domain::SessionEvidenceEvent>>>
{
    static TAILS: std::sync::OnceLock<
        StdMutex<
            HashMap<u32, std::collections::VecDeque<cokernel_domain::SessionEvidenceEvent>>,
        >,
    > = std::sync::OnceLock::new();
    TAILS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn remember_session_event_tail(worker_pid: u32) {
    session_event_tails()
        .lock()
        .expect("Session event tail lock poisoned")
        .insert(
            worker_pid,
            std::collections::VecDeque::with_capacity(RECENT_SESSION_EVENT_CAPACITY),
        );
}

fn take_session_event_tail(worker_pid: u32) -> Vec<cokernel_domain::SessionEvidenceEvent> {
    session_event_tails()
        .lock()
        .expect("Session event tail lock poisoned")
        .remove(&worker_pid)
        .map(|tail| tail.into_iter().collect())
        .unwrap_or_default()
}

fn forget_session_event_tail(worker_pid: u32) {
    let _ = take_session_event_tail(worker_pid);
}

fn record_session_evidence(
    worker_pid: u32,
    kind: cokernel_domain::SessionEvidenceKind,
    operation_id: Option<OperationId>,
    detail: Option<String>,
) {
    let mut tails = session_event_tails()
        .lock()
        .expect("Session event tail lock poisoned");
    let Some(tail) = tails.get_mut(&worker_pid) else {
        return;
    };
    if tail.len() == RECENT_SESSION_EVENT_CAPACITY {
        tail.pop_front();
    }
    tail.push_back(cokernel_domain::SessionEvidenceEvent {
        timestamp: Utc::now(),
        kind,
        operation_id,
        detail: detail.map(|value| truncate_session_evidence_detail(&value)),
    });
}

fn truncate_session_evidence_detail(value: &str) -> String {
    if value.chars().count() <= RECENT_SESSION_EVENT_DETAIL_CHARS {
        return value.to_owned();
    }
    let mut truncated = value
        .chars()
        .take(RECENT_SESSION_EVENT_DETAIL_CHARS.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

fn oom_baselines() -> &'static StdMutex<HashMap<u32, cokernel_domain::LinuxOomEvidence>> {
    static BASELINES: std::sync::OnceLock<
        StdMutex<HashMap<u32, cokernel_domain::LinuxOomEvidence>>,
    > = std::sync::OnceLock::new();
    BASELINES.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn remember_oom_baseline(worker_pid: u32) {
    let Some(baseline) = read_linux_oom_evidence(worker_pid) else {
        return;
    };
    oom_baselines()
        .lock()
        .expect("OOM baseline lock poisoned")
        .insert(worker_pid, baseline);
}

fn take_oom_baseline(worker_pid: u32) -> Option<cokernel_domain::LinuxOomEvidence> {
    oom_baselines()
        .lock()
        .expect("OOM baseline lock poisoned")
        .remove(&worker_pid)
}

fn forget_oom_baseline(worker_pid: u32) {
    let _ = take_oom_baseline(worker_pid);
}

fn collect_failure_evidence(
    worker_pid: u32,
    runtime_event_context: Option<cokernel_domain::RuntimeFailureContext>,
) -> FailureEvidenceSnapshot {
    let baseline = take_oom_baseline(worker_pid);
    let linux_oom_evidence = read_linux_oom_evidence(worker_pid)
        .map(|current| correlate_linux_oom_evidence(current, baseline.as_ref()));
    FailureEvidenceSnapshot {
        worker_memory_snapshot: read_process_memory_snapshot(worker_pid),
        wsl_memory_snapshot: read_system_memory_snapshot(),
        linux_oom_evidence,
        runtime_event_context,
    }
}

fn observed_session_state(context: &SessionActorContext) -> SessionState {
    let current = context.state_tx.borrow();
    *current
}

fn failure_trigger(error: &SessionError) -> cokernel_domain::FailureTrigger {
    match error {
        SessionError::WorkerExitedDuringStartup(_) | SessionError::WorkerExited(_) => {
            cokernel_domain::FailureTrigger::ProcessExit
        }
        SessionError::WorkerDisconnected => cokernel_domain::FailureTrigger::WorkerDisconnected,
        SessionError::WorkerHeartbeatTimeout(_) => cokernel_domain::FailureTrigger::HeartbeatTimeout,
        SessionError::WorkerTransport(_) => cokernel_domain::FailureTrigger::WorkerProtocol,
        SessionError::Io(_) => cokernel_domain::FailureTrigger::RuntimeIo,
        SessionError::CommandChannelClosed | SessionError::StopTimeout(_) => {
            cokernel_domain::FailureTrigger::RuntimeControl
        }
        SessionError::StartupTimeout | SessionError::InvalidReady(_) => {
            cokernel_domain::FailureTrigger::Startup
        }
    }
}

fn read_process_memory_snapshot(pid: u32) -> Option<cokernel_domain::ProcessMemorySnapshot> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let snapshot = cokernel_domain::ProcessMemorySnapshot {
        rss_bytes: parse_kib_field(&status, "VmRSS:"),
        virtual_bytes: parse_kib_field(&status, "VmSize:"),
        swap_bytes: parse_kib_field(&status, "VmSwap:"),
    };
    if snapshot.rss_bytes.is_none()
        && snapshot.virtual_bytes.is_none()
        && snapshot.swap_bytes.is_none()
    {
        None
    } else {
        Some(snapshot)
    }
}

fn read_system_memory_snapshot() -> Option<cokernel_domain::SystemMemorySnapshot> {
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    let snapshot = cokernel_domain::SystemMemorySnapshot {
        total_bytes: parse_kib_field(&meminfo, "MemTotal:"),
        available_bytes: parse_kib_field(&meminfo, "MemAvailable:"),
        swap_total_bytes: parse_kib_field(&meminfo, "SwapTotal:"),
        swap_free_bytes: parse_kib_field(&meminfo, "SwapFree:"),
    };
    if snapshot.total_bytes.is_none()
        && snapshot.available_bytes.is_none()
        && snapshot.swap_total_bytes.is_none()
        && snapshot.swap_free_bytes.is_none()
    {
        None
    } else {
        Some(snapshot)
    }
}

fn parse_kib_field(contents: &str, field: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        let value = line.strip_prefix(field)?.split_whitespace().next()?;
        value.parse::<u64>().ok()?.checked_mul(1024)
    })
}

fn read_linux_oom_evidence(pid: u32) -> Option<cokernel_domain::LinuxOomEvidence> {
    let cgroup_path = read_cgroup_v2_path(pid).or_else(|| read_cgroup_v2_path(std::process::id()))?;
    let relative = cgroup_path.trim_start_matches('/');
    let events_path = Path::new("/sys/fs/cgroup")
        .join(relative)
        .join("memory.events");
    let events = fs::read_to_string(events_path).ok()?;
    Some(cokernel_domain::LinuxOomEvidence {
        cgroup_path,
        oom_count: parse_cgroup_counter(&events, "oom")?,
        oom_kill_count: parse_cgroup_counter(&events, "oom_kill")?,
        baseline_oom_count: None,
        baseline_oom_kill_count: None,
        oom_delta: None,
        oom_kill_delta: None,
    })
}

fn correlate_linux_oom_evidence(
    mut current: cokernel_domain::LinuxOomEvidence,
    baseline: Option<&cokernel_domain::LinuxOomEvidence>,
) -> cokernel_domain::LinuxOomEvidence {
    let Some(baseline) = baseline.filter(|baseline| baseline.cgroup_path == current.cgroup_path)
    else {
        return current;
    };
    current.baseline_oom_count = Some(baseline.oom_count);
    current.baseline_oom_kill_count = Some(baseline.oom_kill_count);
    current.oom_delta = current.oom_count.checked_sub(baseline.oom_count);
    current.oom_kill_delta = current
        .oom_kill_count
        .checked_sub(baseline.oom_kill_count);
    current
}

fn read_cgroup_v2_path(pid: u32) -> Option<String> {
    let cgroup = fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    cgroup.lines().find_map(|line| {
        line.strip_prefix("0::")
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
    })
}

fn parse_cgroup_counter(contents: &str, key: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != key {
            return None;
        }
        fields.next()?.parse::<u64>().ok()
    })
}

fn apply_oom_classification(
    classification: FailureClassification,
    confidence: f32,
    evidence: Option<&cokernel_domain::LinuxOomEvidence>,
) -> (FailureClassification, f32) {
    if evidence
        .and_then(|evidence| evidence.oom_kill_delta)
        .is_some_and(|delta| delta > 0)
    {
        (FailureClassification::OomSuspected, 0.85)
    } else {
        (classification, confidence)
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

fn classify_exit_ref(status: &ExitStatus) -> FailureClassification {
    if status.signal().is_some() {
        FailureClassification::ProcessSignal
    } else {
        FailureClassification::Unknown
    }
}

fn format_exit_status(status: ExitStatus) -> String {
    format_exit_status_ref(&status)
}

fn format_exit_status_ref(status: &ExitStatus) -> String {
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

#[cfg(test)]
mod failure_evidence_tests {
    use super::*;

    fn oom_evidence(path: &str, oom: u64, oom_kill: u64) -> cokernel_domain::LinuxOomEvidence {
        cokernel_domain::LinuxOomEvidence {
            cgroup_path: path.into(),
            oom_count: oom,
            oom_kill_count: oom_kill,
            baseline_oom_count: None,
            baseline_oom_kill_count: None,
            oom_delta: None,
            oom_kill_delta: None,
        }
    }

    #[test]
    fn parses_proc_kib_fields_as_bytes() {
        let sample = "Name:\tpython\nVmSize:\t 2048 kB\nVmRSS:\t 1024 kB\nVmSwap:\t 3 kB\n";
        assert_eq!(parse_kib_field(sample, "VmSize:"), Some(2 * 1024 * 1024));
        assert_eq!(parse_kib_field(sample, "VmRSS:"), Some(1024 * 1024));
        assert_eq!(parse_kib_field(sample, "VmSwap:"), Some(3 * 1024));
        assert_eq!(parse_kib_field(sample, "MemTotal:"), None);
    }

    #[test]
    fn parses_cgroup_v2_memory_event_counters() {
        let sample = "low 0\nhigh 2\nmax 4\noom 3\noom_kill 1\noom_group_kill 0\n";
        assert_eq!(parse_cgroup_counter(sample, "oom"), Some(3));
        assert_eq!(parse_cgroup_counter(sample, "oom_kill"), Some(1));
        assert_eq!(parse_cgroup_counter(sample, "missing"), None);
    }

    #[test]
    fn correlates_same_cgroup_against_session_start_baseline() {
        let baseline = oom_evidence("/user.slice/cokernel", 3, 1);
        let current = correlate_linux_oom_evidence(
            oom_evidence("/user.slice/cokernel", 5, 2),
            Some(&baseline),
        );
        assert_eq!(current.baseline_oom_count, Some(3));
        assert_eq!(current.baseline_oom_kill_count, Some(1));
        assert_eq!(current.oom_delta, Some(2));
        assert_eq!(current.oom_kill_delta, Some(1));
        assert_eq!(
            apply_oom_classification(
                FailureClassification::ProcessSignal,
                0.5,
                Some(&current),
            ),
            (FailureClassification::OomSuspected, 0.85)
        );
    }

    #[test]
    fn cgroup_change_or_counter_reset_does_not_invent_oom_causality() {
        let baseline = oom_evidence("/old", 10, 4);
        let changed = correlate_linux_oom_evidence(oom_evidence("/new", 11, 5), Some(&baseline));
        assert_eq!(changed.oom_kill_delta, None);
        assert_eq!(
            apply_oom_classification(FailureClassification::Unknown, 0.25, Some(&changed)),
            (FailureClassification::Unknown, 0.25)
        );

        let reset = correlate_linux_oom_evidence(oom_evidence("/old", 1, 0), Some(&baseline));
        assert_eq!(reset.oom_delta, None);
        assert_eq!(reset.oom_kill_delta, None);
    }

    #[test]
    fn failure_triggers_are_structured_from_session_errors() {
        assert_eq!(
            failure_trigger(&SessionError::WorkerDisconnected),
            cokernel_domain::FailureTrigger::WorkerDisconnected
        );
        assert_eq!(
            failure_trigger(&SessionError::WorkerHeartbeatTimeout(Duration::from_secs(10))),
            cokernel_domain::FailureTrigger::HeartbeatTimeout
        );
        assert_eq!(
            failure_trigger(&SessionError::CommandChannelClosed),
            cokernel_domain::FailureTrigger::RuntimeControl
        );
    }

    #[test]
    fn recent_session_event_tail_is_bounded_and_truncates_detail() {
        let pid = u32::MAX;
        remember_session_event_tail(pid);
        for index in 0..(RECENT_SESSION_EVENT_CAPACITY + 5) {
            record_session_evidence(
                pid,
                cokernel_domain::SessionEvidenceKind::StateChanged,
                None,
                Some(format!("{index}-{}", "x".repeat(RECENT_SESSION_EVENT_DETAIL_CHARS + 20))),
            );
        }
        let events = take_session_event_tail(pid);
        assert_eq!(events.len(), RECENT_SESSION_EVENT_CAPACITY);
        assert!(events[0].detail.as_deref().unwrap().starts_with("5-"));
        assert!(events.iter().all(|event| {
            event
                .detail
                .as_ref()
                .is_none_or(|detail| detail.chars().count() <= RECENT_SESSION_EVENT_DETAIL_CHARS)
        }));
    }
}
