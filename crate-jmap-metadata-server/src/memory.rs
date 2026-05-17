//! In-memory reference implementation of [`crate::MetadataBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`crate::MetadataBackend`] trait to study when
//!    writing a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Metadata
//!    dispatcher with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and
//! several draft-ietf-jmap-metadata-01 edge cases are simplified (see
//! source comments below).
//!
//! # Feature flag and API stability
//!
//! This module is gated behind `feature = "memory"` and is **not** enabled
//! by default. Its public API stability is opt-in: it may break across
//! minor versions while the crate is pre-1.0.
//!
//! # Metadata-specific behaviour
//!
//! - **Uniqueness (§3.1)**: the backend enforces the uniqueness constraint
//!   on `(relatedType, relatedId, @type, isPrivate)`. A duplicate create
//!   returns
//!   [`BackendSetError::SetError`](crate::BackendSetError::SetError)
//!   wrapping a [`SetErrorType::AlreadyExists`](crate::SetErrorType::AlreadyExists)
//!   with `existing_id` set to the conflicting object's Id.
//!
//!   **Production-backend note.** This reference impl scans every
//!   stored Metadata object on every create (and on update for the
//!   post-patch re-check) — O(N) per call, O(N²) for N successive
//!   creates. That is **fine for tests** and **wrong to copy** into a
//!   real backend. Production-grade uniqueness should be enforced by
//!   one of:
//!   - A database UNIQUE INDEX on
//!     `(account_id, related_type, related_id, type_name, is_private)`
//!     — relational backends.
//!   - An in-memory `HashMap<UniquenessKey, Id>` or equivalent O(1)
//!     lookup table — in-process backends. The `UniquenessKey` type
//!     used internally by this impl is the correct shape for that
//!     map; see the `find_uniqueness_conflict` helper for the key
//!     extraction logic.
//!   A "SELECT * FROM metadata WHERE account_id = ?" + loop-and-compare
//!   in application code replicates this reference impl's O(N) scan
//!   against a database and is the wrong pattern.
//! - **`maySetPrivate` gating (§1.2.1)**: not enforced by the reference
//!   impl — any `isPrivate` value is accepted. Real backends that need
//!   per-account gating should override `create_object` / `update_object`
//!   to consult their capability table.
//! - **Id minting (§3.1.1)**: server-side ids are minted by
//!   `demo_next_id`. The default-feature mode (deterministic) computes
//!   the id from `HashMap.len() + 1`. This is fast and lex-orderable
//!   for readable test output, but **silently recycles ids across
//!   destroy events**: create object1 (id `metadata...001`), destroy
//!   object1, then create object2 — object2 also gets `metadata...001`.
//!   The `realistic-demo-ids` feature switches to a process-global
//!   atomic counter that avoids the recycle (at the cost of
//!   non-deterministic ids across test runs). **Both modes are
//!   demonstration-quality**; production backends must mint ULIDs (or
//!   equivalent globally-unique, monotonic, persistent-across-restarts
//!   ids). A consequence specific to the default mode: a
//!   `Metadata/changes` response can carry an id in `destroyed` and a
//!   later /changes carry the same id in `created`, which clients
//!   relying on JMAP's "ids are unique forever" assumption (RFC 8620
//!   §1.2) will misinterpret as a resurrection.
//! - **Quota (§6)**: not enforced.
//! - **Related-object validation (§3.1)**: not enforced — `relatedType` and
//!   `relatedId` are accepted as-is. Real backends should verify that the
//!   referenced object exists.
//! - **`Metadata/query` filter and sort (§3.4)**: implemented for the five
//!   filter fields (`@type`, `relatedType`, `relatedIds`, `isPrivate`,
//!   `textMatch`) and the five sortable properties (`id`, `@type`,
//!   `relatedType`, `relatedId`, `isPrivate`). `textMatch` walks vendor
//!   string properties via a case-insensitive substring check; servers
//!   that index those properties externally will want a real search
//!   path. Operator filters (`AND`/`OR`/`NOT`) are NOT supported because
//!   `Metadata::Filter = MetadataFilterCondition` (bare); the dispatcher
//!   rejects operator-wrapped filters with `unsupportedFilter` before
//!   reaching the backend.
//! - **`Metadata/queryChanges` filter and sort (§3.5)**: NOT implemented.
//!   The filter and sort arguments are discarded; the result is computed
//!   from the change log without per-filter pruning. A client that
//!   filters or sorts the parent `/query` MUST issue a fresh `/query`
//!   call after a `cannotCalculateChanges`-style mismatch rather than
//!   trusting `/queryChanges`. Override `query_changes` in a real
//!   backend.
//!
//! # Single-user limitation (per-caller scoping is NOT implemented)
//!
//! This reference impl uses `CallerCtx = ()` and treats every caller as
//! a single anonymous user. Several draft-ietf-jmap-metadata-01
//! semantics that the spec defines per-user are implemented here at
//! account scope, which is silently wrong for any multi-user
//! deployment.
//!
//! - **Per-user uniqueness for private metadata (§3.1)**: the spec
//!   requires that two different users may each hold their own private
//!   `Annotation` for the same `(relatedType, relatedId, @type)` tuple.
//!   The internal `find_uniqueness_conflict` helper scans the account's
//!   Metadata store WITHOUT consulting caller identity, so a multi-user
//!   adaptation of this impl would reject the second user's create
//!   with `alreadyExists` — silently denying private metadata that the
//!   spec promises is permitted.
//! - **`isPrivate` visibility scoping**: the workspace AGENTS.md
//!   "Caller identity (foundation seam)" rule requires backends to
//!   filter `Metadata/get` / `Metadata/changes` / `Metadata/query` /
//!   `Metadata/queryChanges` responses by the caller's identity when
//!   `isPrivate: true`. This impl does not — every caller sees every
//!   private record in the account.
//!
//! **What a real backend MUST do.** Read the caller principal via
//! [`JmapBackend::principal_id(caller)`](crate::JmapBackend::principal_id)
//! and:
//!
//! 1. Scope `find_uniqueness_conflict` by `(principal_id, related_type,
//!    related_id, @type, isPrivate)` when `is_private` is true.
//! 2. Filter `Metadata/get` reads so private records authored by a
//!    different principal are invisible.
//! 3. Reject `Metadata/set` updates/destroys against a private record
//!    not authored by the caller with `forbidden`.
//!
//! This reference impl's `JmapBackend::principal_id` returns `None`
//! (the foundation default), so a contributor cargo-grepping for
//! `principal_id` will find no wired call site here. That is
//! intentional — wiring `principal_id` into the scan would not change
//! the behaviour of the single-user demo path. Contributors copying
//! this impl into a multi-user backend MUST add those calls.
//!
//! # Example
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use jmap_metadata_server::{memory::MemoryBackend, register_metadata_handlers};
//! use jmap_server::Dispatcher;
//!
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_metadata_handlers(
//!     &mut dispatcher,
//!     Arc::new(MemoryBackend::new_with_accounts(&["acc1"])),
//! );
//! ```
//!
//! # Concurrency
//!
//! **Single global `std::sync::Mutex` — testing-grade only.** All read
//! and write operations on this backend serialize through one mutex
//! that wraps the entire account state map. Under any concurrent load
//! (multiple simultaneous `/get` / `/set` / `/query` calls), every
//! operation blocks until the previous one finishes. The
//! `await_holding_lock` clippy lint is enabled module-wide so the
//! guard never crosses an `.await`, but the tokio worker thread that
//! happens to be running an operation still cannot yield (no `.await`
//! inside the critical section) until the operation completes.
//!
//! **This is intentional for the reference impl** — a single mutex is
//! straightforward to reason about and keeps the source readable for
//! contributors. It is **NOT a pattern to copy into a real backend.**
//! A production-grade MetadataBackend must use one of:
//!
//! - Per-account or per-record fine-grained locking (e.g.
//!   `DashMap<AccountId, AccountState>` or sharded mutexes).
//! - An ACID storage layer (Postgres / SQLite / FoundationDB) that
//!   handles concurrency internally; the backend impl then becomes a
//!   thin adapter.
//! - Optimistic concurrency with versioning, where reads do not block
//!   writes and vice versa.
//!
//! **The mutex type is `std::sync::Mutex`, not `tokio::sync::Mutex`,
//! because no operation awaits inside the critical section.** If a
//! future change requires holding the guard across an `.await`, switch
//! to `tokio::sync::Mutex` rather than disabling the
//! `await_holding_lock` lint. Swapping to `tokio::sync::Mutex` alone
//! does NOT fix the per-operation serialization — only the
//! coarse-grained locking strategy does.

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, MetadataBackend, QueryChangesResult, QueryObject, QueryResult, SetError,
    SetErrorType, SetObject,
};
// json_merge_patch is the RFC 7396 (JSON Merge Patch) implementation in
// jmap-server (the shared foundation crate). This backend uses it
// because draft-ietf-jmap-metadata-01 §3.3 specifies flat-key /set
// patches that match Merge Patch semantics directly. Backends whose
// spec defines a path-key patch dialect (e.g. jmap-mail-server's
// Email/set, which uses "mailboxIds/abc123"-shaped paths per RFC
// 8621 §4.6.5) implement their own apply_jmap_patch helper instead.
// The canonical RFC 7396 tests for json_merge_patch live with the
// function in jmap-server.
use jmap_metadata_types::{Metadata, MetadataFilterCondition};
use jmap_server::{json_merge_patch, MergePatchError};
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// A per-Id record in the change log.
///
/// `related_type` and `type_name` are populated at log-push time when the
/// stored object is Metadata. For change log entries belonging to other
/// JmapObject types (none in this crate today; MemoryBackend only supports
/// Metadata) the strings are empty — those entries are never consumed by
/// [`MemoryBackend::get_metadata_changes`], which filters on the
/// type-keyed log directly.
#[derive(Clone, Debug)]
struct ChangeRecord {
    id: Id,
    related_type: String,
    type_name: String,
}

