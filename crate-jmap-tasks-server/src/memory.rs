//! In-memory reference implementation of [`TasksBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`TasksBackend`] trait to
//!    study when writing a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Tasks dispatcher
//!    with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and a number
//! of draft-ietf-jmap-tasks-06 edge cases are simplified (see source
//! comments). In particular, UTC-time conversion
//! ([`compute_utc_times`](crate::TasksBackend::compute_utc_times))
//! inherits the trait's default (no time-zone conversion), and
//! [`update_task_per_user`](crate::TasksBackend::update_task_per_user)
//! stores per-user patches on the shared object (single-user backend).
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
//! use jmap_tasks_server::{memory::MemoryBackend, register_tasks_handlers};
//! use jmap_server::Dispatcher;
//!
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_tasks_handlers(&mut dispatcher, Arc::new(MemoryBackend::new()));
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
//! JMAP-hwdv.7 (this crate, mirror of canonical JMAP-hwdv.1 in
//! jmap-mail-server, following the multi-type-store shape established
//! by jmap-chat-server's `MemoryBackend` and the derived-index pattern
//! pioneered in JMAP-hwdv.5 jmap-calendars-server).

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams,
    QueryResult, SetError, SetErrorType, SetObject, TasksBackend,
};
use jmap_server::{apply_patch_object, limit_or_max, resolve_query_start};
use jmap_types::{Id, JmapError, State};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

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
    /// Refcount of TaskList ids by the number of Tasks attached to each
    /// (drives `TaskList/set` destroy rejection with `taskListHasTask`
    /// per draft-ietf-jmap-tasks-06 §3.4 when `onDestroyRemoveTasks` is
    /// `false`). Maintained incrementally: `inc_task_ref` on Task create,
    /// `dec_task_ref` on Task destroy, both on a taskListId change. O(1)
    /// per Task mutation — the previous full-scan recompute was O(N).
    /// Entries are removed when the count drops to 0 so `task_list_has_tasks`
    /// can read presence-as-truthy instead of value-as-truthy.
    task_list_refcount: HashMap<Id, u64>,
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
    /// `account_id` → auxiliary per-account state (derived indexes)
    aux: HashMap<String, AccountAux>,
    /// Explicitly registered account ids (accounts may exist with no objects yet).
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

    /// Read `taskListId` out of a serialized Task value, if present.
    fn task_list_id_of(task: &serde_json::Value) -> Option<Id> {
        task.get("taskListId")
            .and_then(|v| v.as_str())
            .map(Id::from)
    }

    /// Increment the refcount for `list_id` in the given account by one.
    /// Called when a Task is created or its `taskListId` changes to
    /// `list_id`. No-op when `list_id` is `None` (well-formed Tasks
    /// always have a taskListId but the helper is defensive against
    /// partial mid-write state).
    fn inc_task_ref(&mut self, account_id: &str, list_id: Option<Id>) {
        if let Some(id) = list_id {
            *self
                .aux_mut(account_id)
                .task_list_refcount
                .entry(id)
                .or_insert(0) += 1;
        }
    }

    /// Decrement the refcount for `list_id` in the given account by one.
    /// Removes the entry when the count drops to zero so `task_list_has_tasks`
    /// can read presence-as-truthy. Safe to call with `None` or an unknown
    /// id (no-op).
    fn dec_task_ref(&mut self, account_id: &str, list_id: Option<Id>) {
        let Some(id) = list_id else { return };
        let aux = self.aux_mut(account_id);
        if let Some(count) = aux.task_list_refcount.get_mut(&id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                aux.task_list_refcount.remove(&id);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// A fully in-memory implementation of [`TasksBackend`].
///
/// Stores objects as serialized JSON; each mutation bumps a monotonic state
/// counter and records a change log entry. Used as both the integration-test
/// harness and the canonical example for backend implementors.
///
/// Cloning is cheap and **shared-state**: every clone retains an
/// `Arc<Mutex<…>>` handle to the *same* underlying object store and change
/// log. See the [`Clone`] impl below for the contract; a casual reader who
/// reaches for `#[derive(Clone)]` semantics will be wrong. To get a snapshot
/// with independent state, construct a fresh `MemoryBackend::new()` and seed
/// it via `register_account` / the typed test helpers.
#[derive(Default)]
#[non_exhaustive]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

/// Manual `Clone` impl, NOT `#[derive(Clone)]`.
///
/// Cloning a [`MemoryBackend`] does **not** copy any state — both the
/// original and the clone hold `Arc::clone`s of the same `Mutex<Inner>` and
/// observe each other's mutations. This is intentional: handler registration
/// in [`crate::register_tasks_handlers`] takes `Arc<B>` and each method
/// handler closure clones it; without shared mutation semantics, every
/// handler would see a snapshot stale by the time it runs. The manual impl
/// exists (instead of `#[derive(Clone)]`) so the contract is visible at the
/// type, not buried in the module docs.
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

    /// Register an account as known even if it has no objects yet.
    /// Returns `self` for builder-style chaining.
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
    #[doc(hidden)]
    pub fn register_account(&self, account_id: &Id) {
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.as_ref().to_owned());
        inner.aux.entry(account_id.as_ref().to_owned()).or_default();
    }

    /// Seed a pre-existing object into the store without bumping the state
    /// counter or recording a change-log entry. Intended for test fixture
    /// setup; the `type_name` must match `O::TYPE_NAME` of the type being
    /// seeded (e.g. `"TaskList"`, `"Task"`, `"TaskNotification"`).
    pub fn seed_object(
        &self,
        account_id: &str,
        type_name: &'static str,
        id: &str,
        value: serde_json::Value,
    ) {
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.to_owned());
        let list_id = if type_name == "Task" {
            Inner::task_list_id_of(&value)
        } else {
            None
        };
        let prev = inner
            .objects_mut(type_name, account_id)
            .insert(Id::from(id), value);
        if type_name == "Task" {
            // If this id was already seeded, decrement its prior taskListId
            // refcount first to keep the index correct on re-seed.
            if let Some(old) = prev.as_ref() {
                inner.dec_task_ref(account_id, Inner::task_list_id_of(old));
            }
            inner.inc_task_ref(account_id, list_id);
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

        // The deserialize error closure is shared by both branches.
        let deser_err =
            |e: serde_json::Error| MemoryError::new(format!("deserialize {}: {e}", O::TYPE_NAME));

        match ids {
            None => {
                // Collect every stored object; bail on the first deserialize
                // error via the standard `Result<Vec<_>, _>` collect idiom.
                let list = map
                    .map(|m| {
                        m.values()
                            .map(|val| O::deserialize(val).map_err(deser_err))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                Ok((list, vec![]))
            }
            Some(id_slice) => {
                // Two output vecs (found + not_found) — keep the explicit
                // loop, but use `?` instead of a manual `match Err { return }`.
                let mut found = Vec::new();
                let mut not_found = Vec::new();
                for id in id_slice {
                    match map.and_then(|m| m.get(id)) {
                        Some(val) => found.push(O::deserialize(val).map_err(deser_err)?),
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

        let mut created: Vec<Id> = Vec::new();
        let mut updated: Vec<Id> = Vec::new();
        let mut destroyed: Vec<Id> = Vec::new();

        for entry in &relevant {
            for id in &entry.created {
                if !destroyed.contains(id) && !created.contains(id) {
                    created.push(id.clone());
                }
            }
            for id in &entry.updated {
                if !destroyed.contains(id) && !created.contains(id) && !updated.contains(id) {
                    updated.push(id.clone());
                }
            }
            for id in &entry.destroyed {
                created.retain(|c| c != id);
                updated.retain(|u| u != id);
                if !destroyed.contains(id) {
                    destroyed.push(id.clone());
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

    /// Returns all object ids in this account, sorted lexicographically by id,
    /// paginated by `position` and `limit`.
    ///
    /// # Filter and sort are NOT honored
    ///
    /// This reference implementation supports exactly one filter shape: a
    /// Task filter whose only set field is `taskListId` (draft-tasks-06
    /// §4.6), which the `TaskList/set` `onDestroyRemoveTasks` cascade
    /// (draft-tasks-06 §3.4) needs. It does **not** support any other filter
    /// or any sort shape. RFC 8620 §5.5 mandates that an unsupported `filter` produces
    /// `unsupportedFilter` at the method level, and an unsupported `sort`
    /// produces `unsupportedSort`. A production backend MUST inspect both
    /// arguments against the type's [`QueryObject`] supported sets and
    /// reject queries that exceed them.
    ///
    /// To prevent consumers copying the silent-drop shape into a real
    /// backend (a workspace test-integrity hazard documented in the
    /// `MemoryBackend` module rustdoc), this method **fails loud** when
    /// either argument is non-trivial:
    ///
    /// - `filter` is `Some` carrying any non-null field other than a Task
    ///   `taskListId` → returns [`MemoryError`] with a `filter not
    ///   supported` message.
    /// - `sort` is `Some` with one or more comparators → returns
    ///   [`MemoryError`] with a `sort not supported` message.
    ///
    /// The handler maps both to `serverFail` on the wire (a workspace-canonical
    /// approximation; a properly typed `unsupportedFilter` / `unsupportedSort`
    /// path through the backend would require a richer error variant on
    /// the `JmapBackend` trait).
    ///
    /// Callers that want "all objects in this account" pass
    /// `filter = None`, `sort = None`.
    ///
    /// Mirrors the reference-impl correctness teaching applied to the
    /// isDraft re-check in `update_object` (bd:JMAP-h47t.4): the demo
    /// backend models the spec invariant rather than the spec-violating
    /// shortcut.
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
        // Fail loud on any non-trivial filter. A `None` or all-fields-None
        // filter passes (matches everything); any populated field rejects.
        // An RFC 8620 §5.5 operator tree is answered with `unsupportedFilter`
        // ("A server MUST reject the call with an unsupportedFilter error if
        // it cannot process the given filter"), since this backend evaluates
        // no FilterOperator.
        //
        // Type-identity roundtrip (Pattern G, see jmap-mail-server memory): the
        // generic `O::Filter` is the concrete `TaskFilter`,
        // `TaskNotificationFilter` or (TaskList) `Value` here, so
        // `to_value(&f)` is infallible. A future custom-serde change that
        // breaks the roundtrip surfaces as a panic rather than silently
        // dropping the filter.
        let mut in_task_list: Option<Id> = None;
        if let Some(f) = filter {
            let v = serde_json::to_value(f).expect("derive(Serialize) on plain data is infallible");
            if v.get("operator").is_some() {
                return Err(BackendQueryError::Method(JmapError::unsupported_filter()));
            }
            let any_field_set = match v.as_object() {
                Some(m) => {
                    if O::TYPE_NAME == "Task" {
                        in_task_list = Inner::task_list_id_of(&v);
                    }
                    m.iter().any(|(k, val)| {
                        !val.is_null() && !(in_task_list.is_some() && k == "taskListId")
                    })
                }
                // Filter serialized as something other than an object (only
                // possible for TaskList's untyped `Value`): treat as
                // non-trivial.
                None => true,
            };
            if any_field_set {
                return Err(BackendQueryError::Other(MemoryError::new(format!(
                    "MemoryBackend does not support filter on {}/query — reference \
                     implementation; a production backend MUST honor RFC 8620 §5.5 \
                     filter / supported_filter for {}",
                    O::TYPE_NAME,
                    O::TYPE_NAME
                ))));
            }
        }

        // Fail loud on any non-empty sort. RFC 8620 §5.5 default order is
        // implementation-defined; this reference impl returns ids in
        // id-lexicographic order, which a client must not assume.
        if sort.is_some_and(|s| !s.is_empty()) {
            return Err(BackendQueryError::Other(MemoryError::new(format!(
                "MemoryBackend does not support sort on {}/query — reference \
                 implementation returns id-lexicographic order; a production \
                 backend MUST honor RFC 8620 §5.5 sort / supported_sort for {}",
                O::TYPE_NAME,
                O::TYPE_NAME
            ))));
        }

        let inner = self.inner.lock().unwrap();

        let mut ids: Vec<Id> = inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .map(|m| {
                let mut keys: Vec<Id> = m
                    .iter()
                    .filter(|(_, v)| {
                        in_task_list.is_none() || Inner::task_list_id_of(v) == in_task_list
                    })
                    .map(|(k, _)| k.clone())
                    .collect();
                keys.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
                keys
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
    /// `/query` and are applied by re-running [`Self::query_objects`]
    /// (a filter it refuses fails here the same way). `removed` holds the
    /// destroyed ids and the updated ids that no longer match (a Task moved
    /// out of the queried `taskListId`); `added` holds the created and
    /// updated ids that match now, with their index in the current result.
    /// Mirrors the canonical `jmap-mail-server` `MemoryBackend::query_changes`.
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
            .get_changes::<O>(caller, account_id, since_query_state, max_changes)
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

// ---------------------------------------------------------------------------
// TasksBackend impl
// ---------------------------------------------------------------------------

impl TasksBackend for MemoryBackend {
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
        let val = serde_json::to_value(&stored_obj)
            .map_err(|e| BackendSetError::Other(MemoryError::new(format!("serialize: {e}"))))?;

        let mut inner = self.inner.lock().unwrap();

        inner.known_accounts.insert(account_id.as_ref().to_owned());

        let task_list_id = if O::TYPE_NAME == "Task" {
            Inner::task_list_id_of(&val)
        } else {
            None
        };

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

        if O::TYPE_NAME == "Task" {
            inner.inc_task_ref(account_id.as_ref(), task_list_id);
        }

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

        let Some(mut current) = existing else {
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::NotFound,
            ))));
        };

        let patch_val = serde_json::to_value(&patch).map_err(|e| {
            BackendSetError::Other(MemoryError::new(format!("serialize patch: {e}")))
        })?;

        let was_draft = current.get("isDraft").and_then(|v| v.as_bool()) == Some(true);

        // Snapshot the pre-patch taskListId so the refcount update below
        // can compute the (old → new) transition without re-fetching after
        // the patch applies.
        let old_task_list_id = if O::TYPE_NAME == "Task" {
            Inner::task_list_id_of(&current)
        } else {
            None
        };

        // JMAP patch (RFC 8620 §5.3): keys are JSON Pointer paths (e.g.
        // "participants/p1/participationStatus"), applied all-or-nothing by
        // the shared `apply_patch_object`, so an invalidPatch leaves the
        // stored object unchanged. Values replace wholesale; the helper caps
        // path plus value nesting depth.
        if let (serde_json::Value::Object(base), serde_json::Value::Object(patch_map)) =
            (&mut current, &patch_val)
        {
            apply_patch_object(base, patch_map).map_err(|e| {
                BackendSetError::SetError(Box::new(
                    SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()),
                ))
            })?;
        }

        // Re-validate the patched copy against the typed object before
        // storing: a null or wrong-typed value on a required property would
        // otherwise be persisted and make every later /get fail to
        // deserialize. Name the property when the patch touches only one.
        if let Err(e) = O::deserialize(&current) {
            let mut heads: Vec<String> = patch_val
                .as_object()
                .into_iter()
                .flat_map(|m| m.keys())
                .map(|k| {
                    let head = k.split('/').next().unwrap_or(k);
                    head.replace("~1", "/").replace("~0", "~")
                })
                .collect();
            heads.sort_unstable();
            heads.dedup();
            let mut err =
                SetError::new(SetErrorType::InvalidProperties).with_description(e.to_string());
            if heads.len() == 1 {
                err = err.with_properties(heads);
            }
            return Err(BackendSetError::SetError(Box::new(err)));
        }

        // Backend-side isDraft re-check (draft-ietf-jmap-tasks-06 §4 isDraft
        // paragraph): "This may only be set to true upon creation." Compare
        // the effective post-patch value against the stored one so that any
        // path to true (including a null patch followed by true) is rejected.
        // The handler does not pre-check this (bd:JMAP-h47t.29): workspace
        // AGENTS.md "Permission enforcement: backend canonical" makes the
        // backend the only enforcement point, atomically with the mutation.
        if O::TYPE_NAME == "Task"
            && !was_draft
            && current.get("isDraft").and_then(|v| v.as_bool()) == Some(true)
        {
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(vec!["isDraft".to_owned()]),
            )));
        }

        let new_task_list_id = if O::TYPE_NAME == "Task" {
            Inner::task_list_id_of(&current)
        } else {
            None
        };

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

        if O::TYPE_NAME == "Task" && old_task_list_id != new_task_list_id {
            inner.dec_task_ref(account_id.as_ref(), old_task_list_id);
            inner.inc_task_ref(account_id.as_ref(), new_task_list_id);
        }

        Ok(None)
    }

    async fn update_task_per_user(
        &self,
        caller: &(),
        account_id: &Id,
        id: &Id,
        patch: jmap_types::PatchObject,
    ) -> Result<Option<jmap_tasks_types::Task>, BackendSetError<Self::Error>> {
        // ponytail: single-user backend (CallerCtx = ()), so per-user state is
        // the shared object. `update_object` never writes `updated`, which
        // keeps draft-ietf-jmap-tasks-06 §4.5.1 intact. A multi-user backend
        // stores the patch per user instead.
        self.update_object::<jmap_tasks_types::Task>(caller, account_id, id, patch)
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
            Some(val) => {
                let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
                inner
                    .change_log_mut(O::TYPE_NAME, account_id.as_ref())
                    .push(ChangeEntry {
                        new_state,
                        created: vec![],
                        updated: vec![],
                        destroyed: vec![id.clone()],
                    });
                if O::TYPE_NAME == "Task" {
                    inner.dec_task_ref(account_id.as_ref(), Inner::task_list_id_of(&val));
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
        matches!(O::TYPE_NAME, "TaskList" | "Task" | "TaskNotification")
    }

    async fn task_list_has_tasks(
        &self,
        _caller: &(),
        account_id: &Id,
        task_list_id: &Id,
    ) -> Result<bool, MemoryError> {
        let inner = self.inner.lock().unwrap();
        Ok(inner
            .aux_ref(account_id.as_ref())
            .is_some_and(|a| a.task_list_refcount.contains_key(task_list_id)))
    }
}
