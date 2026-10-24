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
//! JMAP-hwdv.7 (this crate, mirror of canonical JMAP-hwdv.1 in
//! jmap-mail-server, following the multi-type-store shape established
//! by jmap-chat-server's `MemoryBackend` and the derived-index pattern
//! pioneered in JMAP-hwdv.5 jmap-calendars-server).

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::TasksBackend;
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
use jmap_tasks_types::{Task, TaskFilterCondition, TaskList};
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
// Change log
// ---------------------------------------------------------------------------

/// Per-ID fate tracker for RFC 8620 §5.2 ID deduplication across change log
/// entries in one `/changes` window.
///
/// - created+updated → `Created`
/// - created+destroyed → omitted (the client never knew the object)
/// - updated+destroyed → `Destroyed`
/// - updated+updated → `Updated`
///
/// Mirrors the canonical jmap-mail-server `IdFate` enum.
#[derive(Debug, Clone, Copy)]
enum IdFate {
    Created,
    Updated,
    Destroyed,
}

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
    /// (-04 §3.2, §4.1), `Task/query` evaluates the -04 §3.6 conditions,
    /// and `Foo/changes` honours `ignoreMetadataOnlyChanges` and reports
    /// `updatedProperties` (-04 §3.3). Advertising the capability in the
    /// session is the server's job: list `info` under `data_type` in the
    /// account's `urn:ietf:params:jmap:metadata` `dataTypes`.
    ///
    /// TaskList writes need `myRights.maySetMetadata` on the TaskList, and
    /// Task writes need it on the Task's TaskList before and after the
    /// update, or on the `taskListId` of a create (-04 §4.1). An absent
    /// `maySetMetadata` allows the write (fail-open, JMAP-bghk). This
    /// backend never sets Task `updated`, so a metadata-only change leaves
    /// it alone, as -04 §3.1 requires.
    ///
    /// # Panics
    ///
    /// Panics if `data_type` is not `"TaskList"` or `"Task"` (the rights
    /// object of draft-ietf-jmap-tasks-06 §3 and the type it contains;
    /// -04 §4.1), or if `info.supports_private` is true: this backend has
    /// no per-user identity (`CallerCtx` is `()`) to own `privateMetadata`
    /// (-04 §2).
    #[must_use]
    pub fn with_metadata_info(self, data_type: &str, info: DataTypeMetadataInfo) -> Self {
        let type_name = metadata_type_name(data_type).unwrap_or_else(|| {
            panic!("MemoryBackend supports metadata only on TaskList and Task, not {data_type:?}")
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
    #[doc(hidden)]
    pub fn register_account(&self, account_id: &Id) {
        debug_assert!(
            !account_id.as_ref().is_empty(),
            "register_account: account_id must not be the empty string \
             (RFC 8620 §1.2, bd:JMAP-dx0s)"
        );
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.as_ref().to_owned());
        inner.aux.entry(account_id.as_ref().to_owned()).or_default();
    }

    /// Seed a pre-existing object into the store without bumping the
    /// state counter or recording a change-log entry. Test-fixture setup
    /// only; registers `account_id` as known. Mirrors
    /// `jmap_mail_server::memory::MemoryBackend::seed_object`.
    ///
    /// `type_name` is the `TYPE_NAME` of a type this backend stores:
    /// `"TaskList"`, `"Task"`, or `"TaskNotification"`. A seeded Task's
    /// `taskListId` is counted in the TaskList reference index.
    ///
    /// # Panics
    ///
    /// If `type_name` is not one of the names above, or `value` is not a
    /// JSON object whose `id` string equals `id`.
    pub fn seed_object(
        &self,
        account_id: &Id,
        type_name: &'static str,
        id: &Id,
        value: serde_json::Value,
    ) {
        const KNOWN_TYPES: &[&str] = &[
            jmap_tasks_types::TaskList::TYPE_NAME,
            jmap_tasks_types::Task::TYPE_NAME,
            jmap_tasks_types::TaskNotification::TYPE_NAME,
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
        let list_id = if type_name == "Task" {
            Inner::task_list_id_of(&value)
        } else {
            None
        };
        let prev = inner
            .objects_mut(type_name, account_id)
            .insert(id.clone(), value);
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

        // The deserialize error closure is shared by both branches.
        let deser_err = |e: serde_json::Error| {
            MemoryError::serde(format_args!("deserialize {}", O::TYPE_NAME), &e)
        };

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

        // RFC 8620 §5.2 ID deduplication across the window, using the
        // canonical jmap-mail-server `IdFate` rules. `order` keeps first-seen
        // order so the response lists are deterministic.
        let mut fates: HashMap<Id, IdFate> = HashMap::new();
        let mut order: Vec<Id> = Vec::new();
        let mut window_props = ChangedProperties::new();
        for entry in &relevant {
            for id in &entry.created {
                if fates.insert(id.clone(), IdFate::Created).is_none() {
                    order.push(id.clone());
                }
            }
            for id in &entry.updated {
                window_props.record(id.as_ref(), entry.updated_props.as_ref());
                let fate = match fates.get(id) {
                    Some(IdFate::Created) => IdFate::Created,
                    Some(IdFate::Destroyed) => IdFate::Destroyed,
                    _ => IdFate::Updated,
                };
                if fates.insert(id.clone(), fate).is_none() {
                    order.push(id.clone());
                }
            }
            for id in &entry.destroyed {
                // "If a record has been created AND destroyed since the old
                // state, the server SHOULD remove the id from the response
                // entirely." (RFC 8620 §5.2)
                if matches!(fates.get(id), Some(IdFate::Created)) {
                    fates.remove(id);
                } else if fates.insert(id.clone(), IdFate::Destroyed).is_none() {
                    order.push(id.clone());
                }
            }
        }

        let mut created: Vec<Id> = Vec::new();
        let mut updated: Vec<Id> = Vec::new();
        let mut destroyed: Vec<Id> = Vec::new();
        // `fates.remove` emits each id once and skips ids omitted above.
        for id in order {
            match fates.remove(&id) {
                Some(IdFate::Created) => created.push(id),
                // -04 §3.3: under Ignore, "exclude from the updated array
                // any id whose only changes since sinceState ... are
                // confined to metadata and/or privateMetadata".
                Some(IdFate::Updated)
                    if mode == MetadataOnlyChanges::Ignore
                        && window_props.is_metadata_only(id.as_ref()) => {}
                Some(IdFate::Updated) => updated.push(id),
                Some(IdFate::Destroyed) => destroyed.push(id),
                None => {}
            }
        }

        // -04 §3.3 `updatedProperties`; no tasks type has the RFC 8621
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

    /// Returns all object ids in this account, sorted lexicographically by id,
    /// paginated by `position` and `limit`.
    ///
    /// # Filter and sort are NOT honored
    ///
    /// This reference implementation supports exactly one filter shape: a
    /// single Task condition whose only set fields are `taskListId`
    /// (draft-tasks-06 §4.6), which the `TaskList/set` `onDestroyRemoveTasks`
    /// cascade (draft-tasks-06 §3.4) needs, and the draft-ietf-jmap-metadata-04
    /// §3.6 shared-metadata conditions once [`MemoryBackend::with_metadata_info`]
    /// opts Task in. It does **not** support any other filter or any sort
    /// shape. RFC 8620 §5.5 mandates that an unsupported `filter` produces
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
    /// - `filter` is `Some` carrying any non-null field other than those
    ///   Task fields, or any operator tree it cannot evaluate → returns
    ///   [`BackendQueryError::Method`] carrying `unsupportedFilter`. A
    ///   metadata condition on a type that is not opted in is also
    ///   `unsupportedFilter`, and a malformed one `invalidArguments`
    ///   (draft-ietf-jmap-metadata-04 §3.6).
    /// - `sort` is `Some` with one or more comparators → returns
    ///   [`BackendQueryError::Method`] carrying `unsupportedSort`.
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
        // filter passes (matches everything); any populated field this
        // backend cannot evaluate, and any operator tree, is answered with
        // `unsupportedFilter` (RFC 8620 §5.5: "A server MUST reject the call
        // with an unsupportedFilter error if it cannot process the given
        // filter").
        //
        // The generic `O::Filter` is the concrete `TaskFilter`,
        // `TaskNotificationFilter` or (TaskList) `Value` here. A serializer
        // failure surfaces as serverFail rather than a panic or a silently
        // dropped filter (see the jmap-mail-server memory backend).
        //
        // draft-ietf-jmap-metadata-04 §3.6: on an opted-in Task the shared
        // `metadata*` conditions are evaluable in the same leaf;
        // [`check_metadata_filter`] validates them first, also inside an
        // operator tree, so a malformed path is `invalidArguments`.
        let inner = self.inner.lock().unwrap();
        let is_task = O::TYPE_NAME == Task::TYPE_NAME;
        let mut in_task_list: Option<Id> = None;
        let mut task_condition: Option<TaskFilterCondition> = None;
        if let Some(f) = filter {
            let v = jmap_server::serialize_value(f).map_err(BackendQueryError::Method)?;
            check_metadata_filter(&v, inner.metadata_info.get(O::TYPE_NAME))?;
            if v.get("operator").is_some() {
                return Err(BackendQueryError::Method(JmapError::unsupported_filter()));
            }
            let any_field_set = match v.as_object() {
                Some(m) => {
                    if is_task {
                        in_task_list = Inner::task_list_id_of(&v);
                    }
                    m.iter().any(|(k, val)| {
                        !val.is_null()
                            && !(in_task_list.is_some() && k == "taskListId")
                            && !(is_task && SHARED_METADATA_CONDITION_KEYS.contains(&k.as_str()))
                    })
                }
                // Filter serialized as something other than an object (only
                // possible for TaskList's untyped `Value`): treat as
                // non-trivial.
                None => true,
            };
            if any_field_set {
                return Err(BackendQueryError::Method(JmapError::unsupported_filter()));
            }
            if is_task {
                task_condition = Some(serde_json::from_value(v).map_err(|e| {
                    BackendQueryError::Method(JmapError::server_fail(e.to_string()))
                })?);
            }
        }

        // Fail loud on any non-empty sort: this reference impl returns ids
        // in id-lexicographic order and honors no comparator. RFC 8620 §5.5:
        // "A server MUST reject the call with an unsupportedSort error if it
        // cannot sort by the given properties."
        if sort.is_some_and(|s| !s.is_empty()) {
            return Err(BackendQueryError::Method(JmapError::unsupported_sort()));
        }

        let mut ids: Vec<Id> = inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .map(|m| {
                let mut keys: Vec<Id> = m
                    .iter()
                    .filter(|(_, v)| {
                        (in_task_list.is_none() || Inner::task_list_id_of(v) == in_task_list)
                            && task_condition.as_ref().is_none_or(|c| {
                                metadata_matches(
                                    v.get("metadata").and_then(serde_json::Value::as_object),
                                    c.metadata_exists.as_deref(),
                                    c.metadata_text_contains.as_ref(),
                                    c.metadata_text_equals.as_ref(),
                                )
                            })
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

/// The `TYPE_NAME` of the type named `data_type` if it is one this backend
/// can give draft-ietf-jmap-metadata-04 properties: TaskList, which has a
/// rights object (draft-ietf-jmap-tasks-06 §3 `myRights`), and Task, which
/// "takes the right from its containers" (-04 §4.1).
fn metadata_type_name(data_type: &str) -> Option<&'static str> {
    [TaskList::TYPE_NAME, Task::TYPE_NAME]
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
    /// before an update (`None` on create), and `containers` the TaskList
    /// ids whose `maySetMetadata` a Task write needs (-04 §4.1). Rule 1's
    /// `notFound` is the caller's: this backend has no read rights, so
    /// only a missing object is unreadable.
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
        // MUST be rejected with a "forbidden" SetError". A TaskList has
        // its own rights; a Task takes the right from its TaskList. An
        // absent right or container allows the write (fail-open,
        // JMAP-bghk).
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
            && if type_name == TaskList::TYPE_NAME {
                denies(stored.and_then(|l| l.get("myRights")))
            } else {
                let lists = self.objects_ref(TaskList::TYPE_NAME, account_id);
                containers.iter().any(|id| {
                    denies(
                        lists
                            .and_then(|l| l.get(*id))
                            .and_then(|list| list.get("myRights")),
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

/// The TaskList ids a Task update's metadata write needs `maySetMetadata`
/// on: the TaskList the Task is in before the update and the one `patch`
/// moves it to (-04 §4.1: "on every container both before and after the
/// change"). draft-ietf-jmap-tasks-06 §4: a Task belongs to exactly one
/// TaskList, named by the string `taskListId`.
fn task_update_containers<'a>(
    stored: &'a serde_json::Map<String, serde_json::Value>,
    patch: &'a serde_json::Map<String, serde_json::Value>,
) -> Vec<&'a str> {
    [stored.get("taskListId"), patch.get("taskListId")]
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect()
}

/// draft-ietf-jmap-metadata-04 §3.2 checks for a created object `val` of
/// type `type_name`: rules 1 to 3 on its metadata properties, then rule 4
/// on the stored value. A Task needs `maySetMetadata` on the TaskList of
/// its `taskListId` (-04 §4.1: "on create, on the containers given in the
/// create"). Types without the -04 properties pass unchecked.
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
    let containers: Vec<&str> = if type_name == Task::TYPE_NAME {
        obj.get("taskListId")
            .and_then(serde_json::Value::as_str)
            .into_iter()
            .collect()
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

/// The three -04 §3.6 conditions on shared `metadata` that this backend
/// evaluates; the `privateMetadata*` three never pass
/// [`check_metadata_filter`] because `supportsPrivate` is false.
const SHARED_METADATA_CONDITION_KEYS: [&str; 3] = [
    "metadataExists",
    "metadataTextContains",
    "metadataTextEquals",
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
            .map_err(|e| BackendSetError::Other(MemoryError::serde("serialize", &e)))?;

        let mut inner = self.inner.lock().unwrap();
        check_metadata_create(&inner, O::TYPE_NAME, account_id.as_ref(), &val)?;

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
                updated_props: None,
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

        let Some(mut current) = existing else {
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::NotFound,
            ))));
        };

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
        let mut updated_props = None;
        if let (serde_json::Value::Object(base), serde_json::Value::Object(patch_map)) =
            (&mut current, &patch_val)
        {
            // -04 §3.2 rules 1 to 3 run before the patch is applied.
            let metadata_info = if metadata_type_name(O::TYPE_NAME).is_some() {
                let containers = if O::TYPE_NAME == Task::TYPE_NAME {
                    task_update_containers(base, patch_map)
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
                updated_props,
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
                        updated_props: None,
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
mod fixture_helper_tests {
    use super::*;
    use serde_json::json;

    /// `with_account` makes the account known (RFC 8620 §3.6.2: an
    /// unknown accountId is `accountNotFound`, so `account_exists` is the
    /// observable).
    #[tokio::test]
    async fn with_account_registers_account() {
        let acc = Id::from("acc1");
        let backend = MemoryBackend::new().with_account(&acc);
        assert!(backend.account_exists(&(), &acc).await.unwrap());
        assert!(
            !backend
                .account_exists(&(), &Id::from("acc2"))
                .await
                .unwrap()
        );
    }

    /// A seeded Task counts toward its TaskList's reference index.
    #[tokio::test]
    async fn seeded_task_marks_task_list_non_empty() {
        let acc = Id::from("acc1");
        let backend = MemoryBackend::new();
        backend.seed_object(
            &acc,
            "Task",
            &Id::from("t1"),
            json!({"id": "t1", "taskListId": "tl1"}),
        );
        assert!(
            backend
                .task_list_has_tasks(&(), &acc, &Id::from("tl1"))
                .await
                .unwrap()
        );
    }

    #[test]
    #[should_panic(expected = "not one of the known")]
    fn seed_object_rejects_unknown_type_name() {
        MemoryBackend::new().seed_object(
            &Id::from("acc1"),
            "Tasklist",
            &Id::from("tl1"),
            json!({"id": "tl1"}),
        );
    }

    /// A non-object value panics at the seed call instead of surfacing
    /// later as an opaque deserialize error from get_objects.
    #[test]
    #[should_panic(expected = "must be a JSON object")]
    fn seed_object_rejects_non_object_value() {
        MemoryBackend::new().seed_object(
            &Id::from("acc1"),
            "TaskList",
            &Id::from("tl1"),
            json!(42),
        );
    }

    /// A value without an `id` field panics instead of storing a fixture
    /// whose `id` is absent on read-back.
    #[test]
    #[should_panic(expected = "must contain an `id` string field")]
    fn seed_object_rejects_value_without_id_field() {
        MemoryBackend::new().seed_object(
            &Id::from("acc1"),
            "TaskList",
            &Id::from("tl1"),
            json!({"name": "Work"}),
        );
    }

    #[test]
    #[should_panic(expected = "does not match the `id` argument")]
    fn seed_object_rejects_id_mismatch() {
        MemoryBackend::new().seed_object(
            &Id::from("acc1"),
            "TaskList",
            &Id::from("tl1"),
            json!({"id": "tl2"}),
        );
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
mod changes_tests {
    use super::*;
    use jmap_tasks_types::Task;

    /// Oracle: RFC 8620 §5.2 /changes rules, expected lists written by hand.
    /// "If a record has been created AND destroyed since the old state, the
    /// server SHOULD remove the id from the response entirely"; a record
    /// created then updated is reported only in `created`; one updated then
    /// destroyed is reported only in `destroyed`.
    #[tokio::test]
    async fn get_changes_dedups_ids_across_window() {
        let backend = MemoryBackend::new();
        let id = |s: &str| Id::from(s);
        {
            let mut inner = backend.inner.lock().unwrap();
            let log = inner.change_log_mut(Task::TYPE_NAME, "acc1");
            log.push(ChangeEntry {
                new_state: 1,
                created: vec![id("eph"), id("fresh")],
                updated: vec![],
                destroyed: vec![],
                updated_props: None,
            });
            log.push(ChangeEntry {
                new_state: 2,
                created: vec![],
                updated: vec![id("fresh"), id("old"), id("doomed")],
                destroyed: vec![id("eph")],
                updated_props: None,
            });
            log.push(ChangeEntry {
                new_state: 3,
                created: vec![],
                updated: vec![id("old")],
                destroyed: vec![id("doomed"), id("gone")],
                updated_props: None,
            });
        }

        let changes = backend
            .get_changes::<Task>(
                &(),
                &id("acc1"),
                jmap_server::ChangesParams::new(&State::from("0")),
            )
            .await
            .expect("get_changes must succeed");

        assert_eq!(changes.created, vec![id("fresh")]);
        assert_eq!(changes.updated, vec![id("old")]);
        assert_eq!(changes.destroyed, vec![id("doomed"), id("gone")]);
    }
}

#[cfg(test)]
mod max_changes_tests {
    use super::*;
    use jmap_tasks_types::Task as T;

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
