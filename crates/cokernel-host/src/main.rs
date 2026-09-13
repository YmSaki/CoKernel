use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("CoKernel Host v{} (v1 scaffold)", env!("CARGO_PKG_VERSION"));
    Ok(())
}