impl ChangeRecord {
    /// Build a [`ChangeRecord`] from a stored object's JSON value,
    /// capturing the `relatedType` / `@type` fields the
    /// `MetadataBackend::get_metadata_changes` override consumes for
    /// draft-ietf-jmap-metadata-01 §3.3 strict conformance.
    ///
    /// Used at every change-log emission point in this MemoryBackend
    /// (`create_object`, `update_object`, `destroy_object`). The
    /// `related_type` / `type_name` snapshot MUST be taken at mutation
    /// time, BEFORE the value moves into or out of the `objects`
    /// store; the destroy path in particular cannot recover these
    /// strings post-mortem. See bd:JMAP-ayoz.19 / bd:JMAP-ayoz.37 for
    /// the rationale and the de-duplication that consolidated three
    /// inline call sites into this helper.
    ///
    /// Non-Metadata object types stored in this MemoryBackend
    /// (currently none, but the storage shape is generic) carry empty
    /// strings for both fields; the get-changes override only
    /// inspects them when the change-log key matches
    /// `Metadata::TYPE_NAME`, so the empty-string fallback is a wash.
    fn from_stored_value(id: Id, val: &serde_json::Value) -> Self {
        Self {
            id,
            related_type: val
                .get("relatedType")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned(),
            type_name: val
                .get("@type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned(),
        }
    }
}

/// A change log entry for one state transition.
#[derive(Clone, Debug)]
struct ChangeEntry {
    /// The state counter AFTER this change.
    new_state: u64,
    created: Vec<ChangeRecord>,
    updated: Vec<ChangeRecord>,
    destroyed: Vec<ChangeRecord>,
}

/// Shared inner state, behind `Arc<Mutex>`.
#[derive(Default)]
struct Inner {
    /// `(type_name, account_id)` → `id → serialized object`
    objects: HashMap<(&'static str, String), HashMap<Id, serde_json::Value>>,
    /// `(type_name, account_id)` → current state counter
    states: HashMap<(&'static str, String), u64>,
    /// `(type_name, account_id)` → ordered change entries
    change_log: HashMap<(&'static str, String), Vec<ChangeEntry>>,
    /// Explicitly registered account ids (accounts may exist with no
    /// objects yet).
    known_accounts: HashSet<String>,
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
}

// ---------------------------------------------------------------------------
// Uniqueness key (§3.1)
// ---------------------------------------------------------------------------

/// The (relatedType, relatedId, `@type`, isPrivate) tuple that
/// draft-ietf-jmap-metadata-01 §3.1 requires to be unique within a user's
/// visible set.
///
/// `isPrivate` defaults to `false` per §2.2.1.5 when absent from the wire.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct UniquenessKey {
    related_type: String,
    related_id: String,
    type_name: String,
    is_private: bool,
}

impl UniquenessKey {
    /// Compute the uniqueness key from a stored Metadata value (the JSON
    /// representation kept in `Inner::objects`). Returns `None` if the
    /// value does not look like a Metadata object (defensive — should not
    /// happen for objects stored under `Metadata::TYPE_NAME`).
    fn from_stored(val: &serde_json::Value) -> Option<Self> {
        Some(Self {
            related_type: val.get("relatedType")?.as_str()?.to_owned(),
            related_id: val.get("relatedId")?.as_str()?.to_owned(),
            type_name: val.get("@type")?.as_str()?.to_owned(),
            is_private: val
                .get("isPrivate")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })
    }

    /// Compute the uniqueness key from a typed `Metadata`. Returns `None`
    /// if `relatedType` is absent on the typed value — partial-response
    /// shapes that omit the field per draft §4.1 cannot participate in
    /// the §3.1 uniqueness constraint because the constraint key is
    /// undefined. Server-side create/update handlers are responsible for
    /// rejecting incoming Metadata that lacks `relatedType` (the spec
    /// mandates it for full objects per §2.2.1.3); this method is
    /// defensive against a post-patch state where the field somehow ends
    /// up cleared.
    fn from_metadata(m: &Metadata) -> Option<Self> {
        Some(Self {
            related_type: m.related_type()?.to_owned(),
            related_id: m.related_id().as_ref().to_owned(),
            type_name: m.type_name().to_owned(),
            is_private: m.is_private(),
        })
    }
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// A fully in-memory implementation of [`crate::MetadataBackend`].
///
/// Stores objects as serialized JSON; each mutation bumps a monotonic state
/// counter and records a change log entry. Enforces the
/// draft-ietf-jmap-metadata-01 §3.1 uniqueness constraint on
/// `(relatedType, relatedId, @type, isPrivate)`.
#[derive(Clone, Default)]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryBackend {
    /// Construct an empty `MemoryBackend` with no accounts or stored
    /// objects. Equivalent to [`Self::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one or more accounts as known even if they have no objects
    /// yet.
    pub fn new_with_accounts(account_ids: &[&str]) -> Self {
        let b = Self::new();
        {
            let mut inner = b.inner.lock().unwrap();
            for id in account_ids {
                inner.known_accounts.insert((*id).to_owned());
            }
        }
        b
    }

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
    ///   is the per-(type, account) object count + 1. Lex-orderable within
    ///   a (type, account) namespace, repeatable across test runs, easy to
    ///   read in test debug output.
    /// - **`realistic-demo-ids` enabled:** returns `"{n:016x}"` matching
    ///   the canonical `jmap-mail-server` pattern at `email.rs:1748` —
    ///   process-start nanos as base, atomic counter, no type prefix,
    ///   no per-account scoping. Lex-orderable globally within a process,
    ///   not repeatable across runs.
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
                // `Duration::as_nanos()` returns u128; explicit try_from
                // makes the u64 narrowing audible. u64 nanos overflow at
                // ~year 2554, so the conversion is in practice always
                // infallible within the lifetime of any deployment, but
                // an explicit `try_from` documents that and avoids the
                // cargo-cult `as u64` pattern downstream contributors
                // might copy. The fallback (1e9 = epoch + 1 second) is
                // unchanged.
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .and_then(|d| u64::try_from(d.as_nanos()).ok())
                    .unwrap_or(1_000_000_000)
            });
            let n = base.wrapping_add(COUNTER.fetch_add(1, Ordering::Relaxed));
            Id::from(format!("{n:016x}"))
        }

