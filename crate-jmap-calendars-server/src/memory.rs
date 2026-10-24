//! In-memory reference implementation of [`CalendarsBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`CalendarsBackend`]
//!    trait to study when writing a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Calendars dispatcher
//!    with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and a number
//! of draft-ietf-jmap-calendars-33 edge cases are simplified (see source
//! comments). In particular, recurrence expansion
//! (`CalendarEvent/query` with `expandRecurrences: true`) and iTIP
//! scheduling-message delivery (`sendSchedulingMessages: true`) are not
//! implemented — the trait's default implementations are inherited.
//! The same holds for the §5.9 uid/privacy rule
//! (`CalendarsBackend::privacy_conflicts`) and the §5.10 private-copy
//! rule (`CalendarsBackend::may_copy_calendar_event`): both fail open.
//! The §5.10 rule needs an account-ownership model, which this backend
//! does not have; the §5.9 rule could be computed from the stored
//! events but is left unimplemented to keep the reference backend small.
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
//! Every `lock()` in this module ends in a bare `.unwrap()` on purpose.
//! `Mutex::lock` fails only when another thread panicked while holding the
//! guard, so the state behind it may be half-updated; that panic is
//! already a bug in this module, and there is no consistent state to
//! recover. Panicking again surfaces it instead of serving corrupt data.
//! The crate's tests never poison the lock. A production backend that must
//! survive a poisoned lock should implement the trait with its own locking
//! rather than copy this pattern.
//!
//! # Tracking
//!
//! Greenfield reference impl per Beads issue JMAP-hwdv (epic) and
//! JMAP-hwdv.5 (this crate, mirror of canonical JMAP-hwdv.1 in
//! jmap-mail-server, following the multi-type-store shape established
//! by jmap-chat-server's `MemoryBackend`).

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::{CalendarsBackend, SetDefaultResult};
use jmap_calendars_types::{Calendar, CalendarEvent, CalendarEventFilter};
use jmap_metadata_server::{
    ChangedProperties, MetadataCondition, MetadataFilterError, MetadataOnlyChanges, SetKind,
    changed_properties, check_metadata_depth, check_metadata_write, is_metadata_property,
    metadata_path_exists, metadata_text_contains, metadata_text_equals,
};
use jmap_metadata_types::{DataTypeMetadataInfo, MetadataTextMatch};
use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams,
    QueryResult, SetError, SetErrorType, SetObject,
};
use jmap_server::{
    PatchPathError, apply_patch_object, check_patch_paths, limit_or_max, resolve_query_start,
};
use jmap_types::{Id, JmapError, State};

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

    /// Construct a [`MemoryError`] for a `serde_json` failure during
    /// `context` (the operation that failed).
    ///
    /// Only the error's [`serde_json::error::Category`] is kept. A serde
    /// data error quotes the offending value (`invalid type: string
    /// "alice@example.com", expected u64`), and the `JmapBackend::Error`
    /// contract forbids PII and blob content in `Display` output
    /// (bd:JMAP-510h.37).
    pub(crate) fn serde(context: impl std::fmt::Display, e: &serde_json::Error) -> Self {
        Self::new(format!("{context}: serde_json {:?} error", e.classify()))
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
    /// The top-level properties this change altered on every id in
    /// `updated`, or `None` when not known. Feeds
    /// draft-ietf-jmap-metadata-04 §3.3 `ignoreMetadataOnlyChanges` and
    /// `updatedProperties`.
    updated_props: Option<BTreeSet<String>>,
}

impl ChangeEntry {
    /// `true` for an update that changed only `metadata` and/or
    /// `privateMetadata` (draft-ietf-jmap-metadata-04 §3.3).
    fn is_metadata_only_update(&self) -> bool {
        self.created.is_empty()
            && self.destroyed.is_empty()
            && self.updated_props.as_ref().is_some_and(|props| {
                !props.is_empty() && props.iter().all(|p| is_metadata_property(p))
            })
    }
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
    /// (draft-ietf-jmap-calendars-33 §4.4.1 when `onDestroyRemoveEvents`
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
    /// (draft-ietf-jmap-calendars-33 §4.3 `onSuccessSetIsDefault`).
    default_calendar: Option<Id>,
    /// Current default ParticipantIdentity id, if set
    /// (draft-ietf-jmap-calendars-33 §3.3 `onSuccessSetIsDefault`).
    default_participant_identity: Option<Id>,
}

/// Shared inner state, behind Arc<Mutex>.
#[derive(Default)]
struct Inner {
    /// `type_name` → `account_id` → `id → serialized object`
    objects: HashMap<&'static str, HashMap<String, HashMap<Id, serde_json::Value>>>,
    /// `type_name` → `account_id` → current state counter
    states: HashMap<&'static str, HashMap<String, u64>>,
    /// `type_name` → `account_id` → ordered change entries
    change_log: HashMap<&'static str, HashMap<String, Vec<ChangeEntry>>>,
    /// `account_id` → auxiliary per-account state (defaults, derived indexes)
    aux: HashMap<String, AccountAux>,
    /// Explicitly registered account ids (accounts may exist with no objects yet).
    known_accounts: HashSet<String>,
    /// `type_name` → the draft-ietf-jmap-metadata-04 §1.2.1
    /// DataTypeMetadataInfo set by [`MemoryBackend::with_metadata_info`].
    /// A type without an entry does not have the metadata properties.
    metadata_info: HashMap<&'static str, DataTypeMetadataInfo>,
}

impl Inner {
    fn current_state(&self, type_name: &'static str, account_id: &str) -> u64 {
        self.states
            .get(type_name)
            .and_then(|m| m.get(account_id))
            .copied()
            .unwrap_or(0)
    }

    fn bump_state(&mut self, type_name: &'static str, account_id: &str) -> u64 {
        let entry = self
            .states
            .entry(type_name)
            .or_default()
            .entry(account_id.to_owned())
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
            .entry(type_name)
            .or_default()
            .entry(account_id.to_owned())
            .or_default()
    }

    fn objects_ref(
        &self,
        type_name: &'static str,
        account_id: &str,
    ) -> Option<&HashMap<Id, serde_json::Value>> {
        self.objects.get(type_name).and_then(|m| m.get(account_id))
    }

