//! In-memory event log (ring of the last 10k events) with live fan-out.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};
use tokio::sync::broadcast;
use tt_core::{EntryView, events::DomainEvent};

pub const RING_CAPACITY: usize = 10_000;

#[derive(Clone, Debug)]
pub struct Published {
    pub seq: u64,
    pub event: Arc<DomainEvent>,
}

impl Published {
    #[must_use]
    pub fn to_json(&self) -> Value {
        self.event.to_json(self.seq)
    }
}

struct Ring {
    events: VecDeque<Published>,
    last_seq: u64,
    capacity: usize,
}

/// Cloneable handle to the event bus.
#[derive(Clone)]
pub struct Bus {
    ring: Arc<Mutex<Ring>>,
    live: broadcast::Sender<Published>,
}

/// Result of positioning a new subscriber.
pub struct Replay {
    /// Events after `since` still in the ring.
    pub events: Vec<Published>,
    /// `(first missing, last missing)` when `since + 1` was evicted.
    pub gap: Option<(u64, u64)>,
    pub last_seq: u64,
    pub live: broadcast::Receiver<Published>,
}

impl Default for Bus {
    fn default() -> Self {
        Self::with_capacity(RING_CAPACITY)
    }
}

impl Bus {
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let (live, _) = broadcast::channel(4096);
        Self {
            ring: Arc::new(Mutex::new(Ring {
                events: VecDeque::with_capacity(capacity.min(1024)),
                last_seq: 0,
                capacity,
            })),
            live,
        }
    }

    /// Assigns the next seq, stores, and fans out.
    pub fn publish(&self, event: DomainEvent) -> Published {
        let mut ring = self.ring.lock().unwrap();
        ring.last_seq += 1;
        let published = Published {
            seq: ring.last_seq,
            event: Arc::new(event),
        };
        if ring.events.len() == ring.capacity {
            ring.events.pop_front();
        }
        ring.events.push_back(published.clone());
        let _ = self.live.send(published.clone());
        published
    }

    #[must_use]
    pub fn last_seq(&self) -> u64 {
        self.ring.lock().unwrap().last_seq
    }

    /// Subscribes atomically with respect to `publish`: every event after
    /// `since` is either in `events` or will arrive on `live`.
    #[must_use]
    pub fn subscribe(&self, since: Option<u64>) -> Replay {
        let ring = self.ring.lock().unwrap();
        let live = self.live.subscribe();
        let last_seq = ring.last_seq;
        let Some(since) = since else {
            return Replay {
                events: Vec::new(),
                gap: None,
                last_seq,
                live,
            };
        };
        let oldest = ring.events.front().map_or(last_seq + 1, |event| event.seq);
        let gap = (since + 1 < oldest && since < last_seq).then(|| (since + 1, oldest - 1));
        let events = ring
            .events
            .iter()
            .filter(|event| event.seq > since)
            .cloned()
            .collect();
        Replay {
            events,
            gap,
            last_seq,
            live,
        }
    }
}

/// The first line every watcher receives.
#[must_use]
pub fn snapshot(seq: u64, running: &[&EntryView]) -> Value {
    json!({"type": "snapshot", "seq": seq, "running": running})
}

#[must_use]
pub fn gap(from: u64, to: u64) -> Value {
    json!({"type": "gap", "from": from, "to": to})
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tt_core::events::Origin;

    fn event() -> DomainEvent {
        DomainEvent {
            kind: "task.created".into(),
            origin: Origin::Local,
            at: Utc::now(),
            entry: None,
            task: None,
            previous_task: None,
            project: None,
            tag: None,
            from: None,
            to: None,
            running: Vec::new(),
        }
    }

    #[test]
    fn replay_since_and_gap_after_eviction() {
        let bus = Bus::with_capacity(3);
        for _ in 0..5 {
            bus.publish(event());
        }
        let replay = bus.subscribe(Some(3));
        assert_eq!(
            replay.events.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![4, 5]
        );
        assert!(replay.gap.is_none());
        let replay = bus.subscribe(Some(1));
        assert_eq!(replay.gap, Some((2, 2)));
        assert_eq!(replay.events.len(), 3);
        let replay = bus.subscribe(Some(5));
        assert!(replay.events.is_empty() && replay.gap.is_none());
        let mut live = bus.subscribe(None).live;
        bus.publish(event());
        assert_eq!(live.try_recv().unwrap().seq, 6);
    }
}
