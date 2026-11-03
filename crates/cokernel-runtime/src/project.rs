use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use cokernel_domain::{Project, ProjectId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::uv::{self, UvError, UvRunner};

const REGISTRY_SCHEMA_VERSION: u32 = 1;

#[cfg(target_os = "linux")]
pub(crate) mod owner;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("Project storage owner is unavailable")]
    OwnerClosed,
    #[error("Project storage queue is full")]
    OwnerBusy,
    #[error("project name is invalid: {0}")]
    InvalidName(String),
    #[error("project path escapes the managed projects root: {0}")]
    PathOutsideRoot(String),
    #[error("project already exists: {0}")]
    AlreadyExists(String),
    #[error("project does not exist: {0}")]
    NotFound(String),
    #[error("project is missing pyproject.toml: {0}")]
    MissingPyproject(String),
    #[error("project path is not valid UTF-8: {0}")]
    NonUtf8Path(String),
    #[error("package operation requires at least one package")]
    EmptyPackageList,
    #[error("package arguments exceed limits or contain an option/control character")]
    InvalidPackages,
    #[error("Project initialization is incomplete; synchronize it before changing packages")]
    InitializationRequired,
    #[error("project environment is not ready; Python executable not found at {0}")]
    PythonMissing(String),
    #[error("unsupported project registry schema: {0}")]
    UnsupportedRegistrySchema(u32),
    #[error(transparent)]
    Uv(#[from] UvError),
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("project registry is invalid JSON: {0}")]
    InvalidRegistry(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub project: Project,
    pub lock_fingerprint: Option<String>,
    #[serde(default)]
    pub environment_reconcile_required: bool,
    #[serde(default)]
    pub initialization_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEnvironmentStatus {
    pub project_id: ProjectId,
    pub environment_generation: u64,
    pub lock_fingerprint: Option<String>,
    pub python_executable: Option<String>,
    pub ready: bool,
    pub reconcile_required: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    schema_version: u32,
    projects: Vec<ProjectRecord>,
}

impl Default for RegistryFile {
    fn default() -> Self {
        Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
            projects: Vec::new(),
        }
    }
}

pub struct ProjectManager<R> {
    projects_root: PathBuf,
    registry_path: PathBuf,
    uv: Arc<R>,
    registry: RegistryFile,
}

#[cfg(target_os = "linux")]
pub(crate) struct EnvironmentPlan<R> {
    pub project_id: ProjectId,
    pub root: PathBuf,
    pub runner: Arc<R>,
    pub commands: Vec<uv::UvInvocation>,
}

impl<R: UvRunner> ProjectManager<R> {
    pub fn load(
        projects_root: impl Into<PathBuf>,
        registry_path: impl Into<PathBuf>,
        uv: R,
    ) -> Result<Self, ProjectError> {
        let projects_root = projects_root.into();
        let projects_root =
            std::path::absolute(&projects_root).map_err(|source| ProjectError::Io {
                path: projects_root.display().to_string(),
                source,
            })?;
        let registry_path = registry_path.into();
        create_dir_all(&projects_root)?;
        #[cfg(target_os = "linux")]
        let projects_root = projects_root
            .canonicalize()
            .map_err(|source| ProjectError::Io {
                path: projects_root.display().to_string(),
                source,
            })?;

        let registry = if registry_path.exists() {
            let bytes = fs::read(&registry_path).map_err(|source| ProjectError::Io {
                path: registry_path.display().to_string(),
                source,
            })?;
            let parsed: RegistryFile = serde_json::from_slice(&bytes)?;
            if parsed.schema_version != REGISTRY_SCHEMA_VERSION {
                return Err(ProjectError::UnsupportedRegistrySchema(
                    parsed.schema_version,
                ));
            }
            parsed
        } else {
            RegistryFile::default()
        };

        Ok(Self {
            projects_root,
            registry_path,
            uv: Arc::new(uv),
            registry,
        })
    }

