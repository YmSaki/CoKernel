use super::{Config, Credentials};
use crate::owned_child::OwnedChild;
use std::{
    fs,
    io::{self, Read},
    net::SocketAddr,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::PathBuf,
    process::Stdio,
};
use tokio::process::Command;
use zeroize::Zeroizing;

pub(super) struct Prepared {
    directory: PathBuf,
    pub socket: PathBuf,
    bearer: Zeroizing<String>,
}
impl Drop for Prepared {
    fn drop(&mut self) {
        // Only paths created for this launch. Never recursively delete product state.
        let _ = fs::remove_file(&self.socket);
        let _ = fs::remove_dir(&self.directory);
    }
}
impl Prepared {
    pub fn clear_bearer(&mut self) {
        use zeroize::Zeroize;
        self.bearer.zeroize();
    }
}
pub(super) fn prepare(config: &Config) -> io::Result<Prepared> {
    let invalid = || io::Error::other("unsafe tunnel configuration");
    if !config.executable.is_absolute()
        || config
            .executable
            .parent()
            .is_none_or(|p| p.canonicalize().ok().as_deref() != Some(p))
    {
        return Err(invalid());
    }
    let exe = fs::symlink_metadata(&config.executable)?;
    if !exe.is_file()
        || exe.mode() & 0o022 != 0
        || exe.mode() & 0o111 == 0
        || (exe.uid() != 0 && exe.uid() != unsafe { libc::geteuid() })
    {
        return Err(invalid());
    }
    crate::unix_api::check_directory(&config.state_directory.join("unused"))?;
    crate::unix_api::check_directory(&config.bearer_file)?;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&config.bearer_file)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
        || meta.len() != 64
    {
        return Err(invalid());
    }
    let mut bearer = Zeroizing::new(String::new());
    (&mut file).take(65).read_to_string(&mut bearer)?;
    if bearer.len() != 64 || !bearer.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let directory = config
        .state_directory
        .join(format!("t-{}", uuid::Uuid::new_v4().simple()));
    let socket = directory.join("h.sock");
    if socket.as_os_str().len() >= 108 {
        return Err(invalid());
    }
    fs::DirBuilder::new().mode(0o700).create(&directory)?;
    Ok(Prepared {
        directory,
        socket,
        bearer,
    })
}

pub(super) fn launch(
    config: &Config,
    prepared: &Prepared,
    credentials: &Credentials,
    address: SocketAddr,
) -> io::Result<OwnedChild> {
    if address.ip() != std::net::Ipv4Addr::LOCALHOST || address.port() == 0 {
        return Err(io::Error::other("invalid local MCP address"));
    }
    let authorization = Zeroizing::new(format!("Bearer {}", prepared.bearer.as_str()));
    let mut command = Command::new(&config.executable);
    command
        .args([
            "run",
            "--control-plane.api-key",
            "env:CONTROL_PLANE_API_KEY",
            "--control-plane.tunnel-id",
            &credentials.id,
            "--control-plane.base-url",
            "https://api.openai.com",
            "--control-plane.poll-timeout",
            "15s",
            "--mcp.extra-headers",
            "Authorization: env:COKERNEL_MCP_AUTHORIZATION",
            "--mcp.discovery-extra-headers",
            "Authorization: env:COKERNEL_MCP_AUTHORIZATION",
            "--log.level",
            "error",
            "--log.format",
            "json",
            "--admin-ui.log-buffer-events",
            "1",
        ])
        .arg("--mcp.server-url")
        .arg(format!("http://{address}/mcp"))
        .arg("--health.unix-socket")
        .arg(&prepared.socket)
        .arg("--profile-dir")
        .arg(&prepared.directory)
        .current_dir(&prepared.directory)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("HOME", &prepared.directory)
        .env("XDG_CONFIG_HOME", &prepared.directory)
        .env("CONTROL_PLANE_API_KEY", credentials.key.as_str())
        .env("COKERNEL_MCP_AUTHORIZATION", authorization.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    let parent = unsafe { libc::getpid() };
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(io::Error::other("Runtime exited before tunnel launch"));
            }
            Ok(())
        });
    }
    let child = command.spawn()?;
    let pid = child
        .id()
        .and_then(|p| i32::try_from(p).ok())
        .filter(|p| *p > 1)
        .ok_or_else(|| io::Error::other("tunnel child identity unavailable"))?;
    Ok(OwnedChild { child, pid })
}
