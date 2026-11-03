//! Explicit real-WSL integration proof. Requires an isolated Runtime test config.
use anyhow::{Context, Result, ensure};
use cokernel_host::{
    bridge::{BridgePolicy, WslConfig},
    state::DesiredState,
    supervisor::{Host, ObservedState, RecoveryPolicy},
};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

async fn rpc(host: &Host, method: &str, params: Value) -> Result<Value> {
    let response = host.call(method, params).await?;
    ensure!(response.ok, "{method}: {:?}", response.error);
    response.result.context("missing result")
}
async fn ready(host: &Host) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(25), async {
        let mut status = host.subscribe();
        loop {
            let state = host.status();
            if state.observed == ObservedState::Healthy {
                return Ok(());
            }
            ensure!(
                state.observed != ObservedState::Error,
                "Host failed: {:?}",
                state.last_error
            );
            status.changed().await?;
        }
    })
    .await?
}
async fn exercise(host: &Host) -> Result<Value> {
    host.set_desired(DesiredState::Running).await?;
    ready(host).await?;
    let created = rpc(host, "projects.create", json!({"name":"host-proof"})).await?;
    let project = created["project_id"].clone();
    tokio::time::timeout(Duration::from_secs(40), async {
        loop {
            let status = rpc(
                host,
                "operations.get",
                json!({"project_id":project,"operation_id":created["operation_id"]}),
            )
            .await?;
            if status["result"]["state"] != "RUNNING" {
                ensure!(
                    status["result"]["state"] == "SUCCEEDED",
                    "project creation failed: {status}"
                );
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    let created = rpc(
        host,
        "notebooks.create",
        json!({"project_id":project,"relative_path":"host.ipynb"}),
    )
    .await?;
    let notebook = created["document"]["notebook_id"].clone();
    let cell = json!({"id":"a","cell_type":"code","metadata":{},"source":"import os\nworker_pid = os.getpid()\nx = 123\nsecret_from_master = 123456789\nx + 1","execution_count":null,"outputs":[]});
    let edited=rpc(host,"notebooks.apply_edits",json!({"project_id":project,"notebook_id":notebook,"expected_revision":1,"edits":[{"op":"insert_cell","index":0,"cell":cell}]})).await?;
    let session = rpc(
        host,
        "sessions.ensure_primary",
        json!({"project_id":project,"notebook_id":notebook}),
    )
    .await?["session_id"]
        .clone();
    let client = host.clone();
    let operation=rpc(&client,"sessions.execute_cell",json!({"project_id":project,"notebook_id":notebook,"session_id":session,"cell_id":"a","expected_revision":edited["document"]["revision"]})).await?["operation_id"].clone();
    drop(client);
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let status = rpc(
                host,
                "operations.get",
                json!({"project_id":project,"operation_id":operation}),
            )
            .await?;
            if status["result"]["state"] != "PENDING" {
                ensure!(status["result"]["state"] == "PERSISTED", "{status}");
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let read = host.clone();
    let value = rpc(
        &read,
        "sessions.get_variable",
        json!({"project_id":project,"session_id":session,"name":"secret_from_master"}),
    )
    .await?;
    ensure!(value["value"] == 123456789, "shared Session value differs");
    let pid = rpc(
        &read,
        "sessions.get_variable",
        json!({"project_id":project,"session_id":session,"name":"worker_pid"}),
    )
    .await?["value"]
        .clone();
    drop(read);
    let snapshot = rpc(
        host,
        "notebooks.get",
        json!({"project_id":project,"notebook_id":notebook}),
    )
    .await?;
    ensure!(
        snapshot["notebook"]["cells"][0]["outputs"][0]["data"]["text/plain"] == "124",
        "persistent expression"
    );
    rpc(host,"notebooks.apply_edits",json!({"project_id":project,"notebook_id":notebook,"expected_revision":snapshot["revision"],"edits":[{"op":"set_notebook_metadata","metadata":{"shutdown_flush":true}}]})).await?;
    // Stop while a real worker still exists and there are dirty document edits.
    host.set_desired(DesiredState::Stopped).await?;
    ensure!(
        host.status().observed == ObservedState::Stopped,
        "unclean Host stop"
    );
    host.set_desired(DesiredState::Running).await?;
    ready(host).await?;
    let reopened = rpc(
        host,
        "notebooks.open",
        json!({"project_id":project,"relative_path":"host.ipynb"}),
    )
    .await?;
    let saved = rpc(
        host,
        "notebooks.get",
        json!({"project_id":project,"notebook_id":reopened["document"]["notebook_id"]}),
    )
    .await?;
    ensure!(
        saved["notebook"]["metadata"]["shutdown_flush"] == true,
        "shutdown lost dirty edit"
    );
    ensure!(
        rpc(host, "sessions.list", json!({"project_id":project}))
            .await?
            .as_array()
            .context("session list")?
            .is_empty(),
        "stopped Session was silently reconstructed"
    );
    host.set_desired(DesiredState::Stopped).await?;
    Ok(
        json!({"proof":"real-windows-host-wsl-runtime","worker_pid":pid,"generation":host.status().generation,"saved_outputs":true,"shutdown_flush":true,"same_session_after_caller_drop":true}),
    )
}
#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let config: WslConfig = serde_json::from_slice(&std::fs::read(
        args.next().context("WSL config path required")?,
    )?)?;
    let state = PathBuf::from(
        args.next()
            .context("isolated absolute Host state directory required")?,
    );
    let host = Host::start(
        &state,
        config.command()?,
        BridgePolicy::default(),
        RecoveryPolicy::default(),
    )?;
    let result = exercise(&host).await;
    host.shutdown().await?;
    println!("{}", serde_json::to_string(&result?)?);
    Ok(())
}