    fn change_log_mut(
        &mut self,
        type_name: &'static str,
        account_id: &str,
    ) -> &mut Vec<ChangeEntry> {
        self.change_log
            .entry(type_name)
            .or_default()
            .entry(account_id.to_owned())
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
        if let Some(map) = self
            .objects
            .get("CalendarEvent")
            .and_then(|m| m.get(account_id))
        {
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
/// Cloning is cheap and **shared-state**; see the [`Clone`] impl.
///
/// # Caveats (bd:JMAP-ic0j.16)
///
/// This is a **reference / study impl**, not a production backend. The
/// following limitations are intentional and would need to be addressed
/// by a production-grade `CalendarsBackend` implementor:
///
/// - **Sort is not supported.** `query_calendar_events` and the generic
///   `query_objects` honor no comparator: ids are returned in lexical-by-id
///   order, and a non-empty `sort` is rejected with `unsupportedSort` (RFC
///   8620 §5.5: "A server MUST reject the call with an unsupportedSort
///   error if it cannot sort by the given properties"). A production
///   backend must honor the draft-ietf-jmap-calendars-33 §5.11.2 sort
///   properties.
/// - **Filter coverage is narrow.** Only the `inCalendar` condition (alone
///   or in an operator tree) is honored on `CalendarEvent/query` (used by
///   `Calendar/set` cleanup), plus the draft-ietf-jmap-metadata-04 §3.6
///   shared-metadata conditions once [`MemoryBackend::with_metadata_info`]
///   opts CalendarEvent in; a filter naming any other
///   `CalendarEventFilterCondition` field, and any non-empty filter on
///   another type, is rejected with `unsupportedFilter`. Production
///   backends MUST honor every documented filter.
/// - **No recurrence expansion.** The `expandRecurrences` argument is
///   not implemented; the handler's spec-mandated bound-check still
///   runs, but recurring events are returned as single rows. Production
///   backends MUST expand per RFC 5545 / RFC 8984.
/// - **No availability calculation.** `get_availability` falls through
///   to the default trait impl, which returns an empty result. Production
///   backends MUST implement free/busy lookup per
///   draft-ietf-jmap-calendars-33 §2.2.
///
/// All four caveats are out of scope for the reference impl's purpose —
/// they are the *reason* production deployments override the backend
/// trait rather than reusing `MemoryBackend`.
#[derive(Default)]
#[non_exhaustive]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

/// Manual `Clone` impl, not `#[derive(Clone)]`: cloning is **shared-state**.
///
/// A clone copies no data. The original and the clone hold `Arc::clone`s
/// of the same `Mutex`-guarded store and observe each other's mutations, so
/// one handle can seed or inspect state while another serves requests. This
/// impl must never become a deep copy; for independent state construct a
/// fresh [`MemoryBackend::new`] instead of calling `.clone()`.
impl Clone for MemoryBackend {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl MemoryBackend {
    /// Create a new, empty `MemoryBackend`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `account_id` as known and return `self`, for
    /// builder-style fixture setup. Same as [`Self::register_account`]
    /// (idempotent; the id is stored without RFC 8620 §1.2 validation).
    ///
    /// # Panics
    ///
    /// In debug builds, if `account_id` is the empty string (RFC 8620
    /// §1.2, bd:JMAP-dx0s).
    #[must_use]
    pub fn with_account(self, account_id: &Id) -> Self {
        self.register_account(account_id);
        self
    }

    /// Give `data_type` the draft-ietf-jmap-metadata-04 `metadata`
    /// property, configured by `info` (the DataTypeMetadataInfo of -04
    /// §1.2.1).
    ///
    /// Without this call a type has no metadata: `Foo/set` rejects a
    /// write to `metadata` or `privateMetadata` with `invalidProperties`
    /// and `Foo/query` rejects a metadata filter condition with
    /// `unsupportedFilter`. With it, `Foo/set` validates metadata writes
    /// (-04 §3.2, §4.1), `CalendarEvent/query` evaluates the -04 §3.6
    /// conditions, and `Foo/changes` honours `ignoreMetadataOnlyChanges`
    /// and reports `updatedProperties` (-04 §3.3). Advertising the
    /// capability in the session is the server's job: list `info` under
    /// `data_type` in the account's `urn:ietf:params:jmap:metadata`
    /// `dataTypes`.
    ///
    /// Calendar writes need `myRights.maySetMetadata` on the Calendar, and
    /// CalendarEvent writes need it on every Calendar the event is in
    /// before and after the update, or on the `calendarIds` of a create
    /// (-04 §4.1). An absent `maySetMetadata` allows the write (fail-open,
    /// JMAP-bghk). This backend never sets `updated`, so a metadata-only
    /// change leaves it alone, as -04 §3.1 requires.
    ///
    /// # Panics
    ///
    /// Panics if `data_type` is not `"Calendar"` or `"CalendarEvent"`, the
    /// two types draft-ietf-jmap-metadata-04 §4.1 names for this
    /// extension, or if `info.supports_private` is true: this backend has
    /// no per-user identity (`CallerCtx` is `()`) to own `privateMetadata`
    /// (-04 §2).
    #[must_use]
    pub fn with_metadata_info(self, data_type: &str, info: DataTypeMetadataInfo) -> Self {
        let type_name = metadata_type_name(data_type).unwrap_or_else(|| {
            panic!(
                "MemoryBackend supports metadata only on Calendar and CalendarEvent, not {data_type:?}"
            )
        });
        assert!(
            !info.supports_private,
            "MemoryBackend does not support privateMetadata (supportsPrivate must be false)"
        );
        self.inner
            .lock()
            .unwrap()
            .metadata_info
            .insert(type_name, info);
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
        debug_assert!(
            !account_id.as_ref().is_empty(),
            "register_account: account_id must not be the empty string \
             (RFC 8620 §1.2, bd:JMAP-dx0s)"
        );
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
        account_id: &Id,
        type_name: &'static str,
        id: &Id,
        value: serde_json::Value,
    ) {
        const KNOWN_TYPES: &[&str] = &[
            jmap_calendars_types::Calendar::TYPE_NAME,
            jmap_calendars_types::CalendarEvent::TYPE_NAME,
            jmap_calendars_types::CalendarEventNotification::TYPE_NAME,
            jmap_calendars_types::ParticipantIdentity::TYPE_NAME,
        ];
        assert!(
            KNOWN_TYPES.contains(&type_name),
            "seed_object: type_name {type_name:?} is not one of the known \
             TYPE_NAME values {KNOWN_TYPES:?}"
        );
        let obj = value.as_object().unwrap_or_else(|| {
            panic!("seed_object: value must be a JSON object with an `id` field, got {value:?}")
        });
        let value_id = obj
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| {
                panic!(
                    "seed_object: value must contain an `id` string field; got value = {value:?}"
                )
            });
        assert_eq!(
            value_id,
            id.as_ref(),
            "seed_object: value's `id` field {value_id:?} does not match the \
             `id` argument {id:?}"
        );
        let account_id = account_id.as_ref();
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.to_owned());
        inner
            .objects_mut(type_name, account_id)
            .insert(id.clone(), value);
        // bd:JMAP-ic0j.8 — seed_object bypasses the trait-impl mutators
        // so we have no before/after pair to feed
        // `apply_calendar_event_index_delta`; fall back to a full rebuild
        // of `calendar_event_counts`. seed_object is only called from
        // test fixtures, so the `O(N)` rebuild cost is acceptable here.
        if type_name == jmap_calendars_types::CalendarEvent::TYPE_NAME {
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
}

// ---------------------------------------------------------------------------
// JmapBackend impl (read-side supertrait)
// ---------------------------------------------------------------------------

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    /// The info set by [`MemoryBackend::with_metadata_info`], the same for
    /// every account.
    fn metadata_info<O: JmapObject>(
        &self,
        _caller: &(),
        _account_id: &Id,
    ) -> Option<DataTypeMetadataInfo> {
        self.inner
            .lock()
            .unwrap()
            .metadata_info
            .get(O::TYPE_NAME)
            .cloned()
    }

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
                                return Err(MemoryError::serde(
                                    format_args!("deserialize {}", O::TYPE_NAME),
                                    &e,
                                ));
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
                                return Err(MemoryError::serde(
                                    format_args!("deserialize {}", O::TYPE_NAME),
                                    &e,
                                ));
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
        params: jmap_server::ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        let since_state = params.since_state;
        let max_changes = params.max_changes;
        // draft-ietf-jmap-metadata-04 §3: jmap_server::handle_changes has
        // already folded a `using` without the capability into this flag.
        let mode = if params.ignore_metadata_only_changes {
            MetadataOnlyChanges::Ignore
        } else {
            MetadataOnlyChanges::Report
        };
        let inner = self.inner.lock().unwrap();

        let since_n: u64 = since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::CannotCalculate)?;

        let log = inner
            .change_log
            .get(O::TYPE_NAME)
            .and_then(|m| m.get(account_id.as_ref()))
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        let mut relevant: Vec<&ChangeEntry> =
            log.iter().filter(|e| e.new_state > since_n).collect();

        let current_state = inner.current_state(O::TYPE_NAME, account_id.as_ref());

        // RFC 8620 §5.2: over maxChanges "the server SHOULD generate an
        // update to take the client to an intermediate state", and only
        // when "unable to calculate an intermediate state" return
        // cannotCalculateChanges. Whole entries are taken, oldest first,
        // while their ids fit; an entry's ids bound what it adds after the
        // dedup below. A metadata-only update under Ignore adds none: -04
        // §3.3 "Excluded ids do not count toward maxChanges". The first
        // entry alone over the limit has no intermediate state.
        let mut has_more = false;
        if let Some(max) = max_changes {
            let mut ids = 0u64;
            if let Some(cut) = relevant.iter().position(|e| {
                if !(mode == MetadataOnlyChanges::Ignore && e.is_metadata_only_update()) {
                    ids += (e.created.len() + e.updated.len() + e.destroyed.len()) as u64;
                }
                ids > max
            }) {
                if cut == 0 {
                    return Err(BackendChangesError::TooManyChanges { limit: max });
                }
                relevant.truncate(cut);
                has_more = true;
            }
        }
        let new_state = match relevant.last() {
            Some(e) if has_more => e.new_state,
            _ => current_state,
        };

        // bd:JMAP-ic0j.50 — port the canonical jmap-mail-server HashMap+IdFate
        // dedup pattern.
        //
        // bd:JMAP-ic0j.75 — align with the canonical SHOULD-preferred path of
        // RFC 8620 §5.2: "If a record was created and destroyed within the
        // window, the server SHOULD remove the id from the response entirely."
        // The canonical jmap-mail-server at crate-jmap-mail-server/src/memory/
        // mod.rs:670-680 chose this path (1). calendars-server previously chose
        // the MAY-path (path 2, "include it in just the 'destroyed' list") as a
        // pre-existing carryover from the Vec-based code; this aligns with the
        // canonical per the workspace cookie-cutter consistency rule.
        //
        // bd:JMAP-ic0j.15 recycling-detection: the SHOULD-preferred destroy arm
        // REMOVES the `Created` fate from the map, so a recycled-create from a
        // later entry would no longer find a `Destroyed` sentinel and the
        // original `debug_assert!` would miss the bug. To preserve the
        // recycling invariant check independently of the dedup state, track a
        // separate `ever_destroyed` set that records every id that appeared
        // in a `destroyed` list within the window. The assertion fires when a
        // later `created` or `updated` entry references an id already in
        // `ever_destroyed`.
        let mut fates: HashMap<Id, IdFate> = HashMap::new();
        let mut ever_destroyed: HashSet<Id> = HashSet::new();
        let mut window_props = ChangedProperties::new();
        for entry in &relevant {
            for id in &entry.created {
                // Under bd:JMAP-ic0j.2's monotonic-counter invariant a
                // mint-after-destroy never re-uses an id, so a `created` id
                // that has already appeared in a `destroyed` list within this
                // window would mean a later entry minted the same id as an
                // earlier `destroyed` entry. This `debug_assert!` trips
                // immediately in tests if a future regression in id minting
                // re-introduces recycling.
                debug_assert!(
                    !ever_destroyed.contains(id),
                    "MemoryBackend change_log invariant violated: id {id} \
                     appears in entry.created after appearing in an earlier \
                     entry.destroyed. This indicates id recycling in the \
                     demo id minter (regression of bd:JMAP-ic0j.2)."
                );
                fates.insert(id.clone(), IdFate::Created);
            }
            for id in &entry.updated {
                // Same recycling guard for updated. An update on a
                // previously-destroyed id is equally impossible under
                // the monotonic-counter invariant.
                debug_assert!(
                    !ever_destroyed.contains(id),
                    "MemoryBackend change_log invariant violated: id {id} \
                     appears in entry.updated after appearing in an earlier \
                     entry.destroyed. This indicates id recycling in the \
                     demo id minter (regression of bd:JMAP-ic0j.2)."
                );
                window_props.record(id.as_ref(), entry.updated_props.as_ref());
                let fate = match fates.get(id) {
                    Some(IdFate::Created) => IdFate::Created,
                    Some(IdFate::Destroyed) => IdFate::Destroyed,
                    _ => IdFate::Updated,
                };
                fates.insert(id.clone(), fate);
            }
            for id in &entry.destroyed {
                // bd:JMAP-ic0j.75 — RFC 8620 §5.2 SHOULD-preferred path: if an
                // id was created within this window and is now destroyed, omit
                // it from BOTH `created` and `destroyed` lists. The client
                // never knew about it (its `since` predates the create).
                // Otherwise (updated-in-window, or fresh destroy of a record
                // the client knew about), include in `destroyed`.
                match fates.remove(id) {
                    Some(IdFate::Created) => {} // created+destroyed in window → omit
                    Some(_) | None => {
                        fates.insert(id.clone(), IdFate::Destroyed);
                    }
                }
                ever_destroyed.insert(id.clone());
            }
        }

        let mut created = Vec::new();
        let mut updated = Vec::new();
        let mut destroyed = Vec::new();
        for (id, fate) in fates {
            match fate {
                IdFate::Created => created.push(id),
                // -04 §3.3: under Ignore, "exclude from the updated array
                // any id whose only changes since sinceState ... are
                // confined to metadata and/or privateMetadata".
                IdFate::Updated
                    if mode == MetadataOnlyChanges::Ignore
                        && window_props.is_metadata_only(id.as_ref()) => {}
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

        // -04 §3.3 `updatedProperties`; no calendars type has the RFC 8621
        // §2.2 count properties that only Mailbox/changes may list.
        let updated_properties = window_props.updated_properties(&updated, mode, &[]);
        Ok(ChangesResult::new(
            created,
            updated,
            destroyed,
            has_more,
            State::from(new_state.to_string()),
        )
        .with_updated_properties(updated_properties))
    }

    /// Reference query_objects impl — see the module-level "Caveats" doc
    /// (bd:JMAP-ic0j.16) for the sort / filter / position semantics the
    /// reference impl does NOT honor.
    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        let QueryParams {
            filter,
            sort,
            limit,
            position,
            anchor,
            anchor_offset,
            ..
        } = params;
        // bd:JMAP-ic0j.16 — this backend honors no comparator and returns
        // ids in lexical-by-id order, so a non-empty sort fails loud. RFC
        // 8620 §5.5: "A server MUST reject the call with an unsupportedSort
        // error if it cannot sort by the given properties."
        if sort.is_some_and(|s| !s.is_empty()) {
            return Err(BackendQueryError::Method(JmapError::unsupported_sort()));
        }
        let inner = self.inner.lock().unwrap();

        // For CalendarEvent, evaluate the RFC 8620 §5.5 filter tree over
        // the `inCalendar` condition property (used by `Calendar/set`
        // cleanup when `onDestroyRemoveEvents: true`). Other types evaluate
        // no condition property and no operator tree. Anything this backend
        // cannot evaluate is answered with `unsupportedFilter` (RFC 8620
        // §5.5: "A server MUST reject the call with an unsupportedFilter
        // error if it cannot process the given filter"), rather than
        // treating the unevaluated part as a match.
        //
        // Type-identity roundtrip: when O::TYPE_NAME == "CalendarEvent" the
        // generic O::Filter is necessarily CalendarEventFilter (see the
        // QueryObject impl in jmap-calendars-types src/backend.rs). A failure
        // of either half means that dispatch has drifted; it surfaces as
        // serverFail rather than a panic or a silently dropped filter.
        //
        // draft-ietf-jmap-metadata-04 §3.6: the shared `metadata*` conditions
        // are evaluable too once the type is opted in
        // ([`MemoryBackend::with_metadata_info`]); [`check_metadata_filter`]
        // validates them first, so a malformed path is `invalidArguments`.
        let is_event = O::TYPE_NAME == CalendarEvent::TYPE_NAME;
        let mut event_filter: Option<CalendarEventFilter> = None;
        if let Some(f) = filter {
            let v = jmap_server::serialize_value(f).map_err(BackendQueryError::Method)?;
            check_metadata_filter(&v, inner.metadata_info.get(O::TYPE_NAME))?;
            let evaluable: &[&str] = if is_event {
                &[
                    "inCalendar",
                    "metadataExists",
                    "metadataTextContains",
                    "metadataTextEquals",
                ]
            } else {
                &[]
            };
            if !filter_is_evaluable(&v, evaluable, is_event) {
                return Err(BackendQueryError::Method(JmapError::unsupported_filter()));
            }
            if is_event {
                event_filter = Some(serde_json::from_value(v).map_err(|e| {
                    BackendQueryError::Method(JmapError::server_fail(e.to_string()))
                })?);
            }
        }

        let mut ids: Vec<Id> = inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .map(|m| {
                let mut keys: Vec<(Id, &serde_json::Value)> =
                    m.iter().map(|(k, v)| (k.clone(), v)).collect();
                keys.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
                keys.into_iter()
                    .filter(|(_, v)| {
                        event_filter.as_ref().is_none_or(|f| {
                            f.matches(&|c| {
                                c.in_calendar.as_ref().is_none_or(|target| {
                                    v.get("calendarIds")
                                        .and_then(|ids| ids.as_object())
                                        .is_some_and(|map| map.contains_key(target.as_ref()))
                                }) && metadata_matches(
                                    v.get("metadata").and_then(serde_json::Value::as_object),
                                    c.metadata_exists.as_deref(),
                                    c.metadata_text_contains.as_ref(),
                                    c.metadata_text_equals.as_ref(),
                                )
                            })
                        })
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

        // RFC 8620 §5.5 position / anchor / anchorOffset.
        let start = resolve_query_start(&ids, position, anchor, anchor_offset)
            .map_err(BackendQueryError::Method)?;

        ids = ids[start..]
            .iter()
            .take(limit_or_max(limit))
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

    /// Returns the changes to a `/query` result set since `since_query_state`.
    ///
    /// RFC 8620 §5.6: `/queryChanges` reports changes to the results of
    /// the same query, so `filter` and `sort` are those of the original
    /// `/query` and are applied by re-running [`Self::query_objects`]. A
    /// filter or sort it refuses (`unsupportedFilter`/`unsupportedSort`)
    /// yields `cannotCalculateChanges` here. `removed` holds the destroyed
    /// and updated ids; `added` holds the created and updated ids that match
    /// now, with their index in the current result. Mirrors
    /// `jmap-tasks-server` `MemoryBackend::query_changes`.
    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        let QueryChangesParams {
            since_query_state,
            filter,
            sort,
            max_changes,
            ..
        } = params;
        let changes = self
            .get_changes::<O>(
                caller,
                account_id,
                jmap_server::ChangesParams::new(since_query_state).max_changes(max_changes),
            )
            .await?;

        // The current result of the same query, unpaginated: `added`
        // indices are positions in it (RFC 8620 §5.6).
        let current = self
            .query_objects::<O>(
                caller,
                account_id,
                QueryParams::new().filter(filter).sort(sort),
            )
            .await
            .map_err(|e| match e {
                BackendQueryError::Other(e) => BackendChangesError::Other(e),
                // Changes to a query the server cannot run cannot be
                // calculated; the client's follow-up /query receives the
                // method error itself.
                _ => BackendChangesError::CannotCalculate,
            })?;
        let created_set: HashSet<&Id> = changes.created.iter().collect();
        let updated_set: HashSet<&Id> = changes.updated.iter().collect();

        // Every destroyed or updated id: an update may touch a filter or
        // sort property, so RFC 8620 §5.6 requires the id in `removed` and,
        // if it still matches, reinserted via `added`.
        let mut removed: Vec<Id> = changes
            .destroyed
            .iter()
            .chain(changes.updated.iter())
            .cloned()
            .collect();
        removed.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        removed.dedup();

        // Created or updated ids that match the query now.
        let added: Vec<AddedItem> = current
            .ids
            .iter()
            .enumerate()
            .filter(|(_, id)| created_set.contains(id) || updated_set.contains(id))
            .map(|(pos, id)| AddedItem::new(id.clone(), pos as u64))
            .collect();

        Ok(QueryChangesResult::new(
            since_query_state.clone(),
            changes.new_state,
            current.total,
            removed,
            added,
        ))
    }
}

/// `true` if this reference backend can evaluate `filter`, a serialized
/// RFC 8620 §5.5 `FilterOperator|FilterCondition`.
///
/// A condition may set only the properties in `evaluable` (a `null` value
/// counts as unset). An operator tree is accepted only when `trees` is set
/// and every sub-filter is itself evaluable.
fn filter_is_evaluable(filter: &serde_json::Value, evaluable: &[&str], trees: bool) -> bool {
    if filter.get("operator").is_some() {
        return trees
            && filter
                .get("conditions")
                .and_then(|c| c.as_array())
                .is_some_and(|c| c.iter().all(|f| filter_is_evaluable(f, evaluable, trees)));
    }
    filter.as_object().is_some_and(|m| {
        m.iter()
            .all(|(k, v)| v.is_null() || evaluable.contains(&k.as_str()))
    })
}

/// The `TYPE_NAME` of the type named `data_type` if it is one this backend
/// can give draft-ietf-jmap-metadata-04 properties: -04 §4.1 names
/// Calendar, and CalendarEvent among the types that take the right from
/// their containers.
fn metadata_type_name(data_type: &str) -> Option<&'static str> {
    [Calendar::TYPE_NAME, CalendarEvent::TYPE_NAME]
        .into_iter()
        .find(|t| *t == data_type)
}

fn invalid_metadata_property(property: &str, description: &str) -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(
        SetError::new(SetErrorType::InvalidProperties)
            .with_properties([property])
            .with_description(description),
    ))
}

impl Inner {
    /// draft-ietf-jmap-metadata-04 §3.2 rules 1 to 3 for a metadata write
    /// on a `type_name` object, before the write is applied. Returns the
    /// type's DataTypeMetadataInfo for the rule 4 check after applying,
    /// or `None` when `entry` writes no metadata.
    ///
    /// `entry` is the create object or update patch, `stored` the object
    /// before an update (`None` on create), and `containers` the Calendar
    /// ids whose `maySetMetadata` a CalendarEvent write needs (-04 §4.1).
    /// Rule 1's `notFound` is the caller's: this backend has no read
    /// rights, so only a missing object is unreadable.
    fn check_metadata_set(
        &self,
        type_name: &'static str,
        account_id: &str,
        kind: SetKind,
        entry: &serde_json::Map<String, serde_json::Value>,
        stored: Option<&serde_json::Map<String, serde_json::Value>>,
        containers: &[&str],
    ) -> Result<Option<DataTypeMetadataInfo>, BackendSetError<MemoryError>> {
        let Some(first) = entry.keys().find(|k| crate::helpers::is_metadata_key(k)) else {
            return Ok(None);
        };
        // -04 §1.2.1: an unlisted type "does not gain the metadata or
        // privateMetadata properties".
        let Some(info) = self.metadata_info.get(type_name) else {
            return Err(invalid_metadata_property(
                first,
                "metadata is not supported on this data type",
            ));
        };
        // Rule 1 / -04 §4.1: "A write to metadata without maySetMetadata
        // MUST be rejected with a "forbidden" SetError". A Calendar has
        // its own rights; a CalendarEvent "takes the right from its
        // containers". An absent right or container allows the write
        // (fail-open, JMAP-bghk).
        let writes_shared = entry.keys().any(|k| {
            crate::helpers::is_metadata_key(k)
                && k.split_once('/').map_or(k.as_str(), |(root, _)| root) == "metadata"
        });
        let denies = |rights: Option<&serde_json::Value>| {
            rights
                .and_then(|r| r.get("maySetMetadata"))
                .and_then(serde_json::Value::as_bool)
                == Some(false)
        };
        let forbidden = writes_shared
            && if type_name == Calendar::TYPE_NAME {
                denies(stored.and_then(|c| c.get("myRights")))
            } else {
                let calendars = self.objects_ref(Calendar::TYPE_NAME, account_id);
                containers.iter().any(|id| {
                    denies(
                        calendars
                            .and_then(|c| c.get(*id))
                            .and_then(|cal| cal.get("myRights")),
                    )
                })
            };
        if forbidden {
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::Forbidden)
                    .with_description("maySetMetadata is required to write metadata"),
            )));
        }
        check_metadata_write(kind, entry, stored, info)
            .map_err(|e| invalid_metadata_property(e.property(), e.description()))?;
        Ok(Some(info.clone()))
    }
}

