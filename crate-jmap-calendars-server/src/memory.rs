//! In-memory reference implementation of [`CalendarsBackend`](crate::CalendarsBackend).
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`CalendarsBackend`](crate::CalendarsBackend)
//!    trait to study when writing a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Calendars dispatcher
//!    with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and a number
//! of draft-ietf-jmap-calendars-26 edge cases are simplified (see source
//! comments). In particular, recurrence expansion
//! (`CalendarEvent/query` with `expandRecurrences: true`) and iTIP
//! scheduling-message delivery (`sendSchedulingMessages: true`) are not
//! implemented — the trait's default implementations are inherited.
//!
//! # Feature flag and API stability
//!
//! This module is gated behind `feature = "memory"` and is **not** enabled
//! by default. Its public API stability is opt-in: it may break across
//! minor versions while the crate is pre-1.0.
//!
//! # Example
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use jmap_calendars_server::{memory::MemoryBackend, register_calendars_handlers};
//! use jmap_server::Dispatcher;
//!
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_calendars_handlers(&mut dispatcher, Arc::new(MemoryBackend::new()));
//! ```
//!
//! # Concurrency
//!
//! `std::sync::Mutex` is used for simplicity. The `await_holding_lock`
//! clippy lint is enabled module-wide and enforces that no lock guard
//! is held across an `.await`. If a future change requires holding a
//! guard across `.await`, switch to `tokio::sync::Mutex` rather than
//! disabling the lint.
//!
//! # Tracking
//!
//! Greenfield reference impl per Beads issue JMAP-hwdv (epic) and
//! JMAP-hwdv.5 (this crate, mirror of canonical JMAP-hwdv.1 in
//! jmap-mail-server, following the multi-type-store shape established
//! by jmap-chat-server's `MemoryBackend`).

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::backend::{CalendarsBackend, SetDefaultResult};
use crate::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};
// json_merge_patch lives in jmap-server (the shared foundation crate)
// since bd:JMAP-sc1b.103. Every reference backend imports it; the
// canonical RFC 7396 tests live with the function there (including the
// bd:JMAP-sc1b.97 depth-cap and bd:JMAP-sc1b.87 absent-field regression
// tests).
use jmap_server::{json_merge_patch, resolve_query_offset, MergePatchError};
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error type returned by [`MemoryBackend`] operations.
///
/// Carries a human-readable description of the underlying failure
/// (serialization round-trip miss, account-not-registered race, etc.).
/// The description is intended for test failure messages and
/// diagnostic logging; it is not a stable wire-format identifier.
///
/// # Forward compatibility
///
/// This type is `#[non_exhaustive]` and uses a named-field shape so
/// future revisions can add structured context (error kind, source
/// reference, account id, etc.) without a breaking change. Outside-
/// crate construction goes through [`MemoryError::new`]; outside-crate
/// reads go through [`MemoryError::description`].
///
/// Mirrors the canonical jmap-mail-server `MemoryError` shape
/// (bd:JMAP-ic0j.1; canonical reshape in commit 2941c50, propagated
/// to jmap-filenode-server in commit e9b66a3).
#[non_exhaustive]
#[derive(Debug)]
pub struct MemoryError {
    description: String,
}

impl MemoryError {
    /// Construct a [`MemoryError`] from a human-readable description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }

    /// Human-readable description of the underlying failure.
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryBackend error: {}", self.description)
    }
}

impl std::error::Error for MemoryError {}

// ---------------------------------------------------------------------------
// IdFate: per-ID fate tracker for RFC 8620 §5.6 deduplication
// ---------------------------------------------------------------------------

/// Per-ID fate tracker for RFC 8620 §5.6 ID deduplication across change log
/// entries (bd:JMAP-ic0j.50).
///
/// Rules across multiple entries in a single `/changes` window:
/// - created+updated → `Created` (update does not change that the object is
///   new to the client)
/// - created+destroyed → removed from the map (client never knew the object;
///   RFC 8620 §5.2 mandates omission from both `created` and `destroyed`)
/// - updated+destroyed → `Destroyed` (client must remove it)
/// - updated+updated → `Updated` (deduplicated)
///
/// Mirrors the canonical jmap-mail-server [`IdFate`] enum at
/// `crate-jmap-mail-server/src/memory/mod.rs:243`.
#[derive(Debug, Clone)]
enum IdFate {
    Created,
    Updated,
    Destroyed,
}

// ---------------------------------------------------------------------------
// Change log
// ---------------------------------------------------------------------------

/// A change log entry for one state transition.
#[derive(Clone, Debug)]
struct ChangeEntry {
    /// The state counter AFTER this change.
    new_state: u64,
    created: Vec<Id>,
    updated: Vec<Id>,
    destroyed: Vec<Id>,
}

// ---------------------------------------------------------------------------
// Inner state
// ---------------------------------------------------------------------------

/// Per-account auxiliary state that is not keyed by object type.
#[derive(Default, Clone)]
struct AccountAux {
    /// Reverse index: `calendar_id → count of CalendarEvent objects that
    /// reference it`. Used to answer "does Calendar X have any events?" in
    /// `O(1)` for the `calendarHasEvent` destroy rejection
    /// (draft-ietf-jmap-calendars-26 §4.4.1 when `onDestroyRemoveEvents`
    /// is `false`).
    ///
    /// Maintained incrementally by `apply_calendar_event_index_delta`,
    /// called from each CalendarEvent `create_object` / `update_object` /
    /// `destroy_object` (which already have the old + new JSON in hand).
    /// `seed_object` falls back to `recompute_calendar_event_counts` since
    /// it bypasses the trait-impl mutators and the old state is unknown.
    /// Entries reach `0` only briefly during a multi-step update; the
    /// delta helper deletes any entry that returns to `0` so
    /// `counts.contains_key(id)` is a valid "has events?" test.
    ///
    /// bd:JMAP-ic0j.8 — replaces an earlier `HashSet<Id>` that was rebuilt
    /// by a full scan of the CalendarEvent store on every mutation
    /// (`O(N)` per write, `O(N²)` ingest). The reverse-index counter
    /// preserves correctness and makes each mutation `O(k)` in the size
    /// of the changed event's `calendarIds` (typically 1–3).
    calendar_event_counts: HashMap<Id, u32>,
    /// Current default Calendar id, if set
    /// (draft-ietf-jmap-calendars-26 §4.3 `onSuccessSetIsDefault`).
    default_calendar: Option<Id>,
    /// Current default ParticipantIdentity id, if set
    /// (draft-ietf-jmap-calendars-26 §3.3 `onSuccessSetIsDefault`).
    default_participant_identity: Option<Id>,
}

