//! Windows selection is read here; only bounded bytes and Project IDs cross WSL.
use crate::{ipc, supervisor::Host};
use base64::{Engine, engine::general_purpose::STANDARD};
use cokernel_domain::{DomainError, ImportId, ProjectId};
use cokernel_protocol::ResponseEnvelope;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Component, Path, PathBuf, Prefix},
};
use tokio::sync::Semaphore;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_SEQUENTIAL_SCAN,
    FILE_SHARE_READ, FILE_TYPE_DISK, GetDriveTypeW, GetFileType, GetFinalPathNameByHandleW,
};
use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const CHUNK: usize = 262_144;
static IMPORTS: Semaphore = Semaphore::const_new(4);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    project_id: ProjectId,
    target_path: String,
    windows_path: PathBuf,
}
#[derive(Deserialize)]
struct Ticket {
    import_id: ImportId,
    accepted_chunk_size: usize,
}
struct Source {
    file: File,
    size: u64,
    hash: String,
}
fn invalid() -> io::Error {
    io::Error::other("select a nonempty local regular .ipynb file up to 64 MiB")
}
fn disk_path(path: &Path) -> io::Result<u8> {
    if !path.is_absolute() || path.as_os_str().len() > 32700 {
        return Err(invalid());
    }
    let mut components = path.components();
    let drive = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
            _ => return Err(invalid()),
        },
        _ => return Err(invalid()),
    };
    if components.any(|component| match component {
        Component::Normal(name) => name
            .to_string_lossy()
            .chars()
            .any(|c| c == ':' || c.is_control()),
        Component::RootDir => false,
        _ => true,
    }) {
        return Err(invalid());
    }
    Ok(drive)
}
fn open_source(path: &Path) -> io::Result<Source> {
    let drive = disk_path(path)?;
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ipynb"))
    {
        return Err(invalid());
    }
    let root = [drive as u16, b':' as u16, b'\\' as u16, 0];
    // SAFETY: root is a terminated drive-root UTF-16 string.
    if !matches!(
        unsafe { GetDriveTypeW(root.as_ptr()) },
        DRIVE_FIXED | DRIVE_REMOVABLE
    ) {
        return Err(invalid());
    }
    // Deny concurrent writers/rename while hashing and streaming the same handle.
    // OPEN_REPARSE_POINT prevents following a final symlink before validation.
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() == 0
        || metadata.len() > MAX_BYTES
        || unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK
    {
        return Err(invalid());
    }
    // A local-looking ancestor junction may resolve to a network share. Verify
    // the opened object as well; the lexical path alone is not authoritative.
    let mut resolved = vec![0u16; 32768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            resolved.as_mut_ptr(),
            resolved.len() as u32,
            0,
        )
    } as usize;
    if length == 0 || length >= resolved.len() {
        return Err(invalid());
    }
    disk_path(Path::new(
        &String::from_utf16(&resolved[..length]).map_err(|_| invalid())?,
    ))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; CHUNK];
    let mut left = metadata.len();
    while left > 0 {
        let count = left.min(CHUNK as u64) as usize;
        file.read_exact(&mut buffer[..count])?;
        digest.update(&buffer[..count]);
        left -= count as u64;
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(Source {
        file,
        size: metadata.len(),
        hash: format!("{:x}", digest.finalize()),
    })
}
fn failure(summary: &str) -> DomainError {
    DomainError {code:"CK-HOST-IMPORT".into(), component:"host".into(), summary:summary.into(), action:"Select a readable local .ipynb file and an unused Project-relative target; inspect the target before retrying after a connection failure".into(), detail:None}
}
async fn call(host: &Host, method: &str, params: Value) -> Result<Value, DomainError> {
    let response = host
        .call(method, params)
        .await
        .map_err(|error| failure(&format!("Import bridge unavailable ({})", error.code)))?;
    if response.ok {
        response
            .result
            .ok_or_else(|| failure("Import response has no result"))
    } else {
        Err(response
            .error
            .unwrap_or_else(|| failure("Import failed without an error")))
    }
}
async fn transfer(
    host: &Host,
    params: &Params,
    mut source: Source,
    ticket: &Ticket,
) -> Result<Value, DomainError> {
    let chunk_size = ticket.accepted_chunk_size;
    if chunk_size == 0 || chunk_size > CHUNK {
        return Err(failure("Runtime returned an invalid import chunk bound"));
    }
    let mut offset = 0;
    while offset < source.size {
        let count = (source.size - offset).min(chunk_size as u64) as usize;
        // Blocking disk reads never occupy the async Host control threads.
        let (returned, bytes) = tokio::task::spawn_blocking(move || {
            let mut bytes = vec![0; count];
            source.file.read_exact(&mut bytes)?;
            Ok::<_, io::Error>((source, bytes))
        })
        .await
        .map_err(|_| failure("Windows file reader stopped"))?
        .map_err(|_| failure("Selected Windows file could not be read"))?;
        source = returned;
        let result = call(host, "notebooks.import_chunk", json!({"project_id":params.project_id,"import_id":ticket.import_id,"offset":offset,"data_base64":STANDARD.encode(bytes)})).await?;
        offset += count as u64;
        if result["next_offset"].as_u64() != Some(offset) {
            return Err(failure(
                "Runtime import acknowledgement has an unexpected offset",
            ));
        }
    }
    call(
        host,
        "notebooks.finish_import",
        json!({"project_id":params.project_id,"import_id":ticket.import_id}),
    )
    .await
}
async fn run(host: &Host, params: Params) -> Result<Value, DomainError> {
    let _permit = IMPORTS
        .try_acquire()
        .map_err(|_| failure("Four Windows imports are already active"))?;
    let path = params.windows_path.clone();
    let source = tokio::task::spawn_blocking(move || open_source(&path)).await
        .map_err(|_| failure("Windows file reader stopped"))?
        .map_err(|_| failure("Select a readable nonempty local regular .ipynb file up to 64 MiB; close other writers"))?;
    let ticket: Ticket = serde_json::from_value(call(host,"notebooks.begin_import", json!({"project_id":params.project_id,"target_path":params.target_path,"size_bytes":source.size,"sha256":source.hash})).await?)
        .map_err(|_| failure("Runtime returned an invalid import ticket"))?;
    let result = transfer(host, &params, source, &ticket).await;
    if result.is_err() {
        // Cancel only our own transaction. After uncertain transport failure,
        // Runtime TTL remains the fallback; never replay an import/overwrite.
        let _ = call(
            host,
            "notebooks.cancel_import",
            json!({"project_id":params.project_id,"import_id":ticket.import_id}),
        )
        .await;
    }
    result
}
pub async fn dispatch(host: &Host, id: String, params: Value) -> ResponseEnvelope {
    let result = match serde_json::from_value::<Params>(params) {
        Ok(params) => run(host, params).await,
        Err(_) => Err(failure(
            "Import requires project_id, target_path and windows_path only",
        )),
    };
    match result {
        Ok(result) => ipc::success(id, result),
        Err(error) => ResponseEnvelope {
            protocol: 1,
            request_id: id,
            ok: false,
            result: None,
            error: Some(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_device_network_relative_and_alternate_stream_paths() {
        for path in [
            r"relative.ipynb",
            r"C:relative.ipynb",
            r"\\server\share\a.ipynb",
            r"\\?\UNC\server\share\a.ipynb",
            r"\\.\pipe\a.ipynb",
            r"C:\x.ipynb:stream",
            r"C:\x:stream.ipynb",
            r"C:\..\x.ipynb",
        ] {
            assert!(disk_path(Path::new(path)).is_err(), "{path}");
        }
    }
    #[test]
    fn stable_handle_blocks_writers_and_hashes_exact_bytes() {
        let path = std::env::temp_dir().join(format!("ck-import-{}.ipynb", uuid::Uuid::new_v4()));
        let bytes = vec![b'x'; CHUNK + 1];
        std::fs::write(&path, &bytes).unwrap();
        let mut source = open_source(&path).unwrap();
        assert_eq!(source.size, bytes.len() as u64);
        assert_eq!(source.hash, format!("{:x}", Sha256::digest(&bytes)));
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(std::fs::remove_file(&path).is_err());
        let mut read = Vec::new();
        source.file.read_to_end(&mut read).unwrap();
        assert_eq!(read, bytes);
        drop(source);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn reject_empty_oversized_directory_and_wrong_extension() {
        let root = std::env::temp_dir().join(format!("ck-import-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        for (name, size) in [
            ("empty.ipynb", 0),
            ("large.ipynb", MAX_BYTES + 1),
            ("data.txt", 1),
        ] {
            let path = root.join(name);
            File::create(&path).unwrap().set_len(size).unwrap();
            assert!(open_source(&path).is_err());
        }
        let folder = root.join("folder.ipynb");
        std::fs::create_dir(&folder).unwrap();
        assert!(open_source(&folder).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
