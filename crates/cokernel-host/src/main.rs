#![cfg_attr(windows, windows_subsystem = "windows")]
use anyhow::{Context, Result, bail};
use cokernel_host::{
    bridge::{BridgePolicy, WslConfig},
    state::DesiredState,
    supervisor::{Host, RecoveryPolicy},
};

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(windows)]
    if !std::env::args().any(|arg| arg == "--background") {
        use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, System::Console::*};
        // Preserve redirected stdio; attach a parent console only for foreground CLI use.
        unsafe {
            let output = GetStdHandle(STD_OUTPUT_HANDLE);
            if output.is_null() || output == INVALID_HANDLE_VALUE {
                AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        #[cfg(windows)]
        Some("remote") => {
            let operation = args
                .next()
                .context("remote requires configure, status, start, stop, restart or clear")?;
            if args.next().is_some() {
                bail!(
                    "remote accepts no extra arguments; enter credentials only in the native dialog"
                );
            }
            if !matches!(
                operation.as_str(),
                "configure" | "status" | "start" | "stop" | "restart" | "clear"
            ) {
                bail!("expected remote configure, status, start, stop, restart or clear");
            }
            use cokernel_host::ipc_pipe::{PipeClient, default_name};
            let mut client = PipeClient::connect(&default_name()?).await.map_err(|_| {
                anyhow::anyhow!(
                    "CoKernel Host is unavailable; start Host before configuring Remote AI"
                )
            })?;
            if operation == "configure" {
                let credentials =
                    tokio::task::spawn_blocking(cokernel_host::remote_setup::prompt_credentials)
                        .await
                        .map_err(|_| anyhow::anyhow!("Credential dialog could not complete"))??;
                if let Some(credentials) = credentials {
                    let result = client.configure_remote(&credentials).await?;
                    drop(credentials);
                    println!("{}", serde_json::to_string(&result)?);
                } else {
                    println!("{{\"cancelled\":true}}");
                }
            } else {
                let methods: &[&str] = match operation.as_str() {
                    "status" => &[
                        "remote_access.get_configuration",
                        "remote_access.get_status",
                    ],
                    "start" => &["remote_access.start"],
                    "stop" => &["remote_access.stop"],
                    "restart" => &["remote_access.restart"],
                    "clear" => &["remote_access.clear_configuration"],
                    _ => bail!("unsupported remote operation"),
                };
                for method in methods {
                    let response = client.call(method, serde_json::json!({})).await.map_err(
                        |_| {
                            anyhow::anyhow!(
                                "Remote AI change was not confirmed; inspect status before retrying"
                            )
                        },
                    )?;
                    if !response.ok {
                        bail!("Remote AI operation was rejected; inspect status before retrying");
                    }
                    println!(
                        "{}",
                        serde_json::json!({"method":method,"result":response.result})
                    );
                }
            }
        }
        #[cfg(windows)]
        Some("autostart") => {
            let operation = args
                .next()
                .context("autostart requires status, enable or disable")?;
            let config = args
                .next()
                .context("autostart requires an absolute WSL config path")?;
            let state = args
                .next()
                .context("autostart requires an absolute state directory")?;
            if args.next().is_some() {
                bail!("unexpected autostart argument");
            }
            let target = cokernel_host::autostart::AutostartTarget::new(
                std::env::current_exe()?,
                config.into(),
                state.into(),
            )?;
            let status = match operation.as_str() {
                "status" => target.status()?,
                "enable" => target.set_enabled(true)?,
                "disable" => target.set_enabled(false)?,
                _ => bail!("expected autostart status, enable or disable"),
            };
            println!("{}", serde_json::to_string(&status)?);
        }
        #[cfg(windows)]
        Some(operation @ ("check" | "start")) => {
            if args.next().is_some() {
                bail!("unexpected Host control argument");
            }
            use cokernel_host::ipc_pipe::{PipeClient, default_name};
            let mut client = PipeClient::connect(&default_name()?).await?;
            let result = client
                .call(
                    if operation == "start" {
                        "runtime.start"
                    } else {
                        "runtime.get_status"
                    },
                    serde_json::json!({}),
                )
                .await?;
            if !result.ok
                || (operation == "check"
                    && result
                        .result
                        .as_ref()
                        .is_none_or(|v| v["observed"] != "HEALTHY"))
            {
                bail!("Host has not reported a healthy runtime");
            }
            println!(
                "{}",
                serde_json::json!({"accepted":true,"operation":operation})
            );
        }
        Some("--version") | Some("version") => {
            println!("cokernel-host {}", env!("CARGO_PKG_VERSION"))
        }
        Some("serve") => {
            let config = std::path::PathBuf::from(
                args.next()
                    .context("serve requires a local WSL config JSON path")?,
            );
            let state = std::path::PathBuf::from(
                args.next()
                    .context("serve requires an absolute Host state directory")?,
            );
            let (initial, background) = match args.next().as_deref() {
                None => (None, false),
                Some("--start") => (Some(DesiredState::Running), false),
                Some("--stop") => (Some(DesiredState::Stopped), false),
                Some("--background") => (None, true),
                Some(_) => bail!("expected --start, --stop or --background"),
            };
            if args.next().is_some() {
                bail!("unexpected Host argument");
            }
            #[cfg(windows)]
            let startup = cokernel_host::autostart::AutostartTarget::new(
                std::env::current_exe()?,
                config.canonicalize()?,
                state.clone(),
            )?;
            let file = std::fs::File::open(config)?;
            use std::io::Read;
            let mut bytes = Vec::new();
            file.take(16_385).read_to_end(&mut bytes)?;
            if bytes.len() > 16_384 {
                bail!("Host configuration exceeds 16 KiB");
            }
            let config: WslConfig = serde_json::from_slice(&bytes)?;
            let command = config.command()?;
            #[cfg(windows)]
            let listener = cokernel_host::ipc_pipe::PipeListener::bind()?;
            if let Some(desired) = initial {
                let (store, _) = cokernel_host::state::StateStore::open(&state)?;
                store.save(desired)?;
            }
            let host = Host::start(
                &state,
                command,
                BridgePolicy::default(),
                RecoveryPolicy::default(),
            )?;
            #[cfg(windows)]
            let host = host.with_autostart(startup);
            #[cfg(windows)]
            let pipe = listener.serve(host.clone());
            let mut status = host.subscribe();
            loop {
                if !background {
                    println!("{}", serde_json::to_string(&host.status())?);
                }
                tokio::select! {
                    signal=tokio::signal::ctrl_c(), if !background=> {
                      signal.context("Cannot receive console interrupt")?;
                      match host.shutdown().await {
                        Ok(())=>break,
                        Err(error)=>eprintln!("{}",serde_json::to_string(&error)?),
                      }
                    },
                    changed=status.changed()=>{if changed.is_err(){bail!("Host stopped after a lifecycle failure; inspect the last status");}}
                }
            }
            #[cfg(windows)]
            pipe.shutdown().await?;
        }
        _ => bail!(
            "usage: cokernel-host version | check | start | serve <wsl-config.json> <absolute-state-directory> [--start|--stop|--background] | autostart <status|enable|disable> <absolute-config> <absolute-state-directory> | remote <configure|status|start|stop|restart|clear> (Windows)"
        ),
    }
    Ok(())
}
