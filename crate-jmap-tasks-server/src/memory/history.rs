//! Object history for the memory backend (draft-ietf-jmap-object-history-03
//! with the -04 editor's copy changes; docs/design/object-history.md §7),
//! for TaskList and Task (TaskNotification is excluded, decision 22).
//!
//! Off until [`MemoryBackend::with_object_history`]. Versions live in a
//! `jmap_object_history_server::HistoryStore` inside the scoped state, so a
//! conditional rollback discards them with the rest.

use std::collections::HashMap;

use jmap_object_history_server::{HistoryRetention, HistoryStore};
use jmap_object_history_types::{HistoryEntry, HistoryObject, ObjectHistoryAccountCapability};
use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError,
    DestroyedQueryChangesParams, DestroyedQueryParams, HistoryGetParams, HistoryGetResult,
    JmapObject, ObjectHistoryBackend, QueryChangesResult, QueryObject, QueryResult, Restored,
    SetError, SetErrorType, SetObject, apply_restore_patch, limit_or_max, resolve_query_start,
};
use jmap_tasks_types::{Task, TaskList};
use jmap_types::{Id, JmapError, PatchObject, State, UTCDate};
use serde_json::Value;

use super::{ChangeEntry, Inner, MemoryBackend, MemoryError, metadata_matches};
use jmap_tasks_types::TaskFilterCondition;

/// The types this backend keeps history for.
const SUPPORTED: &[&str] = &["TaskList", "Task"];

impl Inner {
    /// Record the update of `id` from `previous`, when the type keeps
    /// history.
    pub(super) fn record_replace(
        &mut self,
        type_name: &str,
        account: &Id,
        id: &Id,
        previous: Value,
    ) {
        if let Some(h) = self.history.as_mut() {
            h.record_replace(type_name, account, id, previous);
        }
    }

    /// Record the destruction of `id`, whose last state was `last`, when
    /// the type keeps history; every route counts (§3), so the
    /// `onDestroyRemoveTasks` cascade too.
    pub(super) fn record_destroy(&mut self, type_name: &str, account: &Id, id: &Id, last: Value) {
        if let Some(h) = self.history.as_mut() {
            h.record_destroy(type_name, account, id, last);
        }
    }

    /// Whether minting `id` would reuse the id of a destroyed object with
    /// retained history (§5.1 forbids it).
    pub(super) fn history_retains(&self, type_name: &str, account: &Id, id: &Id) -> bool {
        self.history
            .as_ref()
            .is_some_and(|h| h.retains(type_name, account, id))
    }

    fn prune_history(&mut self) {
        if let Some(h) = self.history.as_mut() {
            h.prune();
        }
    }

    /// Destroyed objects in result order: the window, then for Task the
    /// conditions this backend's Task/query evaluates (`taskListId` and the
    /// metadata conditions), over final versions; most recently destroyed
    /// first (§4). Like Task/query here, any other condition is
    /// `unsupportedFilter` and any sort `unsupportedSort` (§4 allows a
    /// server to support fewer for destroyed objects).
    fn destroyed_results<O: QueryObject>(
        &self,
        account: &Id,
        after: Option<&UTCDate>,
        before: Option<&UTCDate>,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
    ) -> Result<Vec<Id>, BackendQueryError<MemoryError>> {
        let Some(h) = self.history.as_ref() else {
            return Ok(Vec::new());
        };
        if sort.is_some_and(|s| !s.is_empty()) {
            return Err(BackendQueryError::Method(JmapError::unsupported_sort()));
        }
        let mut list: Option<Id> = None;
        let mut condition: Option<TaskFilterCondition> = None;
        if let Some(f) = filter {
            let v = serde_json::to_value(f)
                .map_err(|e| BackendQueryError::Method(JmapError::server_fail(e.to_string())))?;
            let supported = O::TYPE_NAME == Task::TYPE_NAME
                && v.as_object().is_some_and(|m| {
                    m.iter().all(|(k, val)| {
                        val.is_null()
                            || k == "taskListId"
                            || super::SHARED_METADATA_CONDITION_KEYS.contains(&k.as_str())
                    })
                });
            if !supported {
                return Err(BackendQueryError::Method(JmapError::unsupported_filter()));
            }
            list = Inner::task_list_id_of(&v);
            condition =
                Some(serde_json::from_value(v).map_err(|e| {
                    BackendQueryError::Method(JmapError::server_fail(e.to_string()))
                })?);
        }
        let rows = h
            .destroyed(O::TYPE_NAME, account, after, before)
            .map_err(|e| BackendQueryError::Method(JmapError::invalid_arguments(e.to_string())))?;
        Ok(rows
            .into_iter()
            .filter(|d| {
                let v = &d.snapshot;
                (list.is_none() || Inner::task_list_id_of(v) == list)
                    && condition.as_ref().is_none_or(|c| {
                        metadata_matches(
                            v.get("metadata").and_then(Value::as_object),
                            c.metadata_exists.as_deref(),
                            c.metadata_text_contains.as_ref(),
                            c.metadata_text_equals.as_ref(),
                        )
                    })
            })
            .map(|d| d.id)
            .collect())
    }
}