        #[cfg(not(feature = "realistic-demo-ids"))]
        {
            // CARGO-CULT WARNING: do NOT copy this into a production backend.
            // This deterministic mode uses an in-memory `len()` counter that:
            //   - collides after deletes (count goes down, next id re-uses)
            //   - resets to 0 on every process restart
            //   - is not unique across (type, account) namespaces
            // Production-grade id minting needs a real ULID or equivalent.
            let n = inner
                .objects_ref(type_name, account_id)
                .map_or(0, |m| m.len());
            let new_id = Id::from(format!("{}{:016x}", type_name.to_ascii_lowercase(), n + 1));
            debug_assert!(
                !inner
                    .objects_ref(type_name, account_id)
                    .is_some_and(|m| m.contains_key(&new_id)),
                "MemoryBackend demo_next_id collision: deterministic mode uses a len()-based \
                 counter that cannot survive deletes. This is the demo impl — production \
                 backends must use ULIDs."
            );
            new_id
        }
    }

    /// Scan an account's Metadata store for an existing object whose
    /// uniqueness key matches `key`. Returns `Some(id)` if a conflict
    /// exists. Lock must already be held by the caller.
    ///
    /// **Single-user limitation** (bd:JMAP-ayoz.5): the scan is
    /// account-wide, NOT per-caller. draft-ietf-jmap-metadata-01 §3.1
    /// requires per-user uniqueness for private metadata — two
    /// different users may each hold their own private record on the
    /// same `(relatedType, relatedId, @type)` tuple. A multi-user
    /// backend MUST scope this scan by the caller's principal id
    /// (read via [`JmapBackend::principal_id`](crate::JmapBackend::principal_id))
    /// when `key.is_private` is true. See the module-level rustdoc
    /// "Single-user limitation" section.
    fn find_uniqueness_conflict(
        inner: &Inner,
        account_id: &str,
        key: &UniquenessKey,
        exclude_id: Option<&Id>,
    ) -> Option<Id> {
        let map = inner.objects_ref(Metadata::TYPE_NAME, account_id)?;
        for (id, val) in map {
            if Some(id) == exclude_id {
                continue;
            }
            if UniquenessKey::from_stored(val).as_ref() == Some(key) {
                return Some(id.clone());
            }
        }
        None
    }

    /// Walk the change log for `(type_name, account_id)`, collect entries
    /// whose `new_state > since_state`, enforce the `max_changes` cap, and
    /// return the relevant entries together with the current state counter.
    ///
    /// Shared between [`get_changes`](Self::get_changes) and the
    /// Metadata-specific [`get_metadata_changes`](MetadataBackend::get_metadata_changes)
    /// override. Lock must already be held by the caller; the helper does
    /// not acquire `self.inner`.
    fn collect_relevant_changes<'a>(
        inner: &'a Inner,
        type_name: &'static str,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<(Vec<&'a ChangeEntry>, u64), BackendChangesError<MemoryError>> {
        // Note: parsing a malformed since_state into the MemoryBackend state
        // counter (u64) is reported via BackendChangesError::CannotCalculate
        // (bd:JMAP-jfia.31). Previously this used the magic-zero
        // `TooManyChanges { limit: 0 }` alias, which still maps to the
        // same wire error via the permanent legacy-alias path
        // (bd:JMAP-jfia.37).
        let since_n: u64 = since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::CannotCalculate)?;

        let log = inner
            .change_log
            .get(&(type_name, account_id.as_ref().to_owned()))
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        let relevant: Vec<&ChangeEntry> = log.iter().filter(|e| e.new_state > since_n).collect();

        let current_state = inner.current_state(type_name, account_id.as_ref());

        if let Some(max) = max_changes {
            if relevant.len() as u64 > max {
                return Err(BackendChangesError::TooManyChanges { limit: max });
            }
        }

        Ok((relevant, current_state))
    }
}

/// Opaque storage-layer error returned by [`MemoryBackend`] operations.
///
/// The inner description is a human-readable string intended for
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
/// Mirrors the canonical [`jmap-mail-server`'s `MemoryError`] shape
/// per workspace AGENTS.md canonical-template propagation rule.
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
        f.write_str(&self.description)
    }
}