/// Shared inner state, behind Arc<Mutex>.
#[derive(Default)]
struct Inner {
    /// `(type_name, account_id)` → `id → serialized object`
    objects: HashMap<(&'static str, String), HashMap<Id, serde_json::Value>>,
    /// `(type_name, account_id)` → current state counter
    states: HashMap<(&'static str, String), u64>,
    /// `(type_name, account_id)` → ordered change entries
    change_log: HashMap<(&'static str, String), Vec<ChangeEntry>>,
    /// `account_id` → auxiliary per-account state (defaults, derived indexes)
    aux: HashMap<String, AccountAux>,
    /// Explicitly registered account ids (accounts may exist with no objects yet).
    known_accounts: HashSet<String>,
    /// `(type_name, account_id)` → monotonic counter used by
    /// `demo_next_id` to mint deterministic ids without collisions.
    /// Increments on every mint; never decrements on delete (bd:JMAP-ic0j.2,
    /// mirrors bd:JMAP-qz9v.14 in `jmap-contacts-server`).
    ///
    /// Only present in the default (non-`realistic-demo-ids`) mode —
    /// the realistic-demo-ids mode uses a process-global atomic
    /// counter and never touches this field.
    #[cfg(not(feature = "realistic-demo-ids"))]
    next_ids: HashMap<(&'static str, String), u64>,
}

impl Inner {
    fn current_state(&self, type_name: &'static str, account_id: &str) -> u64 {
        self.states
            .get(&(type_name, account_id.to_owned()))
            .copied()
            .unwrap_or(0)
    }

    fn bump_state(&mut self, type_name: &'static str, account_id: &str) -> u64 {
        let entry = self
            .states
            .entry((type_name, account_id.to_owned()))
            .or_insert(0);
        *entry += 1;
        *entry
    }

    fn objects_mut(
        &mut self,
        type_name: &'static str,
        account_id: &str,
    ) -> &mut HashMap<Id, serde_json::Value> {
        self.known_accounts.insert(account_id.to_owned());
        self.objects
            .entry((type_name, account_id.to_owned()))
            .or_default()
    }

    fn objects_ref(
        &self,
        type_name: &'static str,
        account_id: &str,
    ) -> Option<&HashMap<Id, serde_json::Value>> {
        self.objects.get(&(type_name, account_id.to_owned()))
    }

    fn change_log_mut(
        &mut self,
        type_name: &'static str,
        account_id: &str,
    ) -> &mut Vec<ChangeEntry> {
        self.change_log
            .entry((type_name, account_id.to_owned()))
            .or_default()
    }

    fn aux_mut(&mut self, account_id: &str) -> &mut AccountAux {
        self.known_accounts.insert(account_id.to_owned());
        self.aux.entry(account_id.to_owned()).or_default()
    }

    fn aux_ref(&self, account_id: &str) -> Option<&AccountAux> {
        self.aux.get(account_id)
    }

    /// Rebuild `calendar_event_counts` for the given account from scratch
    /// by scanning the CalendarEvent store.
    ///
    /// `O(N)` in the number of events. Reserved for `seed_object`, which
    /// bypasses the trait-impl mutators and therefore has no
    /// before/after pair to feed `apply_calendar_event_index_delta`. The
    /// counter map is built from an iterator chain that takes only an
    /// immutable borrow of `self.objects` so the subsequent
    /// `self.aux_mut(account_id)` call is free to take the mutable borrow.
    /// Avoids cloning the CalendarEvent map. bd:JMAP-ic0j.40 / .8.
    fn recompute_calendar_event_counts(&mut self, account_id: &str) {
        let mut counts: HashMap<Id, u32> = HashMap::new();
        if let Some(map) = self.objects.get(&("CalendarEvent", account_id.to_owned())) {
            for v in map.values() {
                if let Some(cal_ids) = v.get("calendarIds").and_then(|c| c.as_object()) {
                    for k in cal_ids.keys() {
                        *counts.entry(Id::from(k.as_str())).or_insert(0) += 1;
                    }
                }
            }
        }
        self.aux_mut(account_id).calendar_event_counts = counts;
    }

    /// Apply a single CalendarEvent mutation to the reverse-index counter
    /// in `O(k)` where `k` is the size of the symmetric difference between
    /// `old.calendarIds` and `new.calendarIds` (typically 0–3).
    ///
    /// - `old = None, new = Some(v)`: create — increment counter for each
    ///   id in `v.calendarIds`.
    /// - `old = Some(v), new = None`: destroy — decrement counter for each
    ///   id in `v.calendarIds`, removing entries that reach `0`.
    /// - `old = Some(a), new = Some(b)`: update — decrement for ids in
    ///   `a \ b`, increment for ids in `b \ a`, leave intersection alone.
    ///
    /// A value with no `calendarIds` field, or with a non-object
    /// `calendarIds`, contributes no ids on its side of the delta — this
    /// matches the convention that an event without `calendarIds`
    /// references no calendars.
    ///
    /// bd:JMAP-ic0j.8.
    fn apply_calendar_event_index_delta(
        &mut self,
        account_id: &str,
        old: Option<&serde_json::Value>,
        new: Option<&serde_json::Value>,
    ) {
        let extract = |v: Option<&serde_json::Value>| -> HashSet<Id> {
            v.and_then(|v| v.get("calendarIds"))
                .and_then(|c| c.as_object())
                .map(|m| m.keys().map(|k| Id::from(k.as_str())).collect())
                .unwrap_or_default()
        };
        let old_ids = extract(old);
        let new_ids = extract(new);

        let aux = self.aux_mut(account_id);
        for added in new_ids.difference(&old_ids) {
            *aux.calendar_event_counts.entry(added.clone()).or_insert(0) += 1;
        }
        for removed in old_ids.difference(&new_ids) {
            if let Some(n) = aux.calendar_event_counts.get_mut(removed) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    aux.calendar_event_counts.remove(removed);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// A fully in-memory implementation of [`CalendarsBackend`].
///
/// Stores objects as serialized JSON; each mutation bumps a monotonic state
/// counter and records a change log entry. Used as both the integration-test
/// harness and the canonical example for backend implementors.
///
/// Cloning is cheap: every clone shares the same underlying `Arc<Mutex<…>>`.
///
/// # Caveats (bd:JMAP-ic0j.16)
///
/// This is a **reference / study impl**, not a production backend. The
/// following limitations are intentional and would need to be addressed
/// by a production-grade `CalendarsBackend` implementor:
///
/// - **Sort is not honored.** `query_calendar_events` and the generic
///   `query_objects` ignore the `sort` parameter; ids are returned in
///   lexical-by-id order regardless of what the client requested. RFC
///   8620 §5.5 and draft-ietf-jmap-calendars-26 §5.4 require honoring
///   `CalendarEventComparator` properties (`start`, `sortOrder`, etc.).
///   The integration tests do not exercise sort, so this gap is invisible
///   in CI; production tests must.
/// - **Filter coverage is narrow.** Only the `inCalendar` filter is
///   honored on `CalendarEvent/query` (used by `Calendar/set` cleanup);
///   all other `CalendarEventFilterCondition` fields fall through to
///   "match all". Production backends MUST honor every documented filter.
/// - **No recurrence expansion.** The `expandRecurrences` argument is
///   not implemented; the handler's spec-mandated bound-check still
///   runs, but recurring events are returned as single rows. Production
///   backends MUST expand per RFC 5545 / RFC 8984.
/// - **No availability calculation.** `get_availability` falls through
///   to the default trait impl, which returns an empty result. Production
///   backends MUST implement free/busy lookup per
///   draft-ietf-jmap-calendars-26 §2.2.
///
/// All four caveats are out of scope for the reference impl's purpose —
/// they are the *reason* production deployments override the backend
/// trait rather than reusing `MemoryBackend`.
#[derive(Clone, Default)]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryBackend {
    /// Create a new, empty `MemoryBackend`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an account as known even if it has no objects yet.
    /// Use this in tests that need an empty-but-valid account.
    ///
    /// Returns `self` to allow builder-style chaining:
    ///
    /// ```rust,ignore
    /// let backend = MemoryBackend::new().with_account("acc1");
    /// ```
    ///
    /// # Invariants (bd:JMAP-ic0j.65)
    ///
    /// - **No Id validation**: `account_id` is fed unchanged through
    ///   [`Id::from`], which is infallible and accepts any string. The
    ///   `MemoryBackend` silently accepts bogus account ids that
    ///   violate RFC 8620 §1.2's Id alphabet (e.g. empty string, spaces,
    ///   non-printable characters). Production backends MUST validate
    ///   caller-supplied ids — e.g. via [`Id::new_validated`] — before
    ///   invoking this. The reference impl is permissive to keep test
    ///   fixtures concise.
    /// - **Idempotent**: calling `with_account` with the same value
    ///   multiple times is safe and has no additional effect (the
    ///   underlying `HashSet::insert` simply returns `false` on dup).
    ///   This property is stable; callers may rely on it.
    #[must_use]
    pub fn with_account(self, account_id: &str) -> Self {
        self.register_account(&Id::from(account_id));
        self
    }

    /// Test-only entry point that registers an account as known even
    /// if it has no objects yet. Production callers must not use this
    /// method; the API stability disclaimer on the `memory` feature
    /// applies doubly here.
    ///
    /// The name is retained (not renamed to `register_account_for_test`)
    /// for backward compatibility with the workspace's existing
    /// integration test suite, which calls this method through the
    /// public API surface. `#[doc(hidden)]` removes it from `cargo doc`
    /// output so downstream consumers do not see it as a documented
    /// part of the surface.
    ///
    /// # Invariants
    ///
    /// - **No Id validation**: the `account_id` argument is stored
    ///   verbatim. The caller is responsible for validating that the id
    ///   satisfies RFC 8620 §1.2's character-set and length constraints
    ///   (use [`Id::new_validated`] to obtain a validated `Id`). The
    ///   reference impl does not re-validate — this matches how
    ///   production backends typically receive `Id` values that have
    ///   already been validated at the handler / parser boundary.
    /// - **Idempotent**: registering the same account multiple times is
    ///   safe and has no additional effect. The auxiliary
    ///   `AccountAux` slot is created only on first registration.
    #[doc(hidden)]
    pub fn register_account(&self, account_id: &Id) {
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.as_ref().to_owned());
        // Ensure aux entry exists too so default getters work without state.
        inner.aux.entry(account_id.as_ref().to_owned()).or_default();
    }

    /// Seed a pre-existing object into the store without bumping the state
    /// counter or recording a change-log entry.
    ///
    /// Intended for test fixture setup. The `type_name` must be exactly one
    /// of the four `O::TYPE_NAME` values defined by `jmap-calendars-types`:
    /// `"Calendar"`, `"CalendarEvent"`, `"CalendarEventNotification"`, or
    /// `"ParticipantIdentity"`. The `value` must be a JSON object whose `id`
    /// field matches the `id` argument.
    ///
    /// # Panics
    ///
    /// Panics with a clear message if either precondition is violated:
    ///
    /// - `type_name` is not one of the four known values — catches typos
    ///   like `"calendarevent"` (lowercase) or `"CalenderEvent"` (misspelled)
    ///   that would otherwise silently insert into a dead namespace.
    /// - `value` is not a JSON object, or its `id` field is missing or does
    ///   not equal `id` — catches mistakes like `json!(42)`,
    ///   `json!({})`, or `json!({"id": "wrong-id"})` that would otherwise
    ///   surface much later as opaque `MemoryError("deserialize ...")` from
    ///   `get_objects`.
    ///
    /// bd:JMAP-ic0j.32. The fixture-setup contract is documented in two
    /// places (this rustdoc + the panic messages) so a test author who hits
    /// the panic at runtime gets actionable guidance without having to read
    /// the source.
    pub fn seed_object(
        &self,
        account_id: &str,
        type_name: &str,
        id: &str,
        value: serde_json::Value,
    ) {
        // Catch typos in `type_name` at the seed boundary rather than letting
        // them silently corrupt the fixture into a dead namespace.
        //
        // bd:JMAP-ic0j.69 — the type_name parameter is `&str` (not
        // `&'static str`) so test authors can pass computed values from
        // parameterised fixtures. The inner storage keys require
        // `&'static str` though, so the dispatch below rebinds the
        // accepted input to the matching `&'static str` literal.
        const CALENDAR: &str = "Calendar";
        const CALENDAR_EVENT: &str = "CalendarEvent";
        const CALENDAR_EVENT_NOTIFICATION: &str = "CalendarEventNotification";
        const PARTICIPANT_IDENTITY: &str = "ParticipantIdentity";
        const KNOWN_TYPES: &[&str] = &[
            CALENDAR,
            CALENDAR_EVENT,
            CALENDAR_EVENT_NOTIFICATION,
            PARTICIPANT_IDENTITY,
        ];
        let static_type_name: &'static str = match type_name {
            CALENDAR => CALENDAR,
            CALENDAR_EVENT => CALENDAR_EVENT,
            CALENDAR_EVENT_NOTIFICATION => CALENDAR_EVENT_NOTIFICATION,
            PARTICIPANT_IDENTITY => PARTICIPANT_IDENTITY,
            _ => panic!(
                "seed_object: type_name {type_name:?} is not one of the known \
                 jmap-calendars-types TYPE_NAME values {KNOWN_TYPES:?}. \
                 Likely a typo — the lookup would silently store into a dead \
                 namespace no method reads from."
            ),
        };

        // Validate the value's shape at the seed boundary so a malformed
        // fixture fails fast HERE, not in the get_objects deserialize path.
        let obj = value.as_object().unwrap_or_else(|| {
            panic!(
                "seed_object: value must be a JSON object with an `id` field, \
                 got {value:?}"
            )
        });
        let value_id = obj.get("id").and_then(|v| v.as_str()).unwrap_or_else(|| {
            panic!("seed_object: value must contain an `id` string field; got value = {value:?}")
        });
        assert_eq!(
            value_id, id,
            "seed_object: value's `id` field {value_id:?} does not match the \
             `id` argument {id:?} — the object would be reachable by the arg \
             id but its serialized form would carry the value-id, breaking \
             round-trip"
        );

        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.to_owned());
        inner
            .objects_mut(static_type_name, account_id)
            .insert(Id::from(id), value);
        // bd:JMAP-ic0j.8 — seed_object bypasses the trait-impl mutators
        // so we have no before/after pair to feed
        // `apply_calendar_event_index_delta`; fall back to a full rebuild
        // of `calendar_event_counts`. seed_object is only called from
        // test fixtures, so the `O(N)` rebuild cost is acceptable here.
        if static_type_name == CALENDAR_EVENT {
            inner.recompute_calendar_event_counts(account_id);
        }
    }

    // bd:JMAP-ic0j.28 — set_default_calendar_for_test /
    // set_default_participant_identity_for_test / get_default_calendar /
    // get_default_participant_identity were originally exposed as `pub`
    // (gated only by `feature = "memory"`) for tests that never landed.
    // They have no internal or external callers in the workspace and
    // diverge from the canonical jmap-mail-server pattern (which exposes
    // no `*_for_test` mutation knobs from its MemoryBackend). Removed
    // rather than keeping dead public API surface that would become a
    // SemVer lock once consumers start enabling the `memory` feature.
    //
    // The underlying `AccountAux::default_calendar` /
    // `default_participant_identity` fields ARE exercised — by the trait
    // impls of `CalendarsBackend::set_default_calendar` /
    // `set_default_participant_identity` below — so deleting the helpers
    // does not orphan storage state.

    /// Demo-grade id minter for the in-memory reference backend.
    ///
    /// NOT A PRODUCTION PATTERN. Both modes below are explicitly
    /// demonstration-quality; production backends must mint real ULIDs
    /// (or equivalent globally-unique, monotonic, persistent-across-restarts
    /// ids) and never use this helper as a copy-paste source.
    ///
    /// Behavior is controlled by the `realistic-demo-ids` cargo feature:
    ///
    /// - **Default (deterministic):** returns `"<type><n:016x>"` where `n`
    ///   is a per-(type, account) monotonic counter starting at `1` and
    ///   incrementing on every mint. The counter never decrements on
    ///   delete (bd:JMAP-ic0j.2), so a destroyed object's id is never
    ///   re-minted within the lifetime of the process. Lex-orderable
    ///   within a (type, account) namespace, repeatable across test
    ///   runs, easy to read in test debug output.
    /// - **`realistic-demo-ids` enabled:** returns `"{n:016x}"` matching
    ///   the canonical `jmap-mail-server` pattern at `email.rs:1748` —
    ///   process-start nanos as base, atomic counter, no type prefix,
    ///   no per-account scoping. Lex-orderable globally within a process,
    ///   not repeatable across runs.
    ///
    ///   The base is `SystemTime::now().duration_since(UNIX_EPOCH)` in
    ///   nanos; if that subtraction fails (clock set before 1970-01-01),
    ///   the base falls back to the literal `1_000_000_000` (1 second
    ///   after epoch). The fallback path is reachable in practice only
    ///   on a clock-skewed test rig — id `0x...3b9aca01` and adjacent
    ///   values are the fallback signature.
    ///
    ///   `SystemTime` is documented as non-monotonic by `std`: NTP
    ///   adjustments can move it backwards, and across process restarts
    ///   a clock-skew correction can place T2 before T1 such that the
    ///   T2 process mints ids inside the T1 process's range. The
    ///   `OnceLock<u64>` caches the base for the lifetime of one
    ///   process, so non-monotonicity only matters across restarts of
    ///   the same demo binary — irrelevant for the reference impl's
    ///   purpose but a footgun if this helper is copy-pasted into
    ///   anything resembling production.
    ///
    ///   The atomic counter uses `wrapping_add`, which silently rolls
    ///   over after 2^64 ids. A demo process running for ~584 years at
    ///   1 id/ns would be needed; cosmetic concern only.
    ///
    /// # Feature stability (bd:JMAP-ic0j.63)
    ///
    /// The exact id format produced by either mode is NOT part of the
    /// public API surface. Switching the `realistic-demo-ids` feature
    /// on or off across minor versions may silently change the format
    /// (e.g. a future minor could swap "atomic counter base + offset"
    /// for "ULID"); the format may also change within a single mode
    /// across minor versions pre-1.0. The only guarantees are:
    ///
    /// - Ids are valid [`Id`](jmap_types::Id) values (opaque server
    ///   strings per RFC 8620 §1.2).
    /// - Within a single process, ids minted from this helper are
    ///   collision-free (bd:JMAP-ic0j.2).
    ///
    /// Downstream consumers MUST NOT depend on a specific id shape
    /// from `MemoryBackend`. The deterministic-mode format
    /// (`"<type><n:016x>"`) is reserved for use by this crate's own
    /// unit tests under
    /// `#[cfg(all(test, not(feature = "realistic-demo-ids")))]` —
    /// where the test source IS the reference for what the format is.
    ///
    /// This feature has no analog in the canonical `jmap-mail-server`,
    /// which mints unconditionally in the realistic mode. The
    /// divergence is deliberate: the deterministic default makes test
    /// debugging easier (repeatable ids in assertions) without
    /// requiring a separate test backend. Do NOT propagate this
    /// feature to extension-server siblings as a canonical-template
    /// change.
    fn demo_next_id(inner: &mut Inner, type_name: &'static str, account_id: &str) -> Id {
        #[cfg(feature = "realistic-demo-ids")]
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            use std::sync::OnceLock;
            use std::time::{SystemTime, UNIX_EPOCH};

            let _ = (inner, type_name, account_id);
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            static BASE: OnceLock<u64> = OnceLock::new();
            let base = *BASE.get_or_init(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(1_000_000_000)
            });
            let n = base.wrapping_add(COUNTER.fetch_add(1, Ordering::Relaxed));
            Id::from(format!("{n:016x}"))
        }

        #[cfg(not(feature = "realistic-demo-ids"))]
        {
            // CARGO-CULT WARNING: do NOT copy this into a production backend.
            // This deterministic mode uses a per-(type, account) in-memory
            // counter that:
            //   - resets to 0 on every process restart
            //   - is not unique across (type, account) namespaces
            //   - is not globally collision-resistant
            // Production-grade id minting needs a real ULID or equivalent.
            //
            // It IS, however, monotonic within a single (type, account)
            // namespace across deletes (bd:JMAP-ic0j.2, mirroring
            // bd:JMAP-qz9v.14 in `jmap-contacts-server`) — a destroyed
            // object's id will not be re-minted for a later create.
            let key = (type_name, account_id.to_owned());
            let counter = inner.next_ids.entry(key).or_insert(0);
            *counter += 1;
            let n = *counter;
            let new_id = Id::from(format!("{}{:016x}", type_name.to_ascii_lowercase(), n));
            debug_assert!(
                !inner
                    .objects_ref(type_name, account_id)
                    .is_some_and(|m| m.contains_key(&new_id)),
                "MemoryBackend demo_next_id collision: the monotonic counter \
                 produced an id that already exists in the store. This indicates \
                 the counter was somehow rewound, which should not happen — the \
                 counter is increment-only."
            );
            new_id
        }
    }
}

// ---------------------------------------------------------------------------
// JmapBackend impl (read-side supertrait)
// ---------------------------------------------------------------------------

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.known_accounts.contains(account_id.as_ref()))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        let inner = self.inner.lock().unwrap();
        let map = inner.objects_ref(O::TYPE_NAME, account_id.as_ref());

        match ids {
            None => {
                // Return all objects of this type.
                let mut list = Vec::new();
                if let Some(m) = map {
                    for val in m.values() {
                        match O::deserialize(val) {
                            Ok(obj) => list.push(obj),
                            Err(e) => {
                                return Err(MemoryError::new(format!(
                                    "deserialize {}: {e}",
                                    O::TYPE_NAME
                                )))
                            }
                        }
                    }
                }
                Ok((list, vec![]))
            }
            Some(id_slice) => {
                let mut found = Vec::new();
                let mut not_found = Vec::new();
                for id in id_slice {
                    match map.and_then(|m| m.get(id)) {
                        Some(val) => match O::deserialize(val) {
                            Ok(obj) => found.push(obj),
                            Err(e) => {
                                return Err(MemoryError::new(format!(
                                    "deserialize {}: {e}",
                                    O::TYPE_NAME
                                )))
                            }
                        },
                        None => not_found.push(id.clone()),
                    }
                }
                Ok((found, not_found))
            }
        }
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        let inner = self.inner.lock().unwrap();
        let n = inner.current_state(O::TYPE_NAME, account_id.as_ref());
        Ok(State::from(n.to_string()))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        let inner = self.inner.lock().unwrap();

        let since_n: u64 = since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::CannotCalculate)?;

        let log = inner
            .change_log
            .get(&(O::TYPE_NAME, account_id.as_ref().to_owned()))
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        let relevant: Vec<&ChangeEntry> = log.iter().filter(|e| e.new_state > since_n).collect();

        let current_state = inner.current_state(O::TYPE_NAME, account_id.as_ref());

        if let Some(max) = max_changes {
            if relevant.len() as u64 > max {
                return Err(BackendChangesError::TooManyChanges { limit: max });
            }
        }

        // bd:JMAP-ic0j.50 — port the canonical jmap-mail-server HashMap+IdFate
        // dedup pattern, but PRESERVE the previous semantic: create+destroy
        // within one window → `destroyed` only (the previous Vec-based code's
        // behavior, which is RFC 8620 §5.2 "MAY include it in just the
        // 'destroyed' list", a valid choice under the SHOULD/MAY hierarchy).
        // The canonical mail-server uses the SHOULD-preferred "omit from both"
        // path; calendars-server's MAY path was a pre-existing choice and
        // changing it is a semantic shift out of scope for this idiom fix.
        // Only the dedup data structure is changed here: Vec::contains O(n)
        // membership tests → HashMap O(1) lookups (idiom intent of the bead).
        let mut fates: HashMap<Id, IdFate> = HashMap::new();
        for entry in &relevant {
            for id in &entry.created {
                // bd:JMAP-ic0j.15 — guard against id recycling.
                //
                // Under bd:JMAP-ic0j.2's monotonic-counter invariant a
                // mint-after-destroy never re-uses an id, so a `created` id
                // whose existing fate is `Destroyed` would mean a later entry
                // minted the same id as an earlier `destroyed` entry. This
                // `debug_assert!` trips immediately in tests if a future
                // regression in id minting re-introduces recycling, surfacing
                // what would otherwise be a silent-drop bug — the dedup pass
                // below would clobber the `Destroyed` fate with `Created` in
                // release builds.
                debug_assert!(
                    !matches!(fates.get(id), Some(IdFate::Destroyed)),
                    "MemoryBackend change_log invariant violated: id {id} \
                     appears in entry.created after appearing in an earlier \
                     entry.destroyed. This indicates id recycling in the \
                     demo id minter (regression of bd:JMAP-ic0j.2). The \
                     dedup pass below silently clobbers the destroy in \
                     release builds."
                );
                // Preserve previous-Vec-behavior: if already classified as
                // Destroyed, the destroy wins (a no-op create on a
                // destroyed-then-re-destroyed flow). Otherwise mark Created.
                if !matches!(fates.get(id), Some(IdFate::Destroyed)) {
                    fates.insert(id.clone(), IdFate::Created);
                }
            }
            for id in &entry.updated {
                // Same recycling guard for updated. An update on a
                // previously-destroyed id is equally impossible under
                // the monotonic-counter invariant.
                debug_assert!(
                    !matches!(fates.get(id), Some(IdFate::Destroyed)),
                    "MemoryBackend change_log invariant violated: id {id} \
                     appears in entry.updated after appearing in an earlier \
                     entry.destroyed. This indicates id recycling in the \
                     demo id minter (regression of bd:JMAP-ic0j.2)."
                );
                let fate = match fates.get(id) {
                    Some(IdFate::Created) => IdFate::Created,
                    Some(IdFate::Destroyed) => IdFate::Destroyed,
                    _ => IdFate::Updated,
                };
                fates.insert(id.clone(), fate);
            }
            for id in &entry.destroyed {
                // Destroy supersedes Created/Updated: matches the previous
                // Vec-based code's `created.retain + destroyed.push` shape
                // (RFC 8620 §5.2 MAY path: "include it in just the
                // 'destroyed' list").
                fates.insert(id.clone(), IdFate::Destroyed);
            }
        }

        let mut created = Vec::new();
        let mut updated = Vec::new();
        let mut destroyed = Vec::new();
        for (id, fate) in fates {
            match fate {
                IdFate::Created => created.push(id),
                IdFate::Updated => updated.push(id),
                IdFate::Destroyed => destroyed.push(id),
            }
        }

        // Sort each bucket so output is deterministic across HashMap iteration
        // order changes (matches the previous Vec-based code's
        // entry-insertion-order behavior closely enough for test stability).
        created.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        updated.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        destroyed.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));

        Ok(ChangesResult::new(
            created,
            updated,
            destroyed,
            false,
            State::from(current_state.to_string()),
        ))
    }

    /// Reference query_objects impl — see the module-level "Caveats" doc
    /// (bd:JMAP-ic0j.16) for the sort / filter / position semantics the
    /// reference impl does NOT honor.
    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        // bd:JMAP-ic0j.16 — sort is NOT honored. The `_sort` parameter is
        // prefixed with `_` and never consulted; ids are returned in
        // lexical-by-id order. Production backends that override
        // `CalendarsBackend::query_calendar_events` MUST honor sort per
        // RFC 8620 §5.5 and draft-ietf-jmap-calendars-26 §5.4. The
        // reference impl's sort-ignoring shape is documented in the
        // crate-level MemoryBackend rustdoc.
        let inner = self.inner.lock().unwrap();

        // For CalendarEvent, support the `inCalendar` filter (used by
        // `Calendar/set` cleanup when `onDestroyRemoveEvents: true`).
        // Other filters fall through to "match all".
        let in_calendar: Option<String> = if O::TYPE_NAME == "CalendarEvent" {
            filter
                .and_then(|f| serde_json::to_value(f).ok())
                .and_then(|v| {
                    v.get("inCalendar")
                        .and_then(|c| c.as_str())
                        .map(String::from)
                })
        } else {
            None
        };

        let mut ids: Vec<Id> = inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .map(|m| {
                let mut keys: Vec<(Id, &serde_json::Value)> =
                    m.iter().map(|(k, v)| (k.clone(), v)).collect();
                keys.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
                keys.into_iter()
                    .filter(|(_, v)| match &in_calendar {
                        None => true,
                        Some(target) => v
                            .get("calendarIds")
                            .and_then(|c| c.as_object())
                            .map(|map| map.contains_key(target))
                            .unwrap_or(false),
                    })
                    .map(|(id, _)| id)
                    .collect()
            })
            .unwrap_or_default();

        let total = ids.len() as u64;
        let query_state = State::from(
            inner
                .current_state(O::TYPE_NAME, account_id.as_ref())
                .to_string()
                .as_str(),
        );

        // bd:JMAP-qz9v.48 — centralized in jmap_server::resolve_query_offset.
        let start = resolve_query_offset(position, ids.len());

        ids = ids[start..]
            .iter()
            .take(limit.map_or(usize::MAX, |n| n as usize))
            .cloned()
            .collect();

        Ok(QueryResult::new(
            ids,
            start as u64,
            Some(total),
            query_state,
            true,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        let changes = self
            .get_changes::<O>(&(), account_id, since_query_state, max_changes)
            .await?;

        let inner = self.inner.lock().unwrap();
        let new_query_state = State::from(
            inner
                .current_state(O::TYPE_NAME, account_id.as_ref())
                .to_string()
                .as_str(),
        );

        let current_ids: Vec<Id> = inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .map(|m| {
                let mut keys: Vec<Id> = m.keys().cloned().collect();
                keys.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
                keys
            })
            .unwrap_or_default();

        let removed: Vec<Id> = changes.destroyed;
        let added: Vec<AddedItem> = changes
            .created
            .iter()
            .filter_map(|id| {
                current_ids
                    .iter()
                    .position(|cur| cur == id)
                    .map(|idx| AddedItem::new(id.clone(), idx as u64))
            })
            .collect();

        Ok(QueryChangesResult::new(
            since_query_state.clone(),
            new_query_state,
            Some(current_ids.len() as u64),
            removed,
            added,
        ))
    }
}

