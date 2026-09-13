use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use cokernel_domain::ProjectId;
use cokernel_protocol::{
    DEFAULT_MAX_FRAME_BYTES, RequestEnvelope, decode_json_payload, encode_json_frame,
};
use cokernel_runtime::project::ProjectManager;
use cokernel_runtime::service::RuntimeService;
use cokernel_runtime::uv::CommandUvRunner;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("version") | Some("--version") => {
            println!("cokernel-runtime {}", env!("CARGO_PKG_VERSION"));
        }
        Some("project") => run_project_command(args.collect())?,
        Some("bridge") => {
            match args.next().as_deref() {
                Some("--stdio") | None => run_bridge_stdio().await?,
                Some(other) => bail!("unsupported bridge transport: {other}"),
            }
        }
        Some(command) => {
            bail!("unknown or not-yet-implemented command: {command}");
        }
        None => {
            println!(
                "CoKernel Runtime v{} (v1 scaffold)",
                env!("CARGO_PKG_VERSION")
            );
        }
    }
    Ok(())
}

async fn run_bridge_stdio() -> Result<()> {
    let manager = project_manager()?;
    let mut service = RuntimeService::new(manager);
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    loop {
        let mut length = [0_u8; 4];
        match stdin.read_exact(&mut length).await {
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error).context("failed to read Runtime bridge frame length"),
        }

        let length = u32::from_be_bytes(length) as usize;
        if length > DEFAULT_MAX_FRAME_BYTES {
            bail!(
                "Runtime bridge frame length {length} exceeds maximum {DEFAULT_MAX_FRAME_BYTES}"
            );
        }

        let mut payload = vec![0_u8; length];
        stdin
            .read_exact(&mut payload)
            .await
            .context("failed to read Runtime bridge frame payload")?;
        let request: RequestEnvelope = decode_json_payload(&payload, DEFAULT_MAX_FRAME_BYTES)
            .context("failed to decode Runtime bridge request")?;
        let response = service.dispatch(request);
        let frame = encode_json_frame(&response, DEFAULT_MAX_FRAME_BYTES)
            .context("failed to encode Runtime bridge response")?;
        stdout
            .write_all(&frame)
            .await
            .context("failed to write Runtime bridge response")?;
        stdout
            .flush()
            .await
            .context("failed to flush Runtime bridge response")?;
    }

    Ok(())
}

fn run_project_command(args: Vec<String>) -> Result<()> {
    let mut args = args.into_iter();
    let command = args.next().unwrap_or_else(|| "list".to_owned());
    let mut manager = project_manager()?;

    match command.as_str() {
        "list" => print_json(&manager.list())?,
        "create" => {
            let name = required_arg(&mut args, "project name")?;
            print_json(&manager.create_project(&name)?)?;
        }
        "register" => {
            let path = required_arg(&mut args, "project path")?;
            print_json(&manager.register_existing(path)?)?;
        }
        "sync" => {
            let id = parse_project_id(required_arg(&mut args, "project id")?)?;
            print_json(&manager.sync_project(id)?)?;
        }
        "add" => {
            let id = parse_project_id(required_arg(&mut args, "project id")?)?;
            let packages = args.collect::<Vec<_>>();
            print_json(&manager.add_packages(id, &packages)?)?;
        }
        "remove" => {
            let id = parse_project_id(required_arg(&mut args, "project id")?)?;
            let packages = args.collect::<Vec<_>>();
            print_json(&manager.remove_packages(id, &packages)?)?;
        }
        "status" => {
            let id = parse_project_id(required_arg(&mut args, "project id")?)?;
            print_json(&manager.environment_status(id)?)?;
        }
        "forget" => {
            let id = parse_project_id(required_arg(&mut args, "project id")?)?;
            print_json(&manager.remove_from_registry(id)?)?;
        }
        other => bail!(
            "unknown project command: {other}. Expected list|create|register|sync|add|remove|status|forget"
        ),
    }

    Ok(())
}

fn project_manager() -> Result<ProjectManager<CommandUvRunner>> {
    let projects_root = std::env::var_os("COKERNEL_PROJECTS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/cokernel-user/projects"));
    let registry_path = std::env::var_os("COKERNEL_PROJECT_REGISTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/cokernel/state/projects.json"));

    ProjectManager::load(projects_root, registry_path, CommandUvRunner::default())
        .context("failed to initialize Project manager")
}

fn required_arg(args: &mut impl Iterator<Item = String>, label: &str) -> Result<String> {
    args.next().with_context(|| format!("missing {label}"))
}

fn parse_project_id(value: String) -> Result<ProjectId> {
    value
        .parse::<ProjectId>()
        .with_context(|| format!("invalid project id: {value}"))
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
