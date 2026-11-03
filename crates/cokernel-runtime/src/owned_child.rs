//! Child group ownership shared by optional control-plane services.
use std::{io, time::Duration};
use tokio::{
    process::Child,
    time::{sleep, timeout},
};

pub(crate) struct OwnedChild {
    pub(crate) child: Child,
    pub(crate) pid: i32,
}
impl OwnedChild {
    pub(crate) fn signal(&self, signal: i32) {
        if self.pid > 1 {
            unsafe {
                libc::kill(-self.pid, signal);
            }
        }
    }
    pub(crate) async fn exited(&self) -> io::Result<()> {
        loop {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // Observe without reaping: keep the leader PID reserved until the
            // private group is killed, avoiding a recycled process-group ID.
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pid as u32,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result != 0 {
                return Err(io::Error::last_os_error());
            }
            if unsafe { info.si_pid() } != 0 {
                return Ok(());
            }
            sleep(Duration::from_millis(25)).await;
        }
    }
    pub(crate) async fn cleanup(&mut self) -> io::Result<std::process::ExitStatus> {
        self.signal(libc::SIGTERM);
        let _ = timeout(Duration::from_secs(1), self.exited()).await;
        self.signal(libc::SIGKILL);
        let status = self.child.wait().await?;
        self.pid = 0;
        Ok(status)
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.pid > 1 {
            self.signal(libc::SIGKILL);
        }
    }
}
