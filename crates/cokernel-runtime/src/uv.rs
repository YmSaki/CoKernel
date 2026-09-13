use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UvInvocation {
    pub args: Vec<OsString>,
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
}

#[derive(Debug, Clone)]
pub struct CommandUvRunner {
    executable: PathBuf,
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
        }
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
        let output = Command::new(&self.executable)
            .args(&invocation.args)
            .env("UV_NO_PROGRESS", "1")
            .output()
            .map_err(|source| UvError::Launch { source })?;
        self.finish(invocation, output)
    }
}

pub fn init_project(project_root: &Path, name: &str) -> UvInvocation {
    UvInvocation {
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
    ];
    args.extend(
        packages
            .into_iter()
            .map(|package| package.as_ref().to_owned()),
    );
    UvInvocation { args }
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
    ];
    args.extend(
        packages
            .into_iter()
            .map(|package| package.as_ref().to_owned()),
    );
    UvInvocation { args }
}

fn project_command<const N: usize>(project_root: &Path, command: [&str; N]) -> UvInvocation {
    let mut args = command.into_iter().map(OsString::from).collect::<Vec<_>>();
    args.push("--project".into());
    args.push(project_root.as_os_str().to_owned());
    UvInvocation { args }
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
        assert_eq!(add, "add --project /projects/demo numpy torch");

        let remove = remove_packages(Path::new("/projects/demo"), ["numpy"]).display();
        assert_eq!(remove, "remove --project /projects/demo numpy");
    }
}
