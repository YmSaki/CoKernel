#![cfg(target_os = "linux")]
use cokernel_domain::{ExecutionOrigin, NotebookId, ProjectId};
use cokernel_protocol::{RequestEnvelope, ResponseEnvelope};
use cokernel_runtime::{
    notebook::{NotebookRuntime, NotebookService},
    project::ProjectManager,
    service::RuntimeService,
    session::{SessionSupervisor, SessionSupervisorConfig},
    uv::{UvCommandResult, UvError, UvInvocation, UvRunner},
};
use serde_json::{Value, json};
use std::{collections::HashMap, fs, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

struct FakeUv;

#[tokio::test]
async fn diagnostics_omit_document_data_and_remain_available_during_storage_stall() {
    let mut fixture = Fixture::new();
    let documents = fixture.runtime.documents.clone();
    let guard = documents.lock().await;
    fixture
        .send("diagnostics", "runtime.get_diagnostics", json!({}))
        .await;
    let response = fixture.reply("diagnostics").await;
    assert!(response.ok);
    let value = response.result.unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["sessions"]["total"], 0);
    assert_eq!(value["sessions"]["managed_identity_configured"], false);
    assert!(
        value["exclusions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "notebook_content")
    );
    assert!(
        !value
            .to_string()
            .contains(&fixture.root.display().to_string())
    );
    fixture
        .send(
            "bad-diagnostics",
            "runtime.get_diagnostics",
            json!({"token":"synthetic-secret-input"}),
        )
        .await;
    let error = fixture.reply("bad-diagnostics").await;
    assert_eq!(error.error.as_ref().unwrap().code, "CK-DIAG-INPUT");
    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .contains("synthetic-secret-input")
    );
    drop(guard);
    fixture.service.shutdown().await.unwrap();
}
impl UvRunner for FakeUv {
    fn run(&self, invocation: &UvInvocation) -> Result<UvCommandResult, UvError> {
        fs::write(
            PathBuf::from(&invocation.args[2]).join("uv.lock"),
            "fixture",
        )
        .unwrap();
        Ok(UvCommandResult {
            status_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}
struct Fixture {
    root: PathBuf,
    service: RuntimeService<FakeUv>,
    runtime: NotebookRuntime,
    projects: Vec<ProjectId>,
    notebooks: Vec<NotebookId>,
    replies: HashMap<String, ResponseEnvelope>,
}
impl Fixture {
    fn endpoint(
        &mut self,
    ) -> (
        cokernel_runtime::endpoint::RuntimeHandle,
        cokernel_runtime::endpoint::RuntimeOwner,
    ) {
        let empty = ProjectManager::load(
            self.root.join("unused"),
            self.root.join("unused-registry.json"),
            FakeUv,
        )
        .unwrap();
        cokernel_runtime::endpoint::RuntimeOwner::start(std::mem::replace(
            &mut self.service,
            RuntimeService::new(empty),
        ))
    }
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("ckdefer-{}", ProjectId::new()));
        fs::create_dir_all(&root).unwrap();
        let worker = root.join("worker.py");
        fs::write(&worker, include_str!("fixtures/fake_uv.py")).unwrap();
        fs::set_permissions(&worker, fs::Permissions::from_mode(0o700)).unwrap();
        let launcher = root.join("uv");
        fs::write(&launcher, format!("#!/usr/bin/env python3\nimport os,sys,time\nfrom pathlib import Path\np=Path(sys.argv[sys.argv.index('--project')+1])\nif p.name=='blocked' and not (p/'release').exists():\n (p/'pid').write_text(str(os.getpid()))\n while not (p/'release').exists(): time.sleep(.01)\nos.execv({}, [{},*sys.argv[1:]])\n", json!(worker), json!(worker))).unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700)).unwrap();
        let mut manager =
            ProjectManager::load(root.join("projects"), root.join("registry.json"), FakeUv)
                .unwrap();
        let mut documents = NotebookService::default();
        let mut projects = vec![];
        let mut notebooks = vec![];
        for name in ["blocked", "healthy"] {
            let path = root.join("projects").join(name);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("pyproject.toml"), "[project]\nname='fixture'\n").unwrap();
            let project = manager.register_existing(&path).unwrap().project;
            notebooks.push(
                documents
                    .create(&project, "test.ipynb")
                    .unwrap()
                    .document
                    .notebook_id,
            );
            projects.push(project.project_id);
        }
        let mut config =
            SessionSupervisorConfig::development(root.join("worker-package"), root.join("s"));
        config.uv_executable = launcher;
        let runtime = NotebookRuntime::new(documents, SessionSupervisor::new(config));
        Self {
            root,
            service: RuntimeService::with_notebooks(manager, runtime.clone()),
            runtime,
            projects,
            notebooks,
            replies: HashMap::new(),
        }
    }
    async fn send(&mut self, id: &str, method: &str, params: Value) {
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            self.service.admit(
                RequestEnvelope {
                    protocol: 1,
                    request_id: id.into(),
                    method: method.into(),
                    params,
                },
                ExecutionOrigin::Human,
            ),
        )
        .await
        .expect("admission blocked");
        if let Some(response) = response {
            self.replies.insert(id.into(), response);
        }
        self.collect();
    }
    fn collect(&mut self) {
        for r in self.service.take_responses() {
            assert!(self.replies.insert(r.request_id.clone(), r).is_none());
        }
    }
    async fn reply(&mut self, id: &str) -> ResponseEnvelope {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                self.collect();
                if let Some(response) = self.replies.remove(id) {
                    return response;
                }
                let completion = self.service.next_background_completion().await;
                self.service.finish_background_completion(completion).await;
            }
        })
        .await
        .expect("response timed out")
    }
    async fn blocked_pid(&self) -> i32 {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(pid) = fs::read_to_string(self.root.join("projects/blocked/pid"))
                    && let Ok(pid) = pid.parse()
                {
                    return pid;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::write(self.root.join("projects/blocked/release"), "");
        let _ = fs::remove_dir_all(&self.root);
    }
}
async fn gone(pid: i32) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while unsafe { libc::kill(pid, 0) } == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("startup child survived cancellation");
}

