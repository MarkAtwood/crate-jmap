//! [`PushHub`]: an in-memory [`PushSource`] with `pushState` replay.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use futures::StreamExt as _;
use futures::stream::BoxStream;
use jmap_types::{Id, State, StateChange};
use tokio::sync::watch;

use super::PushSource;
use crate::{WebSocketPushEnable, WsStateChange};

/// The `changed` map of a [`StateChange`] (RFC 8620 §7.1).
type Changed = BTreeMap<Id, BTreeMap<String, State>>;

/// Distinguishes hubs made in the same clock tick.
static HUB_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fan-out of state changes to every connection of one principal, with
/// RFC 8887 §4.3.5.1 `pushState` tokens and replay.
///
/// The deployment calls [`publish`](Self::publish) after every write and
/// gives each connection a clone as its [`PushSource`]. Keep one hub per
/// principal (or per set of accounts a caller sees): a hub does no
/// access filtering, and its `pushState` "encodes the entire server state
/// visible to the user".
///
/// The hub keeps the last `capacity` changes. A `pushState` it issued is
/// answered with the changes since then, merged into one notification;
/// if some were already dropped, with the latest state of every type the
/// hub has seen (a superset: a client's `/changes` call for a type that
/// did not change returns nothing). A token it did not issue (another
/// hub, or a hub from before a restart) replays nothing, and the client
/// resyncs with `/changes`. Tokens are opaque strings.
///
/// Each wake of a slow connection merges all changes since its last
/// notification into one, keeping the latest state per type; `extra`
/// members of merged changes are dropped.
///
/// ```
/// use std::collections::BTreeMap;
/// use jmap_websocket::server::PushHub;
/// use jmap_websocket::jmap_types::{Id, State, StateChange};
///
/// let hub = PushHub::new(256);
/// let mut types = BTreeMap::new();
/// types.insert("Email".to_owned(), State::from("e2"));
/// let mut changed = BTreeMap::new();
/// changed.insert(Id::from("a1"), types);
/// hub.publish(StateChange::new(changed));
/// assert!(!hub.push_state().is_empty());
/// ```
#[derive(Clone)]
pub struct PushHub {
    inner: Arc<Inner>,
}

struct Inner {
    epoch: String,
    capacity: usize,
    log: Mutex<Log>,
    wake: watch::Sender<u64>,
}

#[derive(Default)]
struct Log {
    seq: u64,
    entries: VecDeque<(u64, Changed)>,
    latest: Changed,
}

impl fmt::Debug for PushHub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushHub")
            .field("capacity", &self.inner.capacity)
            .field("push_state", &self.push_state())
            .finish_non_exhaustive()
    }
}

impl PushHub {
    /// A hub that keeps the last `capacity` changes for replay (at least
    /// one).
    pub fn new(capacity: usize) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        let n = HUB_COUNTER.fetch_add(1, Ordering::Relaxed);
        let (wake, _) = watch::channel(0);
        Self {
            inner: Arc::new(Inner {
                epoch: format!("{:x}", nanos ^ n.rotate_left(48)),
                capacity: capacity.max(1),
                log: Mutex::new(Log::default()),
                wake,
            }),
        }
    }

    /// Record a state change and wake every subscribed connection.
    /// A change with no accounts is ignored; `extra` members are not kept.
    pub fn publish(&self, change: StateChange) {
        if change.changed.values().all(BTreeMap::is_empty) {
            return;
        }
        let seq = {
            let mut log = self.lock();
            log.seq += 1;
            let seq = log.seq;
            for (account, types) in &change.changed {
                let entry = log.latest.entry(account.clone()).or_default();
                for (name, state) in types {
                    entry.insert(name.clone(), state.clone());
                }
            }
            log.entries.push_back((seq, change.changed));
            while log.entries.len() > self.inner.capacity {
                log.entries.pop_front();
            }
            seq
        };
        self.inner.wake.send_replace(seq);
    }

    /// The `pushState` token for everything published so far.
    pub fn push_state(&self) -> String {
        self.token(self.lock().seq)
    }

    fn lock(&self) -> MutexGuard<'_, Log> {
        // A panic while holding the lock leaves the log consistent: every
        // mutation above is a single push or insert.
        self.inner
            .log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn token(&self, seq: u64) -> String {
        format!("{}.{seq}", self.inner.epoch)
    }

    /// The sequence number a token of this hub names, if it is one.
    fn parse_token(&self, token: &str) -> Option<u64> {
        let (epoch, seq) = token.split_once('.')?;
        if epoch != self.inner.epoch {
            return None;
        }
        seq.parse().ok()
    }

    /// The changes after `since`, merged, and the sequence they bring the
    /// reader to; `None` when there are none.
    fn changes_since(&self, since: u64) -> Option<(WsStateChange, u64)> {
        let log = self.lock();
        if log.seq <= since {
            return None;
        }
        let complete = log
            .entries
            .front()
            .is_some_and(|(first, _)| *first <= since + 1);
        let merged = if complete {
            let mut merged: Changed = BTreeMap::new();
            for (_, changed) in log.entries.iter().filter(|(seq, _)| *seq > since) {
                for (account, types) in changed {
                    let entry = merged.entry(account.clone()).or_default();
                    for (name, state) in types {
                        entry.insert(name.clone(), state.clone());
                    }
                }
            }
            merged
        } else {
            log.latest.clone()
        };
        let seq = log.seq;
        drop(log);
        Some((
            WsStateChange::new(StateChange::new(merged), Some(self.token(seq))),
            seq,
        ))
    }
}

impl PushSource for PushHub {
    fn subscribe(&mut self, enable: &WebSocketPushEnable) -> BoxStream<'static, WsStateChange> {
        let mut wake = self.inner.wake.subscribe();
        wake.mark_unchanged();
        let current = self.lock().seq;
        let start = enable
            .push_state
            .as_deref()
            .and_then(|t| self.parse_token(t))
            .filter(|seq| *seq <= current)
            .unwrap_or(current);
        let hub = self.clone();
        futures::stream::unfold((hub, wake, start), |(hub, mut wake, since)| async move {
            loop {
                if let Some((change, seq)) = hub.changes_since(since) {
                    return Some((change, (hub, wake, seq)));
                }
                if wake.changed().await.is_err() {
                    return None;
                }
            }
        })
        .boxed()
    }
}
