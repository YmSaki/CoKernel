//! Bounded, best-effort observations. Domain state and captured output own truth.
use cokernel_protocol::EventEnvelope;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

const MAX_EVENT_BYTES: usize = 32 * 1024;
const EVENTS: usize = 256;

#[derive(Clone)]
pub struct EventHub {
    sequence: Arc<Mutex<u64>>,
    events: broadcast::Sender<EventEnvelope>,
}
impl Default for EventHub {
    fn default() -> Self {
        Self {
            sequence: Arc::new(Mutex::new(0)),
            events: broadcast::channel(EVENTS).0,
        }
    }
}
impl EventHub {
    pub fn watermark(&self) -> u64 {
        *self
            .sequence
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
    /// Subscription and watermark are atomic with respect to publication.
    pub fn subscribe(&self) -> (broadcast::Receiver<EventEnvelope>, u64) {
        let sequence = self
            .sequence
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        (self.events.subscribe(), *sequence)
    }
    pub fn publish(&self, name: &str, payload: Value) {
        let mut sequence = self
            .sequence
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(next) = sequence.checked_add(1) else {
            return;
        };
        *sequence = next;
        let mut event = EventEnvelope {
            protocol: 1,
            event: name.into(),
            sequence: next,
            payload,
        };
        if serde_json::to_vec(&event).map_or(true, |bytes| bytes.len() > MAX_EVENT_BYTES) {
            // Do not truncate rich output and pretend it is complete. Persistence
            // uses the original captured receipt, never this observation stream.
            event.event = "events.resync_required".into();
            event.payload = json!({"reason":"event_size_limit"});
        }
        let _ = self.events.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_observation_is_explicit_and_lagged_receivers_can_resubscribe() {
        let hub = EventHub::default();
        let (mut subscriber, watermark) = hub.subscribe();
        assert_eq!(watermark, 0);
        hub.publish(
            "session.output",
            json!({"text":"x".repeat(MAX_EVENT_BYTES)}),
        );
        let event = subscriber.try_recv().unwrap();
        assert_eq!(event.event, "events.resync_required");
        assert_eq!(event.sequence, 1);
        for _ in 0..EVENTS + 1 {
            hub.publish("session.state_changed", json!({}));
        }
        assert!(matches!(
            subscriber.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));
        let (mut refreshed, before) = hub.subscribe();
        assert_eq!(before, hub.watermark());
        hub.publish("notebook.changed", json!({"revision":4}));
        assert_eq!(refreshed.try_recv().unwrap().sequence, before + 1);
    }
    #[test]
    fn concurrent_publishers_have_one_monotonic_sequence() {
        let hub = EventHub::default();
        let (mut events, _) = hub.subscribe();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let hub = hub.clone();
                scope.spawn(move || {
                    for _ in 0..16 {
                        hub.publish("changed", json!({}));
                    }
                });
            }
        });
        for expected in 1..=64 {
            assert_eq!(events.try_recv().unwrap().sequence, expected);
        }
    }
}
