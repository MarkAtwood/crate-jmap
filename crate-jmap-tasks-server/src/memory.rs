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
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// A simple string error for `MemoryBackend` failures.
#[derive(Debug, Clone)]
pub struct MemoryError(pub String);

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
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
    /// Set of TaskList ids that have at least one Task attached
    /// (drives `TaskList/set` destroy rejection with `taskListHasTasks`
    /// per draft-ietf-jmap-tasks-06 §3.4 when `onDestroyRemoveTasks` is
    /// `false`). Maintained as a derived index over the Task store;
    /// kept in sync by `create_object` / `update_object` / `destroy_object`
    /// for the Task type.
    task_lists_with_tasks: HashSet<Id>,
}

/// Shared inner state, behind Arc<Mutex>.
#[derive(Default)]
struct Inner {
    /// `(type_name, account_id)` → `id → serialized object`
    objects: HashMap<(String, String), HashMap<Id, serde_json::Value>>,
    /// `(type_name, account_id)` → current state counter
    states: HashMap<(String, String), u64>,
    /// `(type_name, account_id)` → ordered change entries
    change_log: HashMap<(String, String), Vec<ChangeEntry>>,
    /// `account_id` → auxiliary per-account state (derived indexes)
    aux: HashMap<String, AccountAux>,
    /// Explicitly registered account ids (accounts may exist with no objects yet).
    known_accounts: HashSet<String>,
}

impl Inner {
    fn current_state(&self, type_name: &str, account_id: &str) -> u64 {
        *self
            .states
            .get(&(type_name.to_owned(), account_id.to_owned()))
            .unwrap_or(&0)
    }

    fn bump_state(&mut self, type_name: &str, account_id: &str) -> u64 {
        let entry = self
            .states
            .entry((type_name.to_owned(), account_id.to_owned()))
            .or_insert(0);
        *entry += 1;
        *entry
    }

    fn objects_mut(
        &mut self,
        type_name: &str,
        account_id: &str,
    ) -> &mut HashMap<Id, serde_json::Value> {
        self.known_accounts.insert(account_id.to_owned());
        self.objects
            .entry((type_name.to_owned(), account_id.to_owned()))
            .or_default()
    }

    fn objects_ref(
        &self,
        type_name: &str,
        account_id: &str,
    ) -> Option<&HashMap<Id, serde_json::Value>> {
        self.objects
            .get(&(type_name.to_owned(), account_id.to_owned()))
    }

    fn change_log_mut(&mut self, type_name: &str, account_id: &str) -> &mut Vec<ChangeEntry> {
        self.change_log
            .entry((type_name.to_owned(), account_id.to_owned()))
            .or_default()
    }

    fn aux_mut(&mut self, account_id: &str) -> &mut AccountAux {
        self.known_accounts.insert(account_id.to_owned());
        self.aux.entry(account_id.to_owned()).or_default()
    }

    fn aux_ref(&self, account_id: &str) -> Option<&AccountAux> {
        self.aux.get(account_id)
    }

    /// Recompute `task_lists_with_tasks` for the given account by scanning
    /// the Task store. Each Task lives in exactly one TaskList via the
    /// `taskListId` field (draft-tasks-06 §4.1).
    fn recompute_task_lists_with_tasks(&mut self, account_id: &str) {
        let tasks = self
            .objects
            .get(&("Task".to_owned(), account_id.to_owned()))
            .cloned()
            .unwrap_or_default();

        let mut set: HashSet<Id> = HashSet::new();
        for value in tasks.values() {
            if let Some(s) = value.get("taskListId").and_then(|v| v.as_str()) {
                set.insert(Id::from(s));
            }
        }
        self.aux_mut(account_id).task_lists_with_tasks = set;
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
/// Cloning is cheap: every clone shares the same underlying `Arc<Mutex<…>>`.
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
    /// Returns `self` for builder-style chaining.
    #[must_use]
    pub fn with_account(self, account_id: &str) -> Self {
        self.register_account(&Id::from(account_id));
        self
    }

    /// Register an account as known even if it has no objects yet.
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
        type_name: &str,
        id: &str,
        value: serde_json::Value,
    ) {
        let mut inner = self.inner.lock().unwrap();
        inner.known_accounts.insert(account_id.to_owned());
        inner
            .objects_mut(type_name, account_id)
            .insert(Id::from(id), value);
        if type_name == "Task" {
            inner.recompute_task_lists_with_tasks(account_id);
        }
    }

    /// Allocate a server-assigned id for a new object of the given type.
    ///
    /// Pattern: `"<lowercased-type-name><n>"` where `n` is `count + 1`
    /// within the `(type_name, account_id)` namespace.
    fn next_id(inner: &mut Inner, type_name: &str, account_id: &str) -> Id {
        let n = inner
            .objects_ref(type_name, account_id)
            .map_or(0, |m| m.len());
        Id::from(format!("{}{}", type_name.to_ascii_lowercase(), n + 1))
    }
}