impl MemoryBackend {
    /// Keep object history for the types `capability.supportedTypes` lists
    /// (TaskList, Task), in every account, with `retention`;
    /// `capability` is also what
    /// [`ObjectHistoryBackend::object_history_capability`] advertises.
    ///
    /// # Panics
    ///
    /// When `supportedTypes` names a type without history, or another type.
    #[must_use]
    pub fn with_object_history(
        self,
        capability: ObjectHistoryAccountCapability,
        retention: HistoryRetention,
    ) -> Self {
        for t in &capability.supported_types {
            assert!(
                SUPPORTED.contains(&t.as_str()),
                "the memory backend keeps no history for {t}"
            );
        }
        self.inner.lock_unscoped().history = Some(HistoryStore::new(capability, retention));
        self
    }

    /// Fix the history clock at `ms` milliseconds since the epoch.
    pub fn set_history_clock_for_test(&self, ms: i64) {
        if let Some(h) = self.inner.lock_unscoped().history.as_mut() {
            h.set_clock(ms);
        }
    }

    /// Discard one version of an object, in any order (§3.1).
    pub fn discard_version_for_test(&self, type_name: &str, account: &Id, id: &Id, version: u64) {
        if let Some(h) = self.inner.lock_unscoped().history.as_mut() {
            h.discard_version(type_name, account, id, version);
        }
    }

    /// Discard every version of an object: the §8.3 administrative purge.
    pub fn purge_object_history(&self, type_name: &str, account: &Id, id: &Id) {
        if let Some(h) = self.inner.lock_unscoped().history.as_mut() {
            h.purge(type_name, account, id);
        }
    }

    /// Set the §8.2 cut for `type_name` in `account`: versions replaced
    /// before `ms`, and objects destroyed before it, are hidden.
    pub fn set_rights_changed_at_for_test(&self, type_name: &str, account: &Id, ms: i64) {
        if let Some(h) = self.inner.lock_unscoped().history.as_mut() {
            h.set_rights_changed(type_name, account, ms);
        }
    }
}

fn not_found() -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
}

fn invalid(property: &str) -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(
        SetError::new(SetErrorType::InvalidProperties).with_properties([property]),
    ))
}

impl ObjectHistoryBackend for MemoryBackend {
    fn object_history_capability(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Option<ObjectHistoryAccountCapability> {
        let inner = self.inner.lock_unscoped();
        if !inner.known_accounts.contains(account_id.as_ref()) {
            return None;
        }
        inner.history.as_ref().map(|h| h.capability().clone())
    }

    async fn get_object_history<O: HistoryObject>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: HistoryGetParams<'_>,
    ) -> Result<HistoryGetResult<O>, BackendQueryError<MemoryError>> {
        let mut inner = self.inner.lock().await;
        inner.prune_history();
        let empty = HashMap::new();
        let live = inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .unwrap_or(&empty);
        let Some(h) = inner.history.as_ref() else {
            return Ok(HistoryGetResult::new(
                Vec::new(),
                params.ids.map(<[Id]>::to_vec).unwrap_or_default(),
            ));
        };
        let (rows, not_found) = h
            .select(O::TYPE_NAME, account_id, params.ids, live, params.arguments)
            .map_err(|e| BackendQueryError::Method(JmapError::invalid_arguments(e.to_string())))?;
        let mut entries = Vec::with_capacity(rows.len());
        for (snapshot, info) in rows {
            let object = O::deserialize(&snapshot)
                .map_err(|e| BackendQueryError::Other(MemoryError::new(e.to_string())))?;
            entries.push(HistoryEntry::new(object, info));
        }
        Ok(HistoryGetResult::new(entries, not_found))
    }

