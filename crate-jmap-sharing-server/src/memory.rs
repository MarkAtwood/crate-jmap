//! In-memory reference implementation of [`SharingBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`SharingBackend`] trait to study when writing
//!    a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Sharing dispatcher
//!    with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and a number
//! of RFC 9670 edge cases are simplified (see source comments).
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
//! use jmap_sharing_server::{memory::MemoryBackend, register_sharing_handlers};
//! use jmap_server::Dispatcher;
//!
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_sharing_handlers(&mut dispatcher, Arc::new(MemoryBackend::new()));
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
//! Promoted from `tests/common/mod.rs` per Beads issue JMAP-hwdv (epic)
//! and JMAP-hwdv.2 (this crate, mirror of canonical JMAP-hwdv.1 in
//! jmap-mail-server).

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    SharingBackend,
};
// json_merge_patch lives in jmap-server (the shared foundation crate)
// since bd:JMAP-sc1b.103. Every reference backend imports it; the
// canonical RFC 7396 tests live with the function there (including the
// bd:JMAP-sc1b.97 depth-cap and bd:JMAP-sc1b.87 absent-field regression
// tests).
use jmap_server::{json_merge_patch, MergePatchError};
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// Internal state
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

/// Shared inner state, behind Arc<Mutex>.
#[derive(Default)]
struct Inner {
    /// `(type_name, account_id)` → `id → serialized object`
    objects: HashMap<(&'static str, String), HashMap<Id, serde_json::Value>>,
    /// `(type_name, account_id)` → current state counter
    states: HashMap<(&'static str, String), u64>,
    /// `(type_name, account_id)` → ordered change entries
    change_log: HashMap<(&'static str, String), Vec<ChangeEntry>>,
    /// explicitly registered account ids (accounts may exist with no objects yet)
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
        // Note: deliberately no `known_accounts.insert(...)` side-effect.
        // Account registration is the responsibility of the caller (the
        // matching `SharingBackend` write op) — `create_object` registers
        // explicitly after the create commits, while `update_object` and
        // `destroy_object` MUST gate on `known_accounts.contains(...)`
        // before invoking this helper. Coupling registration to lookup
        // would let an unknown accountId pass the handler-layer
        // `account_exists` check on its second visit (RFC 8620 §3.6.2
        // requires consistent `accountNotFound`).
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
// MemoryBackend
// ---------------------------------------------------------------------------

/// A fully in-memory implementation of [`SharingBackend`].
///
/// Stores objects as serialized JSON; each mutation bumps a monotonic state
/// counter and records a change log entry.
///
/// # Clone semantics — shared state, not independent copies
///
/// `MemoryBackend: Clone` is implemented as a cheap [`Arc`] clone:
/// `b1.clone()` produces a second handle to the **same** shared state
/// behind a [`Mutex`]. Mutating through one clone is visible through every
/// other clone. This is the workspace pattern for `Arc<Mutex<_>>`-backed
/// reference impls and matches the canonical `jmap-mail-server`.
///
/// To build an independent backend (e.g. for parameterized tests that need
/// distinct state per case), construct a new instance with
/// [`Self::new`] / [`Self::default`] rather than calling `.clone()`.
#[derive(Clone, Default)]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryBackend {
    /// Construct an empty [`MemoryBackend`] with no accounts or stored
    /// objects. Equivalent to [`Self::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an account as known even if it has no objects yet.
    /// Use this in tests that need an empty-but-valid account.
    ///
    /// Matches the canonical [`jmap_mail_server::memory::MemoryBackend::register_account`](https://docs.rs/jmap-mail-server)
    /// shape (`&self` + `&Id`). Prefer this over the legacy
    /// [`Self::new_with_accounts`] constructor in new code — it composes
    /// with `Arc<MemoryBackend>` since it takes `&self`, and accepts the
    /// strongly-typed [`Id`] rather than a raw `&str`.
    pub fn register_account(&self, account_id: &Id) {
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.as_ref().to_owned());
    }

    /// Register one or more accounts as known even if they have no objects yet.
    ///
    /// Convenience constructor that calls [`Self::register_account`] for
    /// each id. Equivalent to `Self::new()` followed by repeated
    /// `register_account` calls. The canonical sibling
    /// `jmap-mail-server` does NOT ship this shape — it only ships
    /// `register_account(&self, &Id)`. This crate keeps `new_with_accounts`
    /// for ergonomic test fixture construction; new code SHOULD prefer
    /// `register_account` post-construction for canonical-template
    /// alignment.
    pub fn new_with_accounts(account_ids: &[&str]) -> Self {
        let b = Self::new();
        for id in account_ids {
            b.register_account(&Id::from(*id));
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
}

/// Opaque storage-layer error returned by [`MemoryBackend`] operations.
///
/// The inner [`String`] is a human-readable description intended for
/// diagnostic logging; it is not a stable wire-format identifier.
#[derive(Debug)]
pub struct MemoryError(
    /// Human-readable description of the underlying failure.
    pub String,
);

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
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
                                return Err(MemoryError(format!(
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
                                return Err(MemoryError(format!(
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

        // Compute each id's NET outcome across the change interval.
        //
        // RFC 8620 §5.2: every id reported in /changes appears in exactly
        // one of `created`/`updated`/`destroyed`, reflecting the net
        // transition from `sinceState` to the current state. Transitions
        // within the interval (create → destroy → create on a recycled
        // id, update → destroy, etc.) collapse to the final bucket.
        //
        // The previous implementation used three `Vec<Id>`s and
        // `Vec::contains` for dedup. That had two defects: (a) the create
        // branch suppressed ids already in `destroyed`, so a recycled id
        // sequence `create K / destroy K+1 / create K+2` left the id out
        // of `created` despite it being present in the current state
        // (bd:JMAP-3t94.3); (b) `Vec::contains` on each push made the
        // loop O(n*m) (bd:JMAP-3t94.7).
        //
        // The replacement walks each entry once and stores each id's
        // most-recent outcome in a `HashMap`, preserving first-seen order
        // in a parallel `Vec`. Later transitions overwrite earlier ones;
        // a destroy followed by a create overrides the destroy and vice
        // versa. Time complexity is O(total ids in interval); space is
        // O(distinct ids).
        #[derive(Copy, Clone)]
        enum Outcome {
            Created,
            Updated,
            Destroyed,
        }

        let mut outcome: HashMap<Id, Outcome> = HashMap::new();
        let mut order: Vec<Id> = Vec::new();

        for entry in &relevant {
            for id in &entry.created {
                if outcome.insert(id.clone(), Outcome::Created).is_none() {
                    order.push(id.clone());
                }
            }
            for id in &entry.updated {
                // An id already classified as `Created` in this interval
                // stays `Created` (a later update does not demote a fresh
                // creation); otherwise mark `Updated`.
                if matches!(outcome.get(id), Some(Outcome::Created)) {
                    continue;
                }
                if outcome.insert(id.clone(), Outcome::Updated).is_none() {
                    order.push(id.clone());
                }
            }
            for id in &entry.destroyed {
                if outcome.insert(id.clone(), Outcome::Destroyed).is_none() {
                    order.push(id.clone());
                }
            }
        }

        let mut created: Vec<Id> = Vec::new();
        let mut updated: Vec<Id> = Vec::new();
        let mut destroyed: Vec<Id> = Vec::new();
        for id in order {
            match outcome[&id] {
                Outcome::Created => created.push(id),
                Outcome::Updated => updated.push(id),
                Outcome::Destroyed => destroyed.push(id),
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

    /// **Reference-only** implementation of [`JmapBackend::query_objects`].
    ///
    /// This implementation **ignores `filter` and `sort`**: it returns every
    /// stored object for the type/account in lexicographic id order,
    /// independent of any filter expression or comparator list passed by the
    /// caller. Pagination (`position`, `limit`) is honored.
    ///
    /// Production backends MUST evaluate the filter expression and apply the
    /// comparators per RFC 8620 §5.5 / RFC 9670 §2.4 (Principal/query) and
    /// §3.4 (ShareNotification/query). Failing to do so causes clients to
    /// receive ALL objects when they asked for a filtered subset, with no
    /// loud error signal.
    ///
    /// The crate's integration tests wrap this backend in a
    /// `FilteringBackend` (see `tests/query_changes_tests.rs`) when filter
    /// evaluation is required. Downstream implementors building a real
    /// backend should treat that wrapper as the contract, not this method.
    ///
    /// Sibling-scope: six other `MemoryBackend` reference impls
    /// (chat, calendars, contacts, metadata, tasks, filenode) carry the
    /// same gap.
    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        let inner = self.inner.lock().unwrap();

        let mut ids: Vec<Id> = inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .map(|m| {
                let mut keys: Vec<Id> = m.keys().cloned().collect();
                keys.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
                keys
            })
            .unwrap_or_default();

        let total = ids.len() as u64;
        let query_state = State::from(
            inner
                .current_state(O::TYPE_NAME, account_id.as_ref())
                .to_string(),
        );

        let start = if position >= 0 {
            (position as usize).min(ids.len())
        } else {
            let neg = position.saturating_neg() as usize;
            ids.len().saturating_sub(neg)
        };

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

    /// **Reference-only** implementation of [`JmapBackend::query_changes`].
    ///
    /// Delegates to [`Self::get_changes`] for the change interval and
    /// inherits the same filter/sort gap as [`Self::query_objects`]: the
    /// `filter`, `sort`, `up_to_id`, and `collapse_threads` parameters are
    /// ignored. Production backends MUST honor them per RFC 8620 §5.6 to
    /// produce a coherent `added`/`removed` diff against the filtered,
    /// sorted view.
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

impl SharingBackend for MemoryBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();
        let server_id = Self::demo_next_id(&mut inner, O::TYPE_NAME, account_id.as_ref());

        // Serialize, set "id" to the server-assigned id, then deserialize back.
        // Uses `map_err` (not `.expect`) because `O` is an associated type
        // controlled by the consumer: a downstream `SetObject` impl with a
        // hand-rolled `Serialize` could in principle fail. Handler-site
        // serializations of plain crate-defined types use `.expect` since
        // their derive(Serialize) impl is provably infallible
        // (see `helpers::set_error_value`, `principal::handle_principal_set`).
        let mut val = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError(format!("serialize: {e}"))))?;
        val["id"] = serde_json::Value::String(server_id.as_ref().to_owned());
        let stored_obj: O = O::deserialize(&val).map_err(|e| {
            BackendSetError::Other(MemoryError(format!("deserialize after create: {e}")))
        })?;

        inner.known_accounts.insert(account_id.as_ref().to_owned());
        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
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

        // Backend-canonical re-check (AGENTS.md "Caller identity" /
        // "Permission enforcement: backend canonical"). The handler-layer
        // `account_exists` check is the optional pre-check; the backend
        // MUST re-verify atomically with the mutation. `NotFound` is
        // returned per RFC 8620 §5.3: an unknown account has no objects,
        // so any update target is `notFound`.
        if !inner.known_accounts.contains(account_id.as_ref()) {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )));
        }

        // `.cloned()` is deliberate: it preserves atomic-on-failure
        // semantics. The patch is applied to the local clone first,
        // and only the successful result is written back to storage.
        // A failed `json_merge_patch` discards the partially-mutated
        // local without touching the stored value. The canonical
        // `jmap-mail-server::MemoryBackend` uses `.get_mut` and
        // documents that it does NOT preserve this property
        // (memory.rs:1053-1057). The sharing-server sweep (4 sibling
        // backends carry the same shape) should preserve the clone.
        let mut current = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .get(id)
            .cloned()
            .ok_or_else(|| BackendSetError::SetError(SetError::new(SetErrorType::NotFound)))?;

        // Apply JSON Merge Patch (RFC 7396). A `MergePatchError::DepthExceeded`
        // return (bd:JMAP-wlip.1) surfaces as `SetErrorType::InvalidPatch` —
        // the depth cap is a DoS guard, never fires on legitimate JMAP `/set
        // update` shapes. `current` is a clone of the stored value, so a
        // partially-applied patch on error is discarded with the local
        // without touching storage.
        //
        // `map_err` (not `.expect`) on `to_value(&patch)` because
        // `O::Patch` is a consumer-controlled associated type — see the
        // `create_object` comment above for the full rationale.
        let patch_val = serde_json::to_value(&patch)
            .map_err(|e| BackendSetError::Other(MemoryError(format!("serialize patch: {e}"))))?;
        if let Err(MergePatchError::DepthExceeded) = json_merge_patch(&mut current, patch_val) {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidPatch)
                    .with_description("patch nesting exceeds server limit"),
            ));
        }

        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
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

        // Backend-canonical re-check (AGENTS.md "Caller identity" /
        // "Permission enforcement: backend canonical"). See `update_object`
        // for the rationale; `NotFound` is the correct SetErrorType for
        // an unknown account because no object can exist there.
        if !inner.known_accounts.contains(account_id.as_ref()) {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )));
        }

