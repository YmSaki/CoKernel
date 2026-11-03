//! Manual acceptance of the real Host binary, without changing Windows startup settings.
#[cfg(windows)]
mod proof {
    use anyhow::{Context, Result, ensure};
    use cokernel_host::{
        ipc_pipe::{PipeClient, PipeListener, default_name},
        state::{DesiredState, StateStore},
    };
    use serde_json::{Value, json};
    use std::{
        path::Path,
        process::{Child, Command, Stdio},
        time::Duration,
    };

    struct Process(Child);
    impl Drop for Process {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    async fn rpc(client: &mut PipeClient, method: &str) -> Result<Value> {
        let response = client.call(method, json!({})).await?;
        ensure!(response.ok, "{method}: {:?}", response.error);
        response.result.context("missing result")
    }
    async fn connect(process: &mut Process, state: &Path) -> Result<PipeClient> {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                ensure!(
                    process.0.try_wait()?.is_none(),
                    "owned Host exited before connection"
                );
                if let Ok(client) = PipeClient::connect(&default_name()?).await {
                    ensure!(
                        StateStore::open(state).is_err(),
                        "child does not own its state lock"
                    );
                    ensure!(process.0.try_wait()?.is_none(), "owned Host exited");
                    return Ok(client);
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await?
    }
    async fn exercise(
        executable: &Path,
        config: &Path,
        state: &Path,
        desired: DesiredState,
    ) -> Result<()> {
        // Refuse to touch a user's already-running Host. The binary owns the standard user pipe.
        drop(
            PipeListener::bind()
                .context("an existing Host owns the pipe; proof did not proceed")?,
        );
        use std::os::windows::process::CommandExt;
        let mut process = Process(
            Command::new(executable)
                .arg("serve")
                .arg(config)
                .arg(state)
                .arg("--background")
                .creation_flags(0x08000000) // CREATE_NO_WINDOW: this proof never opens a console window.
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );
        let result: Result<()> = async {
            let mut client = connect(&mut process, state).await?;
            let expected = if desired == DesiredState::Running {"HEALTHY"} else {"STOPPED"};
            let status = tokio::time::timeout(Duration::from_secs(35), async {
                loop {
                    let status = rpc(&mut client, "runtime.get_status").await?;
                    ensure!(status["observed"] != "ERROR", "{status}");
                    if status["observed"] == expected {return Ok::<_, anyhow::Error>(status);}
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }).await??;
            if desired == DesiredState::Stopped {ensure!(status["generation"] == 0);}
            // All callers disappear, then independently reconnect to the same background process.
            drop(client);
            let mut again = connect(&mut process, state).await?;
            let reconnected = rpc(&mut again, "runtime.get_status").await?;
            ensure!(reconnected["generation"] == status["generation"]);
            ensure!(reconnected["desired"] == status["desired"]);
            ensure!(reconnected["observed"] == expected);
            rpc(&mut again, "runtime.stop").await?;
            let stopped = rpc(&mut again, "runtime.get_status").await?;
            ensure!(stopped["desired"] == "STOPPED" && stopped["observed"] == "STOPPED");
            println!("{}", json!({"proof":"background-main", "initial_desired":desired, "startup":status, "reconnect":reconnected, "explicit_stop":stopped}));
            Ok(())
        }.await;
        // Stop a bridge before terminating only our test Host, including on a failed assertion.
        if process.0.try_wait()?.is_none()
            && StateStore::open(state).is_err()
            && let Ok(mut client) = PipeClient::connect(&default_name()?).await
        {
            rpc(&mut client, "runtime.stop")
                .await
                .context("proof cleanup could not stop Runtime")?;
        }
        drop(process);
        let (_, desired_after) = StateStore::open(state)?;
        ensure!(desired_after == DesiredState::Stopped);
        result
    }
    pub async fn run() -> Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        ensure!(
            args.len() == 3,
            "expected Host exe, config, fresh state directory"
        );
        let executable = Path::new(&args[0]);
        let config = Path::new(&args[1]);
        let state = Path::new(&args[2]);
        ensure!(executable.is_absolute() && config.is_absolute() && state.is_absolute());
        ensure!(!state.exists(), "use a fresh proof state directory");
        exercise(executable, config, state, DesiredState::Stopped).await?;
        let (store, _) = StateStore::open(state)?;
        store.save(DesiredState::Running)?;
        drop(store);
        exercise(executable, config, state, DesiredState::Running).await?;
        // A subsequent sign-in-style launch must honor the explicit stop just persisted.
        exercise(executable, config, state, DesiredState::Stopped).await?;
        println!(
            "PASS: actual Host --background honors persisted STOPPED/RUNNING and client-independent lifetime; Windows sign-in itself was not exercised"
        );
        Ok(())
    }
}
#[cfg(windows)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    proof::run().await
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows-only background Host proof");
}
