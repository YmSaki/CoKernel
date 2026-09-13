use cokernel_domain::DomainError;
use cokernel_protocol::methods::{
    self, BridgeHelloParams, BridgeHelloResult, CreateProjectParams, PackageMutationParams,
    ProjectIdParams, RegisterProjectParams,
};
use cokernel_protocol::{PROTOCOL_V1, RequestEnvelope, ResponseEnvelope};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::project::{ProjectError, ProjectManager};
use crate::uv::UvRunner;

pub struct RuntimeService<R> {
    projects: ProjectManager<R>,
}

impl<R: UvRunner> RuntimeService<R> {
    pub fn new(projects: ProjectManager<R>) -> Self {
        Self { projects }
    }

    pub fn projects(&self) -> &ProjectManager<R> {
        &self.projects
    }

    pub fn projects_mut(&mut self) -> &mut ProjectManager<R> {
        &mut self.projects
    }

    pub fn dispatch(&mut self, request: RequestEnvelope) -> ResponseEnvelope {
        if request.protocol != PROTOCOL_V1 {
            return error_response(
                request.request_id,
                DomainError {
                    code: "CK-RUN-PROTOCOL".into(),
                    component: "runtime".into(),
                    summary: format!("Unsupported protocol version {}", request.protocol),
                    action: "Update CoKernel Host and Runtime to compatible versions".into(),
                    detail: Some(format!("runtime supports protocol {PROTOCOL_V1}")),
                },
            );
        }

        let request_id = request.request_id;
        let result = self.dispatch_method(&request.method, request.params);
        match result {
            Ok(value) => success_response(request_id, value),
            Err(error) => error_response(request_id, error),
        }
    }

    fn dispatch_method(&mut self, method: &str, params: Value) -> Result<Value, DomainError> {
        match method {
            methods::name::BRIDGE_HELLO => {
                let params: BridgeHelloParams = parse_params(params)?;
                if !params.supported_protocols.contains(&PROTOCOL_V1) {
                    return Err(DomainError {
                        code: "CK-RUN-PROTOCOL".into(),
                        component: "runtime".into(),
                        summary: "No compatible Host/Runtime protocol".into(),
                        action: "Update CoKernel Host and Runtime together".into(),
                        detail: Some(format!(
                            "host supports {:?}; runtime supports {PROTOCOL_V1}",
                            params.supported_protocols
                        )),
                    });
                }
                to_value(BridgeHelloResult {
                    selected_protocol: PROTOCOL_V1,
                    runtime_product_version: env!("CARGO_PKG_VERSION").to_owned(),
                    capabilities: vec![
                        "projects".into(),
                        "packages".into(),
                        "environment-status".into(),
                    ],
                })
            }
            methods::name::BRIDGE_PING => Ok(json!({ "pong": true })),
            methods::name::PROJECTS_LIST => to_value(self.projects.list()),
            methods::name::PROJECTS_CREATE => {
                let params: CreateProjectParams = parse_params(params)?;
                to_value(
                    self.projects
                        .create_project(&params.name)
                        .map_err(project_error)?,
                )
            }
            methods::name::PROJECTS_REGISTER => {
                let params: RegisterProjectParams = parse_params(params)?;
                to_value(
                    self.projects
                        .register_existing(&params.path)
                        .map_err(project_error)?,
                )
            }
            methods::name::PROJECTS_GET => {
                let params: ProjectIdParams = parse_params(params)?;
                to_value(self.projects.get(params.project_id).map_err(project_error)?)
            }
            methods::name::PROJECTS_REMOVE_FROM_REGISTRY => {
                let params: ProjectIdParams = parse_params(params)?;
                to_value(
                    self.projects
                        .remove_from_registry(params.project_id)
                        .map_err(project_error)?,
                )
            }
            methods::name::PACKAGES_SYNC => {
                let params: ProjectIdParams = parse_params(params)?;
                to_value(
                    self.projects
                        .sync_project(params.project_id)
                        .map_err(project_error)?,
                )
            }
            methods::name::PACKAGES_ADD => {
                let params: PackageMutationParams = parse_params(params)?;
                to_value(
                    self.projects
                        .add_packages(params.project_id, &params.packages)
                        .map_err(project_error)?,
                )
            }
            methods::name::PACKAGES_REMOVE => {
                let params: PackageMutationParams = parse_params(params)?;
                to_value(
                    self.projects
                        .remove_packages(params.project_id, &params.packages)
                        .map_err(project_error)?,
                )
            }
            methods::name::ENVIRONMENT_GET_STATUS => {
                let params: ProjectIdParams = parse_params(params)?;
                to_value(
                    self.projects
                        .environment_status(params.project_id)
                        .map_err(project_error)?,
                )
            }
            _ => Err(DomainError {
                code: "CK-RUN-404".into(),
                component: "runtime".into(),
                summary: format!("Unknown Runtime method: {method}"),
                action: "Update the caller or use a supported Runtime method".into(),
                detail: None,
            }),
        }
    }
}