#[tokio::test]
async fn linux_metrics_are_cached_and_do_not_start_notebook_workers() {
    let mut f = Fixture::new();
    f.send(
        "bad-metrics",
        "runtime.get_linux_metrics",
        json!({"path":"/etc/shadow"}),
    )
    .await;
    assert_eq!(
        f.reply("bad-metrics").await.error.unwrap().code,
        "CK-RUN-BAD-PARAMS"
    );
    for n in 0..32 {
        let id = format!("metrics-{n}");
        f.send(&id, "runtime.get_linux_metrics", json!({})).await;
        let reply = f.reply(&id).await;
        assert!(reply.ok);
        let snapshot: cokernel_metrics::Snapshot =
            serde_json::from_value(reply.result.unwrap()).unwrap();
        assert!(snapshot.collecting);
    }
    f.send("ping", "bridge.ping", json!({})).await;
    assert!(f.reply("ping").await.ok);
    assert!(!f.root.join("projects/blocked/pid").exists());
    assert!(f.runtime.sessions.list().await.is_empty());
    f.service.shutdown().await.unwrap();
}

#[tokio::test]
async fn blocked_start_coalesces_rejects_mutation_and_allows_another_notebook_to_start() {
    let mut f = Fixture::new();
    let a = f.projects[0];
    let n = f.notebooks[0];
    f.send(
        "a1",
        "sessions.ensure_primary",
        json!({"project_id":a,"notebook_id":n}),
    )
    .await;
    f.blocked_pid().await;
    f.send(
        "a2",
        "sessions.ensure_primary",
        json!({"project_id":a,"notebook_id":n}),
    )
    .await;
    for (id, method, params, code) in [
        (
            "sync",
            "packages.sync",
            json!({"project_id":a}),
            "CK-SESS-STARTING",
        ),
        (
            "forget",
            "projects.remove_from_registry",
            json!({"project_id":a}),
            "CK-SESS-STARTING",
        ),
        (
            "restart",
            "sessions.restart",
            json!({"project_id":a,"notebook_id":n}),
            "CK-SESS-STARTING",
        ),
    ] {
        f.send(id, method, params).await;
        assert_eq!(f.reply(id).await.error.unwrap().code, code);
    }
    f.send(
        "b",
        "sessions.ensure_primary",
        json!({"project_id":f.projects[1],"notebook_id":f.notebooks[1]}),
    )
    .await;
    assert!(f.reply("b").await.ok, "another notebook start waited for A");
    fs::write(f.root.join("projects/blocked/release"), "").unwrap();
    let a1 = f.reply("a1").await.result.unwrap();
    let a2 = f.reply("a2").await.result.unwrap();
    assert_eq!(a1["session_id"], a2["session_id"]);
    assert_eq!(a1["worker_generation"], 1);
    f.service.shutdown().await.unwrap();
}

