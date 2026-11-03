//! Real Windows pipe -> Host -> WSL -> Project worker acceptance slice.
#[cfg(windows)]
mod proof {
    use anyhow::{Context, Result, ensure};
    use cokernel_host::{
        bridge::{BridgePolicy, WslConfig},
        ipc_pipe::{PipeClient, PipeListener},
        supervisor::{Host, RecoveryPolicy},
    };
    use serde_json::{Value, json};
    use std::{
        path::{Path, PathBuf},
        time::Duration,
    };

    async fn rpc(client: &mut PipeClient, method: &str, params: Value) -> Result<Value> {
        let response = client.call(method, params).await?;
        ensure!(response.ok, "{method}: {:?}", response.error);
        response.result.context("missing result")
    }
    async fn ready(client: &mut PipeClient) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(25), async {
            loop {
                let status = rpc(client, "runtime.get_status", json!({})).await?;
                if status["observed"] == "HEALTHY" {
                    return Ok(());
                }
                ensure!(status["observed"] != "ERROR", "{status}");
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await?
    }
    async fn exercise(name: &str, source: &Path) -> Result<Value> {
        let mut human = PipeClient::connect(name).await?;
        ensure!(human.next_event().await?.event == "host.snapshot");
        rpc(&mut human, "runtime.start", json!({})).await?;
        ready(&mut human).await?;
        let _ = rpc(&mut human, "resources.get_snapshot", json!({})).await?;
        let resources = tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                let resources = rpc(&mut human, "resources.get_snapshot", json!({})).await?;
                if resources["host"]["sequence"].as_u64().unwrap_or(0) >= 2
                    && resources["linux"]["sequence"].as_u64().unwrap_or(0) >= 2
                {
                    for side in ["host", "linux"] {
                        ensure!(
                            resources[side]["stale"] == false,
                            "stale {side}: {resources}"
                        );
                        ensure!(
                            resources[side]["resources"]["memory"]["value"]["total_bytes"]
                                .as_u64()
                                .unwrap_or(0)
                                > 0
                        );
                        ensure!(
                            resources[side]["resources"]["storage"]["value"]["total_bytes"]
                                .as_u64()
                                .unwrap_or(0)
                                > 0
                        );
                    }
                    ensure!(resources["host"]["resources"]["source"] == "WINDOWS_HOST");
                    ensure!(resources["linux"]["resources"]["source"] == "LINUX_RUNTIME");
                    return Ok::<_, anyhow::Error>(resources);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await??;
        println!("{}", json!({"resource_snapshot":resources}));
        let generation =
            rpc(&mut human, "runtime.get_status", json!({})).await?["generation"].clone();
        let project_name = format!("pipe-proof-{}", uuid::Uuid::new_v4().simple());
        let created = rpc(&mut human, "projects.create", json!({"name":project_name})).await?;
        let project = created["project_id"].clone();
        ensure!(created["kind"] == "PROJECT");
        tokio::time::timeout(Duration::from_secs(40), async {
            loop {
                let event = human.next_event().await?;
                let data = &event.payload["data"];
                if event.event == "environment.operation_progress"
                    && data["operation_id"] == created["operation_id"]
                    && data["result"]["state"] != "RUNNING"
                {
                    ensure!(
                        data["result"]["state"] == "SUCCEEDED",
                        "Project creation failed: {data}"
                    );
                    return Ok::<_, anyhow::Error>(());
                }
            }
        })
        .await??;
        let imported = rpc(
            &mut human,
            "notebooks.import_file",
            json!({"project_id":project,"target_path":"host.ipynb","windows_path":source}),
        )
        .await?;
        let notebook = imported["document"]["notebook_id"].clone();
        let collision = human
            .call(
                "notebooks.import_file",
                json!({"project_id":project,"target_path":"host.ipynb","windows_path":source}),
            )
            .await?;
        ensure!(
            !collision.ok,
            "import silently overwrote an existing target"
        );
        let loaded = rpc(
            &mut human,
            "notebooks.get",
            json!({"project_id":project,"notebook_id":notebook}),
        )
        .await?;
        ensure!(loaded["notebook"]["metadata"]["source_marker"] == "Windows 日本語");
        ensure!(
            loaded["notebook"]["cells"][1]["attachments"]["dot.png"]["image/png"] == "iVBORw0KGgo="
        );
        let invalid = source.with_file_name("invalid.ipynb");
        std::fs::write(&invalid, b"not a notebook")?;
        let rejected = human
            .call(
                "notebooks.import_file",
                json!({"project_id":project,"target_path":"invalid.ipynb","windows_path":invalid}),
            )
            .await?;
        ensure!(!rejected.ok, "invalid notebook accepted");
        let absent = human
            .call(
                "notebooks.open",
                json!({"project_id":project,"relative_path":"invalid.ipynb"}),
            )
            .await?;
        ensure!(!absent.ok, "failed import left a product document");
        let session = rpc(
            &mut human,
            "sessions.ensure_primary",
            json!({"project_id":project,"notebook_id":notebook}),
        )
        .await?["session_id"]
            .clone();
        let mut observer = PipeClient::connect(name).await?;
        let initial = observer.next_event().await?;
        ensure!(initial.event == "host.snapshot");
        ensure!(
            initial.payload["runtime"]["sessions"]
                .as_array()
                .context("Runtime reconnect snapshot")?
                .iter()
                .any(|item| item["session_id"] == session)
        );
        let shared = rpc(
            &mut observer,
            "sessions.ensure_primary",
            json!({"project_id":project,"notebook_id":notebook}),
        )
        .await?["session_id"]
            .clone();
        ensure!(
            session == shared,
            "second client created a different Session"
        );
        let mut inspector = PipeClient::connect(name).await?;
        ensure!(inspector.next_event().await?.event == "host.snapshot");
        let operation = rpc(&mut human, "sessions.execute_cell", json!({"project_id":project,"notebook_id":notebook,"session_id":session,"cell_id":"a","expected_revision":imported["document"]["revision"]})).await?["operation_id"].clone();
        let inspection_params =
            json!({"project_id":project,"session_id":session,"name":"secret_from_master"});
        let inspection = tokio::spawn(async move {
            rpc(&mut inspector, "sessions.get_variable", inspection_params).await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let snapshot = rpc(&mut observer, "runtime.get_snapshot", json!({})).await?;
                if snapshot["session_requests"]["pending_requests"]
                    .as_u64()
                    .unwrap_or(0)
                    > 0
                {
                    ensure!(
                        !inspection.is_finished(),
                        "inspection did not wait for execution"
                    );
                    ensure!(
                        rpc(&mut observer, "runtime.get_status", json!({})).await?["observed"]
                            == "HEALTHY"
                    );
                    return Ok::<_, anyhow::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await??;
        // Accepted worker execution must survive the submitting pipe closing.
        drop(human);
        let mut saw_output = false;
        let mut saw_started = false;
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let event = observer.next_event().await?;
                if event.event == "session.output" {
                    saw_output = true;
                }
                if event.event == "session.execution_started" {
                    saw_started = true;
                }
                if event.event == "notebook.execution_completed"
                    && event.payload["data"]["operation_id"] == operation
                {
                    ensure!(event.payload["data"]["result"]["state"] == "PERSISTED");
                    ensure!(event.payload["generation"] == generation);
                    return Ok::<_, anyhow::Error>(());
                }
            }
        })
        .await??;
        ensure!(inspection.await??["value"] == 123456789);
        ensure!(
            saw_started && saw_output,
            "Runtime execution events were not forwarded"
        );
        let value = rpc(
            &mut observer,
            "sessions.get_variable",
            json!({"project_id":project,"session_id":session,"name":"secret_from_master"}),
        )
        .await?;
        ensure!(value["value"] == 123456789, "shared namespace differs");
        let diagnostics = rpc(&mut observer, "runtime.get_diagnostics", json!({})).await?;
        ensure!(diagnostics["schema"] == 1);
        ensure!(
            diagnostics["sessions"]["items"]
                .as_array()
                .context("diagnostic sessions")?
                .iter()
                .any(|item| item["session_id"] == session)
        );
        let exported = serde_json::to_string(&diagnostics)?;
        ensure!(!exported.contains("123456789") && !exported.contains("secret_from_master"));
        ensure!(exported.len() <= 128 * 1024);
        println!(
            "{}",
            json!({"host_diagnostics":{"shared_session":true,"private_values_omitted":true,"schema":1}})
        );
        let pid = rpc(
            &mut observer,
            "sessions.get_variable",
            json!({"project_id":project,"session_id":session,"name":"worker_pid"}),
        )
        .await?["value"]
            .clone();
        let mcp = rpc(&mut observer, "runtime.get_mcp_status", json!({})).await?;
        let mcp_supervision = mcp["state"] != "DISABLED";
        if mcp_supervision {
            ensure!(mcp["state"] == "READY", "MCP did not become ready: {mcp}");
            let old_generation = mcp["generation"].as_u64().context("MCP generation")?;
            ensure!(rpc(&mut observer, "mcp.restart", json!({})).await?["accepted"] == true);
            let recovered = tokio::time::timeout(Duration::from_secs(15), async {
                loop {
                    let status = rpc(&mut observer, "runtime.get_mcp_status", json!({})).await?;
                    if status["state"] == "READY"
                        && status["generation"].as_u64().unwrap_or(0) > old_generation
                    {
                        return Ok::<_, anyhow::Error>(status);
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await??;
            ensure!(recovered["address"] == mcp["address"] && recovered["pid"] != mcp["pid"]);
            ensure!(
                rpc(
                    &mut observer,
                    "sessions.get_variable",
                    json!({"project_id":project,"session_id":session,"name":"worker_pid"})
                )
                .await?["value"]
                    == pid
            );
            ensure!(
                rpc(
                    &mut observer,
                    "sessions.get_variable",
                    json!({"project_id":project,"session_id":session,"name":"secret_from_master"})
                )
                .await?["value"]
                    == 123456789
            );
            println!(
                "{}",
                json!({"mcp_supervision":{"before":mcp,"after":recovered,"same_worker":true}})
            );
        }
        let versions = rpc(&mut observer, "runtime.get_versions", json!({})).await?;
        ensure!(
            versions["runtime_capabilities"]
                .as_array()
                .context("Runtime capabilities")?
                .contains(&json!("async-packages"))
        );
        if std::env::var_os("COKERNEL_HOST_TUNNEL_PROOF").is_some() {
            rpc(&mut observer,"remote_access.configure",json!({"tunnel_id":"tunnel_00000000000000000000000000000000","runtime_key":"synthetic-local-tunnel-proof-key"})).await?;
            rpc(&mut observer, "remote_access.start", json!({})).await?;
            let connected = tokio::time::timeout(Duration::from_secs(25), async {
                loop {
                    let status = rpc(&mut observer, "remote_access.get_status", json!({})).await?;
                    if status["state"] == "CONNECTED" {
                        return Ok::<_, anyhow::Error>(status);
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            })
            .await??;
            ensure!(
                rpc(
                    &mut observer,
                    "sessions.get_variable",
                    json!({"project_id":project,"session_id":session,"name":"worker_pid"})
                )
                .await?["value"]
                    == pid
            );
            ensure!(
                rpc(
                    &mut observer,
                    "sessions.get_variable",
                    json!({"project_id":project,"session_id":session,"name":"secret_from_master"})
                )
                .await?["value"]
                    == 123456789
            );
            // Give the independent Linux fixture time to inspect the live owned
            // worker/tunnel environments without sending key bytes to user code.
            tokio::time::sleep(Duration::from_secs(2)).await;
            rpc(
                &mut observer,
                "remote_access.clear_configuration",
                json!({}),
            )
            .await?;
            ensure!(
                rpc(&mut observer, "remote_access.get_configuration", json!({})).await?["configured"]
                    == false
            );
            ensure!(
                rpc(
                    &mut observer,
                    "sessions.get_variable",
                    json!({"project_id":project,"session_id":session,"name":"worker_pid"})
                )
                .await?["value"]
                    == pid
            );
            println!(
                "{}",
                json!({"host_tunnel_handoff":{"connected":connected,"same_worker":true,"same_value":123456789,"credential_cleared_after_stop":true}})
            );
        }
        let package = rpc(
            &mut observer,
            "packages.sync",
            json!({"project_id":project}),
        )
        .await?;
        ensure!(package["kind"] == "PACKAGE" && package["result"]["state"] == "RUNNING");
        tokio::time::timeout(Duration::from_secs(40), async {
            loop {
                let event = observer.next_event().await?;
                let data = &event.payload["data"];
                if event.event == "environment.operation_progress"
                    && data["operation_id"] == package["operation_id"]
                    && data["result"]["state"] != "RUNNING"
                {
                    ensure!(
                        data["result"]["state"] == "SUCCEEDED",
                        "package sync failed: {data}"
                    );
                    ensure!(event.payload["generation"] == generation);
                    return Ok::<_, anyhow::Error>(());
                }
            }
        })
        .await??;
        ensure!(
            rpc(
                &mut observer,
                "operations.get",
                json!({"project_id":project,"operation_id":package["operation_id"]})
            )
            .await?["result"]["state"]
                == "SUCCEEDED"
        );
        ensure!(
            rpc(
                &mut observer,
                "operations.cancel",
                json!({"project_id":project,"operation_id":package["operation_id"]})
            )
            .await?["accepted"]
                == false
        );
        drop(observer);
        let mut reopened = PipeClient::connect(name).await?;
        let reconnect = reopened.next_event().await?;
        ensure!(reconnect.event == "host.snapshot");
        ensure!(reconnect.payload["host"]["generation"] == generation);
        let snapshot = rpc(
            &mut reopened,
            "notebooks.get",
            json!({"project_id":project,"notebook_id":notebook}),
        )
        .await?;
        ensure!(snapshot["notebook"]["cells"][0]["outputs"][0]["data"]["text/plain"] == "124");
        rpc(&mut reopened, "notebooks.apply_edits", json!({"project_id":project,"notebook_id":notebook,"expected_revision":snapshot["revision"],"edits":[{"op":"set_notebook_metadata","metadata":{"shutdown_flush":true}}]})).await?;
        rpc(&mut reopened, "runtime.stop", json!({})).await?;
        ensure!(
            rpc(&mut reopened, "runtime.get_versions", json!({})).await?["runtime_capabilities"]
                == json!([])
        );
        ensure!(
            rpc(&mut reopened, "runtime.get_status", json!({})).await?["observed"] == "STOPPED"
        );
        rpc(&mut reopened, "runtime.start", json!({})).await?;
        ready(&mut reopened).await?;
        let document = rpc(
            &mut reopened,
            "notebooks.open",
            json!({"project_id":project,"relative_path":"host.ipynb"}),
        )
        .await?["document"]["notebook_id"]
            .clone();
        let saved = rpc(
            &mut reopened,
            "notebooks.get",
            json!({"project_id":project,"notebook_id":document}),
        )
        .await?;
        ensure!(saved["notebook"]["metadata"]["shutdown_flush"] == true);
        ensure!(
            rpc(
                &mut reopened,
                "sessions.list",
                json!({"project_id":project})
            )
            .await?
            .as_array()
            .context("session list")?
            .is_empty()
        );
        rpc(&mut reopened, "runtime.stop", json!({})).await?;
        Ok(
            json!({"proof":"real-windows-named-pipe-host-wsl-runtime","worker_pid":pid,"same_primary_session":true,"accepted_execution_survived_pipe_close":true,"all_clients_closed_without_runtime_restart":true,"saved_outputs":true,"shutdown_flush":true,"windows_chunked_import":true,"runtime_snapshot_and_live_events":true,"completion_observed_without_polling":true,"control_responsive_during_inspection":true,"async_project_creation_without_polling":true,"async_package_completion_without_polling":true,"import_collision_and_invalid_document_rejected":true,"desktop_or_mcp_proof":false}),
        )
    }
    pub async fn run() -> Result<()> {
        let mut args = std::env::args().skip(1);
        let config: WslConfig = serde_json::from_slice(&std::fs::read(
            args.next().context("WSL config path required")?,
        )?)?;
        let state = PathBuf::from(
            args.next()
                .context("isolated absolute Host state directory required")?,
        );
        std::fs::create_dir_all(&state)?;
        struct CredentialCleanup(cokernel_host::secrets::WindowsSecretStore);
        impl Drop for CredentialCleanup {
            fn drop(&mut self) {
                use cokernel_host::secrets::SecretStore;
                let _ = self.0.clear();
            }
        }
        let _credential_cleanup = if std::env::var_os("COKERNEL_HOST_TUNNEL_PROOF").is_some() {
            use cokernel_host::secrets::SecretStore;
            let mut store = cokernel_host::secrets::WindowsSecretStore::for_state(&state)?;
            ensure!(
                store.load()?.is_none(),
                "tunnel proof requires an empty isolated credential scope"
            );
            Some(CredentialCleanup(store))
        } else {
            None
        };
        let source = state.join("日本語 import.ipynb");
        let managed = std::env::var_os("COKERNEL_MANAGED_BOUNDARY_PROOF").is_some();
        let prefix = if managed {
            include_str!("../../../tests/fixtures/managed_boundary.py")
        } else {
            ""
        };
        let code = format!(
            "{prefix}\nimport os, time\nworker_pid = os.getpid()\ntime.sleep(3)\nx = 123\nsecret_from_master = 123456789\nx + 1"
        );
        let cell = json!({"id":"a","cell_type":"code","metadata":{},"source":code,"execution_count":null,"outputs":[]});
        let markdown = json!({"id":"md","cell_type":"markdown","metadata":{},"source":"![dot](attachment:dot.png)","attachments":{"dot.png":{"image/png":"iVBORw0KGgo="}}});
        // Greater than one transport chunk, with metadata and an attachment.
        std::fs::write(
            &source,
            serde_json::to_vec(
                &json!({"nbformat":4,"nbformat_minor":5,"metadata":{"source_marker":"Windows 日本語","padding":"p".repeat(280000)},"cells":[cell,markdown]}),
            )?,
        )?;
        let name = format!(r"\\.\pipe\cokernel-v1-proof-{}", uuid::Uuid::new_v4());
        let listener = PipeListener::bind_at(&name)?;
        let host = Host::start(
            &state,
            config.command()?,
            BridgePolicy::default(),
            RecoveryPolicy::default(),
        )?;
        let server = listener.serve(host.clone());
        let result = exercise(&name, &source).await;
        let server_stop = server.shutdown().await;
        let host_stop = host.shutdown().await;
        server_stop?;
        host_stop?;
        println!("{}", serde_json::to_string(&result?)?);
        if managed {
            println!(
                "{}",
                json!({"managed_worker_boundary":true,"windows_execution_denied":true,"control_file_socket_proc_denied":true})
            );
        }
        Ok(())
    }
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    return proof::run().await;
    #[cfg(not(windows))]
    anyhow::bail!("this acceptance proof requires Windows and WSL")
}
