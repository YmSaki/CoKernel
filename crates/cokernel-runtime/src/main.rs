use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use cokernel_domain::ProjectId;
use cokernel_runtime::project::ProjectManager;
use cokernel_runtime::uv::CommandUvRunner;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("version") | Some("--version") => {
            println!("cokernel-runtime {}", env!("CARGO_PKG_VERSION"));
        }
        Some("project") => run_project_command(args.collect())?,
        Some("bridge") => {
            eprintln!("cokernel-runtime bridge skeleton: implementation tracked by #31");
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
