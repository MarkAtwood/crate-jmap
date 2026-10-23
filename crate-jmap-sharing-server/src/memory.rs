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
//! Promoted from `tests/common/mod.rs` per Beads issue JMAP-hwdv (epic)
//! and JMAP-hwdv.2 (this crate, mirror of canonical JMAP-hwdv.1 in
//! jmap-mail-server).

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams,
    QueryResult, SetError, SetErrorType, SetObject, SharingBackend,
};
use jmap_server::{apply_patch_object, limit_or_max, resolve_query_start};
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
    /// `type_name` → `account_id` → `id → serialized object`
    objects: HashMap<&'static str, HashMap<String, HashMap<Id, serde_json::Value>>>,
    /// `type_name` → `account_id` → current state counter
    states: HashMap<&'static str, HashMap<String, u64>>,
    /// `type_name` → `account_id` → ordered change entries
    change_log: HashMap<&'static str, HashMap<String, Vec<ChangeEntry>>>,
    /// explicitly registered account ids (accounts may exist with no objects yet)
    known_accounts: HashSet<String>,
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
#[non_exhaustive]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryBackend {
    /// Construct an empty [`MemoryBackend`] with no accounts or stored
    /// objects. Equivalent to [`Self::default`].
    pub fn new() -> Self {
        Self::default()
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
    /// Matches the canonical [`jmap_mail_server::memory::MemoryBackend::register_account`](https://docs.rs/jmap-mail-server)
    /// shape (`&self` + `&Id`). Prefer this over the legacy
    /// [`Self::new_with_accounts`] constructor in new code — it composes
    /// with `Arc<MemoryBackend>` since it takes `&self`, and accepts the
    /// strongly-typed [`Id`] rather than a raw `&str`.
    #[doc(hidden)]
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
}

/// Opaque storage-layer error returned by [`MemoryBackend`] operations.
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
/// Mirrors the canonical jmap-mail-server `MemoryError` shape.
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

/// RFC 8620 §5.3 `invalidProperties` for a patch whose result no longer
/// decodes as `type_name`. The property is named only when the patch
/// touches a single top-level property; otherwise the culprit is not known.
fn invalid_patched_properties(
    type_name: &str,
    patch_map: &serde_json::Map<String, serde_json::Value>,
) -> BackendSetError<MemoryError> {
    let mut props: Vec<&str> = patch_map
        .keys()
        .map(|k| k.split_once('/').map_or(k.as_str(), |(top, _)| top))
        .collect();
    props.sort_unstable();
    props.dedup();
    let err = SetError::new(SetErrorType::InvalidProperties).with_description(format!(
        "patched {type_name} violates its type specification"
    ));
    BackendSetError::SetError(match props.as_slice() {
        [prop] => Box::new(err.with_properties([*prop])),
        _ => Box::new(err),
    })
}

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
                                )));
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
                                )));
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
            .get(O::TYPE_NAME)
            .and_then(|m| m.get(account_id.as_ref()))
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        let relevant: Vec<&ChangeEntry> = log.iter().filter(|e| e.new_state > since_n).collect();

        let current_state = inner.current_state(O::TYPE_NAME, account_id.as_ref());

        if let Some(max) = max_changes
            && relevant.len() as u64 > max
        {
            return Err(BackendChangesError::TooManyChanges { limit: max });
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
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        let QueryParams {
            limit,
            position,
            anchor,
            anchor_offset,
            ..
        } = params;
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
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        let QueryChangesParams {
            since_query_state,
            max_changes,
            ..
        } = params;
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
        let server_id = Id::from(uuid::Uuid::new_v4().to_string());
        let stored_obj = build(&server_id);
        // Uses `map_err` (not `.expect`) because `O` is an associated type
        // controlled by the consumer: a downstream `SetObject` impl with a
        // hand-rolled `Serialize` could in principle fail. Handler-site
        // serializations of plain crate-defined types use `.expect` since
        // their derive(Serialize) impl is provably infallible
        // (see `helpers::set_error_value`, `principal::handle_principal_set`).
        let val = serde_json::to_value(&stored_obj)
            .map_err(|e| BackendSetError::Other(MemoryError::new(format!("serialize: {e}"))))?;

        let mut inner = self.inner.lock().unwrap();

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
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::NotFound,
            ))));
        }

        // `.cloned()` is deliberate: it preserves atomic-on-failure
        // semantics. The patch is applied to the local clone first,
        // and only the successful result is written back to storage.
        // A failed patch discards the local without touching the
        // stored value.
        let mut current = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .get(id)
            .cloned()
            .ok_or_else(|| {
                BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
            })?;

        // Apply the JMAP PatchObject (RFC 8620 §5.3): keys are JSON Pointer
        // paths and each value replaces the member at its path wholesale.
        // Any `PatchPathError` is an RFC 8620 §5.3 "invalidPatch", including
        // the shared helper's nesting cap on path plus value depth.
        //
        // `map_err` (not `.expect`) on `to_value(&patch)` because
        // `O::Patch` is a consumer-controlled associated type — see the
        // `create_object` comment above for the full rationale.
        let patch_val = serde_json::to_value(&patch).map_err(|e| {
            BackendSetError::Other(MemoryError::new(format!("serialize patch: {e}")))
        })?;
        let serde_json::Value::Object(patch_map) = patch_val else {
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidPatch)
                    .with_description("PatchObject must be a JSON object"),
            )));
        };
        let serde_json::Value::Object(base) = &mut current else {
            return Err(BackendSetError::Other(MemoryError::new(format!(
                "stored {} is not a JSON object",
                O::TYPE_NAME
            ))));
        };
        apply_patch_object(base, &patch_map).map_err(|e| {
            BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()),
            ))
        })?;
        // A null or wrong-typed value on a required property must not reach
        // storage, or every later /get fails to decode it.
        if O::deserialize(&current).is_err() {
            return Err(invalid_patched_properties(O::TYPE_NAME, &patch_map));
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
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::NotFound,
            ))));
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
        let principal = |id: &Id| -> Principal {
            serde_json::from_value(serde_json::json!({
                "type": "individual",
                "name": "Test User",
                "id": id,
                "description": null,
                "email": null,
                "timeZone": null,
                "capabilities": {},
                "accounts": null
            }))
            .expect("must deserialize")
        };

        let (new_id, _) = backend
            .create_object::<Principal, _>(&(), &Id::from("acc1"), "c1", principal)
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
                .entry(Principal::TYPE_NAME)
                .or_default()
                .insert("acc1".to_owned(), 3);
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
    /// MUST carry the server-assigned `id` as its own `id` field.
    /// Regression guard for bd:JMAP-3t94.17.
    #[tokio::test]
    async fn create_object_returned_o_carries_server_assigned_id() {
        let backend = MemoryBackend::new_with_accounts(&["acc1"]);
        let principal = |id: &Id| -> Principal {
            serde_json::from_value(serde_json::json!({
                "type": "individual",
                "name": "Invariant Probe",
                "id": id,
                "description": null,
                "email": null,
                "timeZone": null,
                "capabilities": {},
                "accounts": null
            }))
            .expect("must deserialize")
        };

        let (new_id, stored_obj) = backend
            .create_object::<Principal, _>(&(), &Id::from("acc1"), "c1", principal)
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
                .entry(Principal::TYPE_NAME)
                .or_default()
                .insert("acc1".to_owned(), 2);
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
