use super::*;
use crate::{
    endpoint::{ResponseTicket, RuntimeHandle, RuntimeOwner},
    notebook::{NotebookRuntime, NotebookService},
    project::owner::ProjectOwner,
    session::{SessionSupervisor, SessionSupervisorConfig},
    uv::{UvCommandResult, UvError, UvInvocation},
};
use cokernel_domain::{ExecutionOrigin, NotebookId, OperationStatus, ProjectId};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

type Gate = (
    tokio::sync::oneshot::Sender<()>,
    std::sync::mpsc::Receiver<()>,
);
#[derive(Clone, Default)]
struct FakeUv {
    calls: Arc<AtomicUsize>,
    gate: Arc<Mutex<Option<Gate>>>,
}
impl UvRunner for FakeUv {
    fn run(&self, invocation: &UvInvocation) -> Result<UvCommandResult, UvError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let gate = self.gate.lock().unwrap().take();
        if let Some((entered, release)) = gate {
            let _ = entered.send(());
            let _ = release.recv();
        }
        fs::write(
            PathBuf::from(&invocation.args[2]).join("uv.lock"),
            format!("revision-{n}"),
        )
        .unwrap();
        Ok(UvCommandResult {
            status_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}
struct Release(Option<std::sync::mpsc::Sender<()>>);
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}
async fn hold(owner: &ProjectOwner<FakeUv>) -> Release {
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let ticket = owner
        .submit(move |_| {
            let _ = entered.send(());
            let _ = wait.recv();
            Ok(())
        })
        .unwrap();
    drop(ticket);
    tokio::time::timeout(Duration::from_secs(2), ready)
        .await
        .unwrap()
        .unwrap();
    Release(Some(release))
}
struct Fixture {
    root: PathBuf,
    uv: FakeUv,
    runtime: NotebookRuntime,
    storage: ProjectOwner<FakeUv>,
    service: Option<RuntimeService<FakeUv>>,
    projects: Vec<ProjectId>,
    notebooks: Vec<NotebookId>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("ckpo-{}", ProjectId::new()));
        fs::create_dir(&root).unwrap();
        let uv = FakeUv::default();
        let mut manager = ProjectManager::load(
            root.join("projects"),
            root.join("registry.json"),
            uv.clone(),
        )
        .unwrap();
        let mut documents = NotebookService::default();
        let mut projects = Vec::new();
        let mut notebooks = Vec::new();
        for name in ["healthy", "target"] {
            let path = root.join("projects").join(name);
            fs::create_dir(&path).unwrap();
            fs::write(path.join("pyproject.toml"), "[project]\nname='fixture'\n").unwrap();
            let record = manager.register_existing(&path).unwrap();
            notebooks.push(
                documents
                    .create(&record.project, "test.ipynb")
                    .unwrap()
                    .document
                    .notebook_id,
            );
            projects.push(record.project.project_id);
        }
        uv.calls.store(0, Ordering::SeqCst);
        let worker = root.join("worker.py");
        fs::write(&worker, include_str!("../../tests/fixtures/fake_uv.py")).unwrap();
        fs::set_permissions(&worker, fs::Permissions::from_mode(0o500)).unwrap();
        let mut config = SessionSupervisorConfig::development(root.join("package"), root.join("s"));
        config.uv_executable = worker;
        config.shutdown_timeout = Duration::from_millis(200);
        let runtime = NotebookRuntime::new(documents, SessionSupervisor::new(config));
        let service = RuntimeService::with_notebooks(manager, runtime.clone());
        let storage = service.projects.owner().unwrap();
        Self {
            root,
            uv,
            runtime,
            storage,
            service: Some(service),
            projects,
            notebooks,
        }
    }
    fn endpoint(&mut self) -> (RuntimeHandle, RuntimeOwner) {
        RuntimeOwner::start(self.service.take().unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn submit(client: &RuntimeHandle, method: &str, params: Value) -> ResponseTicket {
    client
        .submit(
            RequestEnvelope {
                protocol: 1,
                request_id: uuid::Uuid::new_v4().to_string(),
                method: method.into(),
                params,
            },
            ExecutionOrigin::Human,
        )
        .unwrap()
}
async fn reply(ticket: ResponseTicket) -> ResponseEnvelope {
    tokio::time::timeout(Duration::from_secs(2), ticket.wait())
        .await
        .expect("Runtime reply blocked")
        .unwrap()
        .response
}
async fn call(client: &RuntimeHandle, method: &str, params: Value) -> ResponseEnvelope {
    reply(submit(client, method, params)).await
}
async fn settled(client: &RuntimeHandle, project: ProjectId, operation: Value) -> Value {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let r = call(
                client,
                "operations.get",
                json!({"project_id":project,"operation_id":operation}),
            )
            .await
            .result
            .unwrap();
            if r["result"]["state"] != "RUNNING" {
                return r;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn project_prepare_storage_barrier_preserves_session_control_and_write_ahead_order() {
    let mut f = Fixture::new();
    let project = f.storage.get(f.projects[0]).unwrap().project;
    let session = f
        .runtime
        .ensure_primary(&project, f.notebooks[0])
        .await
        .unwrap();
    let (mut events, _) = f.runtime.sessions.events().subscribe();
    let receipt = session
        .execute_cell_captured(ExecutionOrigin::Human, "active", "sleep:30")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while events.recv().await.unwrap().event != "session.execution_started" {}
    })
    .await
    .unwrap();
    let barrier = hold(&f.storage).await;
    let (client, owner) = f.endpoint();
    let target = f.projects[1];
    let pending = submit(
        &client,
        "packages.add",
        json!({"project_id":target,"packages":["one"]}),
    );
    assert!(call(&client, "bridge.ping", json!({})).await.ok);
    assert_eq!(f.uv.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        call(&client, "packages.sync", json!({"project_id":target}))
            .await
            .error
            .unwrap()
            .code,
        "CK-UV-BUSY"
    );
    assert!(
        call(
            &client,
            "sessions.get",
            json!({"project_id":project.project_id,"session_id":session.session_id()})
        )
        .await
        .ok
    );
    assert!(
        call(
            &client,
            "sessions.interrupt",
            json!({"project_id":project.project_id,"session_id":session.session_id()})
        )
        .await
        .ok
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), receipt.completed())
            .await
            .unwrap()
            .unwrap()
            .status,
        OperationStatus::Interrupted
    );
    drop(barrier);
    let admitted = reply(pending).await.result.unwrap();
    assert_eq!(
        settled(&client, target, admitted["operation_id"].clone()).await["result"]["state"],
        "SUCCEEDED"
    );
    assert_eq!(f.uv.calls.load(Ordering::SeqCst), 1);
    owner.finish().await.unwrap();
}