    pub fn projects_root(&self) -> &Path {
        &self.projects_root
    }

    /// Validate and detach only the uv work; the authoritative registry stays here.
    #[cfg(target_os = "linux")]
    pub(crate) fn prepare_package(
        &mut self,
        project_id: ProjectId,
        method: &str,
        packages: &[String],
    ) -> Result<EnvironmentPlan<R>, ProjectError> {
        let root = self.ensure_managed_path(&self.project_root(project_id)?)?;
        let project = self.get(project_id)?;
        if project.initialization_required && method != "packages.sync" {
            return Err(ProjectError::InitializationRequired);
        }
        let mut commands = Vec::new();
        if project.initialization_required && !root.join("pyproject.toml").is_file() {
            commands.push(uv::init_project(&root, &project.project.name));
        }
        let invocation = match method {
            "packages.add" => {
                validate_packages(packages)?;
                uv::add_packages(&root, packages)
            }
            "packages.remove" => {
                validate_packages(packages)?;
                uv::remove_packages(&root, packages)
            }
            _ => uv::sync_project(&root),
        };
        // Write-ahead uncertainty survives Runtime death or a partial uv failure.
        // No subprocess starts unless this marker has been saved to the registry.
        if let Some(record) = self
            .registry
            .projects
            .iter_mut()
            .find(|r| r.project.project_id == project_id)
        {
            record.environment_reconcile_required = true;
        }
        self.save_registry()?;
        commands.push(invocation);
        Ok(EnvironmentPlan {
            project_id,
            root,
            runner: self.uv.clone(),
            commands,
        })
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn resolve_registration(
        &self,
        path: &Path,
    ) -> Result<(PathBuf, Option<ProjectRecord>), ProjectError> {
        let root = self.ensure_managed_path(path)?;
        let existing = self.registered_path(&root)?;
        Ok((root, existing))
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn registered_path(
        &self,
        path: &Path,
    ) -> Result<Option<ProjectRecord>, ProjectError> {
        let root = self.ensure_managed_path(path)?;
        Ok(self
            .registry
            .projects
            .iter()
            .find(|r| Path::new(&r.project.root_path) == root)
            .cloned())
    }

    /// Reserve identity and uncertainty before any slow init/sync command starts.
    #[cfg(target_os = "linux")]
    pub(crate) fn prepare_project(
        &mut self,
        name: Option<&str>,
        path: Option<&Path>,
    ) -> Result<EnvironmentPlan<R>, ProjectError> {
        let (root, initialize) = if let Some(name) = name {
            validate_project_name(name)?;
            let base = self.ensure_managed_path(&self.projects_root)?;
            let root = base.join(name);
            if self
                .registry
                .projects
                .iter()
                .any(|r| Path::new(&r.project.root_path) == root)
            {
                return Err(ProjectError::AlreadyExists(root.display().to_string()));
            }
            fs::create_dir(&root).map_err(|source| {
                if source.kind() == std::io::ErrorKind::AlreadyExists {
                    ProjectError::AlreadyExists(root.display().to_string())
                } else {
                    ProjectError::Io {
                        path: root.display().to_string(),
                        source,
                    }
                }
            })?;
            (root, true)
        } else {
            let path =
                path.ok_or_else(|| ProjectError::InvalidName("missing Project path".into()))?;
            let root = self.ensure_managed_path(path)?;
            if let Some(existing) = self.registered_path(&root)? {
                return self.prepare_package(existing.project.project_id, "packages.sync", &[]);
            }
            if !root.join("pyproject.toml").is_file() {
                return Err(ProjectError::MissingPyproject(root.display().to_string()));
            }
            (root, false)
        };
        let name = root
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ProjectError::NonUtf8Path(root.display().to_string()))?;
        validate_project_name(name)?;
        let record = ProjectRecord {
            project: Project {
                project_id: ProjectId::new(),
                name: name.into(),
                root_path: path_to_string(&root)?,
                environment_generation: 0,
            },
            lock_fingerprint: None,
            environment_reconcile_required: true,
            initialization_required: initialize,
        };
        let project_id = record.project.project_id;
        self.registry.projects.push(record);
        // On save failure retain the directory and uncertain identity for repair;
        // no accepted operation or uv launch is claimed.
        self.save_registry()?;
        let mut commands = Vec::new();
        if initialize {
            commands.push(uv::init_project(&root, name));
        }
        commands.push(uv::sync_project(&root));
        Ok(EnvironmentPlan {
            project_id,
            root,
            runner: self.uv.clone(),
            commands,
        })
    }

    /// Reconcile even a failed uv attempt: it may have modified the environment.
    #[cfg(target_os = "linux")]
    pub(crate) fn finish_package(
        &mut self,
        project_id: ProjectId,
        root: &Path,
        uncertain: bool,
    ) -> Result<ProjectRecord, ProjectError> {
        if self.ensure_managed_path(&self.project_root(project_id)?)? != root {
            return Err(ProjectError::PathOutsideRoot(root.display().to_string()));
        }
        let result = self.refresh_generation_inner(project_id, root, uncertain);
        if result.is_err() {
            // A failed commit must not leave the live cache healthier than the
            // write-ahead marker. Best-effort restore also covers post-rename sync failure.
            if let Some(record) = self
                .registry
                .projects
                .iter_mut()
                .find(|r| r.project.project_id == project_id)
            {
                record.environment_reconcile_required = true;
            }
            let _ = self.save_registry();
        }
        result
    }

    pub fn list(&self) -> Vec<ProjectRecord> {
        let mut projects = self.registry.projects.clone();
        projects.sort_by(|a, b| a.project.name.cmp(&b.project.name));
        projects
    }

    pub fn get(&self, project_id: ProjectId) -> Result<ProjectRecord, ProjectError> {
        self.registry
            .projects
            .iter()
            .find(|record| record.project.project_id == project_id)
            .cloned()
            .ok_or_else(|| ProjectError::NotFound(project_id.to_string()))
    }

    pub fn create_project(&mut self, name: &str) -> Result<ProjectRecord, ProjectError> {
        validate_project_name(name)?;
        let project_root = self.projects_root.join(name);
        // The Project owner reserves the directory before any workload process
        // starts. This also supplies an existing cwd to the common launch policy.
        fs::create_dir(&project_root).map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                ProjectError::AlreadyExists(project_root.display().to_string())
            } else {
                ProjectError::Io {
                    path: project_root.display().to_string(),
                    source,
                }
            }
        })?;

        if let Err(error) = self.uv.run(&uv::init_project(&project_root, name)) {
            let _ = fs::remove_dir_all(&project_root);
            return Err(error.into());
        }
        if let Err(error) = self.uv.run(&uv::sync_project(&project_root)) {
            let _ = fs::remove_dir_all(&project_root);
            return Err(error.into());
        }

        let root_path = path_to_string(&project_root)?;
        let fingerprint = project_fingerprint(&project_root)?;
        let record = ProjectRecord {
            project: Project {
                project_id: ProjectId::new(),
                name: name.to_owned(),
                root_path,
                environment_generation: 1,
            },
            lock_fingerprint: Some(fingerprint),
            environment_reconcile_required: false,
            initialization_required: false,
        };
        self.registry.projects.push(record.clone());
        self.save_registry()?;
        Ok(record)
    }

    pub fn register_existing(
        &mut self,
        project_root: impl AsRef<Path>,
    ) -> Result<ProjectRecord, ProjectError> {
        let project_root = project_root.as_ref();
        let project_root = self.ensure_managed_path(project_root)?;
        let pyproject = project_root.join("pyproject.toml");
        if !pyproject.is_file() {
            return Err(ProjectError::MissingPyproject(
                project_root.display().to_string(),
            ));
        }

        let root_path = path_to_string(&project_root)?;
        if let Some(existing) = self
            .registry
            .projects
            .iter()
            .find(|record| record.project.root_path == root_path)
        {
            return Ok(existing.clone());
        }

        self.uv.run(&uv::sync_project(&project_root))?;
        let name = project_root
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| ProjectError::NonUtf8Path(project_root.display().to_string()))?;
        validate_project_name(name)?;

        let record = ProjectRecord {
            project: Project {
                project_id: ProjectId::new(),
                name: name.to_owned(),
                root_path,
                environment_generation: 1,
            },
            lock_fingerprint: Some(project_fingerprint(&project_root)?),
            environment_reconcile_required: false,
            initialization_required: false,
        };
        self.registry.projects.push(record.clone());
        self.save_registry()?;
        Ok(record)
    }

    pub fn sync_project(&mut self, project_id: ProjectId) -> Result<ProjectRecord, ProjectError> {
        let root = self.project_root(project_id)?;
        self.uv.run(&uv::sync_project(&root))?;
        self.refresh_generation(project_id, &root)
    }

    pub fn add_packages(
        &mut self,
        project_id: ProjectId,
        packages: &[String],
    ) -> Result<ProjectRecord, ProjectError> {
        validate_packages(packages)?;
        let root = self.project_root(project_id)?;
        self.uv.run(&uv::add_packages(&root, packages))?;
        self.refresh_generation(project_id, &root)
    }

    pub fn remove_packages(
        &mut self,
        project_id: ProjectId,
        packages: &[String],
    ) -> Result<ProjectRecord, ProjectError> {
        validate_packages(packages)?;
        let root = self.project_root(project_id)?;
        self.uv.run(&uv::remove_packages(&root, packages))?;
        self.refresh_generation(project_id, &root)
    }

    pub fn environment_status(
        &self,
        project_id: ProjectId,
    ) -> Result<ProjectEnvironmentStatus, ProjectError> {
        let record = self.get(project_id)?;
        let root = PathBuf::from(&record.project.root_path);
        let python = project_python_path(&root);
        let ready = !record.environment_reconcile_required
            && root.join("pyproject.toml").is_file()
            && python.is_file();
        Ok(ProjectEnvironmentStatus {
            project_id,
            environment_generation: record.project.environment_generation,
            lock_fingerprint: record.lock_fingerprint,
            python_executable: ready.then(|| python.display().to_string()),
            ready,
            reconcile_required: record.environment_reconcile_required,
        })
    }

    pub fn require_python(&self, project_id: ProjectId) -> Result<PathBuf, ProjectError> {
        let root = self.project_root(project_id)?;
        let python = project_python_path(&root);
        if python.is_file() {
            Ok(python)
        } else {
            Err(ProjectError::PythonMissing(python.display().to_string()))
        }
    }

    pub fn remove_from_registry(
        &mut self,
        project_id: ProjectId,
    ) -> Result<ProjectRecord, ProjectError> {
        let index = self
            .registry
            .projects
            .iter()
            .position(|record| record.project.project_id == project_id)
            .ok_or_else(|| ProjectError::NotFound(project_id.to_string()))?;
        let record = self.registry.projects.remove(index);
        if let Err(error) = self.save_registry() {
            self.registry.projects.insert(index, record);
            return Err(error);
        }
        Ok(record)
    }

    fn project_root(&self, project_id: ProjectId) -> Result<PathBuf, ProjectError> {
        Ok(PathBuf::from(self.get(project_id)?.project.root_path))
    }

    fn refresh_generation(
        &mut self,
        project_id: ProjectId,
        root: &Path,
    ) -> Result<ProjectRecord, ProjectError> {
        self.refresh_generation_inner(project_id, root, false)
    }

    fn refresh_generation_inner(
        &mut self,
        project_id: ProjectId,
        root: &Path,
        uncertain: bool,
    ) -> Result<ProjectRecord, ProjectError> {
        let fingerprint = project_fingerprint(root)?;
        let record = self
            .registry
            .projects
            .iter_mut()
            .find(|record| record.project.project_id == project_id)
            .ok_or_else(|| ProjectError::NotFound(project_id.to_string()))?;

        record.environment_reconcile_required = uncertain;
        if !uncertain {
            record.initialization_required = false;
        }

        if uncertain || record.lock_fingerprint.as_deref() != Some(fingerprint.as_str()) {
            record.project.environment_generation =
                record.project.environment_generation.saturating_add(1);
            record.lock_fingerprint = Some(fingerprint);
        }
        let updated = record.clone();
        self.save_registry()?;
        Ok(updated)
    }

    fn ensure_managed_path(&self, path: &Path) -> Result<PathBuf, ProjectError> {
        let base = self
            .projects_root
            .canonicalize()
            .map_err(|source| ProjectError::Io {
                path: self.projects_root.display().to_string(),
                source,
            })?;
        let candidate = path.canonicalize().map_err(|source| ProjectError::Io {
            path: path.display().to_string(),
            source,
        })?;
        if candidate.strip_prefix(&base).is_err() {
            return Err(ProjectError::PathOutsideRoot(
                candidate.display().to_string(),
            ));
        }
        Ok(candidate)
    }

    fn save_registry(&self) -> Result<(), ProjectError> {
        if let Some(parent) = self.registry_path.parent() {
            create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(&self.registry)?;
        let temp = self
            .registry_path
            .with_extension(format!("json.tmp-{}", ProjectId::new()));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, &self.registry_path)?;
            #[cfg(unix)]
            if let Some(parent) = self.registry_path.parent() {
                fs::File::open(parent)?.sync_all()?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|source| ProjectError::Io {
            path: self.registry_path.display().to_string(),
            source,
        })?;
        Ok(())
    }
}

