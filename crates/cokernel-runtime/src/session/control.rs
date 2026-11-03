//! Constant-space control intent, independent of the execution/inspection FIFO.
use super::{ControlRequest, SessionError};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Copy, Default)]
struct Lifecycle {
    stop: bool,
    stale: bool,
}

#[derive(Clone)]
pub(super) struct ControlSender {
    lifecycle: watch::Sender<Lifecycle>,
    interrupt: mpsc::Sender<()>,
}

pub(super) struct ControlReceiver {
    lifecycle: watch::Receiver<Lifecycle>,
    interrupt: mpsc::Receiver<()>,
    stale_observed: bool,
}

pub(super) fn channel() -> (ControlSender, ControlReceiver) {
    let (lifecycle_tx, lifecycle_rx) = watch::channel(Lifecycle::default());
    let (interrupt_tx, interrupt_rx) = mpsc::channel(1);
    (
        ControlSender {
            lifecycle: lifecycle_tx,
            interrupt: interrupt_tx,
        },
        ControlReceiver {
            lifecycle: lifecycle_rx,
            interrupt: interrupt_rx,
            stale_observed: false,
        },
    )
}

impl ControlSender {
    pub(super) fn send(&self, request: ControlRequest) -> Result<(), SessionError> {
        if let ControlRequest::Interrupt = request {
            return match self.interrupt.try_send(()) {
                Ok(()) | Err(mpsc::error::TrySendError::Full(())) => Ok(()),
                Err(mpsc::error::TrySendError::Closed(())) => {
                    Err(SessionError::CommandChannelClosed)
                }
            };
        }
        if self.lifecycle.is_closed() {
            return Err(SessionError::CommandChannelClosed);
        }
        self.lifecycle.send_if_modified(|state| {
            let flag = match request {
                ControlRequest::Stop => &mut state.stop,
                ControlRequest::MarkEnvironmentStale => &mut state.stale,
                ControlRequest::Interrupt => return false,
            };
            let changed = !*flag;
            *flag = true;
            changed
        });
        Ok(())
    }
}

impl ControlReceiver {
    pub(super) async fn recv(&mut self) -> Option<ControlRequest> {
        loop {
            let state = *self.lifecycle.borrow_and_update();
            if state.stop {
                return Some(ControlRequest::Stop);
            }
            if state.stale && !self.stale_observed {
                self.stale_observed = true;
                return Some(ControlRequest::MarkEnvironmentStale);
            }
            tokio::select! {
                biased;
                changed = self.lifecycle.changed() => {
                    if changed.is_err() { return None; }
                }
                interrupt = self.interrupt.recv() => {
                    return interrupt.map(|()| ControlRequest::Interrupt);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn burst_does_not_need_a_consumer_and_stop_has_priority() {
        let (sender, mut receiver) = channel();
        for _ in 0..10_000 {
            sender.send(ControlRequest::Interrupt).unwrap();
            sender.send(ControlRequest::MarkEnvironmentStale).unwrap();
        }
        sender.send(ControlRequest::Stop).unwrap();
        assert!(matches!(receiver.recv().await, Some(ControlRequest::Stop)));
    }

    #[tokio::test]
    async fn stale_is_sticky_and_repeated_interrupts_coalesce_until_consumed() {
        let (sender, mut receiver) = channel();
        for _ in 0..100 {
            sender.send(ControlRequest::Interrupt).unwrap();
        }
        sender.send(ControlRequest::MarkEnvironmentStale).unwrap();
        assert!(matches!(
            receiver.recv().await,
            Some(ControlRequest::MarkEnvironmentStale)
        ));
        assert!(matches!(
            receiver.recv().await,
            Some(ControlRequest::Interrupt)
        ));
        sender.send(ControlRequest::MarkEnvironmentStale).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), receiver.recv())
                .await
                .is_err()
        );
        sender.send(ControlRequest::Interrupt).unwrap();
        assert!(matches!(
            receiver.recv().await,
            Some(ControlRequest::Interrupt)
        ));
        drop(sender);
        assert!(receiver.recv().await.is_none());
    }

    #[test]
    fn exited_actor_rejects_every_control_kind() {
        let (sender, receiver) = channel();
        drop(receiver);
        for request in [
            ControlRequest::Interrupt,
            ControlRequest::Stop,
            ControlRequest::MarkEnvironmentStale,
        ] {
            assert!(matches!(
                sender.send(request),
                Err(SessionError::CommandChannelClosed)
            ));
        }
    }
}
