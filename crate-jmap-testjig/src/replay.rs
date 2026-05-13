//! Shared state-change event log for SSE Last-Event-ID replay
//! (bd:JMAP-cf7p.10) and WebSocket pushState replay (bd:JMAP-cf7p.12).
//!
//! # Architecture
//!
//! The log is **producer-driven**:
//!
//! - [`crate::http::AppStateInner::notify_state_changed`] is the only
//!   producer. After every dispatcher invocation it calls
//!   [`StateChangeLog::record_from_snapshot`] with a fresh per-type
//!   state-token snapshot. The log diffs the snapshot against its
//!   internal canonical state, assigns a fresh event id to each
//!   `(type, new_state_token)` change from a process-global
//!   monotonic counter, and appends the entry to that type's ring
//!   buffer (capped at [`RING_CAPACITY`] entries).
//! - SSE and WebSocket push subscribers are **consumers**. They never
//!   compute their own diff; they hold a `last_seen_event_id` and call
//!   [`StateChangeLog::events_since`] on every watch-channel wake to
//!   collect every entry with `event_id > last_seen_event_id` filtered
//!   by their subscribed types.
//!
//! On reconnect with a `Last-Event-ID` header (SSE) or a `pushState`
//! field (WS), the subscriber sets its `last_seen_event_id` to the
//! supplied value and the next `events_since` call replays everything
//! the log still retains past that point. If the supplied id is older
//! than the log's oldest retained entry (ring buffer rolled over), the
//! subscriber falls back to the log's current id and the client must
//! `Foo/changes` to catch up — replay-not-possible signalling is
//! handled per-handler (SSE/WS).
//!
//! # Why producer-driven (and not the previous per-subscriber diff)
//!
//! The previous (bd:JMAP-cf7p.9) signal-driven model had each
//! subscriber compute its own diff against its own previous snapshot,
//! and assign per-subscriber monotonic ids starting at 0. That works
//! for live push but breaks replay:
//!
//! 1. The ids are not consistent across subscribers — `Last-Event-ID: 7`
//!    from subscriber A means a different event than `Last-Event-ID: 7`
//!    from subscriber B.
//! 2. Events that happen while *no* subscriber is connected are not
//!    recorded anywhere — there is nothing to replay on reconnect.
//!
//! A producer-driven log fixes both: ids are process-global, and the
//! log records every change regardless of subscriber count.
//!
//! # Capacity
//!
//! Per-type ring buffer of [`RING_CAPACITY`] (1024) entries. That is
//! a Mark-accepted value (cf7p.12 acceptance comment). 1024 is
//! sized for testjig integration testing — production push wiring
//! would size based on expected reconnect window and write rate.
//!
//! # Locking
//!
//! Single [`std::sync::Mutex`] over the whole log. All operations
//! (record, replay, current-id-read) take the same lock briefly. No
//! async work happens inside the lock — `record_from_snapshot`
//! accepts the snapshot as an argument (the async snapshot is done
//! by the caller before entering the lock).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

/// Per-type ring buffer capacity in event entries.
///
/// Mark accepted "1024-event-per-type ring buffer" in the cf7p.12
/// design comment (2026-05-13T17:34Z). Sized for testjig integration
/// testing — production push wiring would size based on expected
/// reconnect window and write rate.
pub(crate) const RING_CAPACITY: usize = 1024;

/// A single recorded state change. The `event_id` is process-global
/// and strictly monotonic; the `type_name` is the JMAP wire type name
/// (e.g. `"Email"`, `"Space"`); the `state_token` is the type's
/// `/get` state at the moment the change was recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StateChangeEntry {
    pub event_id: u64,
    pub type_name: &'static str,
    pub state_token: String,
}

/// Producer-driven event log.
///
/// Thread-safe via a single internal [`Mutex`]. Cheap to construct
/// (no allocation until the first `record_from_snapshot`).
pub(crate) struct StateChangeLog {
    inner: Mutex<Inner>,
}

struct Inner {
    /// Next event id to assign. Starts at 1; 0 is reserved as the
    /// "no events seen yet" sentinel for [`Inner::initial_subscriber_id`]
    /// and for the SSE/WS reconnect paths.
    next_event_id: u64,

