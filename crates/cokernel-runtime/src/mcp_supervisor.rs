use crate::owned_child::OwnedChild;
#[cfg(test)]
mod tests;
use crate::events::EventHub;
use serde::{Deserialize, Serialize};
use std::{
    io,
    net::SocketAddr,
    os::unix::{fs::MetadataExt, process::ExitStatusExt},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::watch,
    task::JoinHandle,
    time::{Instant, sleep, timeout},
};

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum State {
    Disabled,
    Starting,
    Ready,
    Backoff,
    Failed,
    Stopping,
    Stopped,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Failure {
    pub code: &'static str,
    pub timestamp: String,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub state: State,
    pub generation: u64,
    pub pid: Option<u32>,
    pub address: Option<SocketAddr>,
    pub consecutive_failures: u32,
    pub failures: Vec<Failure>,
}
#[derive(Clone)]
pub struct Monitor {
    state: watch::Sender<Snapshot>,
    restart: watch::Sender<u64>,
    stop: watch::Sender<bool>,
    events: EventHub,
}
impl Default for Monitor {
    fn default() -> Self {
        Self::new(EventHub::default())
    }
}
impl Monitor {
    pub fn new(events: EventHub) -> Self {
        Self {
            state: watch::channel(Snapshot {
                state: State::Disabled,
                generation: 0,
                pid: None,
                address: None,
                consecutive_failures: 0,
                failures: vec![],
            })
            .0,
            restart: watch::channel(0).0,
            stop: watch::channel(false).0,
            events,
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.borrow().clone()
    }
    pub(crate) fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.state.subscribe()
    }
    fn publish(&self, snapshot: Snapshot) {
        self.state.send_replace(snapshot.clone());
        if let Ok(value) = serde_json::to_value(snapshot) {
            self.events.publish("mcp.state_changed", value);
        }
    }
    pub fn failed(&self, code: &'static str) {
        let mut state = self.snapshot();
        state.state = State::Failed;
        state.pid = None;
        state.address = None;
        record(&mut state, code, None);
        self.publish(state);
    }
    pub fn restart(&self) -> Result<(), &'static str> {
        if self.restart.receiver_count() == 0 || *self.stop.borrow() {
            return Err("MCP supervision is not running");
        }
        self.restart.send_modify(|n| *n = n.wrapping_add(1));
        Ok(())
    }
    pub async fn shutdown(&self) -> Result<(), &'static str> {
        if self.stop.receiver_count() == 0 {
            return if matches!(
                self.snapshot().state,
                State::Disabled | State::Failed | State::Stopped
            ) && self.snapshot().pid.is_none()
            {
                Ok(())
            } else {
                Err("MCP supervisor exited before cleanup")
            };
        }
        let mut state = self.state.subscribe();
        self.stop.send_replace(true);
        timeout(Duration::from_secs(3), async {
            loop {
                if state.borrow().state == State::Stopped {
                    return Ok(());
                }
                state
                    .changed()
                    .await
                    .map_err(|_| "MCP supervisor exited before cleanup")?;
            }
        })
        .await
        .map_err(|_| "MCP cleanup is still pending")?
    }
}

#[derive(Clone)]
pub struct Config {
    pub executable: PathBuf,
    pub runtime_socket: PathBuf,
    pub bearer_file: PathBuf,
    pub port: u16,
}
impl Config {
    pub fn from_env() -> Result<Option<Self>, &'static str> {
        let executable = std::env::var_os("COKERNEL_MCP_EXECUTABLE");
        let bearer = std::env::var_os("COKERNEL_MCP_BEARER_FILE");
        let port = std::env::var("COKERNEL_MCP_PORT").ok();
        if executable.is_none() && bearer.is_none() && port.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            executable: executable.ok_or("CK-MCP-CONFIG")?.into(),
            runtime_socket: std::env::var_os("COKERNEL_RUNTIME_SOCKET")
                .ok_or("CK-MCP-CONFIG")?
                .into(),
            bearer_file: bearer.ok_or("CK-MCP-CONFIG")?.into(),
            port: port
                .as_deref()
                .unwrap_or("0")
                .parse()
                .map_err(|_| "CK-MCP-CONFIG")?,
        }))
    }
    fn validate(&self) -> io::Result<()> {
        for path in [&self.executable, &self.runtime_socket, &self.bearer_file] {
            if !path.is_absolute()
                || path
                    .parent()
                    .ok_or_else(|| io::Error::other("missing parent"))?
                    .canonicalize()?
                    != path.parent().unwrap_or(Path::new(""))
            {
                return Err(io::Error::other("noncanonical MCP configuration"));
            }
        }
        let exe = std::fs::symlink_metadata(&self.executable)?;
        if !exe.is_file()
            || exe.mode() & 0o022 != 0
            || exe.mode() & 0o111 == 0
            || (exe.uid() != 0 && exe.uid() != unsafe { libc::geteuid() })
        {
            return Err(io::Error::other("unsafe MCP executable"));
        }
        crate::unix_api::check_directory(&self.runtime_socket)?;
        crate::unix_api::check_directory(&self.bearer_file)?;
        Ok(())
    }
}
fn record(state: &mut Snapshot, code: &'static str, status: Option<std::process::ExitStatus>) {
    if state.failures.len() == 8 {
        state.failures.remove(0);
    }
    state.failures.push(Failure {
        code,
        timestamp: chrono::Utc::now().to_rfc3339(),
        exit_code: status.as_ref().and_then(|s| s.code()),
        signal: status.and_then(|s| s.signal()),
    });
}