impl std::error::Error for MemoryError {}

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

        let (relevant, current_state) = Self::collect_relevant_changes(
            &inner,
            O::TYPE_NAME,
            account_id,
            since_state,
            max_changes,
        )?;

        let mut created: Vec<Id> = Vec::new();
        let mut updated: Vec<Id> = Vec::new();
        let mut destroyed: Vec<Id> = Vec::new();

        for entry in &relevant {
            for rec in &entry.created {
                if !destroyed.contains(&rec.id) && !created.contains(&rec.id) {
                    created.push(rec.id.clone());
                }
            }
            for rec in &entry.updated {
                if !destroyed.contains(&rec.id)
                    && !created.contains(&rec.id)
                    && !updated.contains(&rec.id)
                {
                    updated.push(rec.id.clone());
                }
            }
            for rec in &entry.destroyed {
                created.retain(|c| c != &rec.id);
                updated.retain(|u| u != &rec.id);
                if !destroyed.contains(&rec.id) {
                    destroyed.push(rec.id.clone());
                }
            }
        }

        Ok(ChangesResult::new(
            created,
            updated,
            destroyed,
            false,
            State::from(current_state.to_string()),
        ))
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        // Snapshot the stored objects under the lock, then drop the lock
        // before deserialising for filter/sort. Holding the std Mutex
        // across the deserialise loop would block any concurrent
        // dispatcher thread for the duration of the scan.
        let (mut entries, query_state) = {
            let inner = self.inner.lock().unwrap();
            let entries: Vec<(Id, serde_json::Value)> = inner
                .objects_ref(O::TYPE_NAME, account_id.as_ref())
                .map(|m| {
                    m.iter()
                        .map(|(id, val)| (id.clone(), val.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let qs = State::from(
                inner
                    .current_state(O::TYPE_NAME, account_id.as_ref())
                    .to_string(),
            );
            (entries, qs)
        };

        // Recover the typed filter / sort via a JSON roundtrip. Mirrors
        // the canonical jmap-mail-server pattern (memory.rs:665-792):
        // O::Filter / O::Comparator are Serialize, and the dispatcher has
        // already deserialised the wire JSON into the typed form, so the
        // roundtrip is a type-identity operation that cannot fail. Per
        // Pattern G (jmap-mail-server commit bc79c70), surface a panic
        // here rather than silently dropping the filter — silent-drop
        // would return ALL objects when the client expected a filtered
        // subset (the same query-correctness hazard bd:JMAP-826m.4
        // tracks).
        let metadata_filter: Option<MetadataFilterCondition> =
            if O::TYPE_NAME == Metadata::TYPE_NAME {
                filter.map(|f| {
                    let v = serde_json::to_value(f)
                        .expect("derive(Serialize) on plain data is infallible");
                    serde_json::from_value(v).expect(
                        "type-identity roundtrip on MetadataFilterCondition is infallible: \
                         the JSON came from Serialize on the same concrete type",
                    )
                })
            } else {
                None
            };

        // Decode the comparator list. Metadata::Comparator is
        // serde_json::Value (see jmap-metadata-types::backend), each one
        // shaped as {property, isAscending} per RFC 8620 §5.5. The
        // handler validates property names before reaching the backend;
        // this code accepts any unknown property as "no constraint"
        // (sort comparators with unsupported properties return an empty
        // tuple and contribute nothing to the comparison).
        let metadata_sort: Vec<(String, bool)> = if O::TYPE_NAME == Metadata::TYPE_NAME {
            sort.map(|s| {
                s.iter()
                    .filter_map(|c| {
                        let v = serde_json::to_value(c).ok()?;
                        let prop = v.get("property")?.as_str()?.to_owned();
                        let asc = v
                            .get("isAscending")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(true);
                        Some((prop, asc))
                    })
                    .collect()
            })
            .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Apply filter to Metadata entries. Non-Metadata types have no
        // typed filter on this backend (MemoryBackend only stores
        // Metadata); the filter argument is left unapplied and the
        // entries vector for non-Metadata types is empty anyway because
        // create_object refuses non-Metadata writes.
        if O::TYPE_NAME == Metadata::TYPE_NAME {
            if let Some(ref cond) = metadata_filter {
                entries.retain(|(_, val)| {
                    match serde_json::from_value::<Metadata>(val.clone()) {
                        Ok(meta) => metadata_matches_condition(&meta, cond),
                        // A stored value that does not deserialise into
                        // Metadata is a backend invariant violation, not
                        // a filter miss. Keep the entry so it surfaces
                        // in /get rather than silently disappearing
                        // from /query.
                        Err(_) => true,
                    }
                });
            }
            // Sort. Default order (no comparators): ascending by id —
            // preserves the pre-fix lexicographic ordering used by
            // existing tests and the jmap-test-suite oracle.
            if metadata_sort.is_empty() {
                entries.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
            } else {
                // Pre-deserialise each entry once so the sort comparator
                // does not pay the Metadata::deserialize cost N log N
                // times.
                let mut typed: Vec<(Id, serde_json::Value, Option<Metadata>)> = entries
                    .into_iter()
                    .map(|(id, val)| {
                        let meta = serde_json::from_value::<Metadata>(val.clone()).ok();
                        (id, val, meta)
                    })
                    .collect();
                typed.sort_by(|a, b| compare_metadata_sort(&metadata_sort, a, b));
                entries = typed.into_iter().map(|(id, val, _)| (id, val)).collect();
            }
        } else {
            // Non-Metadata types: preserve the pre-fix ordering for any
            // future O that this backend may be extended to store.
            entries.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
        }

        let ids: Vec<Id> = entries.into_iter().map(|(id, _)| id).collect();
        let total = ids.len() as u64;

        // RFC 8620 §5.5 — clamp effective position to [0, len].
        let start = if position >= 0 {
            (position as usize).min(ids.len())
        } else {
            let neg = position.saturating_neg() as usize;
            ids.len().saturating_sub(neg)
        };

        let page: Vec<Id> = ids[start..]
            .iter()
            .take(limit.map_or(usize::MAX, |n| n as usize))
            .cloned()
            .collect();

        Ok(QueryResult::new(
            page,
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
                .to_string(),
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

impl MetadataBackend for MemoryBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        // Refuse non-Metadata O at the top, before any state is touched
        // (bd:JMAP-826m.13). The trait is generic over O for forward
        // compatibility with the workspace canonical pattern, but this
        // backend only knows Metadata; a non-Metadata create previously
        // bumped state and pushed a change-log record with empty
        // related_type/type_name before partial work silently completed.
        if O::TYPE_NAME != Metadata::TYPE_NAME {
            return Err(unsupported_object_type::<O>());
        }
        let mut inner = self.inner.lock().unwrap();

        // Defense-in-depth account guard (bd:JMAP-ayoz.2). The handler
        // layer (handle_metadata_set) is canonical for RFC 8620 §1.6.2
        // accountNotFound, but a backend `create_object` is also called
        // directly from test seeders and from any future caller that
        // bypasses the handler. Silently extending `known_accounts` on
        // a per-object call would mean a stray create attaches private
        // metadata records to a phantom account.
        if !inner.known_accounts.contains(account_id.as_ref()) {
            return Err(BackendSetError::Other(MemoryError::new(format!(
                "unknown account: {}",
                account_id.as_ref()
            ))));
        }

        // Serialize so we can both inspect for the uniqueness key (when this
        // is a Metadata object) and stash the final JSON in the store.
        let mut val = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError::new(format!("serialize: {e}"))))?;

        // §3.1 uniqueness enforcement — only when the type being created is
        // actually Metadata. The trait is generic over O for forward
        // compatibility; the constraint is type-specific.
        //
        // Extract the uniqueness key directly from the JSON `Value` via
        // `UniquenessKey::from_stored`, skipping the typed deserialize
        // round-trip the previous form did (`from_value(val.clone())
        // -> Metadata`). The serialize-then-deserialize pair was a
        // tautology post-bd:JMAP-826m.13 — `O` is statically Metadata
        // here (the top-of-method guard rejects non-Metadata `O`), so
        // we just round-tripped Metadata's own Serialize output through
        // Metadata's Deserialize, which accepts by construction. No
        // validation was gained, and a Value::clone of the full record
        // (including the `extra` vendor-field map under workspace
        // extras-preservation policy) was paid on every create. The
        // `from_stored` form returns `None` for malformed input; that
        // path should not fire here because the source was `to_value`
        // on a typed `O = Metadata`, but if it ever does we surface
        // it as a backend invariant violation rather than a SetError.
        if O::TYPE_NAME == Metadata::TYPE_NAME {
            let key = UniquenessKey::from_stored(&val).ok_or_else(|| {
                BackendSetError::Other(MemoryError::new(
                    "create_object: serialised Metadata missing required fields \
                     (backend invariant violation)"
                        .to_owned(),
                ))
            })?;
            if let Some(existing_id) =
                Self::find_uniqueness_conflict(&inner, account_id.as_ref(), &key, None)
            {
                return Err(BackendSetError::SetError(
                    SetError::new(SetErrorType::AlreadyExists)
                        .with_description(format!(
                            "Metadata for (relatedType={:?}, relatedId={:?}, @type={:?}, isPrivate={}) already exists",
                            key.related_type, key.related_id, key.type_name, key.is_private,
                        ))
                        .with_existing_id(existing_id),
                ));
            }
        }

        let server_id = Self::demo_next_id(&mut inner, O::TYPE_NAME, account_id.as_ref());

        // Inject the server-assigned id, then deserialize back to echo to
        // the caller per the MetadataBackend invariant.
        val["id"] = serde_json::Value::String(server_id.as_ref().to_owned());
        let stored_obj: O = O::deserialize(&val).map_err(|e| {
            BackendSetError::Other(MemoryError::new(format!("deserialize after create: {e}")))
        })?;

        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        // Capture related_type / type_name from the stored value BEFORE the
        // val moves into objects_mut. For non-Metadata types these strings
        // are empty — get_metadata_changes only consumes them when the
        // change log key matches Metadata::TYPE_NAME.
        let record = ChangeRecord::from_stored_value(server_id.clone(), &val);
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(server_id.clone(), val);
        inner
            .change_log_mut(O::TYPE_NAME, account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![record],
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
        // Refuse non-Metadata O at the top (bd:JMAP-826m.13). See the
        // create_object guard for the same rationale.
        if O::TYPE_NAME != Metadata::TYPE_NAME {
            return Err(unsupported_object_type::<O>());
        }
        let mut inner = self.inner.lock().unwrap();

        let existing = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .get(id)
            .cloned();

        let Some(mut current) = existing else {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )));
        };

        // Apply JSON Merge Patch (RFC 7396). A `MergePatchError::DepthExceeded`
        // return (bd:JMAP-wlip.1) surfaces as `SetErrorType::InvalidPatch` —
        // the depth cap is a DoS guard, never fires on legitimate JMAP `/set
        // update` shapes. `current` is a clone of the stored value, so a
        // partially-applied patch on error is discarded with the local
        // without touching storage.
        let patch_val = serde_json::to_value(&patch).map_err(|e| {
            BackendSetError::Other(MemoryError::new(format!("serialize patch: {e}")))
        })?;
        if let Err(MergePatchError::DepthExceeded) = json_merge_patch(&mut current, patch_val) {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidPatch)
                    .with_description("patch nesting exceeds server limit"),
            ));
        }

        // §3.1 uniqueness re-check — only when the type is Metadata and
        // the patch could have moved a key into a colliding position.
        // If the post-patch value lacks `relatedType` (the field is
        // optional on the wire per draft §4.1 partial-response shape),
        // the uniqueness key is undefined and the constraint is skipped.
        if O::TYPE_NAME == Metadata::TYPE_NAME {
            let typed: Metadata = serde_json::from_value(current.clone()).map_err(|e| {
                BackendSetError::SetError(
                    SetError::new(SetErrorType::InvalidProperties)
                        .with_description(format!("post-patch Metadata deserialize: {e}")),
                )
            })?;
            if let Some(key) = UniquenessKey::from_metadata(&typed) {
                if let Some(existing_id) =
                    Self::find_uniqueness_conflict(&inner, account_id.as_ref(), &key, Some(id))
                {
                    return Err(BackendSetError::SetError(
                        SetError::new(SetErrorType::AlreadyExists)
                            .with_description(
                                "Update would violate Metadata uniqueness constraint".to_owned(),
                            )
                            .with_existing_id(existing_id),
                    ));
                }
            }
        }

        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        // Capture related_type / type_name from the post-patch value
        // BEFORE current moves into objects_mut. For non-Metadata types
        // these strings are empty (see ChangeRecord rustdoc).
        let record = ChangeRecord::from_stored_value(id.clone(), &current);
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(id.clone(), current);
        inner
            .change_log_mut(O::TYPE_NAME, account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![record],
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
        // Refuse non-Metadata O at the top (bd:JMAP-826m.13). See the
        // create_object guard for the same rationale.
        if O::TYPE_NAME != Metadata::TYPE_NAME {
            return Err(unsupported_object_type::<O>());
        }
        let mut inner = self.inner.lock().unwrap();

        let removed = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .remove(id);

        match removed {
            // Capture related_type / type_name from the doomed value
            // BEFORE we drop it. Critical for §3.3 strict conformance:
            // without this snapshot the destroyed array cannot be
            // filtered after the fact (the object no longer exists in
            // `objects` for the override to inspect).
            Some(val) => {
                let record = ChangeRecord::from_stored_value(id.clone(), &val);
                let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
                inner
                    .change_log_mut(O::TYPE_NAME, account_id.as_ref())
                    .push(ChangeEntry {
                        new_state,
                        created: vec![],
                        updated: vec![],
                        destroyed: vec![record],
                    });
                Ok(())
            }
            None => Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            ))),
        }
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        O::TYPE_NAME == Metadata::TYPE_NAME
    }

    /// Override of [`crate::MetadataBackend::get_metadata_changes`] for
    /// strict draft-ietf-jmap-metadata-01 §3.3 conformance.
    ///
    /// Walks the Metadata-keyed change log entries newer than `since_state`
    /// and filters each change record by `(related_type, type_name)`
    /// against the supplied filter args in a single pass. Unlike the
    /// default impl on the trait (which post-filters via re-fetch and
    /// can therefore not filter the destroyed array), this override
    /// honors all three arrays — including destroyed Ids whose objects
    /// no longer exist in `objects`. The per-Id `(related_type,
    /// type_name)` snapshot is captured at mutation time (in
    /// `create_object`, `update_object`, and `destroy_object`) and
    /// stored in the change log entry, so destroyed records carry the
    /// tuple they had at destroy time.
    ///
    /// The state token returned is the current Metadata state for the
    /// account, independent of the filter args (§3.3): a backend MUST
    /// NOT advance state based on filtered-out changes only.
    async fn get_metadata_changes(
        &self,
        _caller: &Self::CallerCtx,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
        filter_related_type: Option<&str>,
        filter_metadata_type: Option<&[String]>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        let inner = self.inner.lock().unwrap();

        let (relevant, current_state) = Self::collect_relevant_changes(
            &inner,
            Metadata::TYPE_NAME,
            account_id,
            since_state,
            max_changes,
        )?;

        let record_matches = |rec: &ChangeRecord| -> bool {
            if let Some(rt) = filter_related_type {
                if rec.related_type != rt {
                    return false;
                }
            }
            if let Some(types) = filter_metadata_type {
                if !types.iter().any(|t| t == &rec.type_name) {
                    return false;
                }
            }
            true
        };

        let mut created: Vec<Id> = Vec::new();
        let mut updated: Vec<Id> = Vec::new();
        let mut destroyed: Vec<Id> = Vec::new();

        for entry in &relevant {
            for rec in &entry.created {
                if !record_matches(rec) {
                    continue;
                }
                if !destroyed.contains(&rec.id) && !created.contains(&rec.id) {
                    created.push(rec.id.clone());
                }
            }
            for rec in &entry.updated {
                if !record_matches(rec) {
                    continue;
                }
                if !destroyed.contains(&rec.id)
                    && !created.contains(&rec.id)
                    && !updated.contains(&rec.id)
                {
                    updated.push(rec.id.clone());
                }
            }
            for rec in &entry.destroyed {
                // Suppress from created/updated even when the destroy
                // entry itself does not pass the filter — a destroy
                // strictly supersedes earlier create/update for the
                // same id. Then conditionally record in destroyed.
                created.retain(|c| c != &rec.id);
                updated.retain(|u| u != &rec.id);
                if !record_matches(rec) {
                    continue;
                }
                if !destroyed.contains(&rec.id) {
                    destroyed.push(rec.id.clone());
                }
            }
        }

        Ok(ChangesResult::new(
            created,
            updated,
            destroyed,
            false,
            State::from(current_state.to_string()),
        ))
    }
}