/// The Calendar ids a CalendarEvent update's metadata write needs
/// `maySetMetadata` on: every Calendar the event is in before the update
/// and every one `patch` adds (-04 §4.1: "on every container both before
/// and after the change").
fn event_update_containers<'a>(
    stored: &'a serde_json::Map<String, serde_json::Value>,
    patch: &'a serde_json::Map<String, serde_json::Value>,
) -> Vec<&'a str> {
    let before = stored
        .get("calendarIds")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|m| m.keys().map(String::as_str));
    let replaced = patch
        .get("calendarIds")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|m| m.iter())
        .filter(|(_, v)| v.as_bool() == Some(true))
        .map(|(k, _)| k.as_str());
    let added = patch.iter().filter_map(|(k, v)| {
        k.strip_prefix("calendarIds/")
            .filter(|_| v.as_bool() == Some(true))
    });
    before.chain(replaced).chain(added).collect()
}

/// draft-ietf-jmap-metadata-04 §3.2 checks for a created object `val` of
/// type `type_name`: rules 1 to 3 on its metadata properties, then rule 4
/// on the stored value. A CalendarEvent needs `maySetMetadata` on the
/// Calendars of its `calendarIds` (-04 §4.1: "on create, on the
/// containers given in the create"). Types without the -04 properties
/// pass unchecked.
fn check_metadata_create(
    inner: &Inner,
    type_name: &'static str,
    account_id: &str,
    val: &serde_json::Value,
) -> Result<(), BackendSetError<MemoryError>> {
    let (Some(type_name), Some(obj)) = (metadata_type_name(type_name), val.as_object()) else {
        return Ok(());
    };
    let entry: serde_json::Map<String, serde_json::Value> = obj
        .iter()
        .filter(|(k, _)| is_metadata_property(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let containers: Vec<&str> = if type_name == CalendarEvent::TYPE_NAME {
        obj.get("calendarIds")
            .and_then(serde_json::Value::as_object)
            .map(|m| m.keys().map(String::as_str).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if let Some(info) = inner.check_metadata_set(
        type_name,
        account_id,
        SetKind::Create,
        &entry,
        None,
        &containers,
    )? {
        check_metadata_depth(&entry, obj, &info)
            .map_err(|e| invalid_metadata_property(e.property(), e.description()))?;
    }
    Ok(())
}

/// The six draft-ietf-jmap-metadata-04 §3.6 FilterCondition fields.
const METADATA_CONDITION_KEYS: [&str; 6] = [
    "metadataExists",
    "privateMetadataExists",
    "metadataTextContains",
    "privateMetadataTextContains",
    "metadataTextEquals",
    "privateMetadataTextEquals",
];

/// Validate the draft-ietf-jmap-metadata-04 §3.6 conditions on every leaf
/// of `filter`, a FilterCondition or FilterOperator tree (RFC 8620 §5.5),
/// before any object is matched.
///
/// A type without `info` has no metadata (-04 §1.2.1: it "does not gain
/// the metadata or privateMetadata properties"), so a metadata leaf is
/// `unsupportedFilter` (RFC 8620 §5.5: "The filter is syntactically valid,
/// but the server cannot process it"). Otherwise
/// [`MetadataCondition::take_from`] applies -04 §3.6: a malformed path is
/// `invalidArguments`, and a `privateMetadata*` condition is
/// `unsupportedFilter` because `supportsPrivate` is false.
fn check_metadata_filter(
    filter: &serde_json::Value,
    info: Option<&DataTypeMetadataInfo>,
) -> Result<(), BackendQueryError<MemoryError>> {
    let unsupported = |description: String| {
        let mut err = JmapError::unsupported_filter();
        err.description = Some(description);
        BackendQueryError::Method(err)
    };
    if filter.get("operator").is_some() {
        return filter
            .get("conditions")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .try_for_each(|f| check_metadata_filter(f, info));
    }
    let Some(leaf) = filter.as_object() else {
        return Ok(());
    };
    if !METADATA_CONDITION_KEYS
        .iter()
        .any(|k| leaf.get(*k).is_some_and(|v| !v.is_null()))
    {
        return Ok(());
    }
    let Some(info) = info else {
        return Err(unsupported(
            "metadata filter conditions are not supported".to_owned(),
        ));
    };
    MetadataCondition::take_from(&mut leaf.clone(), info)
        .map(drop)
        .map_err(|e| match e {
            MetadataFilterError::InvalidArguments(d) => {
                BackendQueryError::Method(JmapError::invalid_arguments(d))
            }
            other => unsupported(other.to_string()),
        })
}

/// Match the draft-ietf-jmap-metadata-04 §3.6 shared-metadata conditions of
/// one filter leaf against an object's `metadata` (absent means `{}`, -04
/// §2). The `privateMetadata*` conditions never reach this point:
/// [`check_metadata_filter`] rejects them, since this backend has
/// `supportsPrivate` false.
fn metadata_matches(
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    exists: Option<&str>,
    contains: Option<&MetadataTextMatch>,
    equals: Option<&MetadataTextMatch>,
) -> bool {
    let empty = serde_json::Map::new();
    let metadata = metadata.unwrap_or(&empty);
    exists.is_none_or(|p| metadata_path_exists(metadata, p))
        && contains.is_none_or(|m| metadata_text_contains(metadata, m))
        && equals.is_none_or(|m| metadata_text_equals(metadata, m))
}

// ---------------------------------------------------------------------------
// CalendarsBackend impl
// ---------------------------------------------------------------------------

impl CalendarsBackend for MemoryBackend {
    async fn create_object<O, F>(
        &self,
        _caller: &(),
        account_id: &Id,
        _create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<Self::Error>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        let mut inner = self.inner.lock().unwrap();

        // accountId existence is enforced at the handler layer (RFC 8620
        // §3.6.2 accountNotFound), but defend here too — silently auto-
        // register so seed_object + create flow without explicit
        // `register_account` calls in tests does not error spuriously.
        inner.known_accounts.insert(account_id.as_ref().to_owned());

        let server_id = Id::from(uuid::Uuid::new_v4().to_string());

        let stored_obj = build(&server_id);
        let val = serde_json::to_value(&stored_obj)
            .map_err(|e| BackendSetError::Other(MemoryError::serde("serialize", &e)))?;
        check_metadata_create(&inner, O::TYPE_NAME, account_id.as_ref(), &val)?;

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
                updated_props: None,
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
        let patch_val = serde_json::to_value(&patch)
            .map_err(|e| BackendSetError::Other(MemoryError::serde("serialize patch", &e)))?;
        let invalid_patch = |e: PatchPathError| {
            BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()),
            ))
        };
        // draft-ietf-jmap-metadata-04 §3.2: "Patch errors that can be
        // determined from the request alone (for example, two paths where
        // one is a prefix of the other) MUST be reported with
        // "invalidPatch" before rule 1 is evaluated."
        if let serde_json::Value::Object(patch_map) = &patch_val {
            check_patch_paths(patch_map).map_err(invalid_patch)?;
        }

        let mut inner = self.inner.lock().unwrap();

        // -04 §3.2 rule 1: "If the user cannot read the related object,
        // reject with "notFound"" comes before "forbidden". Here only a
        // missing object is unreadable.
        let existing = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .get(id)
            .cloned();

        let Some(old_value) = existing else {
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::NotFound,
            ))));
        };
        let mut current = old_value.clone();

        // JMAP patch (RFC 8620 §5.3): keys are JSON Pointer paths (e.g.
        // "participants/p1/participationStatus"), applied all-or-nothing by
        // the shared `apply_patch_object`, so an invalidPatch leaves the
        // stored object unchanged. Values replace wholesale; the helper caps
        // path plus value nesting depth.
        let mut updated_props = None;
        if let (serde_json::Value::Object(base), serde_json::Value::Object(patch_map)) =
            (&mut current, &patch_val)
        {
            // -04 §3.2 rules 1 to 3 run before the patch is applied.
            let metadata_info = if metadata_type_name(O::TYPE_NAME).is_some() {
                let containers = if O::TYPE_NAME == CalendarEvent::TYPE_NAME {
                    event_update_containers(base, patch_map)
                } else {
                    Vec::new()
                };
                inner.check_metadata_set(
                    O::TYPE_NAME,
                    account_id.as_ref(),
                    SetKind::Update,
                    patch_map,
                    Some(base),
                    &containers,
                )?
            } else {
                None
            };
            // -04 §2 `metadata` has "default: {}", so a
            // `metadata/<namespace>` pointer finds its parent.
            if metadata_info.is_some() {
                base.entry("metadata")
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            }
            let before = base.clone();
            // The value-dependent patch errors (missing parent, path through
            // an array) land here, after rule 3 and before rule 4 (-04 §3.2).
            apply_patch_object(base, patch_map).map_err(invalid_patch)?;
            if let Some(info) = &metadata_info {
                // -04 §3.2: "In an update, null is equivalent to {}"; the
                // patch removed the property instead.
                for property in ["metadata", "privateMetadata"] {
                    if patch_map
                        .get(property)
                        .is_some_and(serde_json::Value::is_null)
                    {
                        base.insert(
                            property.to_owned(),
                            serde_json::Value::Object(serde_json::Map::new()),
                        );
                    }
                }
                // -04 §3.2 rule 4 runs on the value as it would be stored.
                check_metadata_depth(patch_map, base, info)
                    .map_err(|e| invalid_metadata_property(e.property(), e.description()))?;
            }
            updated_props = Some(changed_properties(&before, base));
        }

        // RFC 8620 §5.3: the patched object must still be valid. Re-check it
        // against the typed object so a null or wrong-typed required property
        // is rejected here instead of stored and breaking every later /get.
        O::deserialize(&current).map_err(|e| {
            BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_description(format!("patch produced invalid {}: {e}", O::TYPE_NAME)),
            ))
        })?;

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
                updated_props,
            });

        Ok(None)
    }

    async fn update_per_user_properties(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
        patch: jmap_types::PatchObject,
    ) -> Result<Option<jmap_calendars_types::CalendarEvent>, BackendSetError<Self::Error>> {
        // ponytail: single-user backend (CallerCtx = ()), so per-user state is
        // the shared object. `update_object` never writes `updated`, so a
        // per-user-only patch leaves the shared timestamp alone. A multi-user
        // backend stores per-user properties separately.
        self.update_object::<jmap_calendars_types::CalendarEvent>(caller, account_id, id, patch)
            .await
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
                        updated_props: None,
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
            None => Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::NotFound,
            )))),
        }
    }

    fn supports_type<O: JmapObject>(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
    ) -> bool {
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

    // ponytail: MemoryBackend has no iCalendar parser and does not
    // compute busy periods, so every blob is notParsable (§5.13) and
    // every availability window is empty (§2.2). A production backend
    // must implement both; this explicit override is the documented
    // "not supported" shape (bd:JMAP-ic0j.26).
    async fn parse_calendar_event_blobs(
        &self,
        _caller: &(),
        _account_id: &jmap_types::Id,
        blob_ids: &[jmap_types::Id],
        _properties: Option<&[String]>,
    ) -> Result<crate::CalendarEventParseResult, Self::Error> {
        Ok(crate::CalendarEventParseResult {
            parsed: std::collections::BTreeMap::new(),
            not_found: vec![],
            not_parsable: blob_ids.to_vec(),
        })
    }

    async fn get_availability(
        &self,
        _caller: &(),
        _account_id: &jmap_types::Id,
        _principal_id: &jmap_types::Id,
        _utc_start: &jmap_types::UTCDate,
        _utc_end: &jmap_types::UTCDate,
        _show_details: bool,
        _event_properties: Option<&[String]>,
    ) -> Result<Vec<jmap_calendars_types::BusyPeriod>, jmap_server::BackendQueryError<Self::Error>>
    {
        Ok(vec![])
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
        inner
            .change_log
            .entry("Calendar")
            .or_default()
            .insert(account_id.to_owned(), entries);
        inner
            .states
            .entry("Calendar")
            .or_default()
            .insert(account_id.to_owned(), max_state);
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
                    updated_props: None,
                },
                // Entry K=2: destroyed it.
                ChangeEntry {
                    new_state: 2,
                    created: vec![],
                    updated: vec![],
                    destroyed: vec![recycled.clone()],
                    updated_props: None,
                },
                // Entry K=3: re-mints the SAME id (the bug we are
                // guarding against; impossible under bd:JMAP-ic0j.2's
                // invariant).
                ChangeEntry {
                    new_state: 3,
                    created: vec![recycled.clone()],
                    updated: vec![],
                    destroyed: vec![],
                    updated_props: None,
                },
            ],
        );

        // The dedup loop walks entries 1..=3 because since_n=0.
        // Processing entry K=3's `created` triggers the debug_assert.
        let _ = backend
            .get_changes::<Calendar>(
                &(),
                &acc_id,
                jmap_server::ChangesParams::new(&State::from("0")),
            )
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
                    updated_props: None,
                },
                ChangeEntry {
                    new_state: 2,
                    created: vec![],
                    updated: vec![],
                    destroyed: vec![recycled.clone()],
                    updated_props: None,
                },
                ChangeEntry {
                    new_state: 3,
                    created: vec![],
                    updated: vec![recycled.clone()],
                    destroyed: vec![],
                    updated_props: None,
                },
            ],
        );

        let _ = backend
            .get_changes::<Calendar>(
                &(),
                &acc_id,
                jmap_server::ChangesParams::new(&State::from("0")),
            )
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
                    updated_props: None,
                },
                ChangeEntry {
                    new_state: 2,
                    created: vec![],
                    updated: vec![],
                    destroyed: vec![id_a.clone()],
                    updated_props: None,
                },
                // A DIFFERENT id, not the destroyed one.
                ChangeEntry {
                    new_state: 3,
                    created: vec![id_b.clone()],
                    updated: vec![],
                    destroyed: vec![],
                    updated_props: None,
                },
            ],
        );

        let result = backend
            .get_changes::<Calendar>(
                &(),
                &acc_id,
                jmap_server::ChangesParams::new(&State::from("0")),
            )
            .await
            .expect("must not error");

        // bd:JMAP-ic0j.75 — RFC 8620 §5.2 SHOULD-preferred path. id_a was
        // both created and destroyed within the window, so it MUST be
        // omitted from BOTH `created` and `destroyed`: the client (since=0)
        // never knew about it. id_b was created in the window with no
        // matching destroy → final classification is `created`.
        //
        // Oracle: RFC 8620 §5.2 verbatim — "If a record was created and
        // then destroyed within the window … the server SHOULD remove the
        // id from the response entirely."
        assert!(
            result.destroyed.is_empty(),
            "create+destroy-in-window id must be omitted from destroyed: got {:?}",
            result.destroyed
        );
        assert_eq!(
            result.created,
            vec![id_b],
            "newly-minted id appears in created (create+destroy-in-window id is omitted)"
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
    /// jmap-calendars-types `src/backend.rs` (Calendar, CalendarEvent,
    /// CalendarEventNotification, ParticipantIdentity). Anything else
    /// is a test-author typo.
    #[test]
    #[should_panic(expected = "not one of the known")]
    fn seed_object_rejects_unknown_type_name() {
        let backend = MemoryBackend::new();
        backend.seed_object(
            &Id::from("acc1"),
            "calendarevent",
            &Id::from("ev1"),
            json!({"id": "ev1"}),
        );
    }

    /// Regression for bd:JMAP-ic0j.32: a non-object value panics
    /// instead of silently storing data that get_objects cannot
    /// deserialize.
    #[test]
    #[should_panic(expected = "must be a JSON object")]
    fn seed_object_rejects_non_object_value() {
        let backend = MemoryBackend::new();
        backend.seed_object(&Id::from("acc1"), "Calendar", &Id::from("cal1"), json!(42));
    }

    /// Regression for bd:JMAP-ic0j.32: a value missing the `id` field
    /// panics instead of storing a fixture whose `id` field will be
    /// absent on read-back.
    #[test]
    #[should_panic(expected = "must contain an `id` string field")]
    fn seed_object_rejects_value_without_id_field() {
        let backend = MemoryBackend::new();
        backend.seed_object(
            &Id::from("acc1"),
            "Calendar",
            &Id::from("cal1"),
            json!({"name": "Work"}),
        );
    }

    /// Regression for bd:JMAP-ic0j.32: a value whose `id` field does
    /// not match the `id` argument panics rather than producing a
    /// fixture that's reachable by one id but serializes with another.
    #[test]
    #[should_panic(expected = "does not match the `id` argument")]
    fn seed_object_rejects_id_mismatch() {
        let backend = MemoryBackend::new();
        backend.seed_object(
            &Id::from("acc1"),
            "Calendar",
            &Id::from("cal1"),
            json!({"id": "cal2"}),
        );
    }

    /// Positive control: the happy path still works for each of the
    /// four known type_name values.
    #[test]
    fn seed_object_accepts_known_type_names() {
        let backend = MemoryBackend::new();
        backend.seed_object(
            &Id::from("acc1"),
            "Calendar",
            &Id::from("c1"),
            json!({"id": "c1"}),
        );
        backend.seed_object(
            &Id::from("acc1"),
            "CalendarEvent",
            &Id::from("e1"),
            json!({"id": "e1"}),
        );
        backend.seed_object(
            &Id::from("acc1"),
            "CalendarEventNotification",
            &Id::from("n1"),
            json!({"id": "n1"}),
        );
        backend.seed_object(
            &Id::from("acc1"),
            "ParticipantIdentity",
            &Id::from("p1"),
            json!({"id": "p1"}),
        );
    }
}

