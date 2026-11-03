//! Linux workload credential transition. Runtime retains ownership of the child
//! and its process group; no second process supervisor or execution API is added.
use std::{io, path::Path, process::Command};

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkloadIdentity {
    managed: Option<(u32, u32)>,
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "CK-SEC-WORKLOAD: invalid identity boundary",
    )
}

impl WorkloadIdentity {
    /// Absence is the unprivileged development profile. A partial/invalid managed
    /// configuration never falls back to it. Installation supplies both IDs.
    pub fn from_env() -> io::Result<Self> {
        let parse = |name| -> io::Result<Option<u32>> {
            std::env::var_os(name)
                .map(|value| {
                    let value = value.to_str().ok_or_else(invalid)?;
                    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(invalid());
                    }
                    value.parse().map_err(|_| invalid())
                })
                .transpose()
        };
        let result = Self::from_ids(
            parse("COKERNEL_WORKLOAD_UID")?,
            parse("COKERNEL_WORKLOAD_GID")?,
        )?;
        result.validate_control()?;
        Ok(result)
    }

    fn from_ids(uid: Option<u32>, gid: Option<u32>) -> io::Result<Self> {
        match (uid, gid) {
            (None, None) => Ok(Self::default()),
            (Some(uid), Some(gid))
                if (1..u32::MAX).contains(&uid) && (1..u32::MAX).contains(&gid) =>
            {
                Ok(Self {
                    managed: Some((uid, gid)),
                })
            }
            _ => Err(invalid()),
        }
    }

    pub fn is_managed(self) -> bool {
        self.managed.is_some()
    }

    pub fn validate_control(self) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            linux::validate_control(self)
        }
        #[cfg(not(target_os = "linux"))]
        {
            if self.is_managed() {
                Err(invalid())
            } else {
                Ok(())
            }
        }
    }

    /// Called once at Runtime startup before admitting work. Group sharing is
    /// scoped by setgid Project directories; control state uses private parents.
    pub fn initialize(self, projects: &Path) -> io::Result<()> {
        self.validate_control()?;
        #[cfg(target_os = "linux")]
        if self.is_managed() {
            self.validate_project(projects)?;
            unsafe {
                libc::umask(0o007);
            }
        }
        let _ = projects;
        Ok(())
    }

    pub fn validate_project(self, project: &Path) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        if let Some((uid, gid)) = self.managed {
            use std::os::unix::fs::MetadataExt;
            let meta = std::fs::symlink_metadata(project)?;
            if !meta.is_dir()
                || !project.is_absolute()
                || (meta.uid() != unsafe { libc::geteuid() } && meta.uid() != uid)
                || meta.gid() != gid
                || meta.mode() & 0o7777 != 0o2770
            {
                return Err(invalid());
            }
        }
        let _ = project;
        Ok(())
    }

    pub(crate) fn configure(self, command: &mut Command, project: &Path) -> io::Result<()> {
        #[cfg(not(target_os = "linux"))]
        let _ = &command;
        self.validate_control()?;
        self.validate_project(project)?;
        #[cfg(target_os = "linux")]
        if let Some((uid, gid)) = self.managed {
            use std::os::unix::process::CommandExt;
            let parent = unsafe { libc::getpid() };
            // Only async-signal-safe syscall wrappers and stack data after fork.
            unsafe {
                command.pre_exec(move || linux::enter(uid, gid, parent));
            }
        }
        Ok(())
    }

    #[cfg(unix)]
    pub(crate) fn peer_uid(self) -> u32 {
        self.managed
            .map_or_else(|| unsafe { libc::geteuid() }, |ids| ids.0)
    }

    #[cfg(unix)]
    pub(crate) fn prepare_socket_dir(self, path: &Path) -> io::Result<()> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if let Some((_, gid)) = self.managed {
            let meta = std::fs::symlink_metadata(path)?;
            if !meta.is_dir()
                || path.canonicalize()?.as_path() != path
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.gid() != gid
                || meta.mode() & 0o7777 != 0o2710
            {
                return Err(invalid());
            }
        } else {
            std::fs::create_dir_all(path)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    #[cfg(unix)]
    pub(crate) fn socket_mode(self) -> u32 {
        if self.is_managed() { 0o660 } else { 0o600 }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    // CAP_KILL, CAP_SETGID and CAP_SETUID only. No DAC override, ptrace,
    // mount/admin or ambient/inheritable privilege is needed by Runtime.
    const REQUIRED: u32 = (1 << 5) | (1 << 6) | (1 << 7);
    #[repr(C)]
    struct Header {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Data {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }

    fn capabilities() -> io::Result<[Data; 2]> {
        let mut header = Header {
            version: 0x2008_0522,
            pid: 0,
        };
        let mut data = [Data::default(); 2];
        if unsafe { libc::syscall(libc::SYS_capget, &mut header, data.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(data)
    }

    pub(super) fn validate_control(policy: WorkloadIdentity) -> io::Result<()> {
        let uid = unsafe { libc::geteuid() };
        let gid = unsafe { libc::getegid() };
        let caps = capabilities()?;
        if uid == 0 || unsafe { libc::getuid() } != uid || unsafe { libc::getgid() } != gid {
            return Err(invalid());
        }
        if let Some((work_uid, work_gid)) = policy.managed {
            if work_uid == uid
                || work_gid == gid
                || caps[0].effective != REQUIRED
                || caps[0].permitted != REQUIRED
                || caps[0].inheritable != 0
                || caps[1].effective != 0
                || caps[1].permitted != 0
                || caps[1].inheritable != 0
            {
                return Err(invalid());
            }
            let executable = std::fs::metadata("/proc/self/exe")?;
            if executable.uid() != 0
                || executable.gid() != gid
                || executable.mode() & 0o7777 != 0o750
            {
                return Err(invalid());
            }
        } else if caps
            .iter()
            .any(|c| c.effective != 0 || c.permitted != 0 || c.inheritable != 0)
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub(super) fn enter(uid: u32, gid: u32, parent: i32) -> io::Result<()> {
        // Clear supplementary groups, all real/effective/saved IDs and capability
        // sets. no_new_privs blocks setuid/file-capability regain on later exec.
        unsafe {
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                || libc::prctl(
                    libc::PR_CAP_AMBIENT,
                    libc::PR_CAP_AMBIENT_CLEAR_ALL,
                    0,
                    0,
                    0,
                ) != 0
                || libc::setgroups(0, std::ptr::null()) != 0
                || libc::setresgid(gid, gid, gid) != 0
                || libc::setresuid(uid, uid, uid) != 0
            {
                return Err(io::Error::last_os_error());
            }
            let mut header = Header {
                version: 0x2008_0522,
                pid: 0,
            };
            let data = [Data::default(); 2];
            if libc::syscall(libc::SYS_capset, &mut header, data.as_ptr()) != 0
                || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0
            {
                return Err(io::Error::last_os_error());
            }
            // Credential changes clear pdeathsig; arm it afterwards and close
            // the race with Runtime death before the setting was installed.
            if libc::getppid() != parent {
                return Err(io::Error::from_raw_os_error(libc::ESRCH));
            }
            libc::umask(0o007);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn managed_identity_never_accepts_partial_root_or_sentinel_ids() {
        for pair in [
            (Some(1), None),
            (None, Some(1)),
            (Some(0), Some(2)),
            (Some(2), Some(0)),
            (Some(u32::MAX), Some(2)),
        ] {
            assert!(WorkloadIdentity::from_ids(pair.0, pair.1).is_err());
        }
        assert!(!WorkloadIdentity::from_ids(None, None).unwrap().is_managed());
        assert!(
            WorkloadIdentity::from_ids(Some(1001), Some(1002))
                .unwrap()
                .is_managed()
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn managed_identity_rejects_unprivileged_development_control() {
        assert!(
            WorkloadIdentity::from_ids(Some(61001), Some(61002))
                .unwrap()
                .validate_control()
                .is_err()
        );
    }
}