// ---------------------------------------------------------------------------
// Backend guards (bd:JMAP-826m.13)
// ---------------------------------------------------------------------------

/// Return a `BackendSetError::Other` describing an attempted /set call
/// against this backend with an object type that is not Metadata.
///
/// The trait `MetadataBackend` is generic over `O: SetObject` to match
/// the canonical workspace shape, but this reference backend stores
/// only [`Metadata`]. A non-Metadata call previously bumped state and
/// pushed a change-log record with empty `related_type` / `type_name`
/// before partial work silently completed — a future-bug magnet.
///
/// The error is intentionally `BackendSetError::Other` (a storage-layer
/// error) rather than `SetError` (a per-target /set failure): the
/// trait's documented invariant is that backends handle the JMAP object
/// types they claim to support, and "wrong O type at the trait
/// boundary" is a programmer error, not a per-target wire failure.
fn unsupported_object_type<O: JmapObject>() -> BackendSetError<MemoryError> {
    BackendSetError::Other(MemoryError::new(format!(
        "MemoryBackend supports only Metadata (got {})",
        O::TYPE_NAME
    )))
}

// ---------------------------------------------------------------------------
// Filter and sort helpers (bd:JMAP-826m.4)
// ---------------------------------------------------------------------------

/// Return `true` if `meta` satisfies every constraint in `cond`.
///
/// All [`MetadataFilterCondition`] fields are AND-combined (RFC 8620 §5.5
/// — a bare condition with multiple fields is equivalent to splitting
/// into per-field conditions under AND).
///
/// Per draft-ietf-jmap-metadata-01 §3.4.1 each field's semantics:
/// - `@type`: match if the Metadata `@type` discriminator is in the list.
/// - `relatedType`: case-sensitive equality.
/// - `relatedIds`: match if the Metadata `relatedId` is in the list.
/// - `isPrivate`: equality (Metadata default is `false` per §2.2.1.5).
/// - `textMatch`: case-insensitive substring against vendor-specific
///   string properties. For [`Metadata::Annotation`] this means values
///   of the `.extra` map; for `ImapMetadata` / `WebDavMetadata` it means
///   values of the `.metadata` BTreeMap plus the `.extra` map.
fn metadata_matches_condition(meta: &Metadata, cond: &MetadataFilterCondition) -> bool {
    if let Some(ref types) = cond.type_names {
        if !types.iter().any(|t| t == meta.type_name()) {
            return false;
        }
    }
    if let Some(ref rt) = cond.related_type {
        // Records whose wire input omitted `relatedType` (draft §4.1
        // partial-response shape) cannot satisfy a relatedType clause —
        // the clause requires an equality match against a value that
        // does not exist.
        if meta.related_type() != Some(rt.as_str()) {
            return false;
        }
    }
    if let Some(ref rids) = cond.related_ids {
        if !rids.iter().any(|id| id == meta.related_id()) {
            return false;
        }
    }
    if let Some(want_private) = cond.is_private {
        if meta.is_private() != want_private {
            return false;
        }
    }
    if let Some(ref needle) = cond.text_match {
        if !metadata_text_match(meta, needle) {
            return false;
        }
    }
    true
}

