//! Seed one disposable notebook through the existing Host, for actual AI clients.
//! Leaves the primary Session alive. This is preparation, not client acceptance.
#[cfg(windows)]
async fn prepare() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use cokernel_host::ipc_pipe::{PipeClient, default_name};
    use serde_json::{Value, json};
    use std::{
        io::{Read, Seek, Write},
        path::PathBuf,
        time::Duration,
    };

    async fn rpc(client: &mut PipeClient, method: &str, params: Value) -> anyhow::Result<Value> {
        let response = client.call(method, params).await?;
        ensure!(response.ok, "{method}: {:?}", response.error);
        response.result.context("missing domain result")
    }
    let mut args = std::env::args().skip(1);
    let receipt_path = PathBuf::from(args.next().context("new absolute receipt path required")?);
    let inspect = match args.next().as_deref() {
        None => false,
        Some("--inspect") => true,
        _ => anyhow::bail!("only --inspect is supported"),
    };
    ensure!(args.next().is_none() && receipt_path.is_absolute());
    let mut client = PipeClient::connect(&default_name()?).await?;
    let snapshot = client.next_event().await?;
    ensure!(snapshot.event == "host.snapshot");
    ensure!(rpc(&mut client, "runtime.get_status", json!({})).await?["observed"] == "HEALTHY");
    if inspect {
        let mut bytes = Vec::new();
        std::fs::File::open(&receipt_path)?
            .take(65537)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 65536);
        let receipt: Value = serde_json::from_slice(&bytes)?;
        ensure!(receipt["phase"] == "READY_FOR_ACTUAL_CLIENTS");
        let params = json!({"project_id":receipt["project_id"],"session_id":receipt["session_id"]});
        let session = rpc(&mut client, "sessions.get", params.clone()).await?;
        ensure!(session["worker_generation"] == receipt["session"]["worker_generation"]);
        for (name, expected) in [
            ("secret_from_master", &receipt["expected_value"]),
            ("worker_pid", &receipt["worker_pid"]),
        ] {
            let mut query = params.clone();
            query["name"] = json!(name);
            ensure!(
                rpc(&mut client, "sessions.get_variable", query).await?["value"] == *expected,
                "shared worker/value changed"
            );
        }
        let remote = rpc(&mut client, "remote_access.get_status", json!({})).await?;
        println!(
            "{}",
            json!({"same_primary_worker_and_value":true,"session":session,"remote":remote,"actual_client_acceptance":false})
        );
        return Ok(());
    }
    let name = format!("shared-ai-{}", uuid::Uuid::new_v4().simple());
    // Never silently create a replacement if an earlier preparation was interrupted.
    let mut receipt = std::fs::File::create_new(&receipt_path)?;
    receipt.write_all(
        serde_json::to_string(&json!({"phase":"PREPARING","project_name":name}))?.as_bytes(),
    )?;
    receipt.sync_all()?;
    let created = rpc(&mut client, "projects.create", json!({"name":name})).await?;
    let project = created["project_id"].clone();
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let event = client.next_event().await?;
            let data = &event.payload["data"];
            if event.event == "environment.operation_progress"
                && data["operation_id"] == created["operation_id"]
                && data["result"]["state"] != "RUNNING"
            {
                ensure!(
                    data["result"]["state"] == "SUCCEEDED",
                    "Project preparation failed"
                );
                return Ok::<_, anyhow::Error>(());
            }
        }
    })
    .await??;
    let source = receipt_path.with_extension("ipynb");
    let mut source_file = std::fs::File::create_new(&source)?;
    let notebook = json!({"nbformat":4,"nbformat_minor":5,"metadata":{"cokernel_proof":"actual-client-preparation"},"cells":[
        {"cell_type":"code","id":"human-seed","metadata":{},"execution_count":null,"outputs":[],"source":"import os\nworker_pid = os.getpid()\nsecret_from_master = 123456789\nsecret_from_master"},
        {"cell_type":"code","id":"ai-check","metadata":{},"execution_count":null,"outputs":[],"source":"secret_from_master + 1"}
    ]});
    source_file.write_all(serde_json::to_string_pretty(&notebook)?.as_bytes())?;
    source_file.sync_all()?;
    drop(source_file);
    let imported = rpc(
        &mut client,
        "notebooks.import_file",
        json!({"project_id":project,"target_path":"shared.ipynb","windows_path":source}),
    )
    .await?;
    let notebook = imported["document"]["notebook_id"].clone();
    let session = rpc(
        &mut client,
        "sessions.ensure_primary",
        json!({"project_id":project,"notebook_id":notebook}),
    )
    .await?["session_id"]
        .clone();
    let operation = rpc(&mut client, "sessions.execute_cell", json!({"project_id":project,"notebook_id":notebook,"session_id":session,"cell_id":"human-seed","expected_revision":imported["document"]["revision"]})).await?["operation_id"].clone();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let event = client.next_event().await?;
            if event.event == "notebook.execution_completed"
                && event.payload["data"]["operation_id"] == operation
            {
                ensure!(event.payload["data"]["result"]["state"] == "PERSISTED");
                return Ok::<_, anyhow::Error>(());
            }
        }
    })
    .await??;
    let value = rpc(
        &mut client,
        "sessions.get_variable",
        json!({"project_id":project,"session_id":session,"name":"secret_from_master"}),
    )
    .await?;
    ensure!(value["value"] == 123456789);
    let pid = rpc(
        &mut client,
        "sessions.get_variable",
        json!({"project_id":project,"session_id":session,"name":"worker_pid"}),
    )
    .await?["value"]
        .clone();
    let document = rpc(
        &mut client,
        "notebooks.get",
        json!({"project_id":project,"notebook_id":notebook}),
    )
    .await?;
    let observed = rpc(
        &mut client,
        "sessions.get",
        json!({"project_id":project,"session_id":session}),
    )
    .await?;
    let result = json!({"phase":"READY_FOR_ACTUAL_CLIENTS","project_name":name,"project_id":project,"notebook_id":notebook,"session_id":session,"session":observed,"worker_pid":pid,"revision":document["revision"],"variable":"secret_from_master","expected_value":123456789,"ai_cell_id":"ai-check","expected_ai_result":123456790,"actual_client_acceptance":false});
    receipt.rewind()?;
    receipt.set_len(0)?;
    receipt.write_all(serde_json::to_string_pretty(&result)?.as_bytes())?;
    receipt.sync_all()?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    return prepare().await;
    #[cfg(not(windows))]
    anyhow::bail!("requires the actual Windows Host")
}
