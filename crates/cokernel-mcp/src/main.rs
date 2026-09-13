use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("CoKernel MCP v{} (v1 scaffold)", env!("CARGO_PKG_VERSION"));
    Ok(())
}
