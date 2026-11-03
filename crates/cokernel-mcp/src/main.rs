use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--version"] || args == ["version"] {
        println!("CoKernel MCP v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        use cokernel_mcp::{auth::Bearer, http_server::McpServer, runtime_client::RuntimeClient};
        if args.len() != 4 || args[0] != "serve" {
            anyhow::bail!(
                "usage: cokernel-mcp serve <absolute-runtime-socket> <absolute-private-bearer-file> <loopback-port>"
            );
        }
        let runtime = RuntimeClient::new(args[1].clone().into())?;
        let bearer = Bearer::load_or_create(std::path::Path::new(&args[2]))?;
        let port = args[3].parse::<u16>()?;
        // Readiness requires the existing Runtime; this service never creates another one.
        runtime.hello().await?;
        let mut server = McpServer::bind(([127, 0, 0, 1], port).into(), bearer, runtime).await?;
        println!(
            "{}",
            serde_json::json!({"event":"mcp.ready","address":server.address.to_string(),"path":"/mcp","authenticated":true})
        );
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result=tokio::signal::ctrl_c()=>result?,
            _=terminate.recv()=>{},
            result=server.wait()=>{result?;anyhow::bail!("CK-MCP-LISTENER: MCP listener stopped unexpectedly");}
        }
        server.shutdown().await?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    anyhow::bail!("CoKernel MCP runs inside the managed Linux environment");
}