// ---------------------------------------------------------------------------
// JmapBackend impl (read-side supertrait)
// ---------------------------------------------------------------------------

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;

    async fn account_exists(&self, account_id: &Id) -> Result<bool, Self::Error> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.known_accounts.contains(account_id.as_ref()))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
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
                            Err(e) => return Err(MemoryError(e.to_string())),
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
                            Err(e) => return Err(MemoryError(e.to_string())),
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
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        let inner = self.inner.lock().unwrap();
        let n = inner.current_state(O::TYPE_NAME, account_id.as_ref());
        Ok(State::from(n.to_string()))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        let inner = self.inner.lock().unwrap();

        let since_n: u64 = since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::TooManyChanges { limit: 0 })?;

        let log = inner
            .change_log
            .get(&(O::TYPE_NAME.to_owned(), account_id.as_ref().to_owned()))
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

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        account_id: &Id,
        filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        let inner = self.inner.lock().unwrap();

        // For Task, support the `inTaskList` filter (used by `TaskList/set`
        // cleanup when `onDestroyRemoveTasks: true`). Other filters fall
        // through to "match all".
        let in_task_list: Option<String> = if O::TYPE_NAME == "Task" {
            filter
                .and_then(|f| serde_json::to_value(f).ok())
                .and_then(|v| {
                    v.get("inTaskList")
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
                    .filter(|(_, v)| match &in_task_list {
                        None => true,
                        Some(target) => v
                            .get("taskListId")
                            .and_then(|s| s.as_str())
                            .map(|s| s == target.as_str())
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
            start as i64,
            Some(total),
            query_state,
            true,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        account_id: &Id,
        since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        let changes = self
            .get_changes::<O>(account_id, since_query_state, max_changes)
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
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        inner.known_accounts.insert(account_id.as_ref().to_owned());

        let server_id = Self::next_id(&mut inner, O::TYPE_NAME, account_id.as_ref());

        let mut val = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;
        if let Some(map) = val.as_object_mut() {
            map.insert(
                "id".to_owned(),
                serde_json::Value::String(server_id.as_ref().to_owned()),
            );
        }
        let stored_obj: O =
            O::deserialize(&val).map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

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
            inner.recompute_task_lists_with_tasks(account_id.as_ref());
        }

        Ok((server_id, stored_obj))
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        let existing = inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .get(id)
            .cloned();

        let mut current = match existing {
            Some(v) => v,
            None => {
                return Err(BackendSetError::SetError(SetError::new(
                    SetErrorType::NotFound,
                )))
            }
        };

        let patch_val = serde_json::to_value(&patch)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;
        json_merge_patch(&mut current, patch_val);

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

        if O::TYPE_NAME == "Task" {
            inner.recompute_task_lists_with_tasks(account_id.as_ref());
        }

        Ok(None)
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

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
                if O::TYPE_NAME == "Task" {
                    inner.recompute_task_lists_with_tasks(account_id.as_ref());
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

    async fn copy_task(
        &self,
        from_account_id: &Id,
        to_account_id: &Id,
        task: jmap_tasks_types::Task,
    ) -> Result<(Id, jmap_tasks_types::Task), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();

        // Source account must exist.
        if !inner.known_accounts.contains(from_account_id.as_ref()) {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )));
        }

        inner
            .known_accounts
            .insert(to_account_id.as_ref().to_owned());

        let new_id = Self::next_id(&mut inner, "Task", to_account_id.as_ref());

        let mut val = serde_json::to_value(&task)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;
        if let Some(map) = val.as_object_mut() {
            map.insert(
                "id".to_owned(),
                serde_json::Value::String(new_id.as_ref().to_owned()),
            );
        }
        let stored: jmap_tasks_types::Task = serde_json::from_value(val.clone())
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

        let new_state = inner.bump_state("Task", to_account_id.as_ref());
        inner
            .objects_mut("Task", to_account_id.as_ref())
            .insert(new_id.clone(), val);
        inner
            .change_log_mut("Task", to_account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![new_id.clone()],
                updated: vec![],
                destroyed: vec![],
            });
        inner.recompute_task_lists_with_tasks(to_account_id.as_ref());

        Ok((new_id, stored))
    }

    async fn task_list_has_tasks(&self, account_id: &Id, task_list_id: &Id) -> bool {
        let inner = self.inner.lock().unwrap();
        inner
            .aux_ref(account_id.as_ref())
            .map(|a| a.task_lists_with_tasks.contains(task_list_id))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// JSON Merge Patch (RFC 7396)
// ---------------------------------------------------------------------------

/// Apply a JSON Merge Patch to `target` in-place.
fn json_merge_patch(target: &mut serde_json::Value, patch: serde_json::Value) {
    json_merge_patch_inner(target, patch, 0);
}

/// Maximum recursion depth for JSON Merge Patch application.
///
/// Beyond this depth the patch is silently ignored at the affected sub-tree:
/// the target value at that level is left unchanged. Mitigates stack DoS
/// from adversarial `PatchObject` values (bd:JMAP-sc1b.97). 32 levels
/// comfortably exceeds any legitimate JMAP `/set update` shape.
const MAX_MERGE_PATCH_DEPTH: usize = 32;

fn json_merge_patch_inner(target: &mut serde_json::Value, patch: serde_json::Value, depth: usize) {
    if depth > MAX_MERGE_PATCH_DEPTH {
        return;
    }
    match patch {
        serde_json::Value::Object(patch_map) => {
            let target_map = target
                .as_object_mut()
                .expect("merge patch target must be an object");
            for (key, patch_val) in patch_map {
                if patch_val.is_null() {
                    target_map.remove(&key);
                } else {
                    let entry = target_map.entry(key).or_insert(serde_json::Value::Null);
                    json_merge_patch_inner(entry, patch_val, depth + 1);
                }
            }
        }
        other => *target = other,
    }
}