#[tokio::test]
async fn pending_capacity_reserves_control_and_shutdown_cancels_startup() {
    let mut f = Fixture::new();
    let a = f.projects[0];
    let n = f.notebooks[0];
    for i in 0..16 {
        f.send(
            &format!("start{i}"),
            "sessions.ensure_primary",
            json!({"project_id":a,"notebook_id":n}),
        )
        .await;
    }
    let pid = f.blocked_pid().await;
    f.send(
        "extra",
        "sessions.ensure_primary",
        json!({"project_id":a,"notebook_id":n}),
    )
    .await;
    assert_eq!(f.reply("extra").await.error.unwrap().code, "CK-RUN-BUSY");
    f.send("ping", "bridge.ping", json!({})).await;
    assert!(f.reply("ping").await.ok);
    f.send("stop", "bridge.shutdown", json!({})).await;
    assert!(f.reply("stop").await.ok);
    for i in 0..16 {
        assert!(!f.reply(&format!("start{i}")).await.ok);
    }
    gone(pid).await;
}

#[tokio::test]
async fn wrong_project_and_invalid_inspection_parameters_do_not_reserve_startup() {
    let mut f = Fixture::new();
    f.send(
        "wrong",
        "sessions.ensure_primary",
        json!({"project_id":f.projects[1],"notebook_id":f.notebooks[0]}),
    )
    .await;
    assert!(!f.reply("wrong").await.ok);
    f.send("bad", "sessions.list_variables", json!({"project_id":f.projects[0],"session_id":cokernel_domain::SessionId::new(),"name":null})).await;
    assert_eq!(
        f.reply("bad").await.error.unwrap().code,
        "CK-RUN-BAD-PARAMS"
    );
    assert!(!f.root.join("projects/blocked/pid").exists());
    f.service.shutdown().await.unwrap();
}

#[tokio::test]
async fn stop_during_restart_cancels_replacement_and_does_not_revive_worker() {
    let mut f = Fixture::new();
    let a = f.projects[0];
    let n = f.notebooks[0];
    let release = f.root.join("projects/blocked/release");
    fs::write(&release, "").unwrap();
    f.send(
        "first",
        "sessions.ensure_primary",
        json!({"project_id":a,"notebook_id":n}),
    )
    .await;
    let session = f.reply("first").await.result.unwrap()["session_id"].clone();
    // Prove the initial generation remains usable after the handshake before
    // racing restart/stop. A fixture that exits after ready invalidates this proof.
    let initial = f
        .runtime
        .sessions
        .get(serde_json::from_value(session.clone()).unwrap())
        .await
        .unwrap();
    let receipt = initial
        .execute_cell_captured(ExecutionOrigin::Human, "health-proof", "ok")
        .await
        .unwrap();
    let capture = tokio::time::timeout(Duration::from_secs(3), receipt.completed())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        capture.status,
        cokernel_domain::OperationStatus::Succeeded,
        "initial worker was not healthy: {:?}",
        initial.last_failure()
    );
    fs::remove_file(release).unwrap();
    f.send(
        "restart",
        "sessions.restart",
        json!({"project_id":a,"notebook_id":n}),
    )
    .await;
    let pid = f.blocked_pid().await;
    f.send(
        "stop",
        "sessions.stop",
        json!({"project_id":a,"session_id":session}),
    )
    .await;
    assert!(f.reply("stop").await.ok);
    assert!(!f.reply("restart").await.ok);
    gone(pid).await;
    f.send(
        "status",
        "sessions.get",
        json!({"project_id":a,"session_id":session}),
    )
    .await;
    assert_eq!(f.reply("status").await.result.unwrap()["state"], "STOPPED");
    f.service.shutdown().await.unwrap();
}

