//! Bounded synchronous uv runner, used on blocking threads by Runtime operations.
use std::io::Read;
use std::process::{Child, Command, Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use super::{UvError, UvRunControl};

const OUTPUT_LIMIT: usize = 64 * 1024;

struct OwnedChild(Child, bool);
impl OwnedChild {
    fn terminate_group(&mut self) {
        if self.1 {
            return;
        }
        self.1 = true;
        #[cfg(unix)]
        {
            // Created in a private process group before exec. Never signal Runtime.
            let pid = self.0.id();
            if pid > 1 && pid <= i32::MAX as u32 {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
        }
        let _ = self.0.kill();
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.terminate_group();
        let _ = self.0.wait();
    }
}

fn drain(mut reader: impl Read, exceeded: Arc<AtomicBool>) -> std::io::Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(result);
        }
        let remaining = OUTPUT_LIMIT.saturating_sub(result.len());
        result.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            exceeded.store(true, Ordering::Release);
        }
    }
}

pub(super) fn run(
    mut command: Command,
    timeout: Duration,
    control: &UvRunControl,
) -> Result<Output, UvError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = OwnedChild(
        command
            .spawn()
            .map_err(|source| UvError::Launch { source })?,
        false,
    );
    let stdout = child.0.stdout.take().ok_or_else(|| UvError::Launch {
        source: std::io::Error::other("missing uv stdout"),
    })?;
    let stderr = child.0.stderr.take().ok_or_else(|| UvError::Launch {
        source: std::io::Error::other("missing uv stderr"),
    })?;
    let exceeded = Arc::new(AtomicBool::new(false));
    std::thread::scope(|scope| {
        let stdout = scope.spawn(|| drain(stdout, exceeded.clone()));
        let stderr = scope.spawn(|| drain(stderr, exceeded.clone()));
        let start = Instant::now();
        let status = loop {
            if control.is_cancelled() {
                break Err(UvError::Cancelled);
            }
            if exceeded.load(Ordering::Acquire) {
                break Err(UvError::OutputLimit);
            }
            if start.elapsed() >= timeout {
                break Err(UvError::Timeout);
            }
            match child.0.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(source) => break Err(UvError::Launch { source }),
            }
        };
        // Reap ordinary build descendants even when uv exits before them.
        child.terminate_group();
        let _ = child.0.wait();
        let read_result = |thread: std::thread::ScopedJoinHandle<'_, std::io::Result<Vec<u8>>>| {
            thread
                .join()
                .map_err(|_| UvError::Launch {
                    source: std::io::Error::other("uv output reader failed"),
                })?
                .map_err(|source| UvError::Launch { source })
        };
        let stdout = read_result(stdout)?;
        let stderr = read_result(stderr)?;
        let status = status?;
        if exceeded.load(Ordering::Acquire) {
            return Err(UvError::OutputLimit);
        }
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn shell(source: &str) -> Command {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", source]);
        c
    }
    #[test]
    fn timeout_reaps_child_and_descendant_holding_output_open() {
        let start = Instant::now();
        assert!(matches!(
            run(
                shell("sleep 30 & wait"),
                Duration::from_millis(50),
                &UvRunControl::default()
            ),
            Err(UvError::Timeout)
        ));
        assert!(start.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn exited_parent_does_not_leave_output_reader_waiting_for_descendant() {
        let start = Instant::now();
        assert!(
            run(
                shell("sleep 30 & exit 0"),
                Duration::from_secs(2),
                &UvRunControl::default()
            )
            .unwrap()
            .status
            .success()
        );
        assert!(start.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn unbounded_output_is_rejected_and_collector_memory_is_bounded() {
        assert!(matches!(
            run(
                shell("yes x"),
                Duration::from_secs(3),
                &UvRunControl::default()
            ),
            Err(UvError::OutputLimit)
        ));
        let flag = Arc::new(AtomicBool::new(false));
        let bytes = drain(
            std::io::Cursor::new(vec![b'x'; OUTPUT_LIMIT * 2]),
            flag.clone(),
        )
        .unwrap();
        assert_eq!(bytes.len(), OUTPUT_LIMIT);
        assert!(flag.load(Ordering::Acquire));
    }
}
