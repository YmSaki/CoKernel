use crate::{CgroupMemory, CpuCounters, Memory, Storage, Unavailable};
use std::path::Path;
use windows_sys::Win32::{
    Foundation::FILETIME,
    Storage::FileSystem::GetDiskFreeSpaceExW,
    System::{
        SystemInformation::*,
        Threading::{GetActiveProcessorGroupCount, GetSystemTimes},
    },
};
pub(crate) fn cpu() -> Result<CpuCounters, Unavailable> {
    // GetSystemTimes covers one processor group. Do not label a partial value as whole-machine.
    if unsafe { GetActiveProcessorGroupCount() } != 1 {
        return Err(Unavailable::NotSupported);
    }
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return Err(Unavailable::ReadFailed);
    }
    let count = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    let total = count(kernel)
        .checked_add(count(user))
        .ok_or(Unavailable::InvalidData)?;
    // Windows kernel time includes idle time.
    Ok(CpuCounters {
        total,
        idle: count(idle),
    })
}
pub(crate) fn memory() -> Result<Memory, Unavailable> {
    let mut data = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut data) } == 0 {
        return Err(Unavailable::ReadFailed);
    }
    Ok(Memory {
        total_bytes: data.ullTotalPhys,
        available_bytes: data.ullAvailPhys,
        used_bytes: data
            .ullTotalPhys
            .checked_sub(data.ullAvailPhys)
            .ok_or(Unavailable::InvalidData)?,
    })
}
pub(crate) fn storage(path: &Path) -> Result<Storage, Unavailable> {
    use std::os::windows::ffi::OsStrExt;
    let mut path: Vec<_> = path.as_os_str().encode_wide().collect();
    if path.contains(&0) {
        return Err(Unavailable::InvalidData);
    }
    path.push(0);
    let (mut available, mut total, mut free) = (0, 0, 0);
    if unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut available, &mut total, &mut free) } == 0 {
        return Err(Unavailable::ReadFailed);
    }
    Ok(Storage {
        total_bytes: total,
        used_bytes: total.checked_sub(free).ok_or(Unavailable::InvalidData)?,
        available_bytes: available,
    })
}
pub(crate) fn cgroup_memory() -> Result<CgroupMemory, Unavailable> {
    Err(Unavailable::NotSupported)
}