fn request(id: &str, method: &str, params: Value) -> RequestEnvelope {
    RequestEnvelope {
        protocol: 1,
        request_id: id.into(),
        method: method.into(),
        params,
    }
}
async fn endpoint_reply(
    ticket: cokernel_runtime::endpoint::ResponseTicket,
) -> cokernel_runtime::endpoint::RuntimeReply {
    tokio::time::timeout(Duration::from_secs(3), ticket.wait())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn endpoint_clients_correlate_duplicate_ids_and_share_primary_after_disconnect() {
    let mut f = Fixture::new();
    let (human, owner) = f.endpoint();
    let ai = human.clone();
    let params = json!({"project_id":f.projects[0],"notebook_id":f.notebooks[0]});
    let first = human
        .submit(
            request("same", "sessions.ensure_primary", params.clone()),
            ExecutionOrigin::Human,
        )
        .unwrap();
    f.blocked_pid().await;
    let second = ai
        .submit(
            request("same", "sessions.ensure_primary", params.clone()),
            ExecutionOrigin::Mcp,
        )
        .unwrap();
    let ping = ai
        .submit(
            request("same", "bridge.ping", json!({})),
            ExecutionOrigin::Mcp,
        )
        .unwrap();
    let reply = endpoint_reply(ping).await;
    assert!(reply.response.ok);
    assert_eq!(reply.response.request_id, "same");
    assert_eq!(reply.response.result.unwrap()["pong"], true);
    // Neither dropping an accepted reply nor disconnecting that client cancels work.
    drop(first);
    drop(human);
    fs::write(f.root.join("projects/blocked/release"), "").unwrap();
    let session = endpoint_reply(second).await.response.result.unwrap()["session_id"].clone();
    let again = ai
        .submit(
            request("same", "sessions.ensure_primary", params),
            ExecutionOrigin::Mcp,
        )
        .unwrap();
    assert_eq!(
        endpoint_reply(again).await.response.result.unwrap()["session_id"],
        session
    );
    owner.finish().await.unwrap();
}

#[tokio::test]
async fn endpoint_bounds_queued_and_unread_replies_and_recovers_after_consumption() {
    let mut f = Fixture::new();
    let (client, owner) = f.endpoint();
    let mut tickets = Vec::new();
    for _ in 0..32 {
        tickets.push(
            client
                .submit(
                    request("same", "bridge.ping", json!({})),
                    ExecutionOrigin::Human,
                )
                .unwrap(),
        );
    }
    let Err(error) = client.submit(
        request("overflow", "bridge.ping", json!({})),
        ExecutionOrigin::Human,
    ) else {
        panic!("unbounded endpoint");
    };
    assert_eq!(error.code, "CK-RUN-BUSY");
    assert!(endpoint_reply(tickets.remove(0)).await.response.ok);
    // Let already-admitted immediate responses become unread deliveries. They
    // must still consume capacity, independent of whether admission has finished.
    tokio::task::yield_now().await;
    let replacement = client
        .submit(
            request("replacement", "bridge.ping", json!({})),
            ExecutionOrigin::Human,
        )
        .unwrap();
    let Err(error) = client.submit(
        request("overflow", "bridge.ping", json!({})),
        ExecutionOrigin::Human,
    ) else {
        panic!("unread replies released capacity");
    };
    assert_eq!(error.code, "CK-RUN-BUSY");
    assert!(endpoint_reply(replacement).await.response.ok);
    for ticket in tickets {
        assert!(endpoint_reply(ticket).await.response.ok);
    }
    owner.finish().await.unwrap();
}

#[tokio::test]
async fn endpoint_owner_stop_rejects_unadmitted_mutation_and_closed_handle() {
    let mut f = Fixture::new();
    let (client, owner) = f.endpoint();
    // No await: owner stop wins before queued creation can be admitted.
    let create = client
        .submit(
            request(
                "create",
                "projects.create",
                json!({"name":"must-not-exist"}),
            ),
            ExecutionOrigin::Human,
        )
        .unwrap();
    owner.finish().await.unwrap();
    let response = endpoint_reply(create).await.response;
    assert_eq!(response.request_id, "create");
    assert_eq!(response.error.unwrap().code, "CK-RUN-STOPPED");
    assert!(!f.root.join("projects/must-not-exist").exists());
    let Err(error) = client.submit(
        request("after", "bridge.ping", json!({})),
        ExecutionOrigin::Human,
    ) else {
        panic!("closed endpoint accepted request");
    };
    assert_eq!(error.code, "CK-RUN-STOPPED");
}

#[tokio::test]
async fn endpoint_owner_stop_joins_pending_worker_cleanup() {
    let mut f = Fixture::new();
    let (client, owner) = f.endpoint();
    let start = client
        .submit(
            request(
                "start",
                "sessions.ensure_primary",
                json!({"project_id":f.projects[0],"notebook_id":f.notebooks[0]}),
            ),
            ExecutionOrigin::Human,
        )
        .unwrap();
    let pid = f.blocked_pid().await;
    owner.finish().await.unwrap();
    assert!(!endpoint_reply(start).await.response.ok);
    gone(pid).await;
}

#[tokio::test]
async fn endpoint_failed_shutdown_preserves_documents_and_accepts_repair() {
    let mut f = Fixture::new();
    let (client, owner) = f.endpoint();
    let project = f.projects[0];
    let notebook = f.notebooks[0];
    let read = client
        .submit(
            request(
                "get",
                "notebooks.get",
                json!({"project_id":project,"notebook_id":notebook}),
            ),
            ExecutionOrigin::Human,
        )
        .unwrap();
    let revision = endpoint_reply(read).await.response.result.unwrap()["revision"].clone();
    let edit = client.submit(request("edit", "notebooks.apply_edits", json!({"project_id":project,"notebook_id":notebook,"expected_revision":revision,"edits":[{"op":"set_notebook_metadata","metadata":{"retained":true}}]})), ExecutionOrigin::Human).unwrap();
    assert!(endpoint_reply(edit).await.response.ok);
    let path = f.root.join("projects/blocked/test.ipynb");
    let original = fs::read(&path).unwrap();
    fs::write(&path, "external change").unwrap();
    let shutdown = client
        .submit(
            request("shutdown", "bridge.shutdown", json!({})),
            ExecutionOrigin::Human,
        )
        .unwrap();
    let reply = endpoint_reply(shutdown).await;
    assert!(!reply.close_transport);
    assert_eq!(reply.response.error.unwrap().code, "CK-RUN-SHUTDOWN");
    let read = client
        .submit(
            request(
                "get",
                "notebooks.get",
                json!({"project_id":project,"notebook_id":notebook}),
            ),
            ExecutionOrigin::Human,
        )
        .unwrap();
    assert_eq!(
        endpoint_reply(read).await.response.result.unwrap()["notebook"]["metadata"]["retained"],
        true
    );
    fs::write(path, original).unwrap();
    // External-change detection is deliberately latched. Repair is an explicit
    // reload/reapply, not silently clearing conflict because bytes changed back.
    let reload = client.submit(request("reload", "notebooks.reload", json!({"project_id":project,"notebook_id":notebook,"expected_revision":revision.as_u64().unwrap()+1})), ExecutionOrigin::Human).unwrap();
    let reloaded =
        endpoint_reply(reload).await.response.result.unwrap()["document"]["revision"].clone();
    let edit = client.submit(request("reapply", "notebooks.apply_edits", json!({"project_id":project,"notebook_id":notebook,"expected_revision":reloaded,"edits":[{"op":"set_notebook_metadata","metadata":{"retained":true}}]})), ExecutionOrigin::Human).unwrap();
    assert!(endpoint_reply(edit).await.response.ok);
    let shutdown = client
        .submit(
            request("shutdown", "bridge.shutdown", json!({})),
            ExecutionOrigin::Human,
        )
        .unwrap();
    let reply = endpoint_reply(shutdown).await;
    assert!(reply.close_transport && reply.response.ok);
    owner.finish().await.unwrap();
}

#[tokio::test]
async fn endpoint_rejects_oversized_direct_requests_before_admission() {
    let mut f = Fixture::new();
    let (client, owner) = f.endpoint();
    let Err(error) = client.submit(
        request(
            "large",
            "projects.create",
            json!({"name":"x".repeat(cokernel_protocol::DEFAULT_MAX_FRAME_BYTES)}),
        ),
        ExecutionOrigin::Human,
    ) else {
        panic!("oversized request accepted");
    };
    assert_eq!(error.code, "CK-RUN-REQUEST-LIMIT");
    let ping = client
        .submit(
            request("ping", "bridge.ping", json!({})),
            ExecutionOrigin::Human,
        )
        .unwrap();
    assert!(endpoint_reply(ping).await.response.ok);
    owner.finish().await.unwrap();
}

#[tokio::test]
async fn endpoint_human_and_ai_edits_share_revision_authority_and_admission_order() {
    let mut f = Fixture::new();
    let (human, owner) = f.endpoint();
    let ai = human.clone();
    let project = f.projects[0];
    let notebook = f.notebooks[0];
    let params = |writer| json!({"project_id":project,"notebook_id":notebook,"expected_revision":1,"edits":[{"op":"set_notebook_metadata","metadata":{"writer":writer}}]});
    let first = human
        .submit(
            request("same", "notebooks.apply_edits", params("human")),
            ExecutionOrigin::Human,
        )
        .unwrap();
    let second = ai
        .submit(
            request("same", "notebooks.apply_edits", params("ai")),
            ExecutionOrigin::Mcp,
        )
        .unwrap();
    assert!(endpoint_reply(first).await.response.ok);
    assert_eq!(
        endpoint_reply(second).await.response.error.unwrap().code,
        "CK-NB-CONFLICT"
    );
    let read = ai
        .submit(
            request(
                "same",
                "notebooks.get",
                json!({"project_id":project,"notebook_id":notebook}),
            ),
            ExecutionOrigin::Mcp,
        )
        .unwrap();
    let snapshot = endpoint_reply(read).await.response.result.unwrap();
    assert_eq!(snapshot["revision"], 2);
    assert_eq!(snapshot["notebook"]["metadata"]["writer"], "human");
    owner.finish().await.unwrap();
    let saved: Value =
        serde_json::from_slice(&fs::read(f.root.join("projects/blocked/test.ipynb")).unwrap())
            .unwrap();
    assert_eq!(saved["metadata"]["writer"], "human");
}

async fn execution_fixture(f: &Fixture) -> (cokernel_domain::Project, cokernel_domain::SessionId) {
    use cokernel_runtime::notebook::Edit;
    let project = f.service.projects().get(f.projects[1]).unwrap().project;
    {
        let mut documents = f.runtime.documents.lock().await;
        documents.apply_edits(&project, f.notebooks[1], 1, &[Edit::InsertCell {
            index: 0,
            cell: json!({"id":"a","cell_type":"code","metadata":{},"execution_count":null,"outputs":[],"source":"sleep:30"}),
        }]).unwrap();
        documents.save(&project, f.notebooks[1], 2).unwrap();
    }
    let session = f
        .runtime
        .ensure_primary(&project, f.notebooks[1])
        .await
        .unwrap();
    (project, session.session_id())
}

#[tokio::test]
async fn control_burst_and_stale_notification_do_not_wait_for_the_actor() {
    use cokernel_domain::{OperationStatus, SessionState};
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    fn immediately<F: Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("control waited for the actor to consume a queue slot"),
        }
    }
    let mut f = Fixture::new();
    let (project, first_id) = execution_fixture(&f).await;
    let other = f
        .runtime
        .documents
        .lock()
        .await
        .create(&project, "other.ipynb")
        .unwrap()
        .document
        .notebook_id;
    let second = f.runtime.ensure_primary(&project, other).await.unwrap();
    let first = f.runtime.sessions.get(first_id).await.unwrap();
    let (mut events, _) = f.runtime.sessions.events().subscribe();
    let receipt = first
        .execute_cell_captured(ExecutionOrigin::Human, "active", "sleep:30")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while events.recv().await.unwrap().event != "session.execution_started" {}
    })
    .await
    .unwrap();

    // This current-thread test deliberately does not yield to either actor.
    // Exceed the former queue capacity; every control call must finish on its
    // first poll, and stale/stop cannot sit behind the interrupt burst.
    for _ in 0..1_024 {
        immediately(first.interrupt()).unwrap();
    }
    immediately(
        f.runtime
            .sessions
            .mark_project_environment_stale(project.project_id, 2),
    );
    immediately(first.stop()).unwrap();

    let (client, owner) = f.endpoint();
    let status = client
        .submit(
            request(
                "other-status",
                "sessions.get",
                json!({"project_id":project.project_id,"session_id":second.session_id()}),
            ),
            ExecutionOrigin::Mcp,
        )
        .unwrap();
    assert!(endpoint_reply(status).await.response.ok);
    let survivor = second
        .execute_cell_captured(ExecutionOrigin::Mcp, "survivor", "sleep:0.0")
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), survivor.completed())
            .await
            .unwrap()
            .unwrap()
            .status,
        OperationStatus::Succeeded
    );
    assert!(
        // Stop takes the existing 3s graceful deadline plus forced cleanup;
        // the other Session above must finish before we wait for that cleanup.
        tokio::time::timeout(Duration::from_secs(5), receipt.completed())
            .await
            .unwrap()
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while first.state() != SessionState::Stopped
            || second.state() != SessionState::StaleEnvironment
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        f.runtime
            .ensure_primary(&project, other)
            .await
            .unwrap()
            .session_id(),
        second.session_id()
    );
    owner.finish().await.unwrap();
}