    async fn query_destroyed<O: HistoryObject + QueryObject>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: DestroyedQueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        let mut inner = self.inner.lock().await;
        inner.prune_history();
        let q = &params.query;
        let all = inner.destroyed_results::<O>(
            account_id,
            params.destroyed_after,
            params.destroyed_before,
            q.filter,
            q.sort,
        )?;
        let start = resolve_query_start(&all, q.position, q.anchor, q.anchor_offset)
            .map_err(BackendQueryError::Method)?;
        let ids: Vec<Id> = all[start..]
            .iter()
            .take(limit_or_max(q.limit))
            .cloned()
            .collect();
        let state = inner.history.as_ref().map_or_else(String::new, |h| {
            h.destroyed_query_state(
                O::TYPE_NAME,
                account_id,
                params.destroyed_after,
                params.destroyed_before,
            )
        });
        Ok(QueryResult::new(
            ids,
            start as u64,
            Some(all.len() as u64),
            State::from(state),
            true,
        ))
    }

    async fn query_destroyed_changes<O: HistoryObject + QueryObject>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: DestroyedQueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        let mut inner = self.inner.lock().await;
        inner.prune_history();
        let q = &params.query_changes;
        let all = inner
            .destroyed_results::<O>(
                account_id,
                params.destroyed_after,
                params.destroyed_before,
                q.filter,
                q.sort,
            )
            .map_err(|e| match e {
                BackendQueryError::Other(e) => BackendChangesError::Other(e),
                _ => BackendChangesError::CannotCalculate,
            })?;
        let Some(h) = inner.history.as_ref() else {
            return Err(BackendChangesError::CannotCalculate);
        };
        // A state of another kind or window cannot be calculated (decision
        // 25).
        let changed = h
            .destroyed_changes_since(
                O::TYPE_NAME,
                account_id,
                q.since_query_state.as_ref(),
                params.destroyed_after,
                params.destroyed_before,
            )
            .ok_or(BackendChangesError::CannotCalculate)?;
        if let Some(max) = q.max_changes
            && changed.len() as u64 > max
        {
            return Err(BackendChangesError::TooManyChanges { limit: max });
        }
        let new_state = h.destroyed_query_state(
            O::TYPE_NAME,
            account_id,
            params.destroyed_after,
            params.destroyed_before,
        );
        let added: Vec<AddedItem> = all
            .iter()
            .enumerate()
            .filter(|(_, id)| changed.contains(id))
            .map(|(i, id)| AddedItem::new(id.clone(), i as u64))
            .collect();
        Ok(QueryChangesResult::new(
            q.since_query_state.clone(),
            State::from(new_state),
            Some(all.len() as u64),
            changed,
            added,
        ))
    }

    async fn restore_object<O: HistoryObject + SetObject>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
        patch: Option<PatchObject>,
    ) -> Result<Restored<O>, BackendSetError<MemoryError>> {
        let mut inner = self.inner.lock().await;
        inner.require_account_for_target(account_id.as_ref())?;
        inner.prune_history();
        if inner
            .objects_ref(O::TYPE_NAME, account_id.as_ref())
            .is_some_and(|m| m.contains_key(id))
        {
            return Err(not_found());
        }
        let Some(Value::Object(final_version)) = inner
            .history
            .as_ref()
            .and_then(|h| h.final_version(O::TYPE_NAME, account_id, id))
            .cloned()
        else {
            return Err(not_found());
        };
        let baseline = apply_restore_patch::<O>(&final_version, patch.as_ref())
            .map_err(BackendSetError::SetError)?;
        if O::TYPE_NAME == Task::TYPE_NAME {
            // draft-ietf-jmap-tasks-06 §4: a Task is in one TaskList; object
            // history §5.2 rejects a reference to a missing one as an update
            // would.
            let list = baseline
                .get("taskListId")
                .and_then(Value::as_str)
                .map(Id::from);
            let exists = list.as_ref().is_some_and(|l| {
                inner
                    .objects_ref(TaskList::TYPE_NAME, account_id.as_ref())
                    .is_some_and(|m| m.contains_key(l))
            });
            if !exists {
                return Err(invalid("taskListId"));
            }
        }
        let stored = baseline.clone();
        let value = Value::Object(stored);
        let Ok(object) = O::deserialize(&value) else {
            // -04 §5.1: values that no longer make a valid object.
            let props: Vec<String> = patch
                .as_ref()
                .map(|p| p.as_map().keys().cloned().collect())
                .filter(|v: &Vec<String>| !v.is_empty())
                .unwrap_or_else(|| vec!["id".to_owned()]);
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties).with_properties(props),
            )));
        };
        if let Some(h) = inner.history.as_mut() {
            h.restore(O::TYPE_NAME, account_id, id);
        }
        if O::TYPE_NAME == Task::TYPE_NAME {
            let list = Inner::task_list_id_of(&value);
            inner.inc_task_ref(account_id.as_ref(), list);
        }
        inner
            .objects_mut(O::TYPE_NAME, account_id.as_ref())
            .insert(id.clone(), value);
        let new_state = inner.bump_state(O::TYPE_NAME, account_id.as_ref());
        inner
            .change_log_mut(O::TYPE_NAME, account_id.as_ref())
            .push(ChangeEntry {
                new_state,
                created: vec![],
                updated: vec![],
                destroyed: vec![],
                restored: vec![id.clone()],
                updated_props: None,
            });
        Ok(Restored::new(object, baseline))
    }
}