// ---------------------------------------------------------------------------
// Tests for the default CalendarsBackend::limits impl (bd:JMAP-ic0j.31)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod limits_default_tests {
    use super::*;
    use crate::CalendarsLimits;

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
        let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
        backend.seed_object(
            &Id::from("acc1"),
            "CalendarEvent",
            &Id::from("evt1"),
            serde_json::json!({"id": "evt1", "calendarIds": {"cal1": true}}),
        );
        assert_eq!(count(&backend, "acc1", "cal1"), 1);
        assert!(!has_key(&backend, "acc1", "cal2"));
    }

    /// Oracle: two seeded events both referencing cal1 produce a count
    /// of 2 — verifies the counter accumulates rather than overwriting.
    #[test]
    fn seed_two_events_same_calendar_count_is_two() {
        let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
        backend.seed_object(
            &Id::from("acc1"),
            "CalendarEvent",
            &Id::from("evt1"),
            serde_json::json!({"id": "evt1", "calendarIds": {"cal1": true}}),
        );
        backend.seed_object(
            &Id::from("acc1"),
            "CalendarEvent",
            &Id::from("evt2"),
            serde_json::json!({"id": "evt2", "calendarIds": {"cal1": true}}),
        );
        assert_eq!(count(&backend, "acc1", "cal1"), 2);
    }

    /// Oracle: a single event referencing two calendars (cal1 + cal2)
    /// contributes 1 to each — verifies the index handles the JMAP
    /// "event-in-multiple-calendars" case (§5.2 multi-calendar events).
    #[test]
    fn seed_event_with_multiple_calendars_each_gets_one() {
        let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
        backend.seed_object(
            &Id::from("acc1"),
            "CalendarEvent",
            &Id::from("evt1"),
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
        let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
        backend.seed_object(
            &Id::from("acc1"),
            "CalendarEvent",
            &Id::from("evt1"),
            serde_json::json!({"id": "evt1", "calendarIds": {"cal1": true}}),
        );
        backend.seed_object(
            &Id::from("acc1"),
            "CalendarEvent",
            &Id::from("evt2"),
            serde_json::json!({"id": "evt2", "calendarIds": {"cal2": true}}),
        );
        backend.seed_object(
            &Id::from("acc1"),
            "CalendarEvent",
            &Id::from("evt3"),
            serde_json::json!({"id": "evt3", "calendarIds": {"cal2": true, "cal3": true}}),
        );
        assert_eq!(count(&backend, "acc1", "cal1"), 1);
        assert_eq!(count(&backend, "acc1", "cal2"), 2);
        assert_eq!(count(&backend, "acc1", "cal3"), 1);
        assert!(!has_key(&backend, "acc1", "cal4"));
    }

    fn calendar_event(
        uid: &str,
        calendar_ids: &[&str],
    ) -> impl FnOnce(&Id) -> jmap_calendars_types::CalendarEvent + Send + use<> {
        let mut cal_ids = serde_json::Map::new();
        for c in calendar_ids {
            cal_ids.insert((*c).to_owned(), serde_json::Value::Bool(true));
        }
        let mut event: jmap_calendars_types::CalendarEvent =
            serde_json::from_value(serde_json::json!({
                "@type": "Event",
                "uid": uid,
                "calendarIds": cal_ids,
            }))
            .expect("CalendarEvent deserialization must succeed for valid fixture JSON");
        move |id: &Id| {
            event.id = Some(id.clone());
            event
        }
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
        let backend = MemoryBackend::new().with_account(&Id::from("acc1"));

        // Seed two events: one in cal1, one in cal2. Seed triggers a
        // full rescan, so after this the index is whatever rescan
        // produces.
        backend.seed_object(
            &Id::from("acc1"),
            "CalendarEvent",
            &Id::from("seed1"),
            serde_json::json!({"id": "seed1", "calendarIds": {"cal1": true}}),
        );
        backend.seed_object(
            &Id::from("acc1"),
            "CalendarEvent",
            &Id::from("seed2"),
            serde_json::json!({"id": "seed2", "calendarIds": {"cal2": true}}),
        );

        // Create three more events through the real /set path, exercising
        // the incremental delta helper.
        let (id1, _) = backend
            .create_object::<jmap_calendars_types::CalendarEvent, _>(
                &(),
                &Id::from("acc1"),
                "c1",
                calendar_event("u1", &["cal1", "cal3"]),
            )
            .await
            .unwrap();
        let (id2, _) = backend
            .create_object::<jmap_calendars_types::CalendarEvent, _>(
                &(),
                &Id::from("acc1"),
                "c2",
                calendar_event("u2", &["cal2"]),
            )
            .await
            .unwrap();
        let _ = backend
            .create_object::<jmap_calendars_types::CalendarEvent, _>(
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
        let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
        let (id, _) = backend
            .create_object::<jmap_calendars_types::CalendarEvent, _>(
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
        let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
        let (id1, _) = backend
            .create_object::<jmap_calendars_types::CalendarEvent, _>(
                &(),
                &Id::from("acc1"),
                "c1",
                calendar_event("u1", &["cal1"]),
            )
            .await
            .unwrap();
        let _ = backend
            .create_object::<jmap_calendars_types::CalendarEvent, _>(
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
        let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
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
        let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
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
        let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
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

#[cfg(test)]
mod memory_error_tests {
    use super::MemoryError;

    /// The `JmapBackend::Error` contract: "Implementations MUST NOT
    /// include ... PII shaped like an email address" in `Display`
    /// output. serde_json data errors quote the offending value, so
    /// [`MemoryError::serde`] must drop the message (bd:JMAP-510h.37).
    #[test]
    fn serde_error_display_omits_offending_value() {
        let canary = "alice@example.com";
        let e = serde_json::from_value::<u64>(serde_json::Value::String(canary.into()))
            .expect_err("a string is not a u64");
        assert!(
            e.to_string().contains(canary),
            "precondition: serde_json quotes the value"
        );

        let shown = MemoryError::serde("decode", &e).to_string();
        assert!(!shown.contains(canary), "canary leaked: {shown}");
        assert!(
            shown.contains("decode") && shown.contains("Data"),
            "{shown}"
        );
    }
}

#[cfg(test)]
mod clone_tests {
    use super::MemoryBackend;
    use std::sync::Arc;

    /// Oracle (bd:JMAP-q7k6): `MemoryBackend::clone` is a shared-state
    /// `Arc` clone, never a deep copy, so a clone and its original hold
    /// the same store while a fresh instance does not.
    #[test]
    fn clone_shares_store() {
        let a = MemoryBackend::new();
        assert!(Arc::ptr_eq(&a.inner, &a.clone().inner));
        assert!(!Arc::ptr_eq(&a.inner, &MemoryBackend::new().inner));
    }
}

#[cfg(all(test, debug_assertions))]
mod empty_account_id_tests {
    use super::{Id, MemoryBackend};

    /// Oracle: RFC 8620 §1.2, an Id "is a String of at least 1 and a
    /// maximum of 255 octets in size" (bd:JMAP-dx0s).
    #[test]
    #[should_panic(expected = "account_id must not be the empty string")]
    fn register_account_rejects_empty_id() {
        MemoryBackend::new().register_account(&Id::from(""));
    }
}

#[cfg(test)]
mod max_changes_tests {
    use super::*;
    use jmap_calendars_types::Calendar as T;

    const CALLER: () = ();

    /// A backend whose `T` log holds one state per element of `states`,
    /// each creating the listed ids.
    fn with_creates(states: &[&[&str]]) -> MemoryBackend {
        let backend = MemoryBackend::new();
        let mut inner = backend.inner.lock().unwrap();
        for ids in states {
            let new_state = inner.bump_state(T::TYPE_NAME, "acc1");
            inner
                .change_log_mut(T::TYPE_NAME, "acc1")
                .push(ChangeEntry {
                    new_state,
                    created: ids.iter().map(|s| Id::from(*s)).collect(),
                    updated: vec![],
                    destroyed: vec![],
                    updated_props: None,
                });
        }
        drop(inner);
        backend
    }

    /// Oracle: RFC 8620 §5.2, expected pages written by hand: "the server
    /// MUST ensure the number of ids returned across "created", "updated",
    /// and "destroyed" does not exceed this limit. If there are more
    /// changes than this ..., the server SHOULD generate an update to take
    /// the client to an intermediate state, from which the client can
    /// continue to call "Foo/changes" until it is fully up to date." Three
    /// one-create states against `maxChanges` 2 page as states 1-2, then 3.
    #[tokio::test]
    async fn over_max_changes_pages_through_intermediate_state() {
        let backend = with_creates(&[&["a"], &["b"], &["c"]]);

        let first = changes(&backend, "0", 2).await.expect("first page");
        assert_eq!(sorted(first.created), [Id::from("a"), Id::from("b")]);
        assert!(first.has_more_changes);
        assert_eq!(first.new_state.as_ref(), "2");

        let rest = changes(&backend, "2", 2).await.expect("second page");
        assert_eq!(rest.created, [Id::from("c")]);
        assert!(!rest.has_more_changes);
        assert_eq!(rest.new_state.as_ref(), "3");
    }

    /// Oracle: RFC 8620 §5.2 "If it is unable to calculate an intermediate
    /// state, it MUST return a "cannotCalculateChanges" error response
    /// instead." One state creating three ids cannot be split under
    /// `maxChanges` 2.
    #[tokio::test]
    async fn one_state_over_max_changes_cannot_calculate() {
        let backend = with_creates(&[&["a", "b", "c"]]);
        assert!(matches!(
            changes(&backend, "0", 2).await,
            Err(BackendChangesError::TooManyChanges { limit: 2 })
        ));
    }

    fn sorted(mut ids: Vec<Id>) -> Vec<Id> {
        ids.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        ids
    }

    async fn changes(
        backend: &MemoryBackend,
        since: &str,
        max: u64,
    ) -> Result<ChangesResult, BackendChangesError<<MemoryBackend as JmapBackend>::Error>> {
        backend
            .get_changes::<T>(
                &CALLER,
                &Id::from("acc1"),
                jmap_server::ChangesParams::new(&State::from(since)).max_changes(Some(max)),
            )
            .await
    }
}