#[tokio::test]
async fn stalled_document_owner_preserves_control_and_orders_stop_against_preparation() {
    let mut f = Fixture::new();
    let (project, session) = execution_fixture(&f).await;
    let (mut events, _) = f.runtime.sessions.events().subscribe();
    let (client, owner) = f.endpoint();
    let execute = || {
        request(
            "execute",
            "sessions.execute_cell",
            json!({"project_id":project.project_id,"notebook_id":f.notebooks[1],"session_id":session,"cell_id":"a","expected_revision":2}),
        )
    };
    let first = client.submit(execute(), ExecutionOrigin::Human).unwrap();
    assert!(endpoint_reply(first).await.response.ok);
    tokio::time::timeout(Duration::from_secs(3), async {
        while events.recv().await.unwrap().event != "session.execution_started" {}
    })
    .await
    .unwrap();

    let documents = f.runtime.documents.clone();
    let guard = documents.lock().await;
    let foreign_start = client
        .submit(
            request(
                "foreign-start",
                "sessions.ensure_primary",
                json!({"project_id":f.projects[1],"notebook_id":f.notebooks[0]}),
            ),
            ExecutionOrigin::Mcp,
        )
        .unwrap();
    assert_eq!(
        endpoint_reply(foreign_start)
            .await
            .response
            .error
            .unwrap()
            .code,
        "CK-NB-NOT-FOUND"
    );
    let edit = client.submit(request("edit", "notebooks.apply_edits", json!({"project_id":f.projects[0],"notebook_id":f.notebooks[0],"expected_revision":1,"edits":[{"op":"set_notebook_metadata","metadata":{"accepted_during_stall":true}}]})), ExecutionOrigin::Human).unwrap();
    let forget = client
        .submit(
            request(
                "forget",
                "projects.remove_from_registry",
                json!({"project_id":f.projects[0]}),
            ),
            ExecutionOrigin::Human,
        )
        .unwrap();
    assert_eq!(
        endpoint_reply(forget).await.response.error.unwrap().code,
        "CK-NB-BUSY"
    );
    let snapshot = client
        .submit(
            request("snapshot", "runtime.get_snapshot", json!({})),
            ExecutionOrigin::Human,
        )
        .unwrap();
    let start = client
        .submit(
            request(
                "start",
                "sessions.ensure_primary",
                json!({"project_id":f.projects[0],"notebook_id":f.notebooks[0]}),
            ),
            ExecutionOrigin::Mcp,
        )
        .unwrap();
    let preparing = client.submit(execute(), ExecutionOrigin::Mcp).unwrap();
    let restart = client
        .submit(
            request(
                "restart",
                "sessions.restart",
                json!({"project_id":project.project_id,"notebook_id":f.notebooks[1]}),
            ),
            ExecutionOrigin::Human,
        )
        .unwrap();
    assert_eq!(
        endpoint_reply(restart).await.response.error.unwrap().code,
        "CK-SESS-PREPARING"
    );
    let package = client
        .submit(
            request(
                "package",
                "packages.sync",
                json!({"project_id":project.project_id}),
            ),
            ExecutionOrigin::Human,
        )
        .unwrap();
    assert_eq!(
        endpoint_reply(package).await.response.error.unwrap().code,
        "CK-SESS-PREPARING"
    );
    let status = client
        .submit(
            request(
                "status",
                "sessions.get",
                json!({"project_id":project.project_id,"session_id":session}),
            ),
            ExecutionOrigin::Mcp,
        )
        .unwrap();
    assert_eq!(
        endpoint_reply(status).await.response.result.unwrap()["state"],
        "EXECUTING"
    );
    let interrupt = client
        .submit(
            request(
                "interrupt",
                "sessions.interrupt",
                json!({"project_id":project.project_id,"session_id":session}),
            ),
            ExecutionOrigin::Human,
        )
        .unwrap();
    assert!(endpoint_reply(interrupt).await.response.ok);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.event == "session.execution_finished" {
                assert_eq!(event.payload["status"], "INTERRUPTED");
                break;
            }
        }
    })
    .await
    .unwrap();
    let stop = client
        .submit(
            request(
                "stop",
                "sessions.stop",
                json!({"project_id":project.project_id,"session_id":session}),
            ),
            ExecutionOrigin::Human,
        )
        .unwrap();
    assert!(endpoint_reply(stop).await.response.ok);
    assert!(!endpoint_reply(preparing).await.response.ok);
    // No caller owns the accepted edit's lifetime.
    drop(edit);
    drop(guard);
    assert!(endpoint_reply(snapshot).await.response.ok);
    drop(start);
    owner.finish().await.unwrap();
    let saved: Value =
        serde_json::from_slice(&fs::read(f.root.join("projects/blocked/test.ipynb")).unwrap())
            .unwrap();
    assert_eq!(saved["metadata"]["accepted_during_stall"], true);
}