fn parse_params<T: DeserializeOwned>(params: Value) -> Result<T, DomainError> {
    serde_json::from_value(params).map_err(|error| DomainError {
        code: "CK-RUN-BAD-PARAMS".into(),
        component: "runtime".into(),
        summary: "Runtime request parameters are invalid".into(),
        action: "Refresh the client state and retry with valid parameters".into(),
        detail: Some(error.to_string()),
    })
}

fn to_value<T: Serialize>(value: T) -> Result<Value, DomainError> {
    serde_json::to_value(value).map_err(|error| DomainError {
        code: "CK-RUN-SERIALIZE".into(),
        component: "runtime".into(),
        summary: "Runtime could not serialize its response".into(),
        action: "Run diagnostics and update CoKernel if the problem persists".into(),
        detail: Some(error.to_string()),
    })
}

fn project_error(error: ProjectError) -> DomainError {
    let (code, component, summary, action) = match &error {
        ProjectError::InvalidName(_) => (
            "CK-PROJ-INVALID-NAME",
            "project",
            "Project name is invalid",
            "Choose a single safe Project directory name",
        ),
        ProjectError::PathOutsideRoot(_) => (
            "CK-SEC-PROJECT-PATH",
            "project",
            "Project path is outside the managed Project root",
            "Choose or import a Project inside the CoKernel Project root",
        ),
        ProjectError::AlreadyExists(_) => (
            "CK-PROJ-EXISTS",
            "project",
            "Project already exists",
            "Choose another Project name or register the existing Project",
        ),
        ProjectError::NotFound(_) => (
            "CK-PROJ-NOT-FOUND",
            "project",
            "Project was not found",
            "Refresh the Project list or choose another Project",
        ),
        ProjectError::MissingPyproject(_) => (
            "CK-PROJ-NOT-UV",
            "project",
            "Project does not contain pyproject.toml",
            "Initialize the Project with uv or create a new CoKernel Project",
        ),
        ProjectError::EmptyPackageList => (
            "CK-UV-EMPTY-PACKAGES",
            "environment",
            "No packages were specified",
            "Specify at least one package",
        ),
        ProjectError::PythonMissing(_) => (
            "CK-UV-PYTHON-MISSING",
            "environment",
            "Project Python environment is not ready",
            "Synchronize the Project environment",
        ),
        ProjectError::Uv(_) => (
            "CK-UV-COMMAND",
            "environment",
            "uv operation failed",
            "Review the dependency error and retry the Project operation",
        ),
        ProjectError::UnsupportedRegistrySchema(_) => (
            "CK-PROJ-REGISTRY-VERSION",
            "project",
            "Project registry version is unsupported",
            "Update or repair the CoKernel Runtime",
        ),
        ProjectError::InvalidRegistry(_) => (
            "CK-PROJ-REGISTRY",
            "project",
            "Project registry is invalid",
            "Run CoKernel repair or restore the Project registry",
        ),
        ProjectError::NonUtf8Path(_) | ProjectError::Io { .. } => (
            "CK-PROJ-IO",
            "project",
            "Project filesystem operation failed",
            "Check Project storage permissions and run diagnostics",
        ),
    };

    DomainError {
        code: code.into(),
        component: component.into(),
        summary: summary.into(),
        action: action.into(),
        detail: Some(error.to_string()),
    }
}

fn success_response(request_id: String, result: Value) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol: PROTOCOL_V1,
        request_id,
        ok: true,
        result: Some(result),
        error: None,
    }
}