#[tokio::test]
async fn project_reconciliation_remains_reserved_and_uncertain_while_storage_is_held() {
    let mut f = Fixture::new();
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    *f.uv.gate.lock().unwrap() = Some((entered, wait));
    let uv_release = Release(Some(release));
    let (client, owner) = f.endpoint();
    let target = f.projects[1];
    let admitted = call(
        &client,
        "packages.add",
        json!({"project_id":target,"packages":["one"]}),
    )
    .await
    .result
    .unwrap();
    tokio::time::timeout(Duration::from_secs(2), ready)
        .await
        .unwrap()
        .unwrap();
    let barrier = hold(&f.storage).await;
    drop(uv_release);
    assert!(call(&client, "bridge.ping", json!({})).await.ok);
    assert!(
        f.storage
            .get(target)
            .unwrap()
            .environment_reconcile_required
    );
    let recovered = ProjectManager::load(
        f.root.join("projects"),
        f.root.join("registry.json"),
        FakeUv::default(),
    )
    .unwrap();
    assert!(
        recovered
            .get(target)
            .unwrap()
            .environment_reconcile_required
    );
    assert_eq!(
        call(
            &client,
            "operations.get",
            json!({"project_id":target,"operation_id":admitted["operation_id"]})
        )
        .await
        .result
        .unwrap()["result"]["state"],
        "RUNNING"
    );
    assert_eq!(
        call(&client, "packages.sync", json!({"project_id":target}))
            .await
            .error
            .unwrap()
            .code,
        "CK-UV-BUSY"
    );
    drop(barrier);
    assert_eq!(
        settled(&client, target, admitted["operation_id"].clone()).await["result"]["state"],
        "SUCCEEDED"
    );
    assert!(
        !f.storage
            .get(target)
            .unwrap()
            .environment_reconcile_required
    );
    owner.finish().await.unwrap();
}

#[tokio::test]
async fn registry_removal_reserves_identity_and_failed_commit_can_be_retried() {
    let mut f = Fixture::new();
    let target = f.projects[1];
    let barrier = hold(&f.storage).await;
    let (client, owner) = f.endpoint();
    let pending = submit(
        &client,
        "projects.remove_from_registry",
        json!({"project_id":target}),
    );
    assert!(call(&client, "bridge.ping", json!({})).await.ok);
    assert_eq!(
        call(
            &client,
            "notebooks.create",
            json!({"project_id":target,"relative_path":"must-not-exist.ipynb"})
        )
        .await
        .error
        .unwrap()
        .code,
        "CK-PROJ-BUSY"
    );
    assert_eq!(
        call(
            &client,
            "sessions.ensure_primary",
            json!({"project_id":target,"notebook_id":f.notebooks[1]})
        )
        .await
        .error
        .unwrap()
        .code,
        "CK-UV-BUSY"
    );
    fs::rename(f.root.join("registry.json"), f.root.join("prior.json")).unwrap();
    fs::create_dir(f.root.join("registry.json")).unwrap();
    drop(barrier);
    assert_eq!(reply(pending).await.error.unwrap().code, "CK-PROJ-IO");
    assert!(
        call(&client, "projects.get", json!({"project_id":target}))
            .await
            .ok
    );
    fs::remove_dir(f.root.join("registry.json")).unwrap();
    fs::rename(f.root.join("prior.json"), f.root.join("registry.json")).unwrap();
    assert!(
        call(
            &client,
            "projects.remove_from_registry",
            json!({"project_id":target})
        )
        .await
        .ok
    );
    assert!(f.storage.get(target).is_err());
    assert!(f.root.join("projects/target/test.ipynb").is_file());
    assert!(!f.root.join("projects/target/must-not-exist.ipynb").exists());
    owner.finish().await.unwrap();
}

