use super::*;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

struct Fixture(PathBuf);
impl Fixture {
    fn new(source: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ck-tun-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let result = Self(path);
        result.script("tunnel", source);
        result.script("mcp", MCP);
        fs::write(result.0.join("bearer"), "a".repeat(64)).unwrap();
        fs::set_permissions(result.0.join("bearer"), fs::Permissions::from_mode(0o600)).unwrap();
        result
    }
    fn script(&self, name: &str, source: &str) {
        fs::write(self.0.join(name), format!("#!/usr/bin/python3\n{source}\n")).unwrap();
        fs::set_permissions(self.0.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    fn config(&self) -> Config {
        Config {
            executable: self.0.join("tunnel"),
            state_directory: self.0.clone(),
            bearer_file: self.0.join("bearer"),
        }
    }
    fn mcp(&self, monitor: mcp_supervisor::Monitor) -> mcp_supervisor::Supervisor {
        mcp_supervisor::Supervisor::start(
            mcp_supervisor::Config {
                executable: self.0.join("mcp"),
                runtime_socket: self.0.join("runtime.sock"),
                bearer_file: self.0.join("bearer"),
                port: 0,
            },
            monitor,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const MCP: &str = r#"
import http.server,json,sys
class H(http.server.BaseHTTPRequestHandler):
 protocol_version='HTTP/1.1'
 def do_GET(self):self.send_response(401);self.send_header('Content-Length','0');self.end_headers()
 def log_message(self,*args):pass
s=http.server.HTTPServer(('127.0.0.1',int(sys.argv[4])),H)
print(json.dumps(dict(event='mcp.ready',address='127.0.0.1:'+str(s.server_port),path='/mcp',authenticated=True)),flush=True)
s.serve_forever()
"#;
const TUNNEL: &str = r#"
import os,sys,socketserver,http.server,time
assert set(os.environ)<= {'PATH','LC_ALL','HOME','XDG_CONFIG_HOME','CONTROL_PLANE_API_KEY','COKERNEL_MCP_AUTHORIZATION','LC_CTYPE'}
assert os.environ['CONTROL_PLANE_API_KEY']=='synthetic-key-for-tests'
assert os.environ['COKERNEL_MCP_AUTHORIZATION']=='Bearer '+'a'*64
assert all('synthetic-key-for-tests' not in a and 'a'*64 not in a for a in sys.argv)
assert os.getpid()==os.getpgrp()
assert os.environ['HOME']==os.getcwd()
class H(http.server.BaseHTTPRequestHandler):
 def do_GET(self):
  data=b'ready' if self.path=='/readyz' else ('commands_poll_last_successful_timestamp_seconds '+str(int(time.time()))+'\n').encode()
  self.send_response(200);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
 def log_message(self,*args):pass
class Server(socketserver.UnixStreamServer):pass
s=Server(sys.argv[sys.argv.index('--health.unix-socket')+1],H)
s.serve_forever()
"#;
fn credentials() -> Credentials {
    Credentials::new("tunnel_local_test".into(), "synthetic-key-for-tests".into()).unwrap()
}
async fn wait(monitor: &Monitor, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
    timeout(Duration::from_secs(16), async {
        loop {
            let state = monitor.snapshot();
            if predicate(&state) {
                return state;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("unexpected state {:?}", monitor.snapshot()))
}
fn gone(pid: u32) {
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "unreaped child {pid}"
    );
}

#[tokio::test]
async fn desired_dependency_and_owned_process_reconcile_without_session_authority() {
    let fixture = Fixture::new(TUNNEL);
    let mcp = mcp_supervisor::Monitor::default();
    let monitor = Monitor::default();
    let supervisor = Supervisor::start(fixture.config(), monitor.clone(), mcp.clone());
    let epoch = monitor.enable(credentials()).unwrap();
    let waiting = wait(&monitor, |s| s.applied_epoch == epoch).await;
    assert_eq!(waiting.state, State::Degraded);
    assert_eq!(waiting.pid, None);
    assert_eq!(waiting.generation, 0);
    let mcp_owner = fixture.mcp(mcp.clone());
    let first = wait(&monitor, |s| s.state == State::Connected).await;
    assert_eq!(first.mcp_generation, Some(1));
    assert_eq!(monitor.enable(credentials()).unwrap(), epoch);
    sleep(Duration::from_millis(100)).await;
    assert_eq!(monitor.snapshot().pid, first.pid);
    mcp.restart().unwrap();
    let second = wait(&monitor, |s| {
        s.state == State::Connected && s.mcp_generation == Some(2)
    })
    .await;
    gone(first.pid.unwrap());
    assert_eq!(second.consecutive_failures, 0);
    // A new configuration incarnation follows the same cleanup/launch rule.
    let epoch = monitor
        .enable(
            Credentials::new("tunnel_other_test".into(), "synthetic-key-for-tests".into()).unwrap(),
        )
        .unwrap();
    let third = wait(&monitor, |s| {
        s.state == State::Connected && s.applied_epoch == epoch
    })
    .await;
    gone(second.pid.unwrap());
    let epoch = monitor.disable();
    let stopped = wait(&monitor, |s| {
        s.state == State::Disabled && s.applied_epoch == epoch
    })
    .await;
    assert_eq!(stopped.pid, None);
    gone(third.pid.unwrap());
    assert_eq!(mcp.snapshot().state, mcp_supervisor::State::Ready);
    supervisor.shutdown().await.unwrap();
    mcp_owner.shutdown().await.unwrap();
    assert!(monitor.enable(credentials()).is_err());
}

#[tokio::test]
async fn finite_crash_recovery_explicit_new_intent_and_drop_cleanup() {
    let fixture = Fixture::new(TUNNEL);
    let mcp = mcp_supervisor::Monitor::default();
    let mcp_owner = fixture.mcp(mcp.clone());
    let monitor = Monitor::default();
    let supervisor = Supervisor::start(fixture.config(), monitor.clone(), mcp);
    monitor.enable(credentials()).unwrap();
    for generation in 1..=4 {
        let running = wait(&monitor, |s| {
            s.state == State::Connected && s.generation == generation
        })
        .await;
        unsafe {
            libc::kill(running.pid.unwrap() as i32, libc::SIGKILL);
        }
        wait(&monitor, |s| s.failures.len() >= generation as usize).await;
        gone(running.pid.unwrap());
    }
    let failed = wait(&monitor, |s| s.state == State::Error).await;
    assert_eq!(failed.consecutive_failures, 4);
    assert_eq!(failed.pid, None);
    assert!(
        failed
            .failures
            .iter()
            .all(|f| f.code == "CK-TUN-EXIT" && f.signal == Some(libc::SIGKILL))
    );
    monitor.restart().unwrap();
    let resumed = wait(&monitor, |s| {
        s.state == State::Connected && s.generation == 5
    })
    .await;
    assert_eq!(resumed.consecutive_failures, 0);
    assert!(
        !serde_json::to_string(&resumed)
            .unwrap()
            .contains("synthetic-key-for-tests")
    );
    drop(supervisor);
    wait(&monitor, |s| s.state == State::Disabled).await;
    gone(resumed.pid.unwrap());
    mcp_owner.shutdown().await.unwrap();
}

#[tokio::test]
async fn readiness_without_successful_poll_never_reports_connected_and_stop_is_prompt() {
    let fixture = Fixture::new(&TUNNEL.replace("str(int(time.time()))", "'0'"));
    let mcp = mcp_supervisor::Monitor::default();
    let mcp_owner = fixture.mcp(mcp.clone());
    let monitor = Monitor::default();
    let supervisor = Supervisor::start(fixture.config(), monitor.clone(), mcp);
    monitor.enable(credentials()).unwrap();
    let running = wait(&monitor, |s| s.pid.is_some()).await;
    sleep(Duration::from_millis(1200)).await;
    assert_eq!(monitor.snapshot().state, State::Starting);
    supervisor.shutdown().await.unwrap();
    gone(running.pid.unwrap());
    mcp_owner.shutdown().await.unwrap();
}

#[test]
fn credential_and_launch_boundaries_reject_malformed_and_unsafe_inputs() {
    for params in [
        serde_json::json!({"tunnel_id":"tunnel_valid","runtime_key":"synthetic-key-for-tests","path":"/tmp"}),
        serde_json::json!({"tunnel_id":"bad","runtime_key":"synthetic-key-for-tests"}),
        serde_json::json!({"tunnel_id":"tunnel_valid","runtime_key":"secret\nheader"}),
    ] {
        assert_eq!(
            Credentials::from_params(params).err(),
            Some("CK-TUN-CREDENTIAL")
        );
    }
    let fixture = Fixture::new(TUNNEL);
    let config = fixture.config();
    drop(launch::prepare(&config).unwrap());
    fs::set_permissions(&config.bearer_file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(launch::prepare(&config).is_err());
    fs::set_permissions(&config.bearer_file, fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(&config.bearer_file).unwrap();
    std::os::unix::fs::symlink("/dev/zero", &config.bearer_file).unwrap();
    assert!(launch::prepare(&config).is_err());
    let mut config = fixture.config();
    config.executable = fixture.0.join("alias");
    std::os::unix::fs::symlink(fixture.0.join("tunnel"), &config.executable).unwrap();
    assert!(launch::prepare(&config).is_err());
}
