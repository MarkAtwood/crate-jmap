//! Server-Sent Events (SSE) endpoint per RFC 8620 §7.3 (bd:JMAP-cf7p.4).
//!
//! Wires `GET /events` as a `text/event-stream` resource that pushes
//! [RFC 8620 §7.1] `StateChange` events to long-running HTTP clients.
//! Each event names the JMAP type(s) whose `/get` state token has
//! advanced since the previous emit; clients use the new tokens to
//! decide whether to issue a `/changes` request.
//!
//! # Polling vs. signalling
//!
//! Production JMAP servers wire push from the storage layer: a write
//! that mutates `Space` (etc.) signals a state-change condvar, an SSE
//! task wakes up, builds a `StateChange` object, pushes. The testjig's
//! 8 reference [`MemoryBackend`]s do not currently expose such a
//! subscribe API; this slice (bd:JMAP-cf7p.4) ships a tight polling
//! loop instead. A follow-up bead can replace the polling task with
//! a proper signal when one of the [`MemoryBackend`]s grows the
//! plumbing (bd:JMAP-c4hr).
//!
//! The polling tick is currently fixed at the module-private
//! `POLL_INTERVAL` (200 ms). Smaller values reduce latency at the
//! cost of more CPU; the testjig's
//! single-account, single-user posture makes the steady-state cost
//! negligible.
//!
//! [RFC 8620 §7.1]: https://www.rfc-editor.org/rfc/rfc8620.html#section-7.1
//! [`MemoryBackend`]: jmap_mail_server::memory::MemoryBackend
//!
//! # Query parameters
//!
//! Per RFC 8620 §7.3 the URL template carries three variables:
//!
//! | Param | Required | Testjig default | Meaning |
//! |-------|----------|-----------------|---------|
//! | `types` | yes (template) | `*` | comma-separated type names or `*` |
//! | `closeafter` | yes (template) | `no` | `state` (close after first state event) or `no` |
//! | `ping` | yes (template) | `0` | seconds between `ping` events; `0` disables |
//!
//! The testjig accepts missing query parameters and falls back to the
//! defaults above — strictly per spec the URL template substitution
//! always produces a fully-populated URL, but the JmapClient in the
//! workspace's `jmap-base-client` ships its own SSE wrapper that
//! pre-fills these. Accepting the missing case lets curl-driven
//! smoke testing work without surface friction.
//!
//! # Authentication
//!
//! `GET /events` is gated behind the same [`crate::auth`]
//! bearer-token middleware as every other route. Browsers cannot set
//! arbitrary headers on EventSource connections, so the auth layer
//! also honors `?token=<token>`; see [`crate::auth`] for the full
//! middleware contract.
//!
//! # Last-Event-ID replay
//!
//! RFC 8620 §7.3 specifies that the server SHOULD honor a
//! `Last-Event-ID` header on reconnect by replaying any state changes
//! the client missed. The testjig MVP does not implement replay —
//! clients that reconnect after a missed change must issue a
//! `Foo/changes` request to catch up. Tracked for a follow-up
//! bead (bd:JMAP-c4hr).

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Query, State as AxumState},
    response::sse::{Event, Sse},
};
use jmap_server::JmapBackend;
use jmap_types::Id;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::http::{AppState, AppStateInner};
use crate::session;

/// How often the polling task wakes up to snapshot per-type state
/// tokens from the [`MemoryBackend`]s and compare against the
/// previous snapshot.
///
/// Trade-off: lower values reduce push latency at the cost of CPU.
/// 200 ms is short enough that integration tests do not have to wait
/// long for a Space/set update to surface, and long enough that the
/// steady-state CPU cost (one mutex acquisition per backend per type
/// per tick) is invisible.
///
/// Production push wiring would replace this with a condvar signal
/// from the backends; see the module-level docs.
///
/// Visible to `crate::ws` so the WebSocket push poller uses the same
/// cadence as the SSE poller.
///
/// [`MemoryBackend`]: jmap_mail_server::memory::MemoryBackend
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Bound on the mpsc channel between the polling task and the SSE
/// response stream.
///
/// Sized for the typical case where a client makes one or two writes
/// per second and reads as fast as the OS can forward bytes. A backed-up
/// receiver (slow consumer / wedged TCP) will cause the producer to
/// `await` on `send`, which is acceptable — the StateChange semantics
/// per RFC 8620 §7 explicitly tolerate dropped events because the next
/// `/get` will resync.
const SSE_CHANNEL_BOUND: usize = 64;

