//! Server-Sent Events (SSE) endpoint per RFC 8620 §7.3 (bd:JMAP-cf7p.4).
//!
//! Wires `GET /events` as a `text/event-stream` resource that pushes
//! [RFC 8620 §7.1] `StateChange` events to long-running HTTP clients.
//! Each event names the JMAP type(s) whose `/get` state token has
//! advanced since the previous emit; clients use the new tokens to
//! decide whether to issue a `/changes` request.
//!
//! # Signal-driven push with safety-net polling (bd:JMAP-cf7p.9)
//!
//! Mutations in the testjig only happen through the dispatcher. The
//! `POST /jmap` and WebSocket `Request`-envelope handlers both call
//! `AppStateInner::notify_state_changed` after every successful
//! dispatch (private to `crate::http`). That sends an increment on a
//! [`tokio::sync::watch::Sender<u64>`] that each SSE / WS push loop
//! subscribes to at spawn time.
//!
//! The push loop `select!`s between:
//!
//! - The watch [`Receiver::changed`] future. Fires immediately after
//!   every dispatched request, so a `Foo/set` round-trip → SSE
//!   StateChange latency is bounded by the dispatcher round-trip plus
//!   one task wake (typically <1 ms in-process).
//! - A long-interval safety-net tick at `POLL_INTERVAL` (5 s). The
//!   watch wake covers every in-band mutation path; the timer is a
//!   belt-and-suspenders for the (currently nonexistent) case of an
//!   out-of-band mutation that bypasses the dispatcher, and bounds
//!   the worst-case latency if a wake is somehow lost.
//! - The configured ping interval, when enabled.
//!
//! The watch carrier is `u64` (incrementing counter), not `()`,
//! because [`Receiver::changed`] requires the carrier's seen-version
//! to advance — `send_modify` increments it explicitly. This avoids
//! the `Notify::notify_waiters` race where a wake fired between
//! subscriber spawn and the first `.await` would be lost.
//!
//! [`Receiver::changed`]: tokio::sync::watch::Receiver::changed
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
//! bead (bd:JMAP-cf7p.10).

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

/// Safety-net polling interval for the SSE / WS push loops.
///
/// The push loops are primarily woken by the testjig's
/// `AppStateInner::notify_state_changed` after every dispatcher
/// invocation (bd:JMAP-cf7p.9). This timer is a belt-and-suspenders
/// fallback: it bounds the worst-case wake latency if a
/// watch-channel wake were ever lost, and would surface any
/// (currently nonexistent) out-of-band mutation path that bypasses
/// the dispatcher.
///
/// 5 seconds is short enough that a missed wake produces a noticeable-
/// but-not-broken latency floor and long enough that the steady-state
/// CPU cost (one snapshot per loop per 5 s when idle) is negligible.
/// In practice the watch channel almost always wins the `select!` and
/// the timer rarely fires for an active subscriber.
///
/// Visible to `crate::ws` so the WebSocket push loop uses the same
/// cadence as the SSE poller.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_secs(5);

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

    /// Borrow as a [`crate::replay::TypeFilter`] for passing to the
    /// state log without cloning the underlying type-name set.
    pub(crate) fn as_replay_filter(&self) -> crate::replay::TypeFilter<'_> {
        match self {
            TypesFilter::Wildcard => crate::replay::TypeFilter::Wildcard,
            TypesFilter::Only(set) => crate::replay::TypeFilter::Only(set),
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
    headers: axum::http::HeaderMap,
    Query(query): Query<EventQuery>,
) -> Sse<ReceiverStream<Result<Event, Infallible>>> {
    let types_filter = TypesFilter::from_query(query.types.as_deref());
    let ping_mode = PingMode::from_query(query.ping);
    let close_after = CloseAfter::from_query(query.closeafter.as_deref());
    let last_event_id = parse_last_event_id(&headers);

    let account = Id::from(session::ACCOUNT_ID);

    // Subscribe to state-change wakes BEFORE reading the log's
    // current position so any dispatch that races our setup is
    // captured. The watch channel's seen-version semantics guarantee
    // that a `send_modify` between `subscribe()` and the first
    // `changed()` .await still fires.
    let state_changes = state.inner.subscribe_state_changes();

    // Determine the subscriber's starting position. RFC 8620 §7.3:
    // > When a new connection is made to the event-source endpoint,
    // > a client following the server-sent events specification will
    // > send a Last-Event-ID HTTP header field with the last id it
    // > saw, which the server can use to work out whether the client
    // > has missed some changes. If so, it SHOULD send these changes
    // > immediately on connection.
    //
    // Three cases:
    // 1. No `Last-Event-ID` header: start at the log's current id so
    //    only genuinely-new mutations produce events.
    // 2. `Last-Event-ID: N` where N is within the retained window:
    //    start at N; the first `events_since` call will replay
    //    everything missed.
    // 3. `Last-Event-ID: N` where N predates the oldest retained
    //    entry (ring buffer rolled over): start at the log's current
    //    id and drop a replay-incomplete `state` event so the client
    //    knows to resync via `Foo/changes`. The testjig MVP starts
    //    at current and lets the client notice missing ids; a
    //    production server would emit a `Connection: close` after a
    //    distinct error event.
    let log_current = state.inner.state_log.current_event_id();
    let log_oldest = state.inner.state_log.oldest_event_id();
    let last_seen = match last_event_id {
        Some(n) if log_oldest == 0 || n + 1 >= log_oldest => n,
        Some(_) => log_current,
        None => log_current,
    };

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(SSE_CHANNEL_BOUND);

    tokio::spawn(poll_loop(
        Arc::clone(&state.inner),
        account,
        last_seen,
        state_changes,
        types_filter,
        ping_mode,
        close_after,
        tx,
    ));

    Sse::new(ReceiverStream::new(rx))
}