/// Case-insensitive substring search across the vendor-specific string
/// properties of a Metadata object.
///
/// Per draft-ietf-jmap-metadata-01 §3.4.1, `textMatch` searches "vendor-
/// specific string properties". For this reference impl that means:
/// - [`Metadata::Annotation`]: string values of the `.extra` flatten map.
///   The `id`, `relatedType`, `relatedId`, `isPrivate` typed fields are
///   NOT searched (they are not vendor properties).
/// - [`Metadata::ImapMetadata`] / [`Metadata::WebDavMetadata`]: values
///   of the typed `.metadata` BTreeMap (which IS the vendor payload for
///   those variants per §2.2.2.1 / §2.2.3.1) plus any leftover `.extra`
///   map entries.
///
/// Non-string `.extra` values are skipped (only string properties are
/// searchable per the spec's "string properties" language).
fn metadata_text_match(meta: &Metadata, needle: &str) -> bool {
    let needle_lower = needle.to_lowercase();
    let extra_match = |extra: &serde_json::Map<String, serde_json::Value>| -> bool {
        extra.values().any(|v| {
            v.as_str()
                .map(|s| s.to_lowercase().contains(&needle_lower))
                .unwrap_or(false)
        })
    };
    match meta {
        Metadata::Annotation(a) => extra_match(&a.extra),
        Metadata::ImapMetadata(m) => {
            m.metadata
                .values()
                .any(|s| s.to_lowercase().contains(&needle_lower))
                || extra_match(&m.extra)
        }
        Metadata::WebDavMetadata(m) => {
            m.metadata
                .values()
                .any(|s| s.to_lowercase().contains(&needle_lower))
                || extra_match(&m.extra)
        }
        // Metadata is #[non_exhaustive]; a future spec variant cannot be
        // text-matched without per-variant logic. Conservative default
        // is "no match" so an unknown variant does not silently pass a
        // textMatch filter that should have failed.
        _ => false,
    }
}