// ---------------------------------------------------------------------------
// CalendarsBackend impl
// ---------------------------------------------------------------------------

impl CalendarsBackend for MemoryBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        // accountId existence is enforced at the handler layer (RFC 8620
        // §3.6.2 accountNotFound), but defend here too — silently auto-
        // register so seed_object + create flow without explicit
        // `register_account` calls in tests does not error spuriously.
        inner.known_accounts.insert(account_id.as_ref().to_owned());

        let server_id = Self::demo_next_id(&mut inner, O::TYPE_NAME, account_id.as_ref());

        // Serialize, set "id" to the server-assigned id, then deserialize back.
        let mut val = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError::new(format!("serialize: {e}"))))?;
        if let Some(map) = val.as_object_mut() {
            map.insert(
                "id".to_owned(),
                serde_json::Value::String(server_id.as_ref().to_owned()),
            );
        }
        let stored_obj: O = O::deserialize(&val).map_err(|e| {
            BackendSetError::Other(MemoryError::new(format!("deserialize after create: {e}")))
        })?;

        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        // bd:JMAP-ic0j.8 — for CalendarEvent, feed the new value to the
        // incremental index updater before the move into storage so we
        // don't have to look it back up. For other types this branch is
        // unused (the index helper extracts no ids and does nothing).
        if O::TYPE_NAME == "CalendarEvent" {
            inner.apply_calendar_event_index_delta(account_id.as_ref(), None, Some(&val));
        }
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(server_id.clone(), val);
        inner
            .change_log_mut(O::TYPE_NAME, account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![server_id.clone()],
                updated: vec![],
                destroyed: vec![],
            });

        Ok((server_id, stored_obj))
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        let existing = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .get(id)
            .cloned();

        let Some(old_value) = existing else {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )));
        };
        let mut current = old_value.clone();

        // Apply JSON Merge Patch (RFC 7396): merge patch fields into current value.
        // A `MergePatchError::DepthExceeded` return (bd:JMAP-wlip.1) surfaces
        // as `SetErrorType::InvalidPatch` — the depth cap is a DoS guard,
        // never fires on legitimate JMAP `/set update` shapes. `current` is a
        // clone of the stored value, so a partially-applied patch on error is
        // discarded with the local without touching storage.
        let patch_val = serde_json::to_value(&patch).map_err(|e| {
            BackendSetError::Other(MemoryError::new(format!("serialize patch: {e}")))
        })?;
        if let Err(MergePatchError::DepthExceeded) = json_merge_patch(&mut current, patch_val) {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidPatch)
                    .with_description("patch nesting exceeds server limit"),
            ));
        }

        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        // bd:JMAP-ic0j.8 — for CalendarEvent, the incremental index
        // update needs both the pre-patch (`old_value`) and post-patch
        // (`current`) JSON in hand. We pass both before moving `current`
        // into storage.
        if O::TYPE_NAME == "CalendarEvent" {
            inner.apply_calendar_event_index_delta(
                account_id.as_ref(),
                Some(&old_value),
                Some(&current),
            );
        }
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(id.clone(), current);
        inner
            .change_log_mut(O::TYPE_NAME, account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![id.clone()],
                destroyed: vec![],
            });

        Ok(None)
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        let removed = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .remove(id);

        match removed {
            Some(old_value) => {
                let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
                inner
                    .change_log_mut(O::TYPE_NAME, account_id.as_ref())
                    .push(ChangeEntry {
                        new_state,
                        created: vec![],
                        updated: vec![],
                        destroyed: vec![id.clone()],
                    });
                // bd:JMAP-ic0j.8 — for CalendarEvent, feed the removed
                // value as `old` (and no `new`) so the reverse-index
                // counter decrements its calendar ids.
                if O::TYPE_NAME == "CalendarEvent" {
                    inner.apply_calendar_event_index_delta(
                        account_id.as_ref(),
                        Some(&old_value),
                        None,
                    );
                }
                Ok(())
            }
            None => Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            ))),
        }
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        matches!(
            O::TYPE_NAME,
            "Calendar" | "CalendarEvent" | "CalendarEventNotification" | "ParticipantIdentity"
        )
    }

    async fn calendar_has_events(
        &self,
        _caller: &(),
        account_id: &Id,
        calendar_id: &Id,
    ) -> Result<bool, Self::Error> {
        let inner = self.inner.lock().unwrap();
        // bd:JMAP-ic0j.8 — `calendar_event_counts` entries are deleted
        // when they reach 0, so `contains_key` is a valid "has events?"
        // test without inspecting the count value.
        Ok(inner
            .aux_ref(account_id.as_ref())
            .is_some_and(|a| a.calendar_event_counts.contains_key(calendar_id)))
    }

    async fn set_default_calendar(
        &self,
        _caller: &(),
        account_id: &Id,
        calendar_id: &Id,
    ) -> Result<SetDefaultResult, Self::Error> {
        let mut inner = self.inner.lock().unwrap();

        // §4.3: silently ignore if the calendar is unknown.
        let known = inner
            .objects_ref("Calendar", account_id.as_ref())
            .map(|m| m.contains_key(calendar_id))
            .unwrap_or(false);

        if !known {
            return Ok(SetDefaultResult::default());
        }

        let previous = inner
            .aux_ref(account_id.as_ref())
            .and_then(|a| a.default_calendar.clone());
        inner.aux_mut(account_id.as_ref()).default_calendar = Some(calendar_id.clone());

        Ok(SetDefaultResult::new(Some(calendar_id.clone()), previous))
    }

    async fn set_default_participant_identity(
        &self,
        _caller: &(),
        account_id: &Id,
        identity_id: &Id,
    ) -> Result<SetDefaultResult, Self::Error> {
        let mut inner = self.inner.lock().unwrap();

        // §3.3: silently ignore if the identity is unknown.
        let known = inner
            .objects_ref("ParticipantIdentity", account_id.as_ref())
            .map(|m| m.contains_key(identity_id))
            .unwrap_or(false);

        if !known {
            return Ok(SetDefaultResult::default());
        }

        let previous = inner
            .aux_ref(account_id.as_ref())
            .and_then(|a| a.default_participant_identity.clone());
        inner
            .aux_mut(account_id.as_ref())
            .default_participant_identity = Some(identity_id.clone());

        Ok(SetDefaultResult::new(Some(identity_id.clone()), previous))
    }
}