pub fn validate_project_name(name: &str) -> Result<(), ProjectError> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(ProjectError::InvalidName(name.to_owned()));
    }
    let path = Path::new(name);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => {}
        _ => return Err(ProjectError::InvalidName(name.to_owned())),
    }
    if name.chars().any(char::is_control) {
        return Err(ProjectError::InvalidName(name.to_owned()));
    }
    Ok(())
}

fn validate_packages(packages: &[String]) -> Result<(), ProjectError> {
    if packages.is_empty() || packages.iter().any(|package| package.trim().is_empty()) {
        return Err(ProjectError::EmptyPackageList);
    }
    if packages.len() > 128
        || packages.iter().any(|p| {
            p.len() > 4096 || p.trim_start().starts_with('-') || p.chars().any(char::is_control)
        })
    {
        return Err(ProjectError::InvalidPackages);
    }
    Ok(())
}

fn create_dir_all(path: &Path) -> Result<(), ProjectError> {
    fs::create_dir_all(path).map_err(|source| ProjectError::Io {
        path: path.display().to_string(),
        source,
    })
}

fn path_to_string(path: &Path) -> Result<String, ProjectError> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| ProjectError::NonUtf8Path(path.display().to_string()))
}

fn project_python_path(project_root: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        project_root
            .join(".venv")
            .join("Scripts")
            .join("python.exe")
    }
    #[cfg(not(windows))]
    {
        project_root.join(".venv").join("bin").join("python")
    }
}

