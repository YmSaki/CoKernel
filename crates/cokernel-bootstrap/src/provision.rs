//! Linux-side initialization is restricted to an image carrying our manifest.
use crate::image::{CONTROL, Manifest, POLICY, WORKLOAD, hash_file};
use anyhow::{Context, Result, ensure};
use std::{fs, io::Read, os::unix::fs::MetadataExt, path::Path};

fn regular(path: &Path, mode: u32, uid: u64, gid: u64) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file()
            && meta.nlink() == 1
            && meta.uid() as u64 == uid
            && meta.gid() as u64 == gid
            && meta.mode() & 0o7777 == mode,
        "CK-SETUP-PERMISSION: invalid protected file: {}",
        path.display()
    );
    Ok(())
}
pub fn boot_guard() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "CK-SETUP-IDENTITY: root boot guard required"
    );
    regular(Path::new("/etc/cokernel-validation.json"), 0o644, 0, 0)?;
    regular(Path::new("/etc/wsl.conf"), 0o644, 0, 0)?;
    ensure!(
        fs::read_to_string("/etc/wsl.conf")? == POLICY,
        "CK-SETUP-POLICY: WSL policy changed"
    );
    let path = Path::new("/run/WSL");
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_dir() && meta.uid() == 0 && meta.gid() == 0,
        "CK-SETUP-POLICY: invalid interop directory"
    );
    // Distro-local tmpfs directory, never the VM-wide binfmt registration.
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    check_boundary()
}

pub fn check_boundary() -> Result<()> {
    regular(Path::new("/etc/wsl.conf"), 0o644, 0, 0)?;
    ensure!(
        fs::read_to_string("/etc/wsl.conf")? == POLICY,
        "CK-SETUP-POLICY: WSL policy changed"
    );
    let mounts = fs::read_to_string("/proc/self/mountinfo")?;
    ensure!(
        !mounts
            .lines()
            .any(|l| l.contains(" - drvfs ") || l.contains("aname=drvfs")),
        "CK-SETUP-POLICY: Windows drive mount present"
    );
    let meta = fs::symlink_metadata("/run/WSL")?;
    ensure!(
        meta.is_dir() && meta.uid() == 0 && meta.gid() == 0 && meta.mode() & 0o7777 == 0o700,
        "CK-SETUP-POLICY: Windows interop sockets are accessible"
    );
    Ok(())
}