// ---------------------------------------------------------------------------
// Tests for demo_next_id (bd:JMAP-ic0j.2, mirrors bd:JMAP-qz9v.14)
// ---------------------------------------------------------------------------

#[cfg(all(test, not(feature = "realistic-demo-ids")))]
mod demo_id_tests {
    use super::*;

    /// Oracle (bd:JMAP-ic0j.2, mirrors bd:JMAP-qz9v.14 in
    /// `jmap-contacts-server`): minting an id, destroying the object,
    /// then minting again MUST produce a different id. The previous
    /// `len()`-based counter re-minted the destroyed id, causing the
    /// same Id to appear as both `created[K+2]` and `destroyed[K]` in
    /// the change log — silently corrupting client-side caches and
    /// violating RFC 8620 §5.2 'every distinct state MUST be reported
    /// correctly'.
    #[test]
    fn id_not_recycled_after_destroy() {
        let mut inner = Inner::default();
        let acc = "acc1";

        // Mint id1 and stash a placeholder in the objects map so a
        // future debug_assert would catch a re-mint.
        let id1 = MemoryBackend::demo_next_id(&mut inner, "Calendar", acc);
        inner
            .objects_mut("Calendar", acc)
            .insert(id1.clone(), serde_json::Value::Null);

        // Destroy id1 by removing it from the store.
        inner.objects_mut("Calendar", acc).remove(&id1);

        // The next mint MUST yield a different id even though the store
        // is now empty for this (type, account).
        let id2 = MemoryBackend::demo_next_id(&mut inner, "Calendar", acc);
        assert_ne!(
            id1.as_ref(),
            id2.as_ref(),
            "destroyed id must not be re-minted: id1={id1}, id2={id2}"
        );
    }