fn project_fingerprint(project_root: &Path) -> Result<String, ProjectError> {
    let mut digest = Sha256::new();
    for relative in ["pyproject.toml", "uv.lock", ".python-version"] {
        let path = project_root.join(relative);
        digest.update(relative.as_bytes());
        digest.update([0]);
        if path.is_file() {
            let bytes = fs::read(&path).map_err(|source| ProjectError::Io {
                path: path.display().to_string(),
                source,
            })?;
            digest.update((bytes.len() as u64).to_be_bytes());
            digest.update(bytes);
        } else {
            digest.update(0_u64.to_be_bytes());
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::uv::{UvCommandResult, UvInvocation};

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "cokernel-project-test-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Default)]
    struct FakeUv {
        calls: Mutex<Vec<String>>,
    }

    impl FakeUv {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl UvRunner for FakeUv {
        fn run(&self, invocation: &UvInvocation) -> Result<UvCommandResult, UvError> {
            let args = invocation
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            self.calls.lock().unwrap().push(args.join(" "));

            match args.first().map(String::as_str) {
                Some("init") => {
                    let root = PathBuf::from(args.last().unwrap());
                    assert!(
                        root.is_absolute() && root.is_dir(),
                        "Project must reserve the workload cwd before uv init"
                    );
                    assert_eq!(root, invocation.project_root);
                    fs::write(
                        root.join("pyproject.toml"),
                        "[project]\nname = \"demo\"\nversion = \"0.1.0\"\nrequires-python = \">=3.12\"\ndependencies = []\n",
                    )
                    .unwrap();
                }
                Some("sync") => {
                    let root = project_arg(&args);
                    fs::write(root.join("uv.lock"), "version = 1\n").unwrap();
                    let python = project_python_path(&root);
                    fs::create_dir_all(python.parent().unwrap()).unwrap();
                    fs::write(python, "").unwrap();
                }
                Some("add") | Some("remove") => {
                    let root = project_arg(&args);
                    fs::write(root.join("uv.lock"), args.join("\n")).unwrap();
                }
                other => panic!("unexpected fake uv invocation: {other:?}"),
            }

            Ok(UvCommandResult {
                status_code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    fn project_arg(args: &[String]) -> PathBuf {
        let index = args.iter().position(|value| value == "--project").unwrap();
        PathBuf::from(&args[index + 1])
    }

    #[test]
    fn rejects_project_path_components() {
        for name in ["", ".", "..", "../escape", "a/b"] {
            assert!(validate_project_name(name).is_err(), "{name}");
        }
        assert!(validate_project_name("demo-project_1").is_ok());
    }

    #[test]
    fn create_project_initializes_and_syncs_uv_environment() {
        let temp = TempRoot::new();
        let projects = temp.0.join("projects");
        let registry = temp.0.join("state/projects.json");
        let mut manager = ProjectManager::load(&projects, &registry, FakeUv::default()).unwrap();

        let created = manager.create_project("demo").unwrap();
        assert_eq!(created.project.environment_generation, 1);
        assert!(
            Path::new(&created.project.root_path)
                .join("pyproject.toml")
                .is_file()
        );
        assert!(
            manager
                .environment_status(created.project.project_id)
                .unwrap()
                .ready
        );

        let calls = manager.uv.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].starts_with("init --bare --no-workspace --name demo"));
        assert!(calls[1].starts_with("sync --project"));
    }

    #[test]
    fn package_change_increments_generation_but_noop_sync_does_not() {
        let temp = TempRoot::new();
        let projects = temp.0.join("projects");
        let registry = temp.0.join("state/projects.json");
        let mut manager = ProjectManager::load(&projects, &registry, FakeUv::default()).unwrap();
        let project = manager.create_project("demo").unwrap();

        let after_add = manager
            .add_packages(project.project.project_id, &["numpy".to_owned()])
            .unwrap();
        assert_eq!(after_add.project.environment_generation, 2);

        let after_sync = manager.sync_project(project.project.project_id).unwrap();
        assert_eq!(after_sync.project.environment_generation, 3);
        let second_sync = manager.sync_project(project.project.project_id).unwrap();
        assert_eq!(second_sync.project.environment_generation, 3);
    }

    #[test]
    fn remove_from_registry_does_not_delete_project_files() {
        let temp = TempRoot::new();
        let projects = temp.0.join("projects");
        let registry = temp.0.join("state/projects.json");
        let mut manager = ProjectManager::load(&projects, &registry, FakeUv::default()).unwrap();
        let project = manager.create_project("demo").unwrap();
        let root = PathBuf::from(&project.project.root_path);

        manager
            .remove_from_registry(project.project.project_id)
            .unwrap();
        assert!(root.exists());
        assert!(manager.list().is_empty());
    }
}