#[tokio::test]
async fn owner_shutdown_drains_project_preparation_without_starting_uv() {
    let mut f = Fixture::new();
    let barrier = hold(&f.storage).await;
    let (client, owner) = f.endpoint();
    let pending = submit(&client, "projects.create", json!({"name":"retained"}));
    assert!(call(&client, "bridge.ping", json!({})).await.ok);
    let closing = tokio::spawn(owner.finish());
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match client.submit(
                RequestEnvelope {
                    protocol: 1,
                    request_id: "close-check".into(),
                    method: "bridge.ping".into(),
                    params: json!({}),
                },
                ExecutionOrigin::Human,
            ) {
                Err(_) => break,
                Ok(ticket) => {
                    let _ = ticket.wait().await;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(barrier);
    tokio::time::timeout(Duration::from_secs(3), closing)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(reply(pending).await.error.unwrap().code, "CK-RUN-STOPPED");
    assert_eq!(f.uv.calls.load(Ordering::SeqCst), 0);
    let recovered = ProjectManager::load(
        f.root.join("projects"),
        f.root.join("registry.json"),
        FakeUv::default(),
    )
    .unwrap();
    let retained = recovered
        .list()
        .into_iter()
        .find(|r| r.project.name == "retained")
        .unwrap();
    assert!(retained.initialization_required && retained.environment_reconcile_required);
    assert!(!f.root.join("projects/retained/pyproject.toml").exists());
}

#[tokio::test]
async fn project_owner_capacity_and_panic_fail_closed_without_losing_accepted_work() {
    let f = Fixture::new();
    let barrier = hold(&f.storage).await;
    let count = Arc::new(AtomicUsize::new(0));
    let mut tickets = Vec::new();
    for _ in 0..16 {
        let count = count.clone();
        tickets.push(
            f.storage
                .submit(move |_| {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .unwrap(),
        );
    }
    assert!(matches!(
        f.storage.submit(|_| Ok(())),
        Err(ProjectError::OwnerBusy)
    ));
    drop(tickets.remove(0));
    drop(barrier);
    for ticket in tickets {
        ticket.completed().await.unwrap();
    }
    assert_eq!(count.load(Ordering::SeqCst), 16);
    let panic = f
        .storage
        .submit::<()>(|_| panic!("injected Project owner failure"))
        .unwrap();
    assert!(matches!(
        panic.completed().await,
        Err(ProjectError::OwnerClosed)
    ));
    assert!(
        f.storage
            .list()
            .iter()
            .all(|r| r.environment_reconcile_required)
    );
}

#[tokio::test]
async fn registration_aliases_share_one_project_reservation() {
    let mut f = Fixture::new();
    let target = f.projects[1];
    let alias = f.root.join("projects/alias");
    std::os::unix::fs::symlink(f.root.join("projects/target"), &alias).unwrap();
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    *f.uv.gate.lock().unwrap() = Some((entered, wait));
    let uv_release = Release(Some(release));
    let barrier = hold(&f.storage).await;
    let (client, owner) = f.endpoint();
    let first = submit(&client, "projects.register", json!({"path":alias}));
    let second = submit(
        &client,
        "projects.register",
        json!({"path":f.root.join("projects/target")}),
    );
    assert!(call(&client, "bridge.ping", json!({})).await.ok);
    drop(barrier);
    let (a, b) = tokio::join!(reply(first), reply(second));
    let (accepted, rejected) = if a.ok { (a, b) } else { (b, a) };
    assert_eq!(rejected.error.unwrap().code, "CK-UV-BUSY");
    let accepted = accepted.result.unwrap();
    assert_eq!(accepted["project_id"], json!(target));
    tokio::time::timeout(Duration::from_secs(2), ready)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.uv.calls.load(Ordering::SeqCst), 1);
    drop(uv_release);
    assert_eq!(
        settled(&client, target, accepted["operation_id"].clone()).await["result"]["state"],
        "SUCCEEDED"
    );
    owner.finish().await.unwrap();
}