        let removed = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .remove(id);

        match removed {
            Some(_) => {
                let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
                inner
                    .change_log_mut(O::TYPE_NAME, account_id.as_ref())
                    .push(ChangeEntry {
                        new_state,
                        created: vec![],
                        updated: vec![],
                        destroyed: vec![id.clone()],
                    });
                Ok(())
            }
            None => Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            ))),
        }
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_sharing_types::Principal;

    /// Oracle: create a Principal then get it back by id.
    ///
    /// Verifies that create_object stores the object and get_objects can
    /// retrieve it with the server-assigned id.
    #[tokio::test]
    async fn memory_backend_principal_create_get_roundtrip() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let principal: Principal = serde_json::from_value(serde_json::json!({
            "type": "individual",
            "name": "Test User",
            "id": "placeholder",
            "description": null,
            "email": null,
            "timeZone": null,
            "capabilities": {},
            "accounts": null
        }))
        .expect("must deserialize");

        let (new_id, _) = backend
            .create_object::<Principal>(&(), &Id::from("acc1"), "c1", principal)
            .await
            .expect("create must succeed");

        let (found, not_found) = backend
            .get_objects::<Principal>(
                &(),
                &Id::from("acc1"),
                Some(std::slice::from_ref(&new_id)),
                None,
            )
            .await
            .expect("get must succeed");

        assert!(
            not_found.is_empty(),
            "newly created principal must be found, not_found: {not_found:?}"
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id.as_ref(), new_id.as_ref());
    }

    /// Oracle: a synthetic create → destroy → create sequence on the SAME id
    /// (the recycled-id case from bd:JMAP-3t94.3) MUST land in `created` in
    /// the /changes response, not silently disappear.
    ///
    /// Drives change_log_mut directly because the public surface mints fresh
    /// ids and would not naturally produce the recycle. RFC 8620 §5.2: net
    /// effect across the interval determines the bucket; here the id IS
    /// present in the final state so it MUST appear in `created`.
    #[tokio::test]
    async fn get_changes_recycled_id_lands_in_created() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let recycled = Id::from("recycled");

        {
            let mut inner = backend.inner.lock().unwrap();
            let log = inner.change_log_mut(Principal::TYPE_NAME, "acc1");
            log.push(ChangeEntry {
                new_state: 1,
                created: vec![recycled.clone()],
                updated: vec![],
                destroyed: vec![],
            });
            log.push(ChangeEntry {
                new_state: 2,
                created: vec![],
                updated: vec![],
                destroyed: vec![recycled.clone()],
            });
            log.push(ChangeEntry {
                new_state: 3,
                created: vec![recycled.clone()],
                updated: vec![],
                destroyed: vec![],
            });
            inner
                .states
                .insert((Principal::TYPE_NAME, "acc1".to_owned()), 3);
        }

        let changes = backend
            .get_changes::<Principal>(&(), &Id::from("acc1"), &State::from("0"), None)
            .await
            .expect("get_changes must succeed");

        assert!(
            changes.created.contains(&recycled),
            "recycled id must appear in created; got created={:?} destroyed={:?}",
            changes.created,
            changes.destroyed
        );
        assert!(
            !changes.destroyed.contains(&recycled),
            "recycled id must NOT appear in destroyed (final state has it); \
             got destroyed={:?}",
            changes.destroyed
        );
    }

    /// Oracle: `SharingBackend::create_object` invariant — the returned `O`
    /// MUST carry the server-assigned `id` as its own `id` field, not the
    /// caller's placeholder. Regression guard for bd:JMAP-3t94.17.
    #[tokio::test]
    async fn create_object_returned_o_carries_server_assigned_id() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let principal: Principal = serde_json::from_value(serde_json::json!({
            "type": "individual",
            "name": "Invariant Probe",
            "id": "placeholder",
            "description": null,
            "email": null,
            "timeZone": null,
            "capabilities": {},
            "accounts": null
        }))
        .expect("must deserialize");

        let (new_id, stored_obj) = backend
            .create_object::<Principal>(&(), &Id::from("acc1"), "c1", principal)
            .await
            .expect("create must succeed");

        assert_ne!(
            new_id.as_ref(),
            "placeholder",
            "server-assigned id must differ from placeholder"
        );
        assert_eq!(
            stored_obj.id.as_ref(),
            new_id.as_ref(),
            "returned O.id MUST equal the server-assigned tuple Id \
             (SharingBackend::create_object invariant); got O.id={:?}, \
             tuple Id={:?}",
            stored_obj.id.as_ref(),
            new_id.as_ref()
        );
    }

    /// Oracle: a create → destroy sequence on the same id within an interval
    /// SHOULD per RFC 8620 §5.2 either omit the id or report it as destroyed
    /// only; either way it MUST NOT appear in `created` (the final state
    /// does not have it). The MemoryBackend chooses the MAY-allowed
    /// destroyed-only variant.
    #[tokio::test]
    async fn get_changes_create_then_destroy_lands_in_destroyed_only() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let ephemeral = Id::from("ephemeral");

        {
            let mut inner = backend.inner.lock().unwrap();
            let log = inner.change_log_mut(Principal::TYPE_NAME, "acc1");
            log.push(ChangeEntry {
                new_state: 1,
                created: vec![ephemeral.clone()],
                updated: vec![],
                destroyed: vec![],
            });
            log.push(ChangeEntry {
                new_state: 2,
                created: vec![],
                updated: vec![],
                destroyed: vec![ephemeral.clone()],
            });
            inner
                .states
                .insert((Principal::TYPE_NAME, "acc1".to_owned()), 2);
        }

        let changes = backend
            .get_changes::<Principal>(&(), &Id::from("acc1"), &State::from("0"), None)
            .await
            .expect("get_changes must succeed");

        assert!(
            !changes.created.contains(&ephemeral),
            "ephemeral id MUST NOT appear in created (final state lacks it); \
             got created={:?}",
            changes.created
        );
        assert!(
            changes.destroyed.contains(&ephemeral),
            "MemoryBackend reports create→destroy in destroyed-only; \
             got destroyed={:?}",
            changes.destroyed
        );
    }
}
