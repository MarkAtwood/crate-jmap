//! In-memory reference implementation of [`TasksBackend`](crate::TasksBackend).
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`TasksBackend`](crate::TasksBackend) trait to
//!    study when writing a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Tasks dispatcher
//!    with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and a number
//! of draft-ietf-jmap-tasks-06 edge cases are simplified (see source
//! comments). In particular, UTC-time conversion
//! ([`compute_task_utc_times`](crate::TasksBackend::compute_task_utc_times))
//! and the per-user property routing
//! ([`update_task_per_user`](crate::TasksBackend::update_task_per_user))
//! inherit the trait's default implementations (no time-zone conversion;
//! per-user patches are stored on the shared object).
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
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
    TasksBackend,
};
// json_merge_patch lives in jmap-server (the shared foundation crate)
// since bd:JMAP-sc1b.103. Every reference backend imports it; the
// canonical RFC 7396 tests live with the function there (including the
// bd:JMAP-sc1b.97 depth-cap and bd:JMAP-sc1b.87 absent-field regression
// tests).
use jmap_server::{json_merge_patch, MergePatchError};
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// A simple string error for `MemoryBackend` failures.
///
/// The message field is private to leave room for adding structured `kind` /
/// `source` fields without breaking callers. Construct via [`Self::new`];
/// read the message via [`Self::message`] or the [`Display`] impl.
///
/// [`Display`]: std::fmt::Display
#[derive(Debug)]
pub struct MemoryError {
    msg: String,
}

impl MemoryError {
    /// Construct a new `MemoryError` from any string-convertible value.
    pub fn new(msg: impl Into<String>) -> Self {
        Self { msg: msg.into() }
    }