pub fn inspect() -> Result<Manifest> {
    regular(Path::new("/etc/cokernel-validation.json"), 0o644, 0, 0)?;
    let mut bytes = Vec::new();
    fs::File::open("/etc/cokernel-validation.json")?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 2 * 1024 * 1024,
        "CK-SETUP-LIMIT: manifest too large"
    );
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    ensure!(
        manifest.schema == 1 && manifest.base_sha256 == crate::image::BASE_SHA256,
        "CK-SETUP-MANIFEST: unsupported image"
    );
    regular(Path::new("/etc/wsl.conf"), 0o644, 0, 0)?;
    ensure!(
        fs::read_to_string("/etc/wsl.conf")? == POLICY,
        "CK-SETUP-POLICY: WSL policy changed"
    );
    let mounts = fs::read_to_string("/proc/self/mountinfo")?;
    ensure!(
        !mounts
            .lines()
            .any(|l| l.contains(" - drvfs ") || l.contains("aname=drvfs")),
        "CK-SETUP-POLICY: Windows drive mount present"
    );
    check_boundary()?;
    for path in ["/etc/ssl/certs/ca-certificates.crt", "/etc/ssl/cert.pem"] {
        let ca = Path::new(path);
        regular(ca, 0o644, 0, 0)?;
        ensure!(
            hash_file(&mut fs::File::open(ca)?)? == crate::image::CA_SHA256,
            "CK-SETUP-TRUST: missing or changed system CA roots"
        );
    }
    for (path, uid, gid, mode) in [
        ("/opt/cokernel", 0, 0, 0o755),
        ("/var/lib/cokernel", 0, 0, 0o755),
        ("/var/lib/cokernel/state", CONTROL, CONTROL, 0o700),
        ("/var/lib/cokernel/projects", CONTROL, WORKLOAD, 0o2770),
        ("/var/lib/cokernel/sessions", CONTROL, WORKLOAD, 0o2710),
        ("/var/cache/cokernel/uv", WORKLOAD, WORKLOAD, 0o770),
        ("/var/cache/cokernel/python", WORKLOAD, WORKLOAD, 0o770),
    ] {
        let meta = fs::symlink_metadata(path)?;
        ensure!(
            meta.is_dir()
                && meta.uid() as u64 == uid
                && meta.gid() as u64 == gid
                && meta.mode() & 0o7777 == mode,
            "CK-SETUP-PERMISSION: invalid product directory: {path}"
        );
    }
    for (name, expected) in &manifest.files {
        if let Some(name) = name.strip_prefix("link:") {
            ensure!(
                name.starts_with("opt/cokernel/") && !name.contains(".."),
                "CK-SETUP-MANIFEST: unsafe link path"
            );
            ensure!(
                fs::read_link(format!("/{name}"))?.to_str() == Some(expected),
                "CK-SETUP-HASH: payload link changed"
            );
            continue;
        }
        ensure!(
            name.starts_with("opt/cokernel/") && !name.contains(".."),
            "CK-SETUP-MANIFEST: unsafe payload path"
        );
        let path = Path::new("/").join(name);
        let meta = fs::symlink_metadata(&path)?;
        ensure!(
            meta.is_file() && meta.uid() == 0 && meta.nlink() == 1 && meta.mode() & 0o022 == 0,
            "CK-SETUP-PERMISSION: mutable payload"
        );
        ensure!(
            hash_file(&mut fs::File::open(path)?)? == *expected,
            "CK-SETUP-HASH: payload content changed"
        );
    }
    regular(
        Path::new("/opt/cokernel/bin/cokernel-runtime"),
        0o750,
        0,
        CONTROL,
    )?;
    regular(Path::new("/opt/cokernel/launch"), 0o750, 0, CONTROL)?;
    ensure!(
        fs::read("/opt/cokernel/launch")? == include_bytes!("launch.sh"),
        "CK-SETUP-HASH: launch configuration changed"
    );
    Ok(manifest)
}
pub fn finalize(expected: uuid::Uuid) -> Result<Manifest> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "CK-SETUP-IDENTITY: root provisioning required"
    );
    let manifest = inspect()?;
    ensure!(
        manifest.identity == expected,
        "CK-SETUP-IDENTITY: wrong validation image"
    );
    // VFS capability revision 2, effective bit, KILL/SETGID/SETUID permitted.
    let values = [0x02000001u32, 0xe0, 0, 0, 0];
    let bytes: Vec<u8> = values.into_iter().flat_map(u32::to_le_bytes).collect();
    let result = unsafe {
        libc::setxattr(
            c"/opt/cokernel/bin/cokernel-runtime".as_ptr(),
            c"security.capability".as_ptr(),
            bytes.as_ptr().cast(),
            bytes.len(),
            0,
        )
    };
    ensure!(
        result == 0,
        "CK-SETUP-CAPABILITY: {}",
        std::io::Error::last_os_error()
    );
    let mut observed = [0u8; 24];
    let count = unsafe {
        libc::getxattr(
            c"/opt/cokernel/bin/cokernel-runtime".as_ptr(),
            c"security.capability".as_ptr(),
            observed.as_mut_ptr().cast(),
            observed.len(),
        )
    };
    ensure!(
        count == bytes.len() as isize && observed[..bytes.len()] == bytes,
        "CK-SETUP-CAPABILITY: unexpected result"
    );
    let _ = Path::new("/dev/dxg")
        .try_exists()
        .context("CK-SETUP-GPU: device probe failed")?;
    Ok(manifest)
}