    /// Canonical per-type state snapshot. Updated atomically with
    /// each `record_from_snapshot` so consecutive diffs see a
    /// consistent baseline.
    canonical: BTreeMap<&'static str, String>,

    /// Per-type ring buffer of recorded entries. Newest at the back;
    /// pop-front on overflow.
    rings: BTreeMap<&'static str, VecDeque<StateChangeEntry>>,
}

impl StateChangeLog {
    /// Construct an empty log with [`Inner::next_event_id`] at 1.
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                next_event_id: 1,
                canonical: BTreeMap::new(),
                rings: BTreeMap::new(),
            }),
        }
    }

    /// Diff `current` against the log's canonical snapshot, assign
    /// fresh event ids to changes, append them to the per-type rings,
    /// update the canonical snapshot, and return the recorded entries
    /// (caller may use them e.g. to emit immediately to a live
    /// subscriber's stream — though subscribers typically wake on
    /// the watch channel and pull via [`events_since`]).
    ///
    /// A type whose new state equals its canonical state is not
    /// recorded. A type that newly appears in `current` (not present
    /// in canonical) IS recorded. A type that disappears from
    /// `current` but exists in canonical is left alone — the
    /// dispatcher snapshot is the snapshot of what the get_state
    /// calls returned, and absence-from-snapshot means "the backend
    /// failed to report" (typically an unknown account on a
    /// per-backend register-explicitly path), which is not a real
    /// "type went away" event.
    pub(crate) fn record_from_snapshot(
        &self,
        current: BTreeMap<&'static str, String>,
    ) -> Vec<StateChangeEntry> {
        let mut inner = self
            .inner
            .lock()
            .expect("StateChangeLog mutex poisoned by another thread panicking with lock held");

        let mut recorded = Vec::new();
        for (type_name, new_state) in &current {
            let changed = match inner.canonical.get(*type_name) {
                Some(old) => old != new_state,
                None => true, // newly appeared
            };
            if !changed {
                continue;
            }
            let event_id = inner.next_event_id;
            inner.next_event_id = inner.next_event_id.saturating_add(1);
            let entry = StateChangeEntry {
                event_id,
                type_name,
                state_token: new_state.clone(),
            };
            let ring = inner.rings.entry(type_name).or_default();
            if ring.len() >= RING_CAPACITY {
                ring.pop_front();
            }
            ring.push_back(entry.clone());
            recorded.push(entry);
        }

        // Replace canonical with current (carrying over any types
        // that vanished from `current` — see method docstring).
        for (type_name, new_state) in current {
            inner.canonical.insert(type_name, new_state);
        }

        recorded
    }

    /// Return every recorded entry with `event_id > last_seen`,
    /// filtered to types `wanted.admits(type_name)`. Entries are
    /// returned in ascending `event_id` order across the per-type
    /// rings.
    ///
    /// Used by SSE / WS subscribers on every watch-channel wake to
    /// collect the events to emit since their last position.
    pub(crate) fn events_since(
        &self,
        last_seen: u64,
        wanted: &TypeFilter<'_>,
    ) -> Vec<StateChangeEntry> {
        let inner = self
            .inner
            .lock()
            .expect("StateChangeLog mutex poisoned by another thread panicking with lock held");

        let mut out = Vec::new();
        for (type_name, ring) in inner.rings.iter() {
            if !wanted.admits(type_name) {
                continue;
            }
            for entry in ring.iter() {
                if entry.event_id > last_seen {
                    out.push(entry.clone());
                }
            }
        }
        out.sort_by_key(|e| e.event_id);
        out
    }

    /// Return the most-recently-assigned event id (i.e. the largest
    /// `event_id` in any ring), or `0` if the log has never recorded
    /// an event.
    ///
    /// Used as the initial `last_seen_event_id` for newly-connected
    /// subscribers that did NOT supply a Last-Event-ID / pushState —
    /// they start at "current state" so the first wake will emit
    /// only genuinely-new changes.
    pub(crate) fn current_event_id(&self) -> u64 {
        let inner = self
            .inner
            .lock()
            .expect("StateChangeLog mutex poisoned by another thread panicking with lock held");
        inner.next_event_id.saturating_sub(1)
    }

    /// Return the smallest `event_id` still retained across all
    /// rings, or `0` if the log is empty.
    ///
    /// A subscriber supplying `Last-Event-ID: N` where
    /// `N < oldest_event_id()` has missed events that have rolled
    /// out of the ring; the SSE / WS handler signals replay-failure
    /// per its protocol's conventions.
    pub(crate) fn oldest_event_id(&self) -> u64 {
        let inner = self
            .inner
            .lock()
            .expect("StateChangeLog mutex poisoned by another thread panicking with lock held");
        inner
            .rings
            .values()
            .filter_map(|ring| ring.front().map(|e| e.event_id))
            .min()
            .unwrap_or(0)
    }
}