/// Parse the `Last-Event-ID` request header per RFC 8620 §7.3 /
/// HTML Living Standard server-sent events specification.
///
/// Returns `None` when the header is absent, empty, or not a valid
/// `u64`. RFC 8620 says the field is the last event id the client
/// saw; the testjig assigns u64 ids from
/// [`crate::replay::StateChangeLog`], so anything outside that range
/// is treated as "no last id" and the subscriber starts fresh.
fn parse_last_event_id(headers: &axum::http::HeaderMap) -> Option<u64> {
    let v = headers.get("last-event-id")?.to_str().ok()?;
    let s = v.trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<u64>().ok()
}

/// The poller task. Runs until the receiver is dropped or until
/// [`CloseAfter::State`] fires after the first state event.
///
/// On entry, if `last_seen < log_current` the loop immediately emits
/// any retained replay events past `last_seen` (bd:JMAP-cf7p.10).
/// Then enters the live loop, which uses [`tokio::select!`] across:
///
/// - A state-change watch [`tokio::sync::watch::Receiver::changed`]
///   future that fires immediately after every dispatcher invocation
///   (bd:JMAP-cf7p.9). On wake the loop pulls newly-recorded entries
///   from the [`crate::replay::StateChangeLog`] and emits one
///   `state` event per `(account, changed_map)`.
/// - A safety-net poll tick at `POLL_INTERVAL` (5 s) that performs
///   the same log-pull in case a watch wake is somehow lost.
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
    mut last_seen: u64,
    mut state_changes: tokio::sync::watch::Receiver<u64>,
    types_filter: TypesFilter,
    ping_mode: PingMode,
    close_after: CloseAfter,
    tx: mpsc::Sender<Result<Event, Infallible>>,
) {
    // Mark the current watch version as seen so the first
    // `changed()` waits for a genuine post-spawn mutation rather than
    // firing immediately on the channel's initial value.
    state_changes.mark_unchanged();

    // Replay any retained entries past `last_seen` BEFORE entering
    // the live loop. RFC 8620 §7.3: "the server [...] SHOULD send
    // these changes immediately on connection."
    if let Some(()) = emit_pending(&state, &account, &mut last_seen, &types_filter, &tx).await {
        if let CloseAfter::State = close_after {
            return;
        }
    } else if tx.is_closed() {
        return;
    }

    let mut poll = tokio::time::interval(POLL_INTERVAL);
    // The first `interval` tick fires immediately; skip it.
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
        let wake;
        tokio::select! {
            changed = state_changes.changed() => {
                if changed.is_err() {
                    return;
                }
                wake = true;
            }
            _ = poll.tick() => {
                wake = true;
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
                wake = false;
            }
        }

        if !wake {
            continue;
        }

        let emitted = emit_pending(&state, &account, &mut last_seen, &types_filter, &tx).await;
        if tx.is_closed() {
            return;
        }
        if emitted.is_some() {
            if let CloseAfter::State = close_after {
                return;
            }
        }
    }
}

