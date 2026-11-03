#![cfg(target_os = "linux")]

use cokernel_domain::{ExecutionOrigin, ProjectId};
use cokernel_protocol::{RequestEnvelope, ResponseEnvelope};
use cokernel_runtime::{
    notebook::{NotebookRuntime, NotebookService},
    project::ProjectManager,
    service::RuntimeService,
    session::{SessionSupervisor, SessionSupervisorConfig},
    uv::{UvCommandResult, UvError, UvInvocation, UvRunControl, UvRunner},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Default)]
struct Gate {
    released: Mutex<bool>,
    ready: Condvar,
    hold_sync: AtomicBool,
}
impl Gate {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.ready.notify_all();
    }
}
struct FakeUv(Arc<Gate>);
impl UvRunner for FakeUv {
    fn run(&self, invocation: &UvInvocation) -> Result<UvCommandResult, UvError> {
        self.run_controlled(invocation, &UvRunControl::default())
    }
    fn run_controlled(
        &self,
        invocation: &UvInvocation,
        control: &UvRunControl,
    ) -> Result<UvCommandResult, UvError> {
        let args: Vec<_> = invocation
            .args
            .iter()
            .map(|v| v.to_string_lossy().into_owned())
            .collect();
        let root = PathBuf::from(if args[0] == "init" {
            args.last().unwrap()
        } else {
            &args[2]
        });
        if args[0] != "sync" || self.0.hold_sync.load(Ordering::Acquire) {
            let deadline = std::time::Instant::now() + Duration::from_secs(4);
            let mut lock = self.0.released.lock().unwrap();
            while !*lock {
                if control.is_cancelled() {
                    return Err(UvError::Cancelled);
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "test failed to release fake uv"
                );
                lock = self
                    .0
                    .ready
                    .wait_timeout(lock, Duration::from_millis(10))
                    .unwrap()
                    .0;
            }
            drop(lock);
        }
        if control.is_cancelled() {
            return Err(UvError::Cancelled);
        }
        if args[0] == "init" {
            fs::write(root.join("pyproject.toml"), "[project]\nname='test'\n").unwrap();
        } else if args[0] != "sync" {
            if args.iter().any(|a| a == "panic-test") {
                panic!("injected uv task failure");
            }
            fs::write(root.join("uv.lock"), args.join(" ")).unwrap();
            if args.iter().any(|a| a == "fail-test") {
                return Err(UvError::CommandFailed {
                    invocation: "fake".into(),
                    status_code: Some(1),
                    stderr: "partial environment change".into(),
                });
            }
        } else {
            fs::write(root.join("uv.lock"), "initial").unwrap();
        }
        Ok(UvCommandResult {
            status_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

struct Fixture {
    root: PathBuf,
    gate: Arc<Gate>,
    service: RuntimeService<FakeUv>,
    ids: Vec<ProjectId>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("ckpackages-{}", ProjectId::new()));
        fs::create_dir_all(root.join("projects")).unwrap();
        let gate = Arc::new(Gate::default());
        let mut manager = ProjectManager::load(
            root.join("projects"),
            root.join("registry.json"),
            FakeUv(gate.clone()),
        )
        .unwrap();
        let mut ids = vec![];
        for name in ["a", "b", "c", "d", "e"] {
            let project = root.join("projects").join(name);
            fs::create_dir_all(&project).unwrap();
            fs::write(project.join("pyproject.toml"), "[project]\nname='test'\n").unwrap();
            ids.push(
                manager
                    .register_existing(&project)
                    .unwrap()
                    .project
                    .project_id,
            );
        }
        let runtime = NotebookRuntime::new(
            NotebookService::default(),
            SessionSupervisor::new(SessionSupervisorConfig::development(
                root.join("worker"),
                root.join("sockets"),
            )),
        );
        Self {
            root,
            gate,
            service: RuntimeService::with_notebooks(manager, runtime),
            ids,
        }
    }
    async fn rpc(&mut self, method: &str, params: Value) -> ResponseEnvelope {
        tokio::time::timeout(Duration::from_secs(1), async {
            if let Some(response) = self
                .service
                .admit(
                    RequestEnvelope {
                        protocol: 1,
                        request_id: "test".into(),
                        method: method.into(),
                        params,
                    },
                    ExecutionOrigin::Human,
                )
                .await
            {
                return response;
            }
            loop {
                if let Some(response) = self
                    .service
                    .take_responses()
                    .into_iter()
                    .find(|r| r.request_id == "test")
                {
                    return response;
                }
                let completed = self.service.next_background_completion().await;
                self.service.finish_background_completion(completed).await;
            }
        })
        .await
        .expect("RPC blocked behind uv")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.gate.release();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn slow_packages_preserve_responsive_rpc_project_exclusion_and_parallelism() {
    let mut f = Fixture::new();
    let a = f.ids[0];
    let b = f.ids[1];
    let op = f
        .rpc("packages.add", json!({"project_id":a,"packages":["one"]}))
        .await;
    assert!(op.ok);
    let id = op.result.unwrap()["operation_id"].clone();
    let recovered = ProjectManager::load(
        f.root.join("projects"),
        f.root.join("registry.json"),
        FakeUv(f.gate.clone()),
    )
    .unwrap();
    assert!(recovered.get(a).unwrap().environment_reconcile_required);
    assert!(f.rpc("bridge.ping", json!({})).await.ok);
    assert!(
        f.rpc("environment.get_status", json!({"project_id":a}))
            .await
            .ok
    );
    assert_eq!(
        f.rpc("operations.get", json!({"project_id":a,"operation_id":id}))
            .await
            .result
            .unwrap()["result"]["state"],
        "RUNNING"
    );
    assert_eq!(
        f.rpc("operations.get", json!({"project_id":b,"operation_id":id}))
            .await
            .error
            .unwrap()
            .code,
        "CK-UV-OPERATION-NOT-FOUND"
    );
    for method in ["packages.sync", "projects.remove_from_registry"] {
        assert_eq!(
            f.rpc(
                "operations.cancel",
                json!({"project_id":b,"operation_id":id})
            )
            .await
            .error
            .unwrap()
            .code,
            "CK-UV-OPERATION-NOT-FOUND"
        );
        assert_eq!(
            f.rpc(method, json!({"project_id":a}))
                .await
                .error
                .unwrap()
                .code,
            "CK-UV-BUSY"
        );
    }
    assert_eq!(
        f.rpc("bridge.shutdown", json!({}))
            .await
            .error
            .unwrap()
            .code,
        "CK-UV-BUSY"
    );
    for p in f.ids.clone().into_iter().skip(1).take(3) {
        assert!(
            f.rpc("packages.add", json!({"project_id":p,"packages":["two"]}))
                .await
                .ok
        );
    }
    let fifth = f.ids[4];
    assert_eq!(
        f.rpc(
            "packages.add",
            json!({"project_id":fifth,"packages":["three"]})
        )
        .await
        .error
        .unwrap()
        .code,
        "CK-UV-BUSY"
    );
    f.gate.release();
    tokio::time::timeout(Duration::from_secs(3), f.service.settle_packages())
        .await
        .unwrap();
    let result = f
        .rpc("operations.get", json!({"project_id":a,"operation_id":id}))
        .await
        .result
        .unwrap();
    assert_eq!(result["result"]["state"], "SUCCEEDED");
    assert_eq!(
        result["result"]["project"]["project"]["environment_generation"],
        2
    );
    assert!(
        !f.service
            .projects()
            .get(a)
            .unwrap()
            .environment_reconcile_required
    );
    assert!(f.rpc("bridge.shutdown", json!({})).await.ok);
}

#[tokio::test]
async fn failed_or_panicked_uv_reconciles_generation_and_releases_reservation() {
    let mut f = Fixture::new();
    let a = f.ids[0];
    f.gate.release();
    let notebook = f
        .rpc(
            "notebooks.create",
            json!({"project_id":a,"relative_path":"uncertain.ipynb"}),
        )
        .await
        .result
        .unwrap()["document"]["notebook_id"]
        .clone();
    for (package, generation) in [("fail-test", 2), ("panic-test", 3)] {
        let op = f
            .rpc("packages.add", json!({"project_id":a,"packages":[package]}))
            .await
            .result
            .unwrap();
        f.service.settle_packages().await;
        let result = f
            .rpc(
                "operations.get",
                json!({"project_id":a,"operation_id":op["operation_id"]}),
            )
            .await
            .result
            .unwrap();
        assert_eq!(result["result"]["state"], "FAILED");
        assert_eq!(
            f.service
                .projects()
                .get(a)
                .unwrap()
                .project
                .environment_generation,
            generation
        );
        assert!(
            f.service
                .projects()
                .get(a)
                .unwrap()
                .environment_reconcile_required
        );
        assert_eq!(
            f.rpc(
                "sessions.ensure_primary",
                json!({"project_id":a,"notebook_id":notebook})
            )
            .await
            .error
            .unwrap()
            .code,
            "CK-UV-NEEDS-SYNC"
        );
    }
    assert!(f.rpc("packages.sync", json!({"project_id":a})).await.ok);
    f.service.settle_packages().await;
}

#[tokio::test]
async fn malformed_options_and_symlink_escape_are_rejected_before_launch() {
    let mut f = Fixture::new();
    let a = f.ids[0];
    for packages in [
        json!(["--directory=/tmp"]),
        json!(["x\n"]),
        json!(["x".repeat(4097)]),
        json!([""]),
    ] {
        assert!(
            !f.rpc("packages.add", json!({"project_id":a,"packages":packages}))
                .await
                .ok
        );
    }
    assert!(
        !f.rpc("packages.sync", json!({"project_id":a,"extra":true}))
            .await
            .ok
    );
    fs::rename(f.root.join("projects/a"), f.root.join("outside")).unwrap();
    std::os::unix::fs::symlink(f.root.join("outside"), f.root.join("projects/a")).unwrap();
    assert_eq!(
        f.rpc("packages.sync", json!({"project_id":a}))
            .await
            .error
            .unwrap()
            .code,
        "CK-SEC-PROJECT-PATH"
    );
}

#[tokio::test]
async fn retained_operation_history_is_bounded_and_latest_remains_queryable() {
    let mut f = Fixture::new();
    let a = f.ids[0];
    let mut first = Value::Null;
    let mut last = Value::Null;
    for i in 0..258 {
        let op = f
            .rpc("packages.sync", json!({"project_id":a}))
            .await
            .result
            .unwrap();
        if i == 0 {
            first = op["operation_id"].clone();
        }
        last = op["operation_id"].clone();
        f.service.settle_packages().await;
    }
    assert!(
        !f.rpc(
            "operations.get",
            json!({"project_id":a,"operation_id":first})
        )
        .await
        .ok
    );
    assert!(
        f.rpc(
            "operations.get",
            json!({"project_id":a,"operation_id":last})
        )
        .await
        .ok
    );
    let snapshot = f
        .rpc("runtime.get_snapshot", json!({}))
        .await
        .result
        .unwrap();
    assert_eq!(
        snapshot["package_operations"].as_array().unwrap().len(),
        256
    );
}

#[tokio::test]
async fn registry_commit_failure_keeps_environment_uncertain_in_memory() {
    let mut f = Fixture::new();
    let project = f.ids[0];
    let op = f
        .rpc(
            "packages.add",
            json!({"project_id":project,"packages":["one"]}),
        )
        .await
        .result
        .unwrap();
    fs::rename(
        f.root.join("registry.json"),
        f.root.join("prior-registry.json"),
    )
    .unwrap();
    fs::create_dir(f.root.join("registry.json")).unwrap();
    f.gate.release();
    f.service.settle_packages().await;
    let result = f
        .rpc(
            "operations.get",
            json!({"project_id":project,"operation_id":op["operation_id"]}),
        )
        .await
        .result
        .unwrap();
    assert_eq!(result["result"]["state"], "FAILED");
    assert!(
        f.service
            .projects()
            .get(project)
            .unwrap()
            .environment_reconcile_required,
        "failed registry commit must not make the in-memory environment ready"
    );
}

#[tokio::test]
async fn cancelled_project_creation_retains_files_and_sync_resumes_same_identity() {
    let mut f = Fixture::new();
    let created = f
        .rpc("projects.create", json!({"name":"new"}))
        .await
        .result
        .unwrap();
    assert_eq!(created["kind"], "PROJECT");
    let project = created["project_id"].clone();
    assert!(f.rpc("bridge.ping", json!({})).await.ok);
    assert_eq!(
        f.rpc("projects.create", json!({"name":"new"}))
            .await
            .error
            .unwrap()
            .code,
        "CK-PROJ-EXISTS"
    );
    fs::write(f.root.join("projects/new/user.txt"), "preserve me").unwrap();
    assert!(
        f.rpc(
            "operations.cancel",
            json!({"project_id":project,"operation_id":created["operation_id"]})
        )
        .await
        .ok
    );
    f.service.settle_packages().await;
    let state = f
        .rpc("projects.get", json!({"project_id":project}))
        .await
        .result
        .unwrap();
    assert_eq!(state["initialization_required"], true);
    assert_eq!(state["environment_reconcile_required"], true);
    assert_eq!(
        f.rpc(
            "packages.add",
            json!({"project_id":project,"packages":["one"]})
        )
        .await
        .error
        .unwrap()
        .code,
        "CK-UV-NEEDS-SYNC"
    );
    f.gate.release();
    let repair = f
        .rpc("packages.sync", json!({"project_id":project}))
        .await
        .result
        .unwrap();
    f.service.settle_packages().await;
    let result = f
        .rpc(
            "operations.get",
            json!({"project_id":project,"operation_id":repair["operation_id"]}),
        )
        .await
        .result
        .unwrap();
    assert_eq!(result["result"]["state"], "SUCCEEDED");
    assert_eq!(
        result["result"]["project"]["initialization_required"],
        false
    );
    assert_eq!(
        fs::read_to_string(f.root.join("projects/new/user.txt")).unwrap(),
        "preserve me"
    );
}

#[tokio::test]
async fn register_is_async_reserves_existing_identity_and_shares_mutation_capacity() {
    let mut f = Fixture::new();
    f.gate.hold_sync.store(true, Ordering::Release);
    let root = f.root.join("projects/a");
    let registered = f
        .rpc("projects.register", json!({"path":root}))
        .await
        .result
        .unwrap();
    assert_eq!(registered["project_id"], json!(f.ids[0]));
    assert!(f.rpc("bridge.ping", json!({})).await.ok);
    assert_eq!(
        f.rpc("projects.register", json!({"path":root}))
            .await
            .error
            .unwrap()
            .code,
        "CK-UV-BUSY"
    );
    for name in ["one", "two", "three"] {
        assert!(f.rpc("projects.create", json!({"name":name})).await.ok);
    }
    assert_eq!(
        f.rpc("projects.create", json!({"name":"five"}))
            .await
            .error
            .unwrap()
            .code,
        "CK-UV-BUSY"
    );
    assert!(!f.root.join("projects/five").exists());
    f.gate.release();
    f.service.settle_packages().await;
    let result = f
        .rpc(
            "operations.get",
            json!({"project_id":f.ids[0],"operation_id":registered["operation_id"]}),
        )
        .await
        .result
        .unwrap();
    assert_eq!(result["result"]["state"], "SUCCEEDED");
    assert_eq!(f.service.projects().list().len(), 8);
}

#[tokio::test]
async fn failed_project_admission_keeps_repairable_identity_without_launching_uv() {
    let mut f = Fixture::new();
    fs::rename(
        f.root.join("registry.json"),
        f.root.join("prior-registry.json"),
    )
    .unwrap();
    fs::create_dir(f.root.join("registry.json")).unwrap();
    let response = f
        .rpc("projects.create", json!({"name":"admission-failed"}))
        .await;
    assert_eq!(response.error.unwrap().code, "CK-PROJ-IO");
    let records = f.service.projects().list();
    let reserved = records
        .iter()
        .find(|r| r.project.name == "admission-failed")
        .unwrap();
    assert!(reserved.environment_reconcile_required && reserved.initialization_required);
    assert!(
        !f.root
            .join("projects/admission-failed/pyproject.toml")
            .exists()
    );
    let snapshot = f
        .rpc("runtime.get_snapshot", json!({}))
        .await
        .result
        .unwrap();
    assert!(
        snapshot["package_operations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fs::remove_dir(f.root.join("registry.json")).unwrap();
    f.gate.release();
    assert!(
        f.rpc(
            "packages.sync",
            json!({"project_id":reserved.project.project_id})
        )
        .await
        .ok
    );
    f.service.settle_packages().await;
    assert!(
        !f.service
            .projects()
            .get(reserved.project.project_id)
            .unwrap()
            .initialization_required
    );
}
