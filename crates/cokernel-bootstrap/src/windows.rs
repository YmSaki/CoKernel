//! Import only a newly built, uniquely named validation environment.
use crate::image::hash_file;
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::fs::OpenOptionsExt,
    path::Path,
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u8,
    identity: uuid::Uuid,
    image_sha256: String,
    base_sha256: String,
    payload_sha256: String,
}

fn decode(bytes: &[u8]) -> Result<String> {
    if bytes.iter().take(128).any(|b| *b == 0) {
        ensure!(bytes.len().is_multiple_of(2), "CK-SETUP-WSL: invalid UTF16");
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect();
        Ok(String::from_utf16(&units)?
            .trim_start_matches('\u{feff}')
            .into())
    } else {
        Ok(std::str::from_utf8(bytes)?.into())
    }
}
async fn wsl(args: Vec<OsString>) -> Result<String> {
    let system = std::env::var_os("SystemRoot").context("CK-SETUP-WSL: SystemRoot missing")?;
    let mut cmd = Command::new(Path::new(&system).join("System32/wsl.exe"));
    cmd.args(args)
        .env_clear()
        .env("SystemRoot", system)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .creation_flags(0x08000000);
    let mut child = cmd.spawn()?;
    let out = child
        .stdout
        .take()
        .context("CK-SETUP-WSL: missing stdout")?;
    let err = child
        .stderr
        .take()
        .context("CK-SETUP-WSL: missing stderr")?;
    let result = timeout(Duration::from_secs(180), async {
        let capture =
            async |stream: Box<dyn tokio::io::AsyncRead + Unpin + Send>| -> Result<Vec<u8>> {
                let mut bytes = Vec::new();
                stream.take(65_537).read_to_end(&mut bytes).await?;
                ensure!(bytes.len() <= 65_536, "CK-SETUP-WSL: output limit");
                Ok(bytes)
            };
        let (out, _err, status) =
            tokio::try_join!(capture(Box::new(out)), capture(Box::new(err)), async {
                Ok::<_, anyhow::Error>(child.wait().await?)
            })?;
        ensure!(
            status.success(),
            "CK-SETUP-WSL: command failed; validation directory retained"
        );
        decode(&out)
    })
    .await
    .unwrap_or_else(|_| {
        Err(anyhow::anyhow!(
            "CK-SETUP-WSL: observation timeout; inspect before retrying"
        ))
    });
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}
fn state(directory: &Path, identity: uuid::Uuid, phase: &str) -> Result<()> {
    let file = directory.join(format!("{phase}.json"));
    let mut file = OpenOptions::new().write(true).create_new(true).open(file)?;
    file.write_all(
        serde_json::to_string_pretty(&json!({"identity":identity,"phase":phase}))?.as_bytes(),
    )?;
    file.sync_all()?;
    Ok(())
}
pub async fn import(image: &Path, destination: &Path) -> Result<serde_json::Value> {
    import_named(image, destination, false).await
}

pub async fn import_product(image: &Path, destination: &Path) -> Result<serde_json::Value> {
    import_named(image, destination, true).await
}