/// Minimum ping interval (seconds) the testjig will honor.
///
/// RFC 8620 §7.3 says servers MAY clamp the requested interval but
/// MUST NOT have a minimum higher than 30. 1 second is a deliberately
/// permissive choice; production servers running into thundering-herd
/// reconnects would pick a higher minimum.
const PING_MIN_SECS: u64 = 1;

/// Maximum ping interval (seconds) the testjig will honor.
///
/// RFC 8620 §7.3 says the maximum MUST NOT be less than 300. We pick
/// a day to make the ping schedule effectively client-driven.
const PING_MAX_SECS: u64 = 86_400;

/// Parsed query parameters for `GET /events`.
///
/// All fields are optional so unit tests can hit the endpoint with a
/// bare path; the request-time decoder applies the RFC 8620 §7.3
/// defaults documented at the module level.
#[derive(Debug, Default, Deserialize)]
pub struct EventQuery {
    /// `types=<list>` query parameter. RFC 8620 §7.3 accepts either a
    /// comma-separated list of type names (e.g. "Email,Mailbox") or
    /// the single character "*" for "all types".
    ///
    /// Field-level option so a missing query param surfaces as `None`
    /// here; the `TypesFilter::from_query` parser interprets `None`
    /// as "wildcard".
    #[serde(default)]
    pub types: Option<String>,

    /// `closeafter=<mode>` query parameter. RFC 8620 §7.3 accepts
    /// "state" (close after first state event) or "no" (persistent).
    /// Missing value defaults to "no".
    #[serde(default)]
    pub closeafter: Option<String>,

    /// `ping=<secs>` query parameter. `0` (the default) disables
    /// pings entirely; positive values are clamped to the
    /// testjig's `[PING_MIN_SECS, PING_MAX_SECS]` range (currently
    /// 1 second to 1 day).
    #[serde(default)]
    pub ping: Option<u64>,
}

/// Configured ping behavior derived from [`EventQuery::ping`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PingMode {
    /// `ping=0` — no ping events emitted.
    Disabled,
    /// `ping=N` with N clamped to `[PING_MIN_SECS, PING_MAX_SECS]`.
    Every(Duration),
}

impl PingMode {
    fn from_query(secs: Option<u64>) -> Self {
        match secs.unwrap_or(0) {
            0 => PingMode::Disabled,
            n => {
                let clamped = n.clamp(PING_MIN_SECS, PING_MAX_SECS);
                PingMode::Every(Duration::from_secs(clamped))
            }
        }
    }

    /// The reported interval in the `ping` event's `interval` field.
    ///
    /// RFC 8620 §7.3: "The data for the ping event MUST be a JSON
    /// object containing an 'interval' property". The reported value
    /// MAY differ from the client-requested value when the server
    /// clamps; this returns the actually-honored interval so clients
    /// can detect clamping.
    fn reported_interval(self) -> u64 {
        match self {
            PingMode::Disabled => 0,
            PingMode::Every(d) => d.as_secs(),
        }
    }
}

/// Type-name filter derived from [`EventQuery::types`] (SSE) or
/// [`crate::ws::PushEnable::data_types`] (WebSocket).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TypesFilter {
    /// `types=*` (or missing) — every type passes through.
    Wildcard,
    /// `types=Email,Mailbox` — only the named types pass through.
    Only(BTreeSet<String>),
}

impl TypesFilter {
    /// Parse the SSE `types` query parameter per RFC 8620 §7.3.
    ///
    /// `None` and empty string fall back to [`TypesFilter::Wildcard`]
    /// — strictly the URL template always populates `types`, but the
    /// testjig accepts the missing case for ergonomic curl testing.
    /// A literal `*` is wildcard; any other value is parsed as a
    /// comma-separated list. Empty entries (e.g. trailing comma) are
    /// dropped silently rather than producing a never-matching entry.
    fn from_query(types: Option<&str>) -> Self {
        match types.unwrap_or("") {
            "" | "*" => TypesFilter::Wildcard,
            s => TypesFilter::Only(
                s.split(',')
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ),
        }
    }

