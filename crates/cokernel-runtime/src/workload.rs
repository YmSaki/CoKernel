//! Environment boundary shared by uv package builds and persistent workers.
//! This is not an operating-system identity or filesystem isolation boundary.
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

pub(crate) fn configure(command: &mut Command, project: &Path) {
    configure_with(command, project, |name| std::env::var_os(name));
}

fn configure_with(command: &mut Command, project: &Path, read: impl Fn(&str) -> Option<OsString>) {
    command
        .env_clear()
        .current_dir(project)
        .env(
            "PATH",
            read("PATH").unwrap_or_else(|| "/usr/local/bin:/usr/bin:/bin".into()),
        )
        .env("HOME", project)
        .env("XDG_CONFIG_HOME", project.join(".config"))
        .env("XDG_CACHE_HOME", project.join(".cache"))
        .env("XDG_DATA_HOME", project.join(".local/share"))
        .env("IPYTHONDIR", project.join(".cokernel/ipython"))
        .env("LC_ALL", "C.UTF-8")
        .env("UV_NO_PROGRESS", "1");
    // Installation/development tool locations are trusted inputs. Do not forward
    // arbitrary UV_*, Python/loader settings, proxies, credentials or control HOME.
    for name in ["UV_CACHE_DIR", "UV_PYTHON_INSTALL_DIR"] {
        if let Some(value) = read(name) {
            command.env(name, value);
        }
    }
    #[cfg(windows)]
    {
        // Native development checks still need Windows system services, but
        // package code must not inherit the control user's profile directories.
        if let Some(value) = read("SystemRoot") {
            command.env("SystemRoot", value);
        }
        command
            .env("USERPROFILE", project)
            .env("APPDATA", project.join(".config"))
            .env("LOCALAPPDATA", project.join(".cache"))
            .env("TEMP", project)
            .env("TMP", project);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workload_environment_removes_injection_and_uses_project_locations() {
        let project = std::env::temp_dir().join("ck-environment-test");
        let mut command = Command::new("unused");
        command.env("COKERNEL_SECRET", "must-disappear");
        command.env("PYTHONPATH", "must-disappear");
        configure_with(&mut command, &project, |name| match name {
            "PATH" => Some("trusted-tools".into()),
            "UV_CACHE_DIR" => Some("trusted-cache".into()),
            "UV_PYTHON_INSTALL_DIR" => Some("trusted-python".into()),
            "SystemRoot" => Some("trusted-system".into()),
            _ => panic!("unexpected inherited variable: {name}"),
        });
        let env: std::collections::HashMap<_, _> = command.get_envs().collect();
        for name in [
            "COKERNEL_SECRET",
            "PYTHONPATH",
            "UV_INDEX_PASSWORD",
            "LD_PRELOAD",
        ] {
            assert!(!env.contains_key(std::ffi::OsStr::new(name)));
        }
        assert_eq!(command.get_current_dir(), Some(project.as_path()));
        assert_eq!(env[std::ffi::OsStr::new("HOME")], Some(project.as_os_str()));
        assert_eq!(
            env[std::ffi::OsStr::new("UV_CACHE_DIR")],
            Some(std::ffi::OsStr::new("trusted-cache"))
        );
    }
}
