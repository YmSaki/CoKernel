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
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("version") | Some("--version") => {
            println!("cokernel-runtime {}", env!("CARGO_PKG_VERSION"));
        }
        Some("project") => run_project_command(args.collect())?,
        Some("bridge") => match args.next().as_deref() {
            Some("--stdio") | None => run_bridge_stdio().await?,
            Some(other) => bail!("unsupported bridge transport: {other}"),
        },
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
    let identity = cokernel_runtime::workload_identity::WorkloadIdentity::from_env()?;
    let manager = project_manager_with_identity(identity)?;
    #[cfg(target_os = "linux")]
    let service = {
        use cokernel_runtime::{
            notebook::{NotebookRuntime, NotebookService},
            session::{SessionSupervisor, SessionSupervisorConfig},
        };
        let worker = std::env::var_os("COKERNEL_WORKER_PACKAGE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/opt/cokernel/worker"));
        let sockets = std::env::var_os("COKERNEL_SESSION_SOCKET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/run/cokernel/sessions"));
        let mut config = SessionSupervisorConfig::development(worker, sockets);
        config.workload_identity = identity;
        config.failure_dir = std::env::var_os("COKERNEL_FAILURE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/var/lib/cokernel/state/failures"));
        RuntimeService::with_notebooks(
            manager,
            NotebookRuntime::new(NotebookService::default(), SessionSupervisor::new(config)),
        )
    };
    #[cfg(not(target_os = "linux"))]
    let service = RuntimeService::new(manager);
    #[cfg(target_os = "linux")]
    return run_endpoint_stdio(service).await;
    #[cfg(not(target_os = "linux"))]
    run_project_stdio(service).await
}

#[cfg(target_os = "linux")]
async fn run_endpoint_stdio(mut service: RuntimeService<CommandUvRunner>) -> Result<()> {
    use cokernel_runtime::endpoint::{RuntimeOwner, RuntimeReply};
    use cokernel_runtime::mcp_supervisor::{Config, Monitor, Supervisor};
    let mut stdin = cokernel_runtime::bridge_input::BridgeInput::stdin()
        .context("Linux bridge stdin must support nonblocking readiness")?;
    let events = service.events().unwrap_or_default();
    let monitor = Monitor::new(events.clone());
    service.set_mcp_monitor(monitor.clone());
    let tunnel_monitor = cokernel_runtime::tunnel::Monitor::new(events.clone());
    service.set_tunnel_monitor(tunnel_monitor.clone());
    let stdout = cokernel_runtime::bridge_output::BridgeOutput::start(tokio::io::stdout(), events);
    let (runtime, owner) = RuntimeOwner::start(service);
    let unix_api = match std::env::var_os("COKERNEL_RUNTIME_SOCKET") {
        Some(path) => {
            match cokernel_runtime::unix_api::UnixApi::bind(&PathBuf::from(path), runtime.clone())
                .await
            {
                Ok(api) => Some(api),
                Err(_) => {
                    monitor.failed("CK-MCP-API");
                    None
                }
            }
        }
        None => None,
    };
    let mcp = match Config::from_env() {
        Ok(Some(config)) if unix_api.is_some() => Some(Supervisor::start(config, monitor.clone())),
        Ok(Some(_)) => {
            monitor.failed("CK-MCP-API");
            None
        }
        Err(code) => {
            monitor.failed(code);
            None
        }
        Ok(None) => None,
    };
    let tunnel = match cokernel_runtime::tunnel::Config::from_env() {
        Ok(Some(config)) => Some(cokernel_runtime::tunnel::Supervisor::start(
            config,
            tunnel_monitor.clone(),
            monitor.clone(),
        )),
        Ok(None) => None,
        Err(code) => {
            tunnel_monitor.failed(code);
            None
        }
    };
    let mut replies = tokio::task::JoinSet::new();
    let result: Result<()> = async {
        loop {
            // Transport handles only complete frames and reply tickets. Domain
            // completions cannot consume/reset a partially read request frame.
            let read = read_request(&mut stdin);
            tokio::pin!(read);
            let request = loop {
                let reply: RuntimeReply = tokio::select! {
                    result = &mut read, if replies.len() < 32 => break result?,
                    reply = replies.join_next(), if !replies.is_empty() => {
                        reply.context("missing reply task")?.context("reply task failed")?
                    },
                };
                if reply.close_transport {
                    // Shutdown resolves accepted requests before its final ack.
                    while let Some(other) = replies.join_next().await {
                        send_response(&stdout, other.context("reply task failed")?.response)
                            .await?;
                    }
                    send_response(&stdout, reply.response).await?;
                    return Ok(());
                }
                send_response(&stdout, reply.response).await?;
            };
            let Some(request) = request else {
                break;
            };
            let id = request.request_id.clone();
            match runtime.submit(request, cokernel_domain::ExecutionOrigin::Human) {
                Ok(ticket) => {
                    replies.spawn(async move {
                        match ticket.wait().await {
                            Ok(reply) => reply,
                            Err(error) => RuntimeReply {
                                response: response_error(id, error),
                                close_transport: false,
                            },
                        }
                    });
                }
                Err(error) => send_response(&stdout, response_error(id, error)).await?,
            }
        }
        Ok(())
    }
    .await;
    let tunnel_cleanup = if let Some(tunnel) = tunnel {
        tunnel.shutdown().await.map_err(anyhow::Error::msg)
    } else {
        Ok(())
    };
    let mcp_cleanup = if let Some(mcp) = mcp {
        mcp.shutdown().await.map_err(anyhow::Error::msg)
    } else {
        Ok(())
    };
    let unix_cleanup = if let Some(api) = unix_api {
        api.shutdown().await.context("Runtime Unix API cleanup")
    } else {
        Ok(())
    };
    let runtime_cleanup = owner
        .finish()
        .await
        .map_err(|e| anyhow::anyhow!("{}: {}", e.code, e.summary))
        .context("Runtime endpoint cleanup");
    runtime_cleanup?;
    tunnel_cleanup?;
    mcp_cleanup?;
    unix_cleanup?;
    result
}