fn launch(config: &Config) -> io::Result<OwnedChild> {
    let mut command = Command::new(&config.executable);
    command
        .args(["serve"])
        .arg(&config.runtime_socket)
        .arg(&config.bearer_file)
        .arg(config.port.to_string())
        .current_dir(
            config
                .executable
                .parent()
                .ok_or_else(|| io::Error::other("MCP executable directory missing"))?,
        )
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    let parent = unsafe { libc::getpid() };
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(io::Error::other("Runtime exited before MCP launch"));
            }
            Ok(())
        });
    }
    let child = command.spawn()?;
    let pid = child
        .id()
        .and_then(|p| i32::try_from(p).ok())
        .filter(|p| *p > 1)
        .ok_or_else(|| io::Error::other("MCP child identity unavailable"))?;
    Ok(OwnedChild { child, pid })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    event: String,
    address: SocketAddr,
    path: String,
    authenticated: bool,
}
async fn readiness(stdout: &mut tokio::process::ChildStdout, port: u16) -> io::Result<SocketAddr> {
    let mut line = Vec::with_capacity(256);
    loop {
        let byte = stdout.read_u8().await?;
        if byte == b'\n' {
            break;
        }
        if line.len() == 4096 {
            return Err(io::Error::other("MCP readiness exceeds limit"));
        }
        line.push(byte);
    }
    let ready: Ready =
        serde_json::from_slice(&line).map_err(|_| io::Error::other("Invalid MCP readiness"))?;
    if ready.event != "mcp.ready"
        || ready.path != "/mcp"
        || !ready.authenticated
        || ready.address.ip() != std::net::Ipv4Addr::LOCALHOST
        || ready.address.port() == 0
        || (port != 0 && port != ready.address.port())
    {
        return Err(io::Error::other("MCP readiness mismatch"));
    }
    probe(ready.address).await?;
    Ok(ready.address)
}
async fn probe(address: SocketAddr) -> io::Result<()> {
    timeout(Duration::from_secs(1), async {
        let mut stream = tokio::net::TcpStream::connect(address).await?;
        stream
            .write_all(
                format!("GET /mcp HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await?;
        let mut line = Vec::new();
        loop {
            let b = stream.read_u8().await?;
            if b == b'\n' {
                break;
            }
            if line.len() >= 1024 {
                return Err(io::Error::other("MCP health response exceeds limit"));
            }
            line.push(b);
        }
        if !line.starts_with(b"HTTP/1.1 401 ") {
            return Err(io::Error::other("MCP no longer requires authentication"));
        }
        Ok(())
    })
    .await
    .map_err(|_| io::Error::other("MCP health timeout"))?
}
enum Outcome {
    Stopped,
    Restart,
    Failure(&'static str),
}
pub struct Supervisor {
    monitor: Monitor,
    task: JoinHandle<()>,
}
impl Supervisor {
    pub fn start(config: Config, monitor: Monitor) -> Self {
        let mut stopping = monitor.stop.subscribe();
        let mut restarting = monitor.restart.subscribe();
        let observer = monitor.clone();
        let task = tokio::spawn(async move {
            run(config, observer, &mut stopping, &mut restarting).await;
        });
        Self { monitor, task }
    }
    pub async fn shutdown(mut self) -> Result<(), &'static str> {
        self.monitor.shutdown().await?;
        (&mut self.task)
            .await
            .map_err(|_| "MCP supervisor task failed")
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.monitor.stop.send_replace(true);
    }
}
async fn run(
    mut config: Config,
    monitor: Monitor,
    stopping: &mut watch::Receiver<bool>,
    restarting: &mut watch::Receiver<u64>,
) {
    let mut state = monitor.snapshot();
    let mut failures = 0_u32;
    loop {
        if *stopping.borrow() {
            break;
        }
        if failures > 3 {
            state.state = State::Failed;
            monitor.publish(state.clone());
            tokio::select! {biased;_=stopping.changed()=>break,_=restarting.changed()=>{failures=0;}}
            continue;
        }
        if failures > 0 {
            state.state = State::Backoff;
            monitor.publish(state.clone());
            let delay = [250, 1000, 4000][failures.saturating_sub(1) as usize];
            tokio::select! {biased;_=stopping.changed()=>break,_=restarting.changed()=>{failures=0;continue;},_=sleep(Duration::from_millis(delay))=>{}}
        }
        state.state = State::Starting;
        state.generation = state.generation.saturating_add(1);
        state.consecutive_failures = failures;
        monitor.publish(state.clone());
        let validation = config.clone();
        let validated = tokio::task::spawn_blocking(move || validation.validate()).await;
        if *stopping.borrow() {
            break;
        }
        let child = if matches!(validated, Ok(Ok(()))) {
            launch(&config)
        } else {
            Err(io::Error::other("invalid MCP configuration"))
        };
        let mut child = match child {
            Ok(child) => child,
            Err(_) => {
                record(&mut state, "CK-MCP-LAUNCH", None);
                failures += 1;
                state.consecutive_failures = failures;
                continue;
            }
        };
        state.pid = Some(child.pid as u32);
        monitor.publish(state.clone());
        let Some(mut stdout) = child.child.stdout.take() else {
            let _ = child.cleanup().await;
            record(&mut state, "CK-MCP-READINESS", None);
            failures += 1;
            state.consecutive_failures = failures;
            state.pid = None;
            continue;
        };
        let mut healthy_since = None;
        let ready = tokio::select! {biased;
            _=stopping.changed()=>Err(Outcome::Stopped),
            _=restarting.changed()=>Err(Outcome::Restart),
            _=child.exited()=>Err(Outcome::Failure("CK-MCP-EXIT")),
            result=timeout(Duration::from_secs(10),readiness(&mut stdout,config.port))=>match result {Ok(Ok(address))=>Ok(address),_=>Err(Outcome::Failure("CK-MCP-READINESS"))},
        };
        let outcome = match ready {
            Err(outcome) => outcome,
            Ok(address) => {
                healthy_since = Some(Instant::now());
                config.port = address.port();
                state.address = Some(address);
                state.state = State::Ready;
                monitor.publish(state.clone());
                let mut failed_probes = 0;
                loop {
                    let health = tokio::select! {biased;
                        _=stopping.changed()=>break Outcome::Stopped,
                        _=restarting.changed()=>break Outcome::Restart,
                        _=child.exited()=>break Outcome::Failure("CK-MCP-EXIT"),
                        _=stdout.read_u8()=>break Outcome::Failure("CK-MCP-OUTPUT"),
                        _=sleep(Duration::from_secs(2))=>probe(address).await,
                    };
                    if health.is_err() {
                        failed_probes += 1;
                    } else {
                        failed_probes = 0;
                    }
                    if failed_probes == 3 {
                        break Outcome::Failure("CK-MCP-HEALTH");
                    }
                }
            }
        };
        state.state = State::Stopping;
        state.address = None;
        monitor.publish(state.clone());
        // A failed wait must not publish STOPPED or launch another child while
        // ownership is uncertain. Keep cleanup owned; shutdown can report pending.
        let status = loop {
            match child.cleanup().await {
                Ok(status) => break Some(status),
                Err(_) => {
                    record(&mut state, "CK-MCP-CLEANUP", None);
                    monitor.publish(state.clone());
                    sleep(Duration::from_secs(1)).await;
                }
            }
        };
        state.pid = None;
        match outcome {
            Outcome::Stopped => break,
            Outcome::Restart => {
                failures = 0;
                state.consecutive_failures = 0;
            }
            Outcome::Failure(code) => {
                if healthy_since.is_some_and(|start| start.elapsed() >= Duration::from_secs(60)) {
                    failures = 0;
                }
                failures += 1;
                state.consecutive_failures = failures;
                record(&mut state, code, status);
            }
        }
    }
    state.state = State::Stopped;
    state.address = None;
    state.pid = None;
    monitor.publish(state);
}
