//! Per-key delivery windows for the §Rate Limiting SHOULD rules of the
//! WebSocket drafts: at most `max` events per key per `per`, the rest
//! dropped, or, with `keep_latest`, the latest one held and delivered
//! when the window reopens.
//!
//! Each connection's forwarding task owns its limiters, so a limit
//! counts the events delivered to one connection.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::Duration;

use tokio::time::Instant;

/// Above this many keys, [`Limiter::offer`] forgets the idle ones.
const GC_THRESHOLD: usize = 1024;

/// One rule: at most `max` events per key in any `per`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Window {
    pub(crate) max: usize,
    pub(crate) per: Duration,
    /// Hold the latest event over the limit and deliver it when the
    /// window reopens, instead of dropping it.
    pub(crate) keep_latest: bool,
}

struct Slot<E> {
    /// When the events of the current window went out, oldest first.
    sent: VecDeque<Instant>,
    /// The latest event over the limit, if held.
    pending: Option<E>,
}

impl<E> Slot<E> {
    fn prune(&mut self, now: Instant, per: Duration) {
        while self.sent.front().is_some_and(|&t| t + per <= now) {
            self.sent.pop_front();
        }
    }
}

/// The windows of one rule, per key. `None` rule: no limit.
pub(crate) struct Limiter<K, E> {
    rule: Option<Window>,
    slots: HashMap<K, Slot<E>>,
}

impl<K: Hash + Eq + Clone, E> Limiter<K, E> {
    pub(crate) fn new(rule: Option<Window>) -> Self {
        Self {
            rule,
            slots: HashMap::new(),
        }
    }

    /// `event` back if it may be delivered now, counting it. Otherwise
    /// it is dropped, or held in place of an older held one.
    pub(crate) fn offer(&mut self, key: K, event: E, now: Instant) -> Option<E> {
        let Some(rule) = self.rule else {
            return Some(event);
        };
        if self.slots.len() > GC_THRESHOLD {
            self.slots.retain(|_, slot| {
                slot.prune(now, rule.per);
                slot.pending.is_some() || !slot.sent.is_empty()
            });
        }
        let slot = self.slots.entry(key).or_insert_with(|| Slot {
            sent: VecDeque::new(),
            pending: None,
        });
        slot.prune(now, rule.per);
        if slot.sent.len() < rule.max {
            slot.sent.push_back(now);
            // A newer event supersedes a held one.
            slot.pending = None;
            Some(event)
        } else {
            if rule.keep_latest {
                slot.pending = Some(event);
            }
            None
        }
    }

    /// When the earliest held event may go out.
    pub(crate) fn next_due(&self) -> Option<Instant> {
        let per = self.rule?.per;
        self.slots
            .values()
            .filter(|slot| slot.pending.is_some())
            .filter_map(|slot| slot.sent.front().map(|&t| t + per))
            .min()
    }

    /// The held events whose window has reopened, counted as delivered.
    pub(crate) fn take_due(&mut self, now: Instant) -> Vec<(K, E)> {
        let Some(rule) = self.rule else {
            return Vec::new();
        };
        let mut due = Vec::new();
        for (key, slot) in &mut self.slots {
            if slot.pending.is_none() {
                continue;
            }
            slot.prune(now, rule.per);
            if slot.sent.len() < rule.max
                && let Some(event) = slot.pending.take()
            {
                slot.sent.push_back(now);
                due.push((key.clone(), event));
            }
        }
        due
    }
}

/// Sleeps until `deadline`, or forever when there is none.
pub(crate) async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    fn limiter(max: usize, keep_latest: bool) -> Limiter<&'static str, u32> {
        Limiter::new(Some(Window {
            max,
            per: SECOND,
            keep_latest,
        }))
    }

    #[test]
    fn no_rule_passes_everything() {
        let mut l: Limiter<&str, u32> = Limiter::new(None);
        let now = Instant::now();
        assert_eq!(l.offer("a", 1, now), Some(1));
        assert_eq!(l.offer("a", 2, now), Some(2));
        assert_eq!(l.next_due(), None);
    }

    #[test]
    fn over_the_limit_is_dropped_per_key() {
        let mut l = limiter(2, false);
        let t0 = Instant::now();
        assert_eq!(l.offer("a", 1, t0), Some(1));
        assert_eq!(l.offer("a", 2, t0), Some(2));
        assert_eq!(l.offer("a", 3, t0), None);
        // Another key has its own window.
        assert_eq!(l.offer("b", 4, t0), Some(4));
        // Nothing is held.
        assert_eq!(l.next_due(), None);
        // The window slides: one second after the first, one more fits.
        assert_eq!(l.offer("a", 5, t0 + SECOND), Some(5));
    }

    #[test]
    fn keep_latest_holds_the_newest_until_the_window_reopens() {
        let mut l = limiter(1, true);
        let t0 = Instant::now();
        assert_eq!(l.offer("a", 1, t0), Some(1));
        assert_eq!(l.offer("a", 2, t0), None);
        assert_eq!(l.offer("a", 3, t0 + SECOND / 2), None);
        assert_eq!(l.next_due(), Some(t0 + SECOND));
        assert!(l.take_due(t0 + SECOND / 2).is_empty());
        assert_eq!(l.take_due(t0 + SECOND), vec![("a", 3)]);
        // The held event counted: the next one waits again.
        assert_eq!(l.next_due(), None);
        assert_eq!(l.offer("a", 4, t0 + SECOND), None);
        assert_eq!(l.next_due(), Some(t0 + 2 * SECOND));
    }

    #[test]
    fn a_newer_event_in_an_open_window_supersedes_the_held_one() {
        let mut l = limiter(1, true);
        let t0 = Instant::now();
        l.offer("a", 1, t0);
        l.offer("a", 2, t0);
        assert_eq!(l.offer("a", 3, t0 + SECOND), Some(3));
        assert!(l.take_due(t0 + 2 * SECOND).is_empty());
    }
}