/// Compare two Metadata entries against a list of sort comparators.
///
/// Comparators are applied in order; the first non-Equal result wins.
/// Per draft-ietf-jmap-metadata-01 §3.4.2 the sortable properties are
/// `id`, `@type`, `relatedType`, `relatedId`, and `isPrivate`. Unknown
/// property names produce `Ordering::Equal` (no constraint) so the next
/// comparator (if any) takes effect; if every comparator is unknown,
/// the order is unspecified per RFC 8620 §5.5 (we return Equal, which
/// keeps insertion order via the stable sort).
///
/// Entries whose stored JSON did not deserialise into [`Metadata`] are
/// sorted by id only (they are backend-invariant violations and should
/// not influence property-based ordering).
fn compare_metadata_sort(
    sort: &[(String, bool)],
    a: &(Id, serde_json::Value, Option<Metadata>),
    b: &(Id, serde_json::Value, Option<Metadata>),
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (a_id, _, a_meta) = a;
    let (b_id, _, b_meta) = b;
    let (Some(a_m), Some(b_m)) = (a_meta.as_ref(), b_meta.as_ref()) else {
        return a_id.as_ref().cmp(b_id.as_ref());
    };
    for (property, ascending) in sort {
        let ord = match property.as_str() {
            "id" => a_id.as_ref().cmp(b_id.as_ref()),
            "@type" => a_m.type_name().cmp(b_m.type_name()),
            // Option<&str>::cmp orders None < Some(_) (Rust default
            // Option Ord), so records with omitted relatedType (draft
            // §4.1 partial-response shape) sort before records with a
            // relatedType present — acceptable as a tie-breaker.
            "relatedType" => a_m.related_type().cmp(&b_m.related_type()),
            "relatedId" => a_m.related_id().as_ref().cmp(b_m.related_id().as_ref()),
            "isPrivate" => a_m.is_private().cmp(&b_m.is_private()),
            // Unknown sort property — defer to the next comparator.
            // Validation of property names is the handler's job (per
            // workspace AGENTS.md "Caller identity / permission
            // enforcement: backends are canonical for permission
            // enforcement"; sort-property validation is a wire-shape
            // check, handler-canonical).
            _ => Ordering::Equal,
        };
        let ord = if *ascending { ord } else { ord.reverse() };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: create a Metadata Annotation, then get it back by id.
    /// Verifies the server-assigned id is stored and retrievable.
    #[tokio::test]
    async fn create_get_roundtrip() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let meta: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": true,
            "acme.example.com:color": "blue"
        }))
        .expect("fixture must deserialize");

        let (new_id, _) = backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c1", meta)
            .await
            .expect("create must succeed");

        let (found, not_found) = backend
            .get_objects::<Metadata>(
                &(),
                &Id::from("acc1"),
                Some(std::slice::from_ref(&new_id)),
                None,
            )
            .await
            .expect("get must succeed");

        assert!(not_found.is_empty(), "must find newly created object");
        assert_eq!(found.len(), 1);
        match &found[0] {
            Metadata::Annotation(a) => {
                assert_eq!(a.id.as_ref(), Some(&new_id));
                assert_eq!(a.related_type.as_deref(), Some("Email"));
                assert_eq!(a.is_private, Some(true));
                assert_eq!(
                    a.extra.get("acme.example.com:color"),
                    Some(&serde_json::Value::String("blue".to_owned()))
                );
            }
            other => panic!("expected Annotation variant, got {other:?}"),
        }
    }

    /// Oracle: draft §3.1 — creating a second Metadata with the same
    /// (relatedType, relatedId, @type, isPrivate) tuple returns
    /// `alreadyExists` with `existingId` pointing at the first object.
    #[tokio::test]
    async fn uniqueness_constraint_enforced() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);

        let first: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": false
        }))
        .unwrap();
        let (first_id, _) = backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c1", first)
            .await
            .expect("first create must succeed");

        let dup: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": false,
            "acme.example.com:color": "red"
        }))
        .unwrap();
        let err = backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c2", dup)
            .await
            .expect_err("duplicate create must fail");

        match err {
            BackendSetError::SetError(set_err) => {
                assert_eq!(set_err.error_type, SetErrorType::AlreadyExists);
                assert_eq!(
                    set_err.existing_id.as_ref(),
                    Some(&first_id),
                    "existingId must point at first object"
                );
            }
            other => panic!("expected SetError::AlreadyExists, got {other:?}"),
        }
    }

    /// Oracle: §3.1 — `isPrivate` defaults to `false` per §2.2.1.5. A
    /// shared (isPrivate=false) and a private (isPrivate=true) entry for
    /// the same (relatedType, relatedId, @type) are NOT in conflict.
    #[tokio::test]
    async fn shared_and_private_coexist() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);

        let shared: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": false
        }))
        .unwrap();
        backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c1", shared)
            .await
            .expect("shared create");

        let private: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1",
            "isPrivate": true
        }))
        .unwrap();
        backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c2", private)
            .await
            .expect("private create must succeed when shared exists with same triple");
    }

    /// Oracle: §3.1 — different `@type` values for the same
    /// (relatedType, relatedId) are not in conflict.
    #[tokio::test]
    async fn different_type_names_coexist() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);

        let ann: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Mailbox",
            "relatedId": "MB1"
        }))
        .unwrap();
        backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c1", ann)
            .await
            .expect("Annotation create");

        let imap: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "ImapMetadata",
            "relatedType": "Mailbox",
            "relatedId": "MB1",
            "metadata": {"comment": "team mailbox"}
        }))
        .unwrap();
        backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c2", imap)
            .await
            .expect("ImapMetadata create must succeed when Annotation exists for same triple");
    }

    /// Oracle: destroy advances state and removes the object from get.
    #[tokio::test]
    async fn destroy_advances_state_and_removes_object() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let meta: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1"
        }))
        .unwrap();
        let (new_id, _) = backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c1", meta)
            .await
            .unwrap();

        let state_before = backend
            .get_state::<Metadata>(&(), &Id::from("acc1"))
            .await
            .unwrap();

        backend
            .destroy_object::<Metadata>(&(), &Id::from("acc1"), &new_id)
            .await
            .expect("destroy must succeed");

        let state_after = backend
            .get_state::<Metadata>(&(), &Id::from("acc1"))
            .await
            .unwrap();

        assert_ne!(
            state_before.as_ref(),
            state_after.as_ref(),
            "destroy must advance state"
        );

        let (_, not_found) = backend
            .get_objects::<Metadata>(
                &(),
                &Id::from("acc1"),
                Some(std::slice::from_ref(&new_id)),
                None,
            )
            .await
            .unwrap();
        assert_eq!(not_found, vec![new_id]);
    }

    /// Oracle: bd:JMAP-ayoz.2 — `create_object` against an unknown
    /// accountId MUST return an error and MUST NOT auto-register the
    /// account in `known_accounts`. Defense-in-depth for callers that
    /// bypass the handler-level `account_exists` guard.
    #[tokio::test]
    async fn create_object_unknown_account_errors_without_registering() {
        // Backend has no known accounts.
        let backend = MemoryBackend::new_with_accounts(&[]);
        let bogus = Id::from("acc-bogus");

        assert!(
            !backend
                .account_exists(&(), &bogus)
                .await
                .expect("account_exists must succeed"),
            "pre-condition: acc-bogus must not be known",
        );

        let meta: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1"
        }))
        .expect("fixture must deserialize");

        let result = backend
            .create_object::<Metadata>(&(), &bogus, "c1", meta)
            .await;

        match result {
            Err(BackendSetError::Other(e)) => {
                assert!(
                    e.description().contains("unknown account"),
                    "error message must identify the account problem: {}",
                    e.description(),
                );
            }
            other => panic!("expected BackendSetError::Other for unknown account, got: {other:?}"),
        }

        // Post-condition: the bogus account was NOT silently registered.
        assert!(
            !backend
                .account_exists(&(), &bogus)
                .await
                .expect("account_exists must succeed"),
            "create_object must not auto-register unknown accountId",
        );
    }

    /// Oracle: bd:JMAP-826m.18 — the default-feature (deterministic)
    /// `demo_next_id` strategy recycles ids across destroy events. This
    /// test locks in the recycling behavior as a documented invariant
    /// of the demo backend, so a future change that switches to a
    /// strictly-monotonic counter has to revisit this test deliberately
    /// rather than silently break the demo's repeatable-ids property.
    ///
    /// **Behavioural consequence**: under this id-minting strategy a
    /// `Metadata/changes` response can carry the same id in `destroyed`
    /// and in a later `created` array. Clients relying on JMAP's
    /// "ids are unique forever" assumption (RFC 8620 §1.2) will
    /// misinterpret the second create as a resurrection. **Production
    /// backends MUST mint ULIDs / globally-unique ids** — see the
    /// module-level rustdoc 'Id minting' bullet.
    #[cfg(not(feature = "realistic-demo-ids"))]
    #[tokio::test]
    async fn demo_next_id_recycles_ids_across_destroy_in_deterministic_mode() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let meta1: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM1"
        }))
        .unwrap();
        let (id1, _) = backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c1", meta1)
            .await
            .expect("first create");

        backend
            .destroy_object::<Metadata>(&(), &Id::from("acc1"), &id1)
            .await
            .expect("destroy");

        let meta2: Metadata = serde_json::from_value(serde_json::json!({
            "@type": "Annotation",
            "relatedType": "Email",
            "relatedId": "EM2"
        }))
        .unwrap();
        let (id2, _) = backend
            .create_object::<Metadata>(&(), &Id::from("acc1"), "c2", meta2)
            .await
            .expect("second create");

        assert_eq!(
            id1, id2,
            "deterministic mode SHOULD recycle ids — production backends MUST NOT \
             follow this pattern. See module rustdoc 'Id minting' bullet.",
        );
    }

    // -----------------------------------------------------------------------
    // bd:JMAP-826m.13 — non-Metadata O is refused at the trait boundary
    //
    // The MetadataBackend trait is generic over O: SetObject for forward
    // compatibility with the workspace canonical shape, but this backend
    // stores only Metadata. The pre-fix code silently did partial work
    // (state bump, change-log push) for a non-Metadata O before any
    // failure surfaced from the typed objects map. The guards added in
    // this bead refuse the call at the top of each /set method.
    // -----------------------------------------------------------------------

    /// Stub JmapObject used only in regression tests below to verify the
    /// guards. Does not participate in any real JMAP wire format.
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    struct StubObject;

    impl crate::backend::JmapObject for StubObject {
        const TYPE_NAME: &'static str = "StubObject_NotMetadata";
        type Property = ();
    }

    impl crate::backend::SetObject for StubObject {
        type Patch = serde_json::Value;
    }

    /// Oracle: a `create_object::<StubObject>` call MUST be refused with
    /// a BackendSetError::Other carrying an unsupported-type message.
    /// Pre-fix, this call would have bumped state and pushed a
    /// change-log entry with empty related_type/type_name.
    #[tokio::test]
    async fn create_object_non_metadata_refused_at_boundary() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let state_before = backend
            .get_state::<Metadata>(&(), &Id::from("acc1"))
            .await
            .expect("get_state must succeed");

        let err = backend
            .create_object::<StubObject>(&(), &Id::from("acc1"), "c1", StubObject)
            .await
            .expect_err("non-Metadata create must be refused");
        match err {
            BackendSetError::Other(e) => {
                let msg = e.description();
                assert!(
                    msg.contains("MemoryBackend supports only Metadata"),
                    "unsupported-type message must name Metadata: {msg}",
                );
                assert!(
                    msg.contains("StubObject_NotMetadata"),
                    "unsupported-type message must name the rejected type: {msg}",
                );
            }
            other => panic!("expected BackendSetError::Other, got: {other:?}"),
        }

        let state_after = backend
            .get_state::<Metadata>(&(), &Id::from("acc1"))
            .await
            .expect("get_state must succeed");
        assert_eq!(
            state_before, state_after,
            "refused create must not bump state",
        );
    }

    /// Oracle: same guard applies to update_object.
    #[tokio::test]
    async fn update_object_non_metadata_refused_at_boundary() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let err = backend
            .update_object::<StubObject>(
                &(),
                &Id::from("acc1"),
                &Id::from("does-not-matter"),
                serde_json::Value::Null,
            )
            .await
            .expect_err("non-Metadata update must be refused");
        assert!(matches!(err, BackendSetError::Other(_)));
    }

    /// Oracle: same guard applies to destroy_object.
    #[tokio::test]
    async fn destroy_object_non_metadata_refused_at_boundary() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let err = backend
            .destroy_object::<StubObject>(&(), &Id::from("acc1"), &Id::from("does-not-matter"))
            .await
            .expect_err("non-Metadata destroy must be refused");
        assert!(matches!(err, BackendSetError::Other(_)));
    }
}