fn error_response(request_id: String, error: DomainError) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol: PROTOCOL_V1,
        request_id,
        ok: false,
        result: None,
        error: Some(error),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::uv::{UvCommandResult, UvError, UvInvocation};

    use super::*;

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "cokernel-service-test-{}-{unique}",
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
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl UvRunner for FakeUv {
        fn run(&self, invocation: &UvInvocation) -> Result<UvCommandResult, UvError> {
            let args = invocation
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            self.calls.lock().unwrap().push(args.clone());
            match args.first().map(String::as_str) {
                Some("init") => {
                    let root = PathBuf::from(args.last().unwrap());
                    fs::create_dir_all(&root).unwrap();
                    fs::write(
                        root.join("pyproject.toml"),
                        "[project]\nname='demo'\nversion='0.1.0'\nrequires-python='>=3.12'\ndependencies=[]\n",
                    )
                    .unwrap();
                }
                Some("sync") => {
                    let root = project_arg(&args);
                    if !root.join("uv.lock").is_file() {
                        fs::write(root.join("uv.lock"), "version = 1\n").unwrap();
                    }
                    let python = project_python_path(&root);
                    fs::create_dir_all(python.parent().unwrap()).unwrap();
                    fs::write(python, "").unwrap();
                }
                Some("add") | Some("remove") => {
                    let root = project_arg(&args);
                    fs::write(root.join("uv.lock"), args.join("\n")).unwrap();
                }
                other => panic!("unexpected fake uv command: {other:?}"),
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

    fn project_python_path(root: &Path) -> PathBuf {
        #[cfg(windows)]
        {
            root.join(".venv").join("Scripts").join("python.exe")
        }
        #[cfg(not(windows))]
        {
            root.join(".venv").join("bin").join("python")
        }
    }

    fn request(method: &str, params: Value) -> RequestEnvelope {
        RequestEnvelope {
            protocol: PROTOCOL_V1,
            request_id: "req-1".into(),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn create_list_and_environment_status_share_typed_domain_state() {
        let temp = TempRoot::new();
        let manager = ProjectManager::load(
            temp.0.join("projects"),
            temp.0.join("state/projects.json"),
            FakeUv::default(),
        )
        .unwrap();
        let mut service = RuntimeService::new(manager);

        let created = service.dispatch(request(
            methods::name::PROJECTS_CREATE,
            json!({ "name": "demo" }),
        ));
        assert!(created.ok);
        let project_id = created.result.as_ref().unwrap()["project"]["project_id"]
            .as_str()
            .unwrap()
            .to_owned();

        let listed = service.dispatch(request(methods::name::PROJECTS_LIST, json!({})));
        assert_eq!(listed.result.unwrap().as_array().unwrap().len(), 1);

        let status = service.dispatch(request(
            methods::name::ENVIRONMENT_GET_STATUS,
            json!({ "project_id": project_id }),
        ));
        assert!(status.ok);
        assert_eq!(status.result.unwrap()["ready"], true);
    }

    #[test]
    fn bad_params_and_unknown_methods_are_structured_errors() {
        let temp = TempRoot::new();
        let manager = ProjectManager::load(
            temp.0.join("projects"),
            temp.0.join("state/projects.json"),
            FakeUv::default(),
        )
        .unwrap();
        let mut service = RuntimeService::new(manager);

        let bad = service.dispatch(request(methods::name::PROJECTS_CREATE, json!({})));
        assert!(!bad.ok);
        assert_eq!(bad.error.unwrap().code, "CK-RUN-BAD-PARAMS");

        let unknown = service.dispatch(request("not.real", json!({})));
        assert!(!unknown.ok);
        assert_eq!(unknown.error.unwrap().code, "CK-RUN-404");
    }

    #[test]
    fn protocol_mismatch_fails_before_dispatch() {
        let temp = TempRoot::new();
        let manager = ProjectManager::load(
            temp.0.join("projects"),
            temp.0.join("state/projects.json"),
            FakeUv::default(),
        )
        .unwrap();
        let mut service = RuntimeService::new(manager);
        let response = service.dispatch(RequestEnvelope {
            protocol: 99,
            request_id: "req-99".into(),
            method: methods::name::PROJECTS_LIST.into(),
            params: json!({}),
        });
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "CK-RUN-PROTOCOL");
    }
}
