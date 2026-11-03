use super::*;
use std::os::unix::fs::PermissionsExt;

struct Fixture(PathBuf);
impl Fixture {
    fn new(source: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ck-mcp-supervisor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let fixture = Self(path);
        fixture.script(source);
        fixture
    }
    fn script(&self, source: &str) {
        std::fs::write(
            self.0.join("mcp"),
            format!("#!/usr/bin/python3\n{source}\n"),
        )
        .unwrap();
        std::fs::set_permissions(self.0.join("mcp"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
    }
    fn config(&self) -> Config {
        Config {
            executable: self.0.join("mcp"),
            runtime_socket: self.0.join("runtime.sock"),
            bearer_file: self.0.join("bearer"),
            port: 0,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const HEALTHY: &str = r#"
import http.server,json,os,sys
assert set(os.environ) <= {'PATH','LC_ALL','LC_CTYPE'}, list(os.environ)
assert os.getpgrp()==os.getpid()
class Handler(http.server.BaseHTTPRequestHandler):
 protocol_version='HTTP/1.1'
 def do_GET(self):
  self.send_response(401);self.send_header('Content-Length','0');self.end_headers()
 def log_message(self,*args): pass
server=http.server.HTTPServer(('127.0.0.1',int(sys.argv[4])),Handler)
print(json.dumps(dict(event='mcp.ready',address='127.0.0.1:'+str(server.server_port),path='/mcp',authenticated=True)),flush=True)
server.serve_forever()
"#;

async fn wait(monitor: &Monitor, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
    timeout(Duration::from_secs(16), async {
        loop {
            let value = monitor.snapshot();
            if predicate(&value) {
                return value;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("unexpected supervisor state: {:?}", monitor.snapshot()))
}
fn gone(pid: u32) {
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "unreaped MCP {pid}"
    );
}

#[tokio::test]
async fn crash_recovery_pins_port_and_manual_restart_preserves_failure_evidence() {
    let fixture = Fixture::new(HEALTHY);
    let monitor = Monitor::default();
    let supervisor = Supervisor::start(fixture.config(), monitor.clone());
    let first = wait(&monitor, |s| s.state == State::Ready).await;
    unsafe {
        libc::kill(first.pid.unwrap() as i32, libc::SIGKILL);
    }
    let second = wait(&monitor, |s| s.state == State::Ready && s.generation == 2).await;
    gone(first.pid.unwrap());
    assert_eq!(first.address, second.address);
    assert_eq!(second.failures.len(), 1);
    assert_eq!(second.failures[0].signal, Some(libc::SIGKILL));
    monitor.restart().unwrap();
    let third = wait(&monitor, |s| s.state == State::Ready && s.generation == 3).await;
    gone(second.pid.unwrap());
    assert_eq!(third.address, first.address);
    assert_eq!(third.consecutive_failures, 0);
    assert_eq!(third.failures.len(), 1);
    supervisor.shutdown().await.unwrap();
    gone(third.pid.unwrap());
    assert_eq!(monitor.snapshot().state, State::Stopped);
    assert!(monitor.restart().is_err());
}

#[tokio::test]
async fn repeated_failure_opens_circuit_until_explicit_recovery() {
    let fixture =
        Fixture::new("import sys\nprint('DO-NOT-EXPOSE-MCP-SECRET',file=sys.stderr)\nsys.exit(23)");
    let monitor = Monitor::default();
    let supervisor = Supervisor::start(fixture.config(), monitor.clone());
    let failed = wait(&monitor, |s| s.state == State::Failed).await;
    assert_eq!(failed.generation, 4);
    assert_eq!(failed.consecutive_failures, 4);
    assert_eq!(failed.failures.len(), 4);
    assert!(failed.failures.iter().all(|f| f.exit_code == Some(23)));
    assert!(
        !serde_json::to_string(&failed)
            .unwrap()
            .contains("DO-NOT-EXPOSE")
    );
    sleep(Duration::from_millis(400)).await;
    assert_eq!(monitor.snapshot().generation, 4);
    fixture.script(HEALTHY);
    monitor.restart().unwrap();
    let recovered = wait(&monitor, |s| s.state == State::Ready).await;
    assert_eq!(recovered.generation, 5);
    assert_eq!(recovered.consecutive_failures, 0);
    supervisor.shutdown().await.unwrap();
    gone(recovered.pid.unwrap());
}

#[tokio::test]
async fn hung_service_fails_health_and_is_reaped_before_recovery() {
    let fixture = Fixture::new(HEALTHY);
    let monitor = Monitor::default();
    let supervisor = Supervisor::start(fixture.config(), monitor.clone());
    let first = wait(&monitor, |s| s.state == State::Ready).await;
    unsafe {
        libc::kill(first.pid.unwrap() as i32, libc::SIGSTOP);
    }
    let second = wait(&monitor, |s| s.state == State::Ready && s.generation == 2).await;
    gone(first.pid.unwrap());
    assert_eq!(second.failures[0].code, "CK-MCP-HEALTH");
    supervisor.shutdown().await.unwrap();
    gone(second.pid.unwrap());
}

#[tokio::test]
async fn drop_while_starting_cleans_child_without_readiness_timeout() {
    let fixture = Fixture::new("import time\ntime.sleep(100)");
    let monitor = Monitor::default();
    let supervisor = Supervisor::start(fixture.config(), monitor.clone());
    let running = wait(&monitor, |s| s.pid.is_some()).await;
    drop(supervisor);
    wait(&monitor, |s| s.state == State::Stopped).await;
    gone(running.pid.unwrap());
}

#[tokio::test]
async fn silent_startup_has_a_deadline_and_remains_cancellable_in_backoff() {
    let fixture = Fixture::new("import time\ntime.sleep(100)");
    let monitor = Monitor::default();
    let supervisor = Supervisor::start(fixture.config(), monitor.clone());
    let running = wait(&monitor, |s| s.pid.is_some()).await;
    let failed = wait(&monitor, |s| !s.failures.is_empty()).await;
    assert_eq!(failed.failures[0].code, "CK-MCP-READINESS");
    gone(running.pid.unwrap());
    supervisor.shutdown().await.unwrap();
    assert_eq!(monitor.snapshot().state, State::Stopped);
}

#[tokio::test]
async fn rejects_oversized_false_and_unauthenticated_readiness() {
    for source in [
        "print('x'*4097,flush=True)\nimport time\ntime.sleep(100)".to_string(),
        "print('{\"event\":\"mcp.ready\",\"address\":\"0.0.0.0:1234\",\"path\":\"/mcp\",\"authenticated\":true}',flush=True)\nimport time\ntime.sleep(100)".to_string(),
        HEALTHY.replace("send_response(401)","send_response(200)"),
    ] {
        let fixture=Fixture::new(&source);
        let monitor=Monitor::default();
        let supervisor=Supervisor::start(fixture.config(),monitor.clone());
        let failed=wait(&monitor,|s|!s.failures.is_empty()).await;
        assert_eq!(failed.failures[0].code,"CK-MCP-READINESS");
        assert_ne!(failed.state,State::Ready);
        supervisor.shutdown().await.unwrap();
    }
}

#[test]
fn configuration_rejects_symlink_executable_and_public_state() {
    let fixture = Fixture::new(HEALTHY);
    let mut config = fixture.config();
    config.validate().unwrap();
    std::os::unix::fs::symlink(&config.executable, fixture.0.join("linked")).unwrap();
    config.executable = fixture.0.join("linked");
    assert!(config.validate().is_err());
    config = fixture.config();
    std::fs::set_permissions(&fixture.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(config.validate().is_err());
}

#[test]
fn bounded_failure_history_and_disabled_control() {
    let monitor = Monitor::default();
    assert!(monitor.restart().is_err());
    for _ in 0..20 {
        monitor.failed("CK-MCP-CONFIG");
    }
    assert_eq!(monitor.snapshot().failures.len(), 8);
}