/// Borrowed view of a type filter for [`StateChangeLog::events_since`].
///
/// Mirrors the shape of [`crate::sse::TypesFilter`] but borrows the
/// underlying type-name set so we can hand it to the log without
/// cloning. The log doesn't depend on `crate::sse` to avoid the cycle
/// the other direction would create.
pub(crate) enum TypeFilter<'a> {
    Wildcard,
    Only(&'a BTreeSet<String>),
}

impl<'a> TypeFilter<'a> {
    pub(crate) fn admits(&self, type_name: &str) -> bool {
        match self {
            TypeFilter::Wildcard => true,
            TypeFilter::Only(set) => set.contains(type_name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(pairs: &[(&'static str, &str)]) -> BTreeMap<&'static str, String> {
        pairs.iter().map(|(t, s)| (*t, s.to_string())).collect()
    }

    /// Oracle: a fresh log starts at `current_event_id() == 0` and
    /// `oldest_event_id() == 0` — the "no events yet" sentinel that
    /// `events_since(0, ...)` returns nothing against.
    #[test]
    fn fresh_log_has_no_events() {
        let log = StateChangeLog::new();
        assert_eq!(log.current_event_id(), 0);
        assert_eq!(log.oldest_event_id(), 0);
        assert!(log.events_since(0, &TypeFilter::Wildcard).is_empty());
    }

    /// Oracle: first recorded snapshot turns every type into a "newly
    /// appeared" event, each with a strictly-increasing event id
    /// starting at 1.
    #[test]
    fn first_record_assigns_ids_from_one() {
        let log = StateChangeLog::new();
        let recorded = log.record_from_snapshot(snap(&[("Email", "a"), ("Mailbox", "x")]));
        assert_eq!(recorded.len(), 2);
        let mut ids: Vec<u64> = recorded.iter().map(|e| e.event_id).collect();
        ids.sort();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(log.current_event_id(), 2);
    }

    /// Oracle: unchanged types are NOT re-recorded. Only the type
    /// whose state advanced gets a fresh id.
    #[test]
    fn unchanged_types_are_not_re_recorded() {
        let log = StateChangeLog::new();
        log.record_from_snapshot(snap(&[("Email", "a"), ("Mailbox", "x")]));
        let recorded = log.record_from_snapshot(snap(&[("Email", "b"), ("Mailbox", "x")]));
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].type_name, "Email");
        assert_eq!(recorded[0].state_token, "b");
        assert_eq!(recorded[0].event_id, 3);
        assert_eq!(log.current_event_id(), 3);
    }

    /// Oracle: `events_since(0, Wildcard)` returns the full retained
    /// history in ascending event-id order.
    #[test]
    fn events_since_zero_returns_full_history() {
        let log = StateChangeLog::new();
        log.record_from_snapshot(snap(&[("Email", "a"), ("Mailbox", "x")]));
        log.record_from_snapshot(snap(&[("Email", "b"), ("Mailbox", "x")]));
        log.record_from_snapshot(snap(&[("Email", "b"), ("Mailbox", "y")]));
        let all = log.events_since(0, &TypeFilter::Wildcard);
        let ids: Vec<u64> = all.iter().map(|e| e.event_id).collect();
        assert_eq!(ids, vec![1, 2, 3, 4]);
    }

    /// Oracle: `events_since(N, ...)` filters to entries with
    /// `event_id > N`. Used by reconnect replay.
    #[test]
    fn events_since_filters_to_strictly_greater() {
        let log = StateChangeLog::new();
        log.record_from_snapshot(snap(&[("Email", "a")]));
        log.record_from_snapshot(snap(&[("Email", "b")]));
        log.record_from_snapshot(snap(&[("Email", "c")]));
        let recent = log.events_since(2, &TypeFilter::Wildcard);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].state_token, "c");
        assert_eq!(recent[0].event_id, 3);
    }

    /// Oracle: a non-wildcard `TypeFilter::Only` restricts the
    /// returned entries to types in the set.
    #[test]
    fn events_since_respects_type_filter() {
        let log = StateChangeLog::new();
        log.record_from_snapshot(snap(&[("Email", "a"), ("Mailbox", "x")]));
        log.record_from_snapshot(snap(&[("Email", "b"), ("Mailbox", "y")]));
        let only_email = BTreeSet::from(["Email".to_string()]);
        let filter = TypeFilter::Only(&only_email);
        let got = log.events_since(0, &filter);
        let types: Vec<&str> = got.iter().map(|e| e.type_name).collect();
        assert_eq!(types, vec!["Email", "Email"]);
    }

    /// Oracle: ring buffer caps at [`RING_CAPACITY`] per type; older
    /// entries roll out FIFO. After `RING_CAPACITY + 5` changes on a
    /// single type, the oldest 5 entries are gone and the rest are
    /// retained in event-id order.
    #[test]
    fn ring_buffer_caps_at_capacity_per_type() {
        let log = StateChangeLog::new();
        for i in 0..(RING_CAPACITY + 5) {
            log.record_from_snapshot(snap(&[("Email", &format!("v{i}"))]));
        }
        let entries = log.events_since(0, &TypeFilter::Wildcard);
        assert_eq!(entries.len(), RING_CAPACITY);
        // Oldest retained entry should have event_id == 6 (first 5
        // were rolled out: 1, 2, 3, 4, 5).
        assert_eq!(entries.first().unwrap().event_id, 6);
        assert_eq!(entries.last().unwrap().event_id, (RING_CAPACITY + 5) as u64);
    }

    /// Oracle: when the requested `last_seen` predates the oldest
    /// retained entry, the caller can detect ring-rollover by
    /// comparing `oldest_event_id() > last_seen`. The log itself
    /// returns whatever it still has — the no-replay signal is
    /// per-protocol (SSE / WS handlers' concern).
    #[test]
    fn oldest_event_id_signals_rollover() {
        let log = StateChangeLog::new();
        for i in 0..(RING_CAPACITY + 5) {
            log.record_from_snapshot(snap(&[("Email", &format!("v{i}"))]));
        }
        // Oldest retained id == 6 (entries 1..=5 rolled out).
        assert_eq!(log.oldest_event_id(), 6);
        assert!(log.oldest_event_id() > 3); // a client at Last-Event-ID:3 missed events
    }

    /// Oracle: each type has its own ring; high write rate on one
    /// type does not evict entries from another type's ring.
    #[test]
    fn rings_are_per_type() {
        let log = StateChangeLog::new();
        log.record_from_snapshot(snap(&[("Mailbox", "x")]));
        // Email churns past its cap.
        for i in 0..(RING_CAPACITY + 5) {
            log.record_from_snapshot(snap(&[("Email", &format!("v{i}"))]));
        }
        // Mailbox's single entry is still retained.
        let mailbox_only = BTreeSet::from(["Mailbox".to_string()]);
        let mailbox_entries = log.events_since(0, &TypeFilter::Only(&mailbox_only));
        assert_eq!(mailbox_entries.len(), 1);
        assert_eq!(mailbox_entries[0].state_token, "x");
        assert_eq!(mailbox_entries[0].event_id, 1);
    }

    /// Oracle: a new type appearing in a later snapshot is recorded
    /// as a "newly appeared" change (not silently ignored). This
    /// matters for backends that only report a type after first use.
    #[test]
    fn newly_appearing_type_is_recorded() {
        let log = StateChangeLog::new();
        log.record_from_snapshot(snap(&[("Email", "a")]));
        let recorded = log.record_from_snapshot(snap(&[("Email", "a"), ("Mailbox", "x")]));
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].type_name, "Mailbox");
        assert_eq!(recorded[0].state_token, "x");
    }
}
