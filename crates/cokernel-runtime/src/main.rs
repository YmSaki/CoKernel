use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("version") | Some("--version") => {
            println!("cokernel-runtime {}", env!("CARGO_PKG_VERSION"));
        }
        Some("bridge") => {
            eprintln!("cokernel-runtime bridge skeleton: implementation tracked by #31");
        }
        Some(command) => {
            anyhow::bail!("unknown or not-yet-implemented command: {command}");
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