#[cfg(target_os = "linux")]
fn response_error(
    id: String,
    error: cokernel_domain::DomainError,
) -> cokernel_protocol::ResponseEnvelope {
    cokernel_protocol::ResponseEnvelope {
        protocol: 1,
        request_id: id,
        ok: false,
        result: None,
        error: Some(error),
    }
}

// Native developer Project adapter. Production notebook execution uses Linux.
#[cfg(not(target_os = "linux"))]
async fn run_project_stdio(mut service: RuntimeService<CommandUvRunner>) -> Result<()> {
    let mut stdin = tokio::io::stdin();
    let stdout = cokernel_runtime::bridge_output::BridgeOutput::start(
        tokio::io::stdout(),
        cokernel_runtime::events::EventHub::default(),
    );
    while let Some(request) = read_request(&mut stdin).await? {
        send_response(&stdout, service.dispatch(request)).await?;
    }
    Ok(())
}

async fn send_response(
    output: &cokernel_runtime::bridge_output::BridgeOutput,
    response: cokernel_protocol::ResponseEnvelope,
) -> Result<()> {
    let frame = match encode_json_frame(&response, DEFAULT_MAX_FRAME_BYTES) {
        Ok(frame) => frame,
        Err(_) => encode_json_frame(
            &cokernel_protocol::ResponseEnvelope {
                protocol: 1,
                request_id: response.request_id,
                ok: false,
                result: None,
                error: Some(cokernel_domain::DomainError {
                    code: "CK-RUN-RESPONSE-LIMIT".into(),
                    component: "runtime".into(),
                    summary: "Response exceeds transport limits".into(),
                    action: "Use a bounded or chunked domain read".into(),
                    detail: None,
                }),
            },
            DEFAULT_MAX_FRAME_BYTES,
        )
        .context("failed to encode bounded error response")?,
    };
    output
        .send(frame)
        .await
        .context("failed to write Runtime bridge response")
}

async fn read_request(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
) -> Result<Option<RequestEnvelope>> {
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length).await {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error).context("failed to read Runtime bridge frame length"),
    }
    let length = u32::from_be_bytes(length) as usize;
    if length > DEFAULT_MAX_FRAME_BYTES {
        bail!("Runtime bridge frame length {length} exceeds maximum {DEFAULT_MAX_FRAME_BYTES}");
    }
    let mut payload = vec![0; length];
    reader
        .read_exact(&mut payload)
        .await
        .context("failed to read Runtime bridge frame payload")?;
    Ok(Some(
        decode_json_payload(&payload, DEFAULT_MAX_FRAME_BYTES)
            .context("failed to decode Runtime bridge request")?,
    ))
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
    project_manager_with_identity(
        cokernel_runtime::workload_identity::WorkloadIdentity::from_env()?,
    )
}

fn project_manager_with_identity(
    identity: cokernel_runtime::workload_identity::WorkloadIdentity,
) -> Result<ProjectManager<CommandUvRunner>> {
    let projects_root = std::env::var_os("COKERNEL_PROJECTS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/cokernel-user/projects"));
    let registry_path = std::env::var_os("COKERNEL_PROJECT_REGISTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/cokernel/state/projects.json"));

    identity.initialize(&projects_root)?;
    ProjectManager::load(
        projects_root,
        registry_path,
        CommandUvRunner::default().with_identity(identity),
    )
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