    /// Construct a [`TypesFilter`] from an RFC 8887 §4.3.5.2
    /// `dataTypes` array. `None` (the spec-defined "all types"
    /// sentinel) and an empty list both map to
    /// [`TypesFilter::Wildcard`].
    pub(crate) fn from_data_types(types: Option<Vec<String>>) -> Self {
        match types {
            None => TypesFilter::Wildcard,
            Some(v) if v.is_empty() => TypesFilter::Wildcard,
            Some(v) => TypesFilter::Only(v.into_iter().collect()),
        }
    }

    /// Whether the given JMAP type name should be reported in
    /// StateChange events to this client.
    pub(crate) fn admits(&self, type_name: &str) -> bool {
        match self {
            TypesFilter::Wildcard => true,
            TypesFilter::Only(set) => set.contains(type_name),
        }
    }
}

/// Whether to terminate the stream after the first state event.
///
/// Derived from the `closeafter` query parameter per RFC 8620 §7.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseAfter {
    /// `closeafter=state` — close the response after the first
    /// state event the client sees.
    State,
    /// `closeafter=no` (default) — persistent connection.
    No,
}

impl CloseAfter {
    fn from_query(value: Option<&str>) -> Self {
        match value.unwrap_or("no") {
            "state" => CloseAfter::State,
            _ => CloseAfter::No,
        }
    }
}

/// `GET /events` — RFC 8620 §7.3 EventSource resource.
///
/// Parses the query parameters, captures a baseline snapshot of every
/// known type's state token (so the very first poll cannot fire a
/// spurious state event), then spawns a poller that emits one SSE
/// event per state delta and (optionally) one ping event per
/// configured interval.
///
/// The returned [`Sse<ReceiverStream<...>>`] is wired into axum's
/// response pipeline. When the client disconnects, the receiver is
/// dropped and the polling task's next `send` will fail, terminating
/// the loop and releasing the backend Arc clones.
pub async fn get_events(
    AxumState(state): AxumState<AppState>,
    Query(query): Query<EventQuery>,
) -> Sse<ReceiverStream<Result<Event, Infallible>>> {
    let types_filter = TypesFilter::from_query(query.types.as_deref());
    let ping_mode = PingMode::from_query(query.ping);
    let close_after = CloseAfter::from_query(query.closeafter.as_deref());

    let account = Id::from(session::ACCOUNT_ID);
    let baseline = snapshot_all_states(&state.inner, &account).await;

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(SSE_CHANNEL_BOUND);

    tokio::spawn(poll_loop(
        Arc::clone(&state.inner),
        account,
        baseline,
        types_filter,
        ping_mode,
        close_after,
        tx,
    ));

    Sse::new(ReceiverStream::new(rx))
}