    /// Oracle: the counter is per-(type, account), so two distinct
    /// (type, account) pairs do not share id space. A CalendarEvent in
    /// account "acc1" and a Calendar in account "acc1" can both start
    /// at the n=1 numbering without colliding, and two different
    /// accounts each get their own Calendar counter starting at 1.
    #[test]
    fn id_namespace_is_per_type_and_account() {
        let mut inner = Inner::default();

        let cal_a = MemoryBackend::demo_next_id(&mut inner, "Calendar", "acc1");
        let ev_a = MemoryBackend::demo_next_id(&mut inner, "CalendarEvent", "acc1");
        let cal_b = MemoryBackend::demo_next_id(&mut inner, "Calendar", "acc2");

        // Different type → different prefix even at same counter value.
        assert_ne!(cal_a.as_ref(), ev_a.as_ref());
        // Different account → independent counter, same prefix but n=1.
        assert_eq!(
            cal_a.as_ref(),
            "calendar0000000000000001",
            "counter starts at 1 for the first mint in a namespace"
        );
        assert_eq!(
            cal_b.as_ref(),
            "calendar0000000000000001",
            "different account namespace gets its own counter starting at 1"
        );
    }

    /// Oracle: the counter is monotonic — successive mints in the same
    /// (type, account) namespace yield strictly increasing ids.
    #[test]
    fn ids_are_monotonic_within_namespace() {
        let mut inner = Inner::default();
        let acc = "acc1";

        let id1 = MemoryBackend::demo_next_id(&mut inner, "Calendar", acc);
        let id2 = MemoryBackend::demo_next_id(&mut inner, "Calendar", acc);
        let id3 = MemoryBackend::demo_next_id(&mut inner, "Calendar", acc);

        assert!(id1.as_ref() < id2.as_ref());
        assert!(id2.as_ref() < id3.as_ref());
    }
}

