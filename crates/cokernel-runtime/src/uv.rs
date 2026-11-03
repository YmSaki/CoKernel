use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UvInvocation {
    pub args: Vec<OsString>,
    pub project_root: PathBuf,
}

impl UvInvocation {
    pub fn display(&self) -> String {
        self.args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UvCommandResult {
    pub status_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Error)]
pub enum UvError {
    #[error("uv operation was cancelled; synchronize the environment before restarting a Session")]
    Cancelled,
    #[error("uv exceeded its execution time limit")]
    Timeout,
    #[error("uv exceeded its output limit")]
    OutputLimit,
    #[error("failed to launch uv: {source}")]
    Launch {
        #[source]
        source: std::io::Error,
    },
    #[error("uv command failed ({invocation}): {stderr}")]
    CommandFailed {
        invocation: String,
        status_code: Option<i32>,
        stderr: String,
    },
}

pub trait UvRunner: Send + Sync {
    fn run(&self, invocation: &UvInvocation) -> Result<UvCommandResult, UvError>;

    fn run_controlled(
        &self,
        invocation: &UvInvocation,
        control: &UvRunControl,
    ) -> Result<UvCommandResult, UvError> {
        if control.is_cancelled() {
            return Err(UvError::Cancelled);
        }
        self.run(invocation)
    }
}

#[derive(Clone, Default)]
pub struct UvRunControl(Arc<AtomicBool>);
impl UvRunControl {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone)]
pub struct CommandUvRunner {
    executable: PathBuf,
    timeout: Duration,
    identity: crate::workload_identity::WorkloadIdentity,
}

impl Default for CommandUvRunner {
    fn default() -> Self {
        Self::new("uv")
    }
}

impl CommandUvRunner {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            timeout: Duration::from_secs(30 * 60),
            identity: Default::default(),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_identity(mut self, identity: crate::workload_identity::WorkloadIdentity) -> Self {
        self.identity = identity;
        self
    }

    fn finish(
        &self,
        invocation: &UvInvocation,
        output: Output,
    ) -> Result<UvCommandResult, UvError> {
        let result = UvCommandResult {
            status_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };

        if output.status.success() {
            Ok(result)
        } else {
            Err(UvError::CommandFailed {
                invocation: invocation.display(),
                status_code: result.status_code,
                stderr: result.stderr,
            })
        }
    }
}

impl UvRunner for CommandUvRunner {
    fn run(&self, invocation: &UvInvocation) -> Result<UvCommandResult, UvError> {
        self.run_controlled(invocation, &UvRunControl::default())
    }

    fn run_controlled(
        &self,
        invocation: &UvInvocation,
        control: &UvRunControl,
    ) -> Result<UvCommandResult, UvError> {
        if control.is_cancelled() {
            return Err(UvError::Cancelled);
        }
        let mut command = Command::new(&self.executable);
        crate::workload::configure(&mut command, &invocation.project_root);
        self.identity
            .configure(&mut command, &invocation.project_root)
            .map_err(|source| UvError::Launch { source })?;
        command.args(&invocation.args);
        let output = process::run(command, self.timeout, control)?;
        self.finish(invocation, output)
    }
}

mod process;

pub fn init_project(project_root: &Path, name: &str) -> UvInvocation {
    UvInvocation {
        project_root: project_root.to_owned(),
        args: vec![
            "init".into(),
            "--bare".into(),
            "--no-workspace".into(),
            "--name".into(),
            name.into(),
            project_root.as_os_str().to_owned(),
        ],
    }
}

pub fn sync_project(project_root: &Path) -> UvInvocation {
    project_command(project_root, ["sync"])
}

pub fn add_packages<I, S>(project_root: &Path, packages: I) -> UvInvocation
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut args = vec![
        OsString::from("add"),
        OsString::from("--project"),
        project_root.as_os_str().to_owned(),
        OsString::from("--"),
    ];
    args.extend(
        packages
            .into_iter()
            .map(|package| package.as_ref().to_owned()),
    );
    UvInvocation {
        args,
        project_root: project_root.to_owned(),
    }
}

pub fn remove_packages<I, S>(project_root: &Path, packages: I) -> UvInvocation
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut args = vec![
        OsString::from("remove"),
        OsString::from("--project"),
        project_root.as_os_str().to_owned(),
        OsString::from("--"),
    ];
    args.extend(
        packages
            .into_iter()
            .map(|package| package.as_ref().to_owned()),
    );
    UvInvocation {
        args,
        project_root: project_root.to_owned(),
    }
}

fn project_command<const N: usize>(project_root: &Path, command: [&str; N]) -> UvInvocation {
    let mut args = command.into_iter().map(OsString::from).collect::<Vec<_>>();
    args.push("--project".into());
    args.push(project_root.as_os_str().to_owned());
    UvInvocation {
        args,
        project_root: project_root.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_bare_and_does_not_join_parent_workspace() {
        let invocation = init_project(Path::new("/projects/demo"), "demo");
        let rendered = invocation.display();
        assert!(rendered.contains("init --bare --no-workspace --name demo"));
        assert!(rendered.ends_with("/projects/demo"));
    }

    #[test]
    fn project_commands_use_explicit_project_root() {
        let sync = sync_project(Path::new("/projects/demo")).display();
        assert_eq!(sync, "sync --project /projects/demo");

        let add = add_packages(Path::new("/projects/demo"), ["numpy", "torch"]).display();
        assert_eq!(add, "add --project /projects/demo -- numpy torch");

        let remove = remove_packages(Path::new("/projects/demo"), ["numpy"]).display();
        assert_eq!(remove, "remove --project /projects/demo -- numpy");
    }
}
