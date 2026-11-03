use std::sync::atomic::{AtomicU32, Ordering};

/// Owns the private process group created before the worker launcher executes.
/// Retaining its id also permits cleanup after the launcher has already exited.
struct WorkerProcessGroup {
    id: AtomicU32,
}

impl WorkerProcessGroup {
    fn new(id: u32) -> Self {
        Self { id: AtomicU32::new(id) }
    }

    fn terminate(&self) -> Result<(), std::io::Error> {
        let id = self.id.swap(0, Ordering::AcqRel);
        if id == 0 {
            return Ok(());
        }
        let pid = i32::try_from(id)
            .ok()
            .filter(|pid| *pid > 1)
            .ok_or_else(|| std::io::Error::other("invalid private worker process group"))?;
        // The child is launched with process_group(0); a negative PID selects
        // only that owned group, never the Runtime's inherited process group.
        let result = unsafe { libc::kill(-pid, libc::SIGKILL) };
        if result == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        self.id.store(id, Ordering::Release);
        Err(error)
    }

    #[cfg(test)]
    fn disarmed() -> Self {
        Self::new(0)
    }
}

impl Drop for WorkerProcessGroup {
    fn drop(&mut self) {
        // Covers startup errors, cancelled start futures and actor cancellation.
        let _ = self.terminate();
    }
}
