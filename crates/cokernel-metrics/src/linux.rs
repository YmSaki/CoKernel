use crate::{CgroupMemory, CpuCounters, Memory, Storage, Unavailable};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path},
};
fn read(path: &Path) -> Result<String, Unavailable> {
    let mut text = String::new();
    File::open(path)
        .and_then(|file| file.take(65_537).read_to_string(&mut text))
        .map_err(|_| Unavailable::ReadFailed)?;
    if text.len() > 65_536 {
        return Err(Unavailable::InvalidData);
    }
    Ok(text)
}
pub(crate) fn cpu() -> Result<CpuCounters, Unavailable> {
    parse_cpu(&read(Path::new("/proc/stat"))?)
}
fn parse_cpu(text: &str) -> Result<CpuCounters, Unavailable> {
    let line = text.lines().next().ok_or(Unavailable::InvalidData)?;
    let mut values = line.split_whitespace();
    if values.next() != Some("cpu") {
        return Err(Unavailable::InvalidData);
    }
    // guest and guest_nice are already included in user/nice. Never add them twice.
    let values = values
        .take(8)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Unavailable::InvalidData)?;
    if values.len() != 8 {
        return Err(Unavailable::InvalidData);
    }
    let total = values
        .iter()
        .try_fold(0u64, |sum, n| sum.checked_add(*n))
        .ok_or(Unavailable::InvalidData)?;
    let idle = values[3]
        .checked_add(values[4])
        .ok_or(Unavailable::InvalidData)?;
    Ok(CpuCounters { total, idle })
}
pub(crate) fn memory() -> Result<Memory, Unavailable> {
    parse_memory(&read(Path::new("/proc/meminfo"))?)
}
fn parse_memory(text: &str) -> Result<Memory, Unavailable> {
    let field = |name: &str| -> Result<u64, Unavailable> {
        let mut matching = text.lines().filter_map(|line| line.strip_prefix(name));
        let mut tokens = matching
            .next()
            .ok_or(Unavailable::NotSupported)?
            .split_whitespace();
        let value = tokens
            .next()
            .and_then(|n| n.parse::<u64>().ok())
            .and_then(|n| n.checked_mul(1024))
            .ok_or(Unavailable::InvalidData)?;
        if tokens.next() != Some("kB") || tokens.next().is_some() || matching.next().is_some() {
            return Err(Unavailable::InvalidData);
        }
        Ok(value)
    };
    let total_bytes = field("MemTotal:")?;
    let available_bytes = field("MemAvailable:")?;
    let used_bytes = total_bytes
        .checked_sub(available_bytes)
        .filter(|_| total_bytes > 0)
        .ok_or(Unavailable::InvalidData)?;
    Ok(Memory {
        total_bytes,
        used_bytes,
        available_bytes,
    })
}
pub(crate) fn storage(path: &Path) -> Result<Storage, Unavailable> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Unavailable::InvalidData)?;
    let mut data = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: live C string and correctly sized output structure.
    if unsafe { libc::statvfs(path.as_ptr(), data.as_mut_ptr()) } != 0 {
        return Err(Unavailable::ReadFailed);
    }
    let data = unsafe { data.assume_init() };
    let total_bytes = data
        .f_blocks
        .checked_mul(data.f_frsize)
        .ok_or(Unavailable::InvalidData)?;
    let free = data
        .f_bfree
        .checked_mul(data.f_frsize)
        .ok_or(Unavailable::InvalidData)?;
    let available_bytes = data
        .f_bavail
        .checked_mul(data.f_frsize)
        .ok_or(Unavailable::InvalidData)?;
    let used_bytes = total_bytes
        .checked_sub(free)
        .filter(|_| available_bytes <= free)
        .ok_or(Unavailable::InvalidData)?;
    Ok(Storage {
        total_bytes,
        used_bytes,
        available_bytes,
    })
}
pub(crate) fn cgroup_memory() -> Result<CgroupMemory, Unavailable> {
    let text = read(Path::new("/proc/self/cgroup"))?;
    cgroup_at(Path::new("/sys/fs/cgroup"), &text)
}
fn cgroup_at(root: &Path, text: &str) -> Result<CgroupMemory, Unavailable> {
    let path = text
        .lines()
        .find_map(|s| s.strip_prefix("0::"))
        .ok_or(Unavailable::NotSupported)?;
    if !path.starts_with('/')
        || Path::new(path).components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::CurDir | Component::Prefix(_)
            )
        })
    {
        return Err(Unavailable::InvalidData);
    }
    let mut current = root.join(path.trim_start_matches('/'));
    let current_bytes = read(&current.join("memory.current"))?
        .trim()
        .parse()
        .map_err(|_| Unavailable::InvalidData)?;
    let mut limit_bytes = None;
    for _ in 0..128 {
        let max = read(&current.join("memory.max"));
        // The cgroup v2 root has no memory.max; every non-root ancestor must be readable.
        if current != root || current.join("memory.max").exists() {
            let max = max?;
            if max.trim() != "max" {
                let value = max
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| Unavailable::InvalidData)?;
                limit_bytes = Some(limit_bytes.map_or(value, |old: u64| old.min(value)));
            }
        }
        if current == root {
            return Ok(CgroupMemory {
                current_bytes,
                limit_bytes,
            });
        }
        if !current.pop() || !current.starts_with(root) {
            return Err(Unavailable::InvalidData);
        }
    }
    Err(Unavailable::InvalidData)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proc_fields_do_not_double_count_guests_or_treat_free_as_available() {
        let cpu = parse_cpu("cpu 10 2 3 40 5 1 1 8 6 2\ncpu0 9").unwrap();
        assert_eq!((cpu.total, cpu.idle), (70, 45));
        let memory = parse_memory("MemTotal: 100 kB\nMemFree: 1 kB\nMemAvailable: 40 kB").unwrap();
        assert_eq!(
            (memory.used_bytes, memory.available_bytes),
            (60 * 1024, 40 * 1024)
        );
        assert!(parse_cpu("cpu 1 bad").is_err());
        assert!(parse_memory("MemTotal: 1 kB\nMemAvailable: 2 kB").is_err());
        assert!(parse_memory("MemTotal: 1 kB\nMemFree: 1 kB").is_err());
        assert!(parse_memory("MemTotal: 1 kB\nMemTotal: 1 kB\nMemAvailable: 0 kB").is_err());
    }
    #[test]
    fn cgroup_limit_includes_ancestors_and_rejects_escape() {
        let root = std::env::temp_dir().join(format!("ckmetric-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("parent/child")).unwrap();
        for (name, value) in [
            ("parent/memory.max", "100"),
            ("parent/child/memory.max", "max"),
            ("parent/child/memory.current", "50"),
        ] {
            std::fs::write(root.join(name), value).unwrap();
        }
        let memory = cgroup_at(&root, "0::/parent/child").unwrap();
        assert_eq!((memory.current_bytes, memory.limit_bytes), (50, Some(100)));
        assert!(cgroup_at(&root, "0::/../escape").is_err());
        std::fs::write(root.join("parent/memory.max"), "nonsense").unwrap();
        assert!(cgroup_at(&root, "0::/parent/child").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