// ---------------------------------------------------------------------------
// Tests for change_log dedup invariant (bd:JMAP-ic0j.15)
// ---------------------------------------------------------------------------

#[cfg(all(test, debug_assertions))]
mod change_log_dedup_tests {
    use super::*;
    use jmap_calendars_types::Calendar;

    /// Seed a `MemoryBackend` with a synthetic change_log so the dedup
    /// loop can be exercised against arbitrary patterns. The state
    /// counter is set to the highest `new_state` in `entries`.
    fn seed_change_log(backend: &MemoryBackend, account_id: &str, entries: Vec<ChangeEntry>) {
        let mut inner = backend.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.to_owned());
        let max_state = entries.iter().map(|e| e.new_state).max().unwrap_or(0);
        let key = ("Calendar", account_id.to_owned());
        inner.change_log.insert(key.clone(), entries);
        inner.states.insert(key, max_state);
    }

    /// Regression for bd:JMAP-ic0j.15 + bd:JMAP-ic0j.2.
    ///
    /// Constructs a synthetic change_log that contains the id recycling
    /// pattern (id X appears in entry K's `destroyed` and entry K+2's
    /// `created`), then drives `get_changes` to walk the dedup loop and
    /// confirm the `debug_assert!` fires.
    ///
    /// The synthetic state directly pokes private `Inner` fields,
    /// reproducing what the pre-bd:JMAP-ic0j.2 id minter would have
    /// produced in a destroy+create round-trip on the same id. Without
    /// the assertion, the loop's existing dedup would silently collapse
    /// the create into a destroy-only report, breaking client-side
    /// caches per RFC 8620 §5.2.
    #[tokio::test]
    #[should_panic(expected = "MemoryBackend change_log invariant violated")]
    async fn dedup_pass_panics_on_recycled_id_in_created() {
        let backend = MemoryBackend::new();
        let acc_id = Id::from("acc1");
        let recycled = Id::from("calendar0000000000000001");

        seed_change_log(
            &backend,
            acc_id.as_ref(),
            vec![
                // Entry K=1: created the original.
                ChangeEntry {
                    new_state: 1,
                    created: vec![recycled.clone()],
                    updated: vec![],
                    destroyed: vec![],
                },
                // Entry K=2: destroyed it.
                ChangeEntry {
                    new_state: 2,
                    created: vec![],
                    updated: vec![],
                    destroyed: vec![recycled.clone()],
                },
                // Entry K=3: re-mints the SAME id (the bug we are
                // guarding against; impossible under bd:JMAP-ic0j.2's
                // invariant).
                ChangeEntry {
                    new_state: 3,
                    created: vec![recycled.clone()],
                    updated: vec![],
                    destroyed: vec![],
                },
            ],
        );

        // The dedup loop walks entries 1..=3 because since_n=0.
        // Processing entry K=3's `created` triggers the debug_assert.
        let _ = backend
            .get_changes::<Calendar>(&(), &acc_id, &State::from("0"), None)
            .await;
    }

    /// Same recycling pattern but the recycled id appears in
    /// entry K=3's `updated` slot rather than `created`. Equally
    /// impossible under the monotonic-counter invariant — an update
    /// on a destroyed id requires re-minting, which the invariant
    /// forbids.
    #[tokio::test]
    #[should_panic(expected = "MemoryBackend change_log invariant violated")]
    async fn dedup_pass_panics_on_recycled_id_in_updated() {
        let backend = MemoryBackend::new();
        let acc_id = Id::from("acc1");
        let recycled = Id::from("calendar0000000000000001");

        seed_change_log(
            &backend,
            acc_id.as_ref(),
            vec![
                ChangeEntry {
                    new_state: 1,
                    created: vec![recycled.clone()],
                    updated: vec![],
                    destroyed: vec![],
                },
                ChangeEntry {
                    new_state: 2,
                    created: vec![],
                    updated: vec![],
                    destroyed: vec![recycled.clone()],
                },
                ChangeEntry {
                    new_state: 3,
                    created: vec![],
                    updated: vec![recycled.clone()],
                    destroyed: vec![],
                },
            ],
        );

        let _ = backend
            .get_changes::<Calendar>(&(), &acc_id, &State::from("0"), None)
            .await;
    }

    /// Control: the non-recycling create-then-destroy-then-create-different-id
    /// pattern (which is what bd:JMAP-ic0j.2 guarantees) must NOT trip
    /// the assertion. Two distinct ids, both round-tripped through
    /// destroy and re-create, end up correctly classified.
    #[tokio::test]
    async fn dedup_pass_does_not_panic_on_distinct_recreated_ids() {
        let backend = MemoryBackend::new();
        let acc_id = Id::from("acc1");
        let id_a = Id::from("calendar0000000000000001");
        let id_b = Id::from("calendar0000000000000002");

        seed_change_log(
            &backend,
            acc_id.as_ref(),
            vec![
                ChangeEntry {
                    new_state: 1,
                    created: vec![id_a.clone()],
                    updated: vec![],
                    destroyed: vec![],
                },
                ChangeEntry {
                    new_state: 2,
                    created: vec![],
                    updated: vec![],
                    destroyed: vec![id_a.clone()],
                },
                // A DIFFERENT id, not the destroyed one.
                ChangeEntry {
                    new_state: 3,
                    created: vec![id_b.clone()],
                    updated: vec![],
                    destroyed: vec![],
                },
            ],
        );

        let result = backend
            .get_changes::<Calendar>(&(), &acc_id, &State::from("0"), None)
            .await
            .expect("must not error");

        // id_a was created and then destroyed within the window → final
        // classification is destroyed only. id_b was created in the
        // window → final classification is created only.
        assert_eq!(
            result.destroyed,
            vec![id_a],
            "destroyed-then-not-recreated id stays in destroyed"
        );
        assert_eq!(
            result.created,
            vec![id_b],
            "newly-minted id appears in created"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests for seed_object precondition assertions (bd:JMAP-ic0j.32)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod seed_object_validation_tests {
    use super::*;
    use serde_json::json;

    /// Regression for bd:JMAP-ic0j.32: a misspelled `type_name` panics
    /// instead of silently storing into a dead namespace.
    ///
    /// Oracle: the four valid `type_name` values are the
    /// `O::TYPE_NAME` constants defined in
    /// `jmap_calendars_types::backend` (Calendar, CalendarEvent,
    /// CalendarEventNotification, ParticipantIdentity). Anything else
    /// is a test-author typo.
    #[test]
    #[should_panic(expected = "not one of the known")]
    fn seed_object_rejects_unknown_type_name() {
        let backend = MemoryBackend::new();
        backend.seed_object("acc1", "calendarevent", "ev1", json!({"id": "ev1"}));
    }

    /// Regression for bd:JMAP-ic0j.32: a non-object value panics
    /// instead of silently storing data that get_objects cannot
    /// deserialize.
    #[test]
    #[should_panic(expected = "must be a JSON object")]
    fn seed_object_rejects_non_object_value() {
        let backend = MemoryBackend::new();
        backend.seed_object("acc1", "Calendar", "cal1", json!(42));
    }

    /// Regression for bd:JMAP-ic0j.32: a value missing the `id` field
    /// panics instead of storing a fixture whose `id` field will be
    /// absent on read-back.
    #[test]
    #[should_panic(expected = "must contain an `id` string field")]
    fn seed_object_rejects_value_without_id_field() {
        let backend = MemoryBackend::new();
        backend.seed_object("acc1", "Calendar", "cal1", json!({"name": "Work"}));
    }

    /// Regression for bd:JMAP-ic0j.32: a value whose `id` field does
    /// not match the `id` argument panics rather than producing a
    /// fixture that's reachable by one id but serializes with another.
    #[test]
    #[should_panic(expected = "does not match the `id` argument")]
    fn seed_object_rejects_id_mismatch() {
        let backend = MemoryBackend::new();
        backend.seed_object("acc1", "Calendar", "cal1", json!({"id": "cal2"}));
    }

    /// Positive control: the happy path still works for each of the
    /// four known type_name values.
    #[test]
    fn seed_object_accepts_known_type_names() {
        let backend = MemoryBackend::new();
        backend.seed_object("acc1", "Calendar", "c1", json!({"id": "c1"}));
        backend.seed_object("acc1", "CalendarEvent", "e1", json!({"id": "e1"}));
        backend.seed_object(
            "acc1",
            "CalendarEventNotification",
            "n1",
            json!({"id": "n1"}),
        );
        backend.seed_object("acc1", "ParticipantIdentity", "p1", json!({"id": "p1"}));
    }

    /// Regression for bd:JMAP-ic0j.69: a non-`'static` `&str` (e.g. a
    /// `String` slice produced by `format!`) is now accepted. The
    /// original signature `type_name: &'static str` forced every caller
    /// to use a string literal; the new signature lets parameterised
    /// fixtures factor out their type names.
    #[test]
    fn seed_object_accepts_non_static_type_name() {
        let backend = MemoryBackend::new();
        // Compute the type name at runtime — would have failed to
        // compile against the previous `&'static str` signature.
        let prefix = "Calendar";
        let kind = format!("{prefix}Event");
        backend.seed_object("acc1", &kind, "e1", json!({"id": "e1"}));
    }
}

// ---------------------------------------------------------------------------
// Tests for the default CalendarsBackend::limits impl (bd:JMAP-ic0j.31)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod limits_default_tests {
    use super::*;
    use crate::backend::CalendarsLimits;

    /// Oracle (bd:JMAP-ic0j.31): the default
    /// [`CalendarsBackend::limits`] impl returns
    /// [`CalendarsLimits::default`] for any (caller, account_id) pair,
    /// because [`MemoryBackend`] does not override the method.
    /// Production backends override the trait method to vary caps
    /// per-account; the reference impl pegs every account to defaults.
    #[test]
    fn memory_backend_limits_returns_default() {
        let backend = MemoryBackend::new();
        let account = Id::from("acc1");
        let got = backend.limits(&(), &account);
        assert_eq!(
            got,
            CalendarsLimits::default(),
            "MemoryBackend::limits must return CalendarsLimits::default for any account"
        );
    }

    /// Oracle (bd:JMAP-ic0j.31): the `caller` and `account_id`
    /// arguments are plumbed through even though the default impl
    /// ignores them. Verify by passing distinct account ids and
    /// observing the identical Default-shaped result.
    #[test]
    fn memory_backend_limits_ignores_account_in_default_impl() {
        let backend = MemoryBackend::new();
        let a = backend.limits(&(), &Id::from("acc1"));
        let b = backend.limits(&(), &Id::from("acc2"));
        assert_eq!(
            a, b,
            "default impl returns the same struct for every account"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests for the incremental calendar_event_counts reverse index
// (bd:JMAP-ic0j.8)
//
// These tests probe the index directly by inspecting the AccountAux state
// after a sequence of mutations. The oracle is hand-computed expected
// counter values per mutation step, NOT a round-trip through the
// `recompute_calendar_event_counts` full-rescan (which would be a
// self-test against the same code path). The full-rescan is used as a
// separate cross-check: an "equivalence" test verifies that the
// incremental index after a sequence of mutations matches the full-rescan
// result, where the full-rescan is invoked manually only as the oracle.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod calendar_event_counts_tests {
    use super::*;

    fn count(backend: &MemoryBackend, account_id: &str, calendar_id: &str) -> u32 {
        let inner = backend.inner.lock().unwrap();
        inner
            .aux_ref(account_id)
            .and_then(|a| a.calendar_event_counts.get(&Id::from(calendar_id)))
            .copied()
            .unwrap_or(0)
    }

    fn has_key(backend: &MemoryBackend, account_id: &str, calendar_id: &str) -> bool {
        let inner = backend.inner.lock().unwrap();
        inner
            .aux_ref(account_id)
            .is_some_and(|a| a.calendar_event_counts.contains_key(&Id::from(calendar_id)))
    }

    /// Oracle: a single seeded event referencing cal1 produces a count of
    /// exactly 1 under cal1, and no entry for any other calendar.
    #[test]
    fn seed_single_event_single_calendar_count_is_one() {
        let backend = MemoryBackend::new().with_account("acc1");
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "evt1",
            serde_json::json!({"id": "evt1", "calendarIds": {"cal1": true}}),
        );
        assert_eq!(count(&backend, "acc1", "cal1"), 1);
        assert!(!has_key(&backend, "acc1", "cal2"));
    }

    /// Oracle: two seeded events both referencing cal1 produce a count
    /// of 2 — verifies the counter accumulates rather than overwriting.
    #[test]
    fn seed_two_events_same_calendar_count_is_two() {
        let backend = MemoryBackend::new().with_account("acc1");
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "evt1",
            serde_json::json!({"id": "evt1", "calendarIds": {"cal1": true}}),
        );
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "evt2",
            serde_json::json!({"id": "evt2", "calendarIds": {"cal1": true}}),
        );
        assert_eq!(count(&backend, "acc1", "cal1"), 2);
    }

    /// Oracle: a single event referencing two calendars (cal1 + cal2)
    /// contributes 1 to each — verifies the index handles the JMAP
    /// "event-in-multiple-calendars" case (§5.2 multi-calendar events).
    #[test]
    fn seed_event_with_multiple_calendars_each_gets_one() {
        let backend = MemoryBackend::new().with_account("acc1");
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "evt1",
            serde_json::json!({
                "id": "evt1",
                "calendarIds": {"cal1": true, "cal2": true}
            }),
        );
        assert_eq!(count(&backend, "acc1", "cal1"), 1);
        assert_eq!(count(&backend, "acc1", "cal2"), 1);
    }

    /// Oracle: seeding an event referencing cal1, then seeding another
    /// in cal2, then "destroying" (re-seeding without cal1) leaves
    /// counters that match a full rescan of the resulting CalendarEvent
    /// store.
    ///
    /// `seed_object` performs a full rescan after each call (it bypasses
    /// the incremental delta helper), so this test is principally a
    /// correctness probe of `recompute_calendar_event_counts` against
    /// independent expected values. It establishes that the full-rescan
    /// oracle used elsewhere in this module is itself sound.
    #[test]
    fn seed_then_seed_yields_correct_counts() {
        let backend = MemoryBackend::new().with_account("acc1");
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "evt1",
            serde_json::json!({"id": "evt1", "calendarIds": {"cal1": true}}),
        );
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "evt2",
            serde_json::json!({"id": "evt2", "calendarIds": {"cal2": true}}),
        );
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "evt3",
            serde_json::json!({"id": "evt3", "calendarIds": {"cal2": true, "cal3": true}}),
        );
        assert_eq!(count(&backend, "acc1", "cal1"), 1);
        assert_eq!(count(&backend, "acc1", "cal2"), 2);
        assert_eq!(count(&backend, "acc1", "cal3"), 1);
        assert!(!has_key(&backend, "acc1", "cal4"));
    }

    fn calendar_event(uid: &str, calendar_ids: &[&str]) -> jmap_calendars_types::CalendarEvent {
        let mut cal_ids = serde_json::Map::new();
        for c in calendar_ids {
            cal_ids.insert((*c).to_owned(), serde_json::Value::Bool(true));
        }
        serde_json::from_value(serde_json::json!({
            "@type": "Event",
            "uid": uid,
            "calendarIds": cal_ids,
        }))
        .expect("CalendarEvent deserialization must succeed for valid fixture JSON")
    }

    /// Oracle: a sequence of `create_object` / `update_object` /
    /// `destroy_object` calls through the real `CalendarsBackend` trait
    /// path produces a final reverse-index counter map that is
    /// byte-identical to the result of a from-scratch full rescan over
    /// the current CalendarEvent store. The full-rescan implementation
    /// (`recompute_calendar_event_counts`) is the independent oracle: it
    /// re-derives the map from the storage, never consulting the
    /// incremental state.
    ///
    /// This is the canonical correctness check for the incremental
    /// algorithm in `apply_calendar_event_index_delta`. If any of the
    /// three /set production paths fail to maintain the index, the final
    /// equality check fails.
    #[tokio::test]
    async fn incremental_matches_full_rescan_under_mixed_workload() {
        let backend = MemoryBackend::new().with_account("acc1");

        // Seed two events: one in cal1, one in cal2. Seed triggers a
        // full rescan, so after this the index is whatever rescan
        // produces.
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "seed1",
            serde_json::json!({"id": "seed1", "calendarIds": {"cal1": true}}),
        );
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            "seed2",
            serde_json::json!({"id": "seed2", "calendarIds": {"cal2": true}}),
        );

        // Create three more events through the real /set path, exercising
        // the incremental delta helper.
        let (id1, _) = backend
            .create_object::<jmap_calendars_types::CalendarEvent>(
                &(),
                &Id::from("acc1"),
                "c1",
                calendar_event("u1", &["cal1", "cal3"]),
            )
            .await
            .unwrap();
        let (id2, _) = backend
            .create_object::<jmap_calendars_types::CalendarEvent>(
                &(),
                &Id::from("acc1"),
                "c2",
                calendar_event("u2", &["cal2"]),
            )
            .await
            .unwrap();
        let _ = backend
            .create_object::<jmap_calendars_types::CalendarEvent>(
                &(),
                &Id::from("acc1"),
                "c3",
                calendar_event("u3", &["cal3"]),
            )
            .await
            .unwrap();

        // Destroy two of them, also via the real /set path.
        backend
            .destroy_object::<jmap_calendars_types::CalendarEvent>(&(), &Id::from("acc1"), &id1)
            .await
            .unwrap();
        backend
            .destroy_object::<jmap_calendars_types::CalendarEvent>(&(), &Id::from("acc1"), &id2)
            .await
            .unwrap();

        // Cross-check against the from-scratch oracle.
        let mut inner = backend.inner.lock().unwrap();
        let incremental = inner.aux_ref("acc1").unwrap().calendar_event_counts.clone();
        inner.recompute_calendar_event_counts("acc1");
        let from_scratch = inner.aux_ref("acc1").unwrap().calendar_event_counts.clone();
        assert_eq!(
            incremental, from_scratch,
            "incremental index must match full-rescan after a mixed workload\n\
             incremental={incremental:?}\nfull-rescan={from_scratch:?}"
        );
    }

    /// Oracle: create followed by destroy of the same event leaves the
    /// counter at 0 for the calendar id it referenced, and the entry is
    /// deleted (so `contains_key` returns false). Exercises the
    /// incremental path through both production code branches.
    #[tokio::test]
    async fn create_then_destroy_removes_entry() {
        let backend = MemoryBackend::new().with_account("acc1");
        let (id, _) = backend
            .create_object::<jmap_calendars_types::CalendarEvent>(
                &(),
                &Id::from("acc1"),
                "c1",
                calendar_event("u1", &["cal1"]),
            )
            .await
            .unwrap();
        assert_eq!(count(&backend, "acc1", "cal1"), 1);

        backend
            .destroy_object::<jmap_calendars_types::CalendarEvent>(&(), &Id::from("acc1"), &id)
            .await
            .unwrap();
        assert!(
            !has_key(&backend, "acc1", "cal1"),
            "entry must be removed when count reaches 0"
        );
    }

    /// Oracle: when two events share a calendar id and only one is
    /// destroyed, the counter drops from 2 to 1 and the entry is
    /// retained. Verifies the counter doesn't collapse to 0 prematurely.
    #[tokio::test]
    async fn create_two_destroy_one_keeps_entry() {
        let backend = MemoryBackend::new().with_account("acc1");
        let (id1, _) = backend
            .create_object::<jmap_calendars_types::CalendarEvent>(
                &(),
                &Id::from("acc1"),
                "c1",
                calendar_event("u1", &["cal1"]),
            )
            .await
            .unwrap();
        let _ = backend
            .create_object::<jmap_calendars_types::CalendarEvent>(
                &(),
                &Id::from("acc1"),
                "c2",
                calendar_event("u2", &["cal1"]),
            )
            .await
            .unwrap();
        assert_eq!(count(&backend, "acc1", "cal1"), 2);

        backend
            .destroy_object::<jmap_calendars_types::CalendarEvent>(&(), &Id::from("acc1"), &id1)
            .await
            .unwrap();
        assert_eq!(count(&backend, "acc1", "cal1"), 1);
        assert!(has_key(&backend, "acc1", "cal1"));
    }

    /// Oracle: `apply_calendar_event_index_delta` handles the
    /// update-style transition where calendarIds change from {cal1} to
    /// {cal2}. The unit-level test bypasses the full /set machinery and
    /// drives the helper directly so the diff math is exercised in
    /// isolation. Expected: cal1 count drops to 0 and the entry is
    /// removed; cal2 count rises to 1.
    #[test]
    fn apply_delta_old_to_new_swaps_calendar_ids() {
        let backend = MemoryBackend::new().with_account("acc1");
        let old = serde_json::json!({"calendarIds": {"cal1": true}});
        let new = serde_json::json!({"calendarIds": {"cal2": true}});

        // First, pretend an event in cal1 already exists.
        {
            let mut inner = backend.inner.lock().unwrap();
            inner.apply_calendar_event_index_delta("acc1", None, Some(&old));
        }
        assert_eq!(count(&backend, "acc1", "cal1"), 1);

        // Now apply the swap.
        {
            let mut inner = backend.inner.lock().unwrap();
            inner.apply_calendar_event_index_delta("acc1", Some(&old), Some(&new));
        }
        assert!(!has_key(&backend, "acc1", "cal1"));
        assert_eq!(count(&backend, "acc1", "cal2"), 1);
    }

    /// Oracle: the no-op case — when `old` and `new` have identical
    /// calendarIds, the delta is empty and counters are unchanged.
    #[test]
    fn apply_delta_identical_old_new_is_noop() {
        let backend = MemoryBackend::new().with_account("acc1");
        let v = serde_json::json!({"calendarIds": {"cal1": true, "cal2": true}});

        // Establish baseline.
        {
            let mut inner = backend.inner.lock().unwrap();
            inner.apply_calendar_event_index_delta("acc1", None, Some(&v));
        }
        assert_eq!(count(&backend, "acc1", "cal1"), 1);
        assert_eq!(count(&backend, "acc1", "cal2"), 1);

        // Apply old==new; nothing should change.
        {
            let mut inner = backend.inner.lock().unwrap();
            inner.apply_calendar_event_index_delta("acc1", Some(&v), Some(&v));
        }
        assert_eq!(count(&backend, "acc1", "cal1"), 1);
        assert_eq!(count(&backend, "acc1", "cal2"), 1);
    }

    /// Oracle: a value with no `calendarIds` field contributes no ids.
    /// Tests the defensive code path that handles malformed or partial
    /// JSON without panicking.
    #[test]
    fn apply_delta_value_without_calendar_ids_contributes_nothing() {
        let backend = MemoryBackend::new().with_account("acc1");
        let bare = serde_json::json!({"id": "evt1"}); // no calendarIds
        let with_ids = serde_json::json!({"calendarIds": {"cal1": true}});

        {
            let mut inner = backend.inner.lock().unwrap();
            // bare -> with_ids: incr cal1 only
            inner.apply_calendar_event_index_delta("acc1", Some(&bare), Some(&with_ids));
        }
        assert_eq!(count(&backend, "acc1", "cal1"), 1);

        {
            let mut inner = backend.inner.lock().unwrap();
            // with_ids -> bare: decr cal1 to 0, remove
            inner.apply_calendar_event_index_delta("acc1", Some(&with_ids), Some(&bare));
        }
        assert!(!has_key(&backend, "acc1", "cal1"));
    }
}