    /// Borrow the underlying error message.
    pub fn message(&self) -> &str {
        &self.msg
    }
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
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
    /// `(type_name, account_id)` → `id → serialized object`
    objects: HashMap<(&'static str, String), HashMap<Id, serde_json::Value>>,
    /// `(type_name, account_id)` → current state counter
    states: HashMap<(&'static str, String), u64>,
    /// `(type_name, account_id)` → ordered change entries
    change_log: HashMap<(&'static str, String), Vec<ChangeEntry>>,
    /// `account_id` → auxiliary per-account state (derived indexes)
    aux: HashMap<String, AccountAux>,
    /// Explicitly registered account ids (accounts may exist with no objects yet).
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
    /// This reference implementation does **not** support any filter or sort
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
    /// - `filter` is `Some` carrying any non-null field → returns
    ///   [`MemoryError`] with a `filter not supported` message.
    /// - `sort` is `Some` with one or more comparators → returns
    ///   [`MemoryError`] with a `sort not supported` message.
    ///
    /// The handler maps both to `serverFail` on the wire (a workspace-canonical
    /// approximation; a properly typed `unsupportedFilter` / `unsupportedSort`
    /// path through the backend would require a richer error variant on
    /// the `JmapBackend` trait).
    ///
    /// Callers (handlers, internal `TaskList/set` destroy cascade if it
    /// grew one in future) that want "all tasks in this account" continue
    /// to work — they pass `filter = None`, `sort = None`.
    ///
    /// Mirrors the reference-impl correctness teaching applied to
    /// [`TasksBackend::enforce_is_draft_atomically`] (bd:JMAP-h47t.4): the
    /// demo backend models the spec invariant rather than the spec-violating
    /// shortcut.
    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        // Fail loud on any non-trivial filter. A `None` or all-fields-None
        // filter passes (matches everything); any populated field rejects.
        //
        // Type-identity roundtrip (Pattern G, see jmap-mail-server memory): the
        // generic `O::Filter` is the concrete `Task/TaskList/TaskNotification
        // FilterCondition` here, so `to_value(&f)` is infallible. A future
        // custom-serde change that breaks the roundtrip surfaces as a panic
        // rather than silently dropping the filter.
        if let Some(f) = filter {
            let v = serde_json::to_value(f).expect("derive(Serialize) on plain data is infallible");
            let any_field_set = match v.as_object() {
                Some(m) => m.values().any(|val| !val.is_null()),
                // Filter serialized as something other than an object (e.g. a
                // FilterOperator variant from a future spec revision): treat
                // as non-trivial.
                None => true,
            };
            if any_field_set {
                return Err(MemoryError::new(format!(
                    "MemoryBackend does not support filter on {}/query — reference \
                     implementation; a production backend MUST honor RFC 8620 §5.5 \
                     filter / supported_filter for {}",
                    O::TYPE_NAME,
                    O::TYPE_NAME
                )));
            }
        }

        // Fail loud on any non-empty sort. RFC 8620 §5.5 default order is
        // implementation-defined; this reference impl returns ids in
        // id-lexicographic order, which a client must not assume.
        if sort.is_some_and(|s| !s.is_empty()) {
            return Err(MemoryError::new(format!(
                "MemoryBackend does not support sort on {}/query — reference \
                 implementation returns id-lexicographic order; a production \
                 backend MUST honor RFC 8620 §5.5 sort / supported_sort for {}",
                O::TYPE_NAME,
                O::TYPE_NAME
            )));
        }

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
                .to_string()
                .as_str(),
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

    /// Returns the changes to a `/query` result set since `since_query_state`.
    ///
    /// # Filter and sort are NOT honored
    ///
    /// RFC 8620 §5.6 requires `/queryChanges` to report `added` /
    /// `removed` entries that reflect filter and sort over **mutable**
    /// properties — i.e. ids that were in the previous query result and
    /// are now excluded by an updated filter-affecting property must
    /// appear in `removed`. This reference implementation cannot
    /// implement that faithfully without re-deriving each type's filter
    /// algebra, so it ignores `filter` and `sort` entirely and returns
    /// `added` / `removed` purely from the change-log (`created` →
    /// `added`, `destroyed` → `removed`, with `updated` ignored).
    ///
    /// The trade-off is documented (rather than fixed) because:
    ///
    /// - `query_objects` here is itself filter-and-sort-blind (and now
    ///   fails loud on non-trivial filter/sort), so a `/queryChanges`
    ///   that did honor filter/sort would be lying about consistency
    ///   with its `/query` sibling.
    /// - Re-deriving the type's filter algebra in the reference impl
    ///   would couple `MemoryBackend` to every QueryObject's wire-format
    ///   property names — a layering hazard for a reference impl whose
    ///   purpose is to demonstrate the trait shape, not to be feature-
    ///   complete.
    ///
    /// A production backend MUST track filter-affecting property
    /// transitions in its change log and emit `removed` for them per
    /// §5.6. See the canonical sibling `jmap-mail-server`'s
    /// `MemoryBackend::query_changes` for the shape that an
    /// implementation with mature filter/sort would take.
    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        caller: &(),
        account_id: &Id,
        since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        let changes = self
            .get_changes::<O>(caller, account_id, since_query_state, max_changes)
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
// TasksBackend impl
// ---------------------------------------------------------------------------

impl TasksBackend for MemoryBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        inner.known_accounts.insert(account_id.as_ref().to_owned());

        let server_id = Self::demo_next_id(&mut inner, O::TYPE_NAME, account_id.as_ref());

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

        // Backend-side isDraft re-check (draft-ietf-jmap-tasks-06 §4 isDraft
        // paragraph): once isDraft transitions to false, the value MUST NOT
        // be updated back to true. The handler at task.rs pre-fetches and
        // rejects this same transition when `enforce_is_draft_atomically()`
        // returns false, but workspace AGENTS.md "Permission enforcement:
        // backend canonical" requires the backend to also re-verify
        // atomically with the mutation. A reference impl that relied on
        // the handler-only check would teach consumers a foot-gun.
        if O::TYPE_NAME == "Task"
            && patch_val.get("isDraft").and_then(|v| v.as_bool()) == Some(true)
            && current.get("isDraft").and_then(|v| v.as_bool()) == Some(false)
        {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(vec!["isDraft".to_owned()]),
            ));
        }

        // Snapshot the pre-patch taskListId so the refcount update below
        // can compute the (old → new) transition without re-fetching after
        // the patch applies.
        let old_task_list_id = if O::TYPE_NAME == "Task" {
            Inner::task_list_id_of(&current)
        } else {
            None
        };

        if let Err(MergePatchError::DepthExceeded) = json_merge_patch(&mut current, patch_val) {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidPatch)
                    .with_description("patch nesting exceeds server limit"),
            ));
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
            None => Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            ))),
        }
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        matches!(O::TYPE_NAME, "TaskList" | "Task" | "TaskNotification")
    }

    async fn task_list_has_tasks(&self, _caller: &(), account_id: &Id, task_list_id: &Id) -> bool {
        let inner = self.inner.lock().unwrap();
        inner
            .aux_ref(account_id.as_ref())
            .is_some_and(|a| a.task_list_refcount.contains_key(task_list_id))
    }

    /// MemoryBackend self-enforces the isDraft invariant atomically in
    /// `update_object`, so the handler's pre-fetch fast-path is unnecessary.
    fn enforce_is_draft_atomically(&self) -> bool {
        true
    }
}
