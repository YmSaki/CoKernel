//! Cancellable Linux bridge input. Tokio's blocking stdin reader can keep the
//! executor alive after a shutdown reply while the Host still holds its writer.
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, ReadBuf, unix::AsyncFd};

pub struct BridgeInput(AsyncFd<OwnedFd>);
impl BridgeInput {
    pub fn stdin() -> io::Result<Self> {
        // Duplicate without taking ownership of process fd 0; handle an absent
        // stdin as an ordinary startup error. The duplicate is close-on-exec.
        let fd = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fcntl returned a new owned descriptor above.
        Self::new(unsafe { OwnedFd::from_raw_fd(fd) })
    }
    fn new(fd: OwnedFd) -> io::Result<Self> {
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(AsyncFd::new(fd)?))
    }
}
impl AsyncRead for BridgeInput {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buffer.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            let mut ready = std::task::ready!(self.0.poll_read_ready(cx))?;
            let bytes = buffer.initialize_unfilled();
            match ready.try_io(|inner| {
                // SAFETY: writable slice is valid for its entire length; fd is
                // owned for the call and configured nonblocking above.
                let size = unsafe {
                    libc::read(
                        inner.get_ref().as_raw_fd(),
                        bytes.as_mut_ptr().cast(),
                        bytes.len(),
                    )
                };
                if size < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(size as usize)
                }
            }) {
                Ok(Ok(size)) => {
                    buffer.advance(size);
                    return Poll::Ready(Ok(()));
                }
                Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => continue,
                Ok(Err(error)) => return Poll::Ready(Err(error)),
                Err(_) => continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, os::unix::net::UnixStream, time::Duration};
    use tokio::io::AsyncReadExt;

    #[test]
    fn pending_input_does_not_keep_executor_alive_while_peer_is_open() {
        let (reader, _writer) = UnixStream::pair().unwrap();
        let (finished, observed) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let mut input = BridgeInput::new(reader.into()).unwrap();
                let mut byte = [0];
                assert!(
                    tokio::time::timeout(Duration::from_millis(10), input.read_exact(&mut byte))
                        .await
                        .is_err()
                );
            });
            drop(runtime);
            finished.send(()).unwrap();
        });
        observed
            .recv_timeout(Duration::from_secs(2))
            .expect("pending input kept executor alive");
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn reads_fragmented_bytes_and_eof_without_losing_data() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let mut input = BridgeInput::new(reader.into()).unwrap();
        writer.write_all(b"ab").unwrap();
        let mut bytes = [0; 4];
        let read = input.read_exact(&mut bytes);
        tokio::pin!(read);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut read)
                .await
                .is_err()
        );
        writer.write_all(b"cd").unwrap();
        read.await.unwrap();
        assert_eq!(&bytes, b"abcd");
        drop(writer);
        assert_eq!(input.read(&mut [0]).await.unwrap(), 0);
    }
}
