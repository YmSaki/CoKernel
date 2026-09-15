#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use anyhow::{bail, Context, Result};
#[cfg(unix)]
use cokernel_domain::{ExecutionOrigin, NotebookId, OperationId, OperationStatus, Project, ProjectId};
#[cfg(unix)]
use cokernel_runtime::session::{
    SessionEvent, SessionInspectionError, SessionSupervisor, SessionSupervisorConfig,
};
#[cfg(unix)]
use tokio::sync::broadcast;
#[cfg(unix)]
use tokio::time::timeout;

#[cfg(unix)]
struct TempWorkspace(PathBuf);

#[cfg(unix)]
impl TempWorkspace {
    fn new() -> Result<Self> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before UNIX_EPOCH")?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cokernel-session-inspection-smoke-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self(path))
    }
}

#[cfg(unix)]
impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<()> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .context("could not locate repository root")?
        .to_path_buf();
    let temp = TempWorkspace::new()?;
    let project = prepare_project(&temp.0.join("project"))?;
    let supervisor = SessionSupervisor::new(SessionSupervisorConfig::development(
        repo_root.join("worker"),
        temp.0.join("sockets"),
    ));
    let session = supervisor
        .ensure_primary(&project, NotebookId::new())
        .await?;
    let mut events = session.subscribe();

    let seed = session
        .execute_cell(
            ExecutionOrigin::Human,
            "seed",
            "x = {'answer': 42}\nnone_value = None\nclass Secret: pass\nsecret = Secret()",
        )
        .await?;
    let seed_status = wait_operation(&mut events, seed).await?;
    if seed_status != OperationStatus::Succeeded {
        bail!("seed execution failed with {seed_status:?}");
    }

    let variables = session.list_variables().await?;
    let x = variables
        .iter()
        .find(|variable| variable.name == "x")
        .context("list_variables did not return x")?;
    if !x.supported || x.type_module != "builtins" || x.type_name != "dict" {
        bail!("unexpected x summary: {x:?}");
    }
    let none_summary = variables
        .iter()
        .find(|variable| variable.name == "none_value")
        .context("list_variables did not return none_value")?;
    if !none_summary.supported
        || none_summary.type_module != "builtins"
        || none_summary.type_name != "NoneType"
    {
        bail!("unexpected none_value summary: {none_summary:?}");
    }
    let secret = variables
        .iter()
        .find(|variable| variable.name == "secret")
        .context("list_variables did not return secret")?;
    if secret.supported {
        bail!("custom object was incorrectly marked supported");
    }

    let x_value = session.get_variable("x").await?;
    if !x_value.supported || x_value.value != Some(serde_json::json!({"answer": 42})) {
        bail!("unexpected x value: {x_value:?}");
    }

    // Serde maps JSON null to Option::None. The supported bit distinguishes a
    // valid Python None from an unsupported value; Runtime validation separately
    // requires the raw `value` field to be present so omission cannot masquerade
    // as Python None.
    let none_value = session.get_variable("none_value").await?;
    if !none_value.supported || none_value.value.is_some() || none_value.reason.is_some() {
        bail!("unexpected supported None value: {none_value:?}");
    }

    let secret_value = session.get_variable("secret").await?;
    if secret_value.supported || secret_value.value.is_some() || secret_value.reason.is_none() {
        bail!("unexpected unsupported secret value: {secret_value:?}");
    }

    match session.get_variable("x + 1").await {
        Err(SessionInspectionError::WorkerRequest { code, .. }) if code == "CK-WORKER-REQUEST" => {}
        other => bail!("invalid identifier returned unexpected result: {other:?}"),
    }

    let survivor = session
        .execute_cell(ExecutionOrigin::Mcp, "survivor", "x['answer'] + 1")
        .await?;
    let survivor_status = wait_operation(&mut events, survivor).await?;
    if survivor_status != OperationStatus::Succeeded {
        bail!("worker did not survive rejected inspection: {survivor_status:?}");
    }

    session.stop().await?;
    println!("[session-inspection-smoke] PASS");
    Ok(())
}

#[cfg(unix)]
fn prepare_project(root: &Path) -> Result<Project> {
    fs::create_dir_all(root).with_context(|| format!("failed to create {}", root.display()))?;
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"cokernel-session-inspection-smoke\"\nversion = \"0.0.0\"\nrequires-python = \">=3.11\"\ndependencies = []\n",
    )?;

    let status = Command::new("uv")
        .arg("sync")
        .arg("--project")
        .arg(root)
        .env("UV_NO_PROGRESS", "1")
        .status()
        .context("failed to launch uv sync for Session inspection smoke")?;
    if !status.success() {
        bail!("uv sync failed with {status}");
    }

    Ok(Project {
        project_id: ProjectId::new(),
        name: "cokernel-session-inspection-smoke".into(),
        root_path: root.to_string_lossy().into_owned(),
        environment_generation: 1,
    })
}

#[cfg(unix)]
async fn wait_operation(
    events: &mut broadcast::Receiver<SessionEvent>,
    operation_id: OperationId,
) -> Result<OperationStatus> {
    timeout(Duration::from_secs(20), async {
        loop {
            match events.recv().await {
                Ok(SessionEvent::OperationFinished {
                    operation_id: finished,
                    status,
                    ..
                }) if finished == operation_id => return Ok(status),
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    bail!("Session event stream closed while waiting for {operation_id}")
                }
            }
        }
    })
    .await
    .with_context(|| format!("timed out waiting for operation {operation_id}"))?
}

#[cfg(not(unix))]
fn main() {
    println!("[session-inspection-smoke] SKIP: Unix-domain Session runtime is exercised in WSL/Linux");
}
