//! Reconciliation is independent of the event that changed the target.
//! Disable, credential replacement, MCP loss/replacement and shutdown all use
//! the same target/owned-incarnation comparison.
use super::State;
use std::net::SocketAddr;
use tokio::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Identity {
    pub epoch: u64,
    pub mcp_generation: u64,
    pub address: SocketAddr,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Decision {
    Stop,
    Observe,
    Finish,
    Wait,
    Exhausted,
    Backoff,
    Launch(Identity),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Connection {
    Starting,
    Connected,
    Degraded,
}
#[derive(Default)]
pub(super) struct Recovery {
    pub failures: u32,
    pub retry_at: Option<Instant>,
}
impl Recovery {
    pub fn failed(&mut self, now: Instant, stable: bool) {
        if stable {
            self.failures = 0;
        }
        self.failures = self.failures.saturating_add(1);
        self.retry_at = [250, 1000, 4000]
            .get(self.failures as usize - 1)
            .map(|ms| now + std::time::Duration::from_millis(*ms));
    }
}
pub(super) fn decide(
    target: Option<Identity>,
    owned: Option<Identity>,
    shutdown: bool,
    recovery: &Recovery,
    now: Instant,
) -> Decision {
    if let Some(owned) = owned {
        return if shutdown || target != Some(owned) {
            Decision::Stop
        } else {
            Decision::Observe
        };
    }
    if shutdown {
        return Decision::Finish;
    }
    if recovery.failures >= 4 {
        return Decision::Exhausted;
    }
    let Some(target) = target else {
        return Decision::Wait;
    };
    if recovery.retry_at.is_some_and(|deadline| now < deadline) {
        return Decision::Backoff;
    }
    Decision::Launch(target)
}
pub(super) fn display(decision: Decision, enabled: bool, health: Option<Connection>) -> State {
    match decision {
        Decision::Stop => State::Stopping,
        Decision::Observe => match health {
            Some(Connection::Connected) => State::Connected,
            Some(Connection::Degraded) => State::Degraded,
            _ => State::Starting,
        },
        Decision::Finish => State::Disabled,
        Decision::Wait if !enabled => State::Disabled,
        Decision::Wait => State::Degraded,
        Decision::Launch(_) => State::Starting,
        Decision::Exhausted => State::Error,
        Decision::Backoff => State::Reconnecting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(epoch: u64, generation: u64) -> Identity {
        Identity {
            epoch,
            mcp_generation: generation,
            address: "127.0.0.1:1234".parse().unwrap(),
        }
    }
    #[test]
    fn all_normal_target_changes_use_one_reconciliation_rule() {
        let a = id(1, 1);
        let recovery = Recovery::default();
        let now = Instant::now();
        // No cause-specific branches: disable, changed credentials, changed MCP,
        // unchanged target, and shutdown are products of the same comparison.
        for target in [None, Some(a), Some(id(2, 1)), Some(id(1, 2))] {
            for owned in [None, Some(a), Some(id(2, 1)), Some(id(1, 2))] {
                for shutdown in [false, true] {
                    let result = decide(target, owned, shutdown, &recovery, now);
                    if let Some(owned) = owned {
                        assert_eq!(
                            result,
                            if shutdown || target != Some(owned) {
                                Decision::Stop
                            } else {
                                Decision::Observe
                            }
                        );
                    } else if shutdown {
                        assert_eq!(result, Decision::Finish);
                    } else {
                        assert_eq!(result, target.map_or(Decision::Wait, Decision::Launch));
                    }
                    assert!(!matches!(result, Decision::Launch(_)) || owned.is_none());
                }
            }
        }
    }
    #[test]
    fn only_failures_consume_retry_budget_and_cleanup_has_precedence() {
        let now = Instant::now();
        let a = id(1, 1);
        let mut recovery = Recovery::default();
        for n in 1..=4 {
            recovery.failed(now, false);
            assert_eq!(recovery.failures, n);
            assert_eq!(
                decide(Some(a), None, false, &recovery, now),
                if n == 4 {
                    Decision::Exhausted
                } else {
                    Decision::Backoff
                }
            );
            assert_eq!(decide(None, Some(a), true, &recovery, now), Decision::Stop);
        }
        recovery.failed(now, true);
        assert_eq!(recovery.failures, 1);
    }
}
