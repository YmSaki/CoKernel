use anyhow::{Context, Result, bail};
use cokernel_bootstrap::image;
use std::{fs::OpenOptions, io::Write, path::Path};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        #[cfg(target_os = "linux")]
        [command] if command == "boot-guard" => cokernel_bootstrap::provision::boot_guard()?,
        #[cfg(target_os = "linux")]
        [command] if command == "check-boundary" => {
            cokernel_bootstrap::provision::check_boundary()?
        }
        #[cfg(windows)]
        [command, image, destination] if command == "import-validation" => {
            println!(
                "{}",
                cokernel_bootstrap::windows::import(Path::new(image), Path::new(destination))
                    .await?
            );
        }
        [command, base, payload, output] if command == "image" => {
            let manifest = image::build(Path::new(base), Path::new(payload), Path::new(output))?;
            let digest = image::hash_file(&mut std::fs::File::open(output)?)?;
            let receipt = serde_json::json!({"schema":1,"identity":manifest.identity,"image_sha256":digest,"base_sha256":manifest.base_sha256,"payload_sha256":manifest.payload_sha256});
            let receipt_path = format!("{output}.json");
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&receipt_path)
                .context("CK-SETUP-RECEIPT: cannot publish image receipt")?;
            file.write_all(serde_json::to_string_pretty(&receipt)?.as_bytes())?;
            file.sync_all()?;
            println!(
                "{}",
                serde_json::json!({"receipt":receipt_path,"identity":manifest.identity,"image_sha256":digest})
            );
        }
        #[cfg(target_os = "linux")]
        [command, identity] if command == "finalize" => {
            let manifest = cokernel_bootstrap::provision::finalize(identity.parse()?)?;
            println!(
                "{}",
                serde_json::json!({"schema":1,"identity":manifest.identity,"provisioned":true,"workload_uid":image::WORKLOAD,"control_uid":image::CONTROL,"gpu_device_present":Path::new("/dev/dxg").exists()})
            );
        }
        #[cfg(target_os = "linux")]
        [command] if command == "inspect" => {
            let manifest = cokernel_bootstrap::provision::inspect()?;
            println!(
                "{}",
                serde_json::json!({"schema":1,"identity":manifest.identity,"image_files_valid":true,"drive_interop_policy_valid":true})
            );
        }
        _ => bail!(
            "usage: cokernel-bootstrap image <pinned-base.tar.gz> <payload.tar> <new-image.tar> | import-validation <image.tar> <new-directory> (Windows) | finalize <image-uuid> (Linux) | inspect | boot-guard | check-boundary (Linux)"
        ),
    }
    Ok(())
}