async fn import_named(
    image: &Path,
    destination: &Path,
    product: bool,
) -> Result<serde_json::Value> {
    ensure!(
        image.is_absolute() && destination.is_absolute(),
        "CK-SETUP-PATH: absolute paths required"
    );
    let mut file = OpenOptions::new().read(true).share_mode(1).open(image)?;
    let mut bytes = Vec::new();
    File::open(format!("{}.json", image.display()))?
        .take(4097)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 4096, "CK-SETUP-LIMIT: receipt too large");
    let receipt: Receipt = serde_json::from_slice(&bytes)?;
    ensure!(
        receipt.schema == 1
            && receipt.base_sha256 == crate::image::BASE_SHA256
            && receipt.payload_sha256.len() == 64
            && receipt
                .payload_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            && receipt.image_sha256 == hash_file(&mut file)?,
        "CK-SETUP-HASH: image receipt mismatch"
    );
    let name = if product {
        crate::PRODUCT_DISTRO_NAME.to_owned()
    } else {
        format!("CoKernelV1-Validation-{}", receipt.identity.simple())
    };
    let listed = wsl(vec!["--list".into(), "--quiet".into()]).await?;
    ensure!(
        !listed
            .lines()
            .any(|line| line.trim().eq_ignore_ascii_case(&name)),
        "CK-SETUP-COLLISION: WSL name already registered"
    );
    fs::create_dir(destination).context("CK-SETUP-COLLISION: destination must be new")?;
    state(destination, receipt.identity, "prepared")?;
    let install = destination.join("distro");
    fs::create_dir(&install)?;
    let imported = wsl(vec![
        "--import".into(),
        name.clone().into(),
        install.into_os_string(),
        image.as_os_str().into(),
        "--version".into(),
        "2".into(),
    ])
    .await;
    if imported.is_err() {
        state(destination, receipt.identity, "import-uncertain")?;
        bail!(
            "CK-SETUP-IMPORT: inspect retained validation environment; no automatic retry or deletion"
        );
    }
    state(destination, receipt.identity, "imported")?;
    let proof = wsl(vec![
        "--distribution".into(),
        name.clone().into(),
        "--user".into(),
        "root".into(),
        "--cd".into(),
        "/".into(),
        "--exec".into(),
        "/opt/cokernel/bin/cokernel-bootstrap".into(),
        "finalize".into(),
        receipt.identity.to_string().into(),
    ])
    .await?;
    let proof: serde_json::Value = serde_json::from_str(proof.trim())
        .context("CK-SETUP-PROOF: invalid provisioning receipt")?;
    ensure!(
        proof["identity"] == receipt.identity.to_string() && proof["provisioned"] == true,
        "CK-SETUP-PROOF: wrong provisioning receipt"
    );
    state(destination, receipt.identity, "provisioned")?;
    let config = json!({"distribution":name,"linux_user":"cokernel-control","runtime_path":"/opt/cokernel/launch"});
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination.join("host-wsl.json"))?;
    output.write_all(serde_json::to_string_pretty(&config)?.as_bytes())?;
    output.sync_all()?;
    Ok(
        json!({"distribution":name,"image_identity":receipt.identity,"provisioning":proof,"host_config":destination.join("host-wsl.json"),"actual_client_acceptance":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("ck-bootstrap-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn wsl_decoding_accepts_utf8_and_utf16_but_rejects_corruption() {
        let expected = "CoKernelV1-Validation-テスト\r\n";
        assert_eq!(decode(expected.as_bytes()).unwrap(), expected);
        let bytes: Vec<_> = format!("\u{feff}{expected}")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(decode(&bytes).unwrap(), expected);
        assert!(decode(&[0, 1, 0]).is_err());
        assert!(decode(&[0, 0xd8]).is_err());
        assert!(decode(&[0xff]).is_err());
    }

    #[test]
    fn phase_evidence_never_overwrites_an_existing_receipt() {
        let fixture = Fixture::new();
        let identity = uuid::Uuid::new_v4();
        state(&fixture.0, identity, "prepared").unwrap();
        let path = fixture.0.join("prepared.json");
        let before = fs::read(&path).unwrap();
        assert!(state(&fixture.0, uuid::Uuid::new_v4(), "prepared").is_err());
        assert_eq!(fs::read(path).unwrap(), before);
    }

    #[tokio::test]
    async fn invalid_artifacts_fail_before_wsl_or_destination_creation() {
        let fixture = Fixture::new();
        let image = fixture.0.join("image.tar");
        let destination = fixture.0.join("new-install");
        let receipt_path = fixture.0.join("image.tar.json");
        fs::write(&image, b"not an image").unwrap();
        assert!(import(&image, &destination).await.is_err());
        let digest = hash_file(&mut File::open(&image).unwrap()).unwrap();
        let valid = json!({"schema":1,"identity":uuid::Uuid::new_v4(),
            "base_sha256":crate::image::BASE_SHA256,"image_sha256":digest,"payload_sha256":"a".repeat(64)});
        for (key, value) in [
            ("schema", json!(2)),
            ("identity", json!("invalid")),
            ("base_sha256", json!("0".repeat(64))),
            ("image_sha256", json!("0".repeat(64))),
            ("payload_sha256", json!("z".repeat(64))),
            ("extra", json!(true)),
        ] {
            let mut bad = valid.clone();
            bad[key] = value;
            fs::write(&receipt_path, serde_json::to_vec(&bad).unwrap()).unwrap();
            assert!(import(&image, &destination).await.is_err(), "{key}");
            assert!(!destination.exists());
        }
        fs::write(&receipt_path, vec![b' '; 4097]).unwrap();
        assert!(import(&image, &destination).await.is_err());
        assert!(!destination.exists());
        assert!(
            import(Path::new("relative.tar"), &destination)
                .await
                .is_err()
        );
        assert!(import(&image, Path::new("relative-install")).await.is_err());
    }
}