#[tokio::test]
async fn cancelled_middle_preparation_cannot_let_later_execution_overtake_first() {
    let f = Fixture::new();
    let (project, session) = execution_fixture(&f).await;
    let (mut events, _) = f.runtime.sessions.events().subscribe();
    let first = f
        .runtime
        .prepare_execution(
            &project,
            session,
            f.notebooks[1],
            "a",
            2,
            ExecutionOrigin::Human,
        )
        .await
        .unwrap();
    let cancelled = f
        .runtime
        .prepare_execution(
            &project,
            session,
            f.notebooks[1],
            "a",
            2,
            ExecutionOrigin::Mcp,
        )
        .await
        .unwrap();
    let third = f
        .runtime
        .prepare_execution(
            &project,
            session,
            f.notebooks[1],
            "a",
            2,
            ExecutionOrigin::Mcp,
        )
        .await
        .unwrap();
    drop(cancelled);
    let third = third.completed();
    tokio::pin!(third);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut third)
            .await
            .is_err(),
        "later execution bypassed the unpolled first admission"
    );
    let first = first.completed().await.unwrap();
    let third = third.await.unwrap();
    let mut queued = Vec::new();
    while queued.len() < 2 {
        let event = tokio::time::timeout(Duration::from_secs(3), events.recv())
            .await
            .unwrap()
            .unwrap();
        if event.event == "session.execution_queued" {
            queued.push(event.payload);
        }
    }
    assert_eq!(queued[0]["operation_id"], json!(first.operation_id));
    assert_eq!(queued[0]["origin"], "HUMAN");
    assert_eq!(queued[1]["operation_id"], json!(third.operation_id));
    assert_eq!(queued[1]["origin"], "MCP");
    f.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn stalled_document_shutdown_refuses_without_aborting_the_accepted_edit() {
    let mut f = Fixture::new();
    let documents = f.runtime.documents.clone();
    let guard = documents.lock().await;
    let (client, owner) = f.endpoint();
    let edit = client.submit(request("edit", "notebooks.apply_edits", json!({"project_id":f.projects[0],"notebook_id":f.notebooks[0],"expected_revision":1,"edits":[{"op":"set_notebook_metadata","metadata":{"kept":true}}]})), ExecutionOrigin::Human).unwrap();
    let shutdown = client
        .submit(
            request("shutdown", "bridge.shutdown", json!({})),
            ExecutionOrigin::Human,
        )
        .unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(7), shutdown.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!reply.close_transport);
    assert_eq!(reply.response.error.unwrap().code, "CK-RUN-SHUTDOWN");
    let ping = client
        .submit(
            request("ping", "bridge.ping", json!({})),
            ExecutionOrigin::Human,
        )
        .unwrap();
    assert!(endpoint_reply(ping).await.response.ok);
    drop(guard);
    assert!(endpoint_reply(edit).await.response.ok);
    owner.finish().await.unwrap();
    let saved: Value =
        serde_json::from_slice(&fs::read(f.root.join("projects/blocked/test.ipynb")).unwrap())
            .unwrap();
    assert_eq!(saved["metadata"]["kept"], true);
}