/// The poller task. Runs until the receiver is dropped or until
/// [`CloseAfter::State`] fires after the first state event.
///
/// Loop body uses [`tokio::select!`] across:
///
/// - A 200 ms poll tick that diffs the current snapshot against
///   `previous` and emits a `state` event if any tracked type's
///   token changed.
/// - The configured ping interval (when not [`PingMode::Disabled`])
///   that emits a `ping` event with the honored interval.
///
/// All channel-send failures break the loop — the receiver is gone
/// (client disconnected, or `Sse` dropped). This is the normal
/// shutdown path; no error is propagated.
#[allow(clippy::too_many_arguments)]
async fn poll_loop(
    state: Arc<AppStateInner>,
    account: Id,
    baseline: BTreeMap<&'static str, String>,
    types_filter: TypesFilter,
    ping_mode: PingMode,
    close_after: CloseAfter,
    tx: mpsc::Sender<Result<Event, Infallible>>,
) {
    let mut previous = baseline;
    let mut state_event_id: u64 = 0;
    let mut poll = tokio::time::interval(POLL_INTERVAL);
    // The first `interval` tick fires immediately; skip it so we don't
    // re-snapshot the same baseline we just captured.
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    poll.tick().await;

    // Ping ticker only matters when enabled; when disabled, we
    // construct a never-resolving future via `pending`.
    let mut ping = match ping_mode {
        PingMode::Every(d) => Some(tokio::time::interval(d)),
        PingMode::Disabled => None,
    };
    if let Some(p) = ping.as_mut() {
        // Don't fire a ping immediately on connect; wait one interval.
        p.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        p.tick().await;
    }

    loop {
        tokio::select! {
            _ = poll.tick() => {
                let current = snapshot_all_states(&state, &account).await;
                let changes = diff_snapshots(&previous, &current, &types_filter);
                previous = current;
                if !changes.is_empty() {
                    state_event_id = state_event_id.wrapping_add(1);
                    let event = build_state_event(&account, &changes, state_event_id);
                    if tx.send(Ok(event)).await.is_err() {
                        return; // receiver gone
                    }
                    if let CloseAfter::State = close_after {
                        return;
                    }
                }
            }
            _ = async {
                match ping.as_mut() {
                    Some(p) => { p.tick().await; }
                    None => std::future::pending().await,
                }
            } => {
                let event = build_ping_event(ping_mode.reported_interval());
                if tx.send(Ok(event)).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Snapshot every well-known JMAP type's state token for the given
/// account across all 8 reference backends.
///
/// Returns a `(type_name, state)` map keyed by the JmapObject
/// `TYPE_NAME` constant so the result is stable across compiler
/// versions / hashing schemes. Backends that error out for a given
/// type are silently skipped — for the testjig's in-memory backends
/// this only happens when an account does not exist on a backend
/// that requires explicit registration; the SSE consumer can't
/// distinguish a missing entry from a never-changed entry either way.
///
/// Visible to `crate::ws` because the WebSocket push handler
/// (bd:JMAP-cf7p.5) reuses the same polling primitive.
pub(crate) async fn snapshot_all_states(
    state: &AppStateInner,
    account: &Id,
) -> BTreeMap<&'static str, String> {
    let mut out: BTreeMap<&'static str, String> = BTreeMap::new();

    macro_rules! poll_type {
        ($backend:expr, $ty:path) => {{
            type T = $ty;
            if let Ok(s) = $backend.get_state::<T>(&(), account).await {
                out.insert(<T as jmap_server::JmapObject>::TYPE_NAME, s.into_inner());
            }
        }};
    }

    // Mail (RFC 8621).
    poll_type!(state.mail, jmap_mail_types::Mailbox);
    poll_type!(state.mail, jmap_mail_types::Thread);
    poll_type!(state.mail, jmap_mail_types::Email);
    poll_type!(state.mail, jmap_mail_types::Identity);
    poll_type!(state.mail, jmap_mail_types::EmailSubmission);
    poll_type!(state.mail, jmap_mail_types::VacationResponse);

    // Chat (draft-atwood-jmap-chat-00).
    poll_type!(state.chat, jmap_chat_types::Chat);
    poll_type!(state.chat, jmap_chat_types::Message);
    poll_type!(state.chat, jmap_chat_types::Space);
    poll_type!(state.chat, jmap_chat_types::ChatContact);
    poll_type!(state.chat, jmap_chat_types::ReadPosition);
    poll_type!(state.chat, jmap_chat_types::CustomEmoji);
    poll_type!(state.chat, jmap_chat_types::SpaceInvite);
    poll_type!(state.chat, jmap_chat_types::SpaceBan);
    poll_type!(state.chat, jmap_chat_types::PresenceStatus);

    // Calendars (draft-ietf-jmap-calendars).
    poll_type!(state.calendars, jmap_calendars_types::Calendar);
    poll_type!(state.calendars, jmap_calendars_types::CalendarEvent);
    poll_type!(state.calendars, jmap_calendars_types::ParticipantIdentity);

    // Tasks (draft-ietf-jmap-tasks).
    poll_type!(state.tasks, jmap_tasks_types::Task);
    poll_type!(state.tasks, jmap_tasks_types::TaskList);

    // Contacts (draft-ietf-jmap-contacts).
    poll_type!(state.contacts, jmap_contacts_types::AddressBook);
    poll_type!(state.contacts, jmap_contacts_types::ContactCard);

    // FileNode (draft-atwood-jmap-chat-filenode-00).
    poll_type!(state.filenode, jmap_filenode_types::FileNode);

    // Sharing (RFC 9670).
    poll_type!(state.sharing, jmap_sharing_types::Principal);

    // Metadata (draft-ietf-jmap-metadata).
    poll_type!(state.metadata, jmap_metadata_types::Metadata);

    out
}

/// Compute the per-type StateChange map between two snapshots.
///
/// Filters by `types_filter` so a client that only subscribed to
/// `Email` does not see a `Space` change. Returns entries where the
/// new state differs from the old state for that type; types that
/// appear in only one snapshot are also reported (using whichever
/// value exists).
///
/// Visible to `crate::ws` so the WebSocket push handler can reuse
/// the same diff algorithm.
pub(crate) fn diff_snapshots(
    previous: &BTreeMap<&'static str, String>,
    current: &BTreeMap<&'static str, String>,
    types_filter: &TypesFilter,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (type_name, new_state) in current {
        if !types_filter.admits(type_name) {
            continue;
        }
        match previous.get(type_name) {
            Some(old) if old == new_state => {}
            _ => {
                out.insert((*type_name).to_owned(), new_state.clone());
            }
        }
    }
    out
}

/// Build the `state` SSE event carrying an RFC 8620 §7.1 StateChange
/// JSON object.
///
/// The event id is a monotonic counter rather than the
/// "encodes the entire server state" guidance from RFC 8620 §7.3 —
/// the testjig does not implement Last-Event-ID replay (tracked at
/// bd:JMAP-c4hr), so the id is decorative. A future signal-driven
/// implementation can swap this for a real snapshot id.
fn build_state_event(account: &Id, changes: &BTreeMap<String, String>, id: u64) -> Event {
    let body = json!({
        "@type": "StateChange",
        "changed": {
            account.as_ref(): changes,
        }
    });
    Event::default()
        .event("state")
        .id(id.to_string())
        .data(body.to_string())
}

/// Build a `ping` SSE event per RFC 8620 §7.3.
///
/// The `interval` field reports the actually-honored interval (which
/// may differ from the client's requested value when the server
/// clamped). The event MUST NOT carry an id per the spec; we do not
/// call [`Event::id`] here.
fn build_ping_event(interval_secs: u64) -> Event {
    Event::default()
        .event("ping")
        .data(json!({ "interval": interval_secs }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: RFC 8620 §7.3 — missing `types` query falls back to
    /// "wildcard" semantics ("*"). The testjig accepts the missing
    /// case so curl smoke-testing works.
    #[test]
    fn types_filter_missing_is_wildcard() {
        assert_eq!(TypesFilter::from_query(None), TypesFilter::Wildcard);
    }

    /// Oracle: RFC 8620 §7.3 — `types=*` is wildcard.
    #[test]
    fn types_filter_star_is_wildcard() {
        assert_eq!(TypesFilter::from_query(Some("*")), TypesFilter::Wildcard);
    }

    /// Oracle: RFC 8620 §7.3 — a comma-separated list of type names
    /// filters the StateChange map to only those types. Whitespace
    /// trimming is permissive ("Email, Mailbox" works).
    #[test]
    fn types_filter_comma_list_is_only() {
        let f = TypesFilter::from_query(Some("Email, Mailbox"));
        let TypesFilter::Only(set) = f else {
            panic!("expected Only");
        };
        assert!(set.contains("Email"));
        assert!(set.contains("Mailbox"));
        assert_eq!(set.len(), 2);
    }

    /// Oracle: empty / trailing-comma entries are dropped silently
    /// rather than producing a never-matching empty type-name in the
    /// filter set.
    #[test]
    fn types_filter_drops_empty_entries() {
        let TypesFilter::Only(set) = TypesFilter::from_query(Some("Email,,Mailbox,")) else {
            panic!("expected Only");
        };
        assert_eq!(set.len(), 2);
    }

    /// Oracle: wildcard admits every type name.
    #[test]
    fn types_filter_wildcard_admits_anything() {
        assert!(TypesFilter::Wildcard.admits("Email"));
        assert!(TypesFilter::Wildcard.admits("MadeUpType"));
    }

    /// Oracle: a typed filter rejects names not in the set.
    #[test]
    fn types_filter_only_rejects_unlisted() {
        let f = TypesFilter::from_query(Some("Email"));
        assert!(f.admits("Email"));
        assert!(!f.admits("Mailbox"));
    }

    /// Oracle: RFC 8620 §7.3 — `ping=0` disables pings.
    #[test]
    fn ping_zero_is_disabled() {
        assert_eq!(PingMode::from_query(Some(0)), PingMode::Disabled);
    }

    /// Oracle: missing `ping` query defaults to disabled.
    #[test]
    fn ping_missing_is_disabled() {
        assert_eq!(PingMode::from_query(None), PingMode::Disabled);
    }

    /// Oracle: RFC 8620 §7.3 — positive ping values are honored.
    /// 5 seconds is comfortably inside the testjig's clamp range so
    /// it round-trips unchanged.
    #[test]
    fn ping_positive_is_every() {
        let mode = PingMode::from_query(Some(5));
        assert_eq!(mode, PingMode::Every(Duration::from_secs(5)));
        assert_eq!(mode.reported_interval(), 5);
    }

    /// Oracle: RFC 8620 §7.3 — values below the testjig's minimum
    /// (`PING_MIN_SECS`) are clamped up. The client cannot starve
    /// the server with very low intervals.
    #[test]
    fn ping_clamps_to_min() {
        // PING_MIN_SECS is 1; values can only be < 1 if they are 0,
        // which is the Disabled path — confirm 1 is the actual floor.
        assert_eq!(
            PingMode::from_query(Some(1)).reported_interval(),
            PING_MIN_SECS
        );
    }

    /// Oracle: RFC 8620 §7.3 — values above the testjig's maximum
    /// (`PING_MAX_SECS`) are clamped down. Spec requires the max
    /// MUST NOT be less than 300; the testjig uses one day.
    #[test]
    fn ping_clamps_to_max() {
        let mode = PingMode::from_query(Some(u64::MAX));
        assert_eq!(mode.reported_interval(), PING_MAX_SECS);
    }

    /// Oracle: RFC 8620 §7.3 — `closeafter=state` is recognised.
    #[test]
    fn closeafter_state_is_close() {
        assert_eq!(CloseAfter::from_query(Some("state")), CloseAfter::State);
    }

    /// Oracle: RFC 8620 §7.3 — `closeafter=no` is persistent.
    #[test]
    fn closeafter_no_is_persistent() {
        assert_eq!(CloseAfter::from_query(Some("no")), CloseAfter::No);
    }

    /// Oracle: missing or unrecognised closeafter values fall back to
    /// "no" rather than producing an error. The URL template usually
    /// fills the value in; tolerating a missing one keeps curl-driven
    /// smoke testing ergonomic.
    #[test]
    fn closeafter_missing_or_bogus_is_persistent() {
        assert_eq!(CloseAfter::from_query(None), CloseAfter::No);
        assert_eq!(CloseAfter::from_query(Some("garbage")), CloseAfter::No);
    }

    /// Oracle: snapshot diffs surface the new state for types whose
    /// token changed AND for types that newly appeared. Stable types
    /// (same token before and after) are omitted.
    #[test]
    fn diff_surfaces_changed_and_new_types() {
        let mut prev = BTreeMap::new();
        prev.insert("Email", "0".to_owned());
        prev.insert("Mailbox", "5".to_owned());

        let mut cur = BTreeMap::new();
        cur.insert("Email", "1".to_owned()); // changed
        cur.insert("Mailbox", "5".to_owned()); // unchanged → omitted
        cur.insert("Space", "1".to_owned()); // new → surfaced

        let out = diff_snapshots(&prev, &cur, &TypesFilter::Wildcard);
        assert_eq!(out.get("Email").map(String::as_str), Some("1"));
        assert!(!out.contains_key("Mailbox"));
        assert_eq!(out.get("Space").map(String::as_str), Some("1"));
    }

    /// Oracle: a non-wildcard `types` filter restricts the diff to
    /// the requested type names. Spec §7.3: "The server MUST only
    /// push changes for the types in this list".
    #[test]
    fn diff_respects_types_filter() {
        let mut prev = BTreeMap::new();
        prev.insert("Email", "0".to_owned());
        prev.insert("Space", "0".to_owned());

        let mut cur = BTreeMap::new();
        cur.insert("Email", "1".to_owned());
        cur.insert("Space", "1".to_owned());

        let filter = TypesFilter::Only(["Email".to_owned()].into_iter().collect());
        let out = diff_snapshots(&prev, &cur, &filter);
        assert!(out.contains_key("Email"));
        assert!(!out.contains_key("Space"));
    }

    /// Oracle: when no types changed (identical snapshots), the diff
    /// is empty. The poll loop uses this to decide whether to emit a
    /// state event at all.
    #[test]
    fn diff_empty_when_unchanged() {
        let mut prev = BTreeMap::new();
        prev.insert("Email", "0".to_owned());
        let cur = prev.clone();
        let out = diff_snapshots(&prev, &cur, &TypesFilter::Wildcard);
        assert!(out.is_empty());
    }

    /// Oracle: RFC 8620 §7.1 — StateChange shape:
    /// `{"@type":"StateChange","changed":{"<accountId>":{...}}}`.
    /// The id field of the SSE event is the monotonic counter the
    /// loop maintains.
    #[tokio::test]
    async fn state_event_shape_matches_rfc_8620_section_7_1() {
        let account = Id::from("acct-1");
        let mut changes = BTreeMap::new();
        changes.insert("Space".to_owned(), "42".to_owned());
        let event = build_state_event(&account, &changes, 7);
        let serialized = render_single_event(event).await;
        assert!(
            serialized.contains("event: state"),
            "expected 'event: state' line, got:\n{serialized}"
        );
        assert!(
            serialized.contains("id: 7"),
            "expected 'id: 7' line, got:\n{serialized}"
        );
        assert!(
            serialized.contains("\"@type\":\"StateChange\""),
            "expected StateChange tag in data, got:\n{serialized}"
        );
        assert!(
            serialized.contains("\"acct-1\""),
            "expected accountId in changed map, got:\n{serialized}"
        );
        assert!(
            serialized.contains("\"Space\":\"42\""),
            "expected Space→state in changed map, got:\n{serialized}"
        );
    }

    /// Oracle: RFC 8620 §7.3 — the ping event carries an `interval`
    /// field and MUST NOT include an id line.
    #[tokio::test]
    async fn ping_event_carries_interval_and_no_id() {
        let event = build_ping_event(30);
        let serialized = render_single_event(event).await;
        assert!(
            serialized.contains("event: ping"),
            "expected 'event: ping' line, got:\n{serialized}"
        );
        assert!(
            serialized.contains("\"interval\":30"),
            "expected interval field in data, got:\n{serialized}"
        );
        // SSE field lines are `<field>: <value>`. The state event uses
        // `id:` for its event id; the ping event MUST NOT.
        assert!(
            !serialized.contains("\nid:"),
            "ping events MUST NOT set an event id per RFC 8620 §7.3, got:\n{serialized}"
        );
    }

    /// Helper: drive a single [`Event`] through axum's `Sse` response
    /// pipeline and return the rendered SSE wire-format bytes as a
    /// UTF-8 string.
    ///
    /// axum does not expose `Event::finalize` publicly, so the only
    /// way to inspect the rendered bytes is through the same
    /// `IntoResponse` → body-collect path the production code uses.
    /// This keeps the test honest — it asserts on the bytes a real
    /// SSE client would observe, not on a stand-in.
    async fn render_single_event(event: Event) -> String {
        use axum::response::IntoResponse;
        use http_body_util::BodyExt;

        let stream = tokio_stream::iter([Ok::<_, std::convert::Infallible>(event)]);
        let response = Sse::new(stream).into_response();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("SSE body collect")
            .to_bytes();
        String::from_utf8(bytes.to_vec()).expect("SSE bytes must be UTF-8")
    }
}