/// Pull entries past `last_seen` from the state log, emit them as a
/// single `state` SSE event (if non-empty), and advance `last_seen`
/// to the largest emitted id.
///
/// Returns `Some(())` iff an event was emitted. Returns `None` when
/// the log has no new entries OR when the `tx.send` failed (caller
/// checks `tx.is_closed()` to distinguish).
async fn emit_pending(
    state: &Arc<AppStateInner>,
    account: &Id,
    last_seen: &mut u64,
    types_filter: &TypesFilter,
    tx: &mpsc::Sender<Result<Event, Infallible>>,
) -> Option<()> {
    let filter = types_filter.as_replay_filter();
    let entries = state.state_log.events_since(*last_seen, &filter);
    if entries.is_empty() {
        return None;
    }

    // Coalesce per-type: the wire StateChange object carries one
    // state token per type (the latest one), not a per-event list.
    // Walk the entries in ascending id order; the last entry for
    // each type wins.
    let mut latest_per_type: BTreeMap<&'static str, &str> = BTreeMap::new();
    let mut max_id = *last_seen;
    for entry in &entries {
        latest_per_type.insert(entry.type_name, entry.state_token.as_str());
        if entry.event_id > max_id {
            max_id = entry.event_id;
        }
    }
    let changes: BTreeMap<String, String> = latest_per_type
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

    let event = build_state_event(account, &changes, max_id);
    if tx.send(Ok(event)).await.is_err() {
        return None;
    }
    *last_seen = max_id;
    Some(())
}

/// Snapshot every well-known JMAP type's state token for the given
/// account across all 7 reference backends.
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

    // Metadata (draft-ietf-jmap-metadata-02): no standalone object type.
    // Per-type metadata properties are tracked through each extension's
    // own state tokens.

    out
}

/// Build the `state` SSE event carrying an RFC 8620 §7.1 StateChange
/// JSON object.
///
/// The event id is the largest [`crate::replay::StateChangeLog`]
/// event id in the emitted batch. Per RFC 8620 §7.3, this is the
/// `Last-Event-ID` the SSE client will echo on reconnect; the server
/// replays retained entries with `event_id > Last-Event-ID` and then
/// resumes live push (bd:JMAP-cf7p.10).
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

    /// Oracle: RFC 8620 §7.1 — StateChange shape:
    /// `{"@type":"StateChange","changed":{"<accountId>":{...}}}`.
    /// The id field of the SSE event is the largest
    /// [`crate::replay::StateChangeLog`] event id in the batch.
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

    /// Oracle: RFC 8620 §7.3 + HTML SSE spec — the
    /// `Last-Event-ID` header is the last event id the client saw.
    /// Missing or empty values map to `None`.
    #[test]
    fn parse_last_event_id_handles_missing_and_empty() {
        let h = axum::http::HeaderMap::new();
        assert_eq!(parse_last_event_id(&h), None);

        let mut h = axum::http::HeaderMap::new();
        h.insert("Last-Event-ID", "".parse().unwrap());
        assert_eq!(parse_last_event_id(&h), None);

        let mut h = axum::http::HeaderMap::new();
        h.insert("Last-Event-ID", "   ".parse().unwrap());
        assert_eq!(parse_last_event_id(&h), None);
    }

    /// Oracle: a numeric Last-Event-ID parses as the u64 event id.
    #[test]
    fn parse_last_event_id_numeric() {
        let mut h = axum::http::HeaderMap::new();
        h.insert("Last-Event-ID", "42".parse().unwrap());
        assert_eq!(parse_last_event_id(&h), Some(42));
    }

    /// Oracle: non-numeric or out-of-range values fall back to `None`
    /// so the subscriber starts at "current" rather than crashing.
    #[test]
    fn parse_last_event_id_non_numeric_is_none() {
        let mut h = axum::http::HeaderMap::new();
        h.insert("Last-Event-ID", "abc".parse().unwrap());
        assert_eq!(parse_last_event_id(&h), None);

        let mut h = axum::http::HeaderMap::new();
        h.insert("Last-Event-ID", "99999999999999999999999".parse().unwrap());
        assert_eq!(parse_last_event_id(&h), None);
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
