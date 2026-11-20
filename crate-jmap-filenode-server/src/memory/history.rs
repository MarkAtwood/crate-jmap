//! Object history for FileNode in the memory backend
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes;
//! docs/design/object-history.md §7).
//!
//! Off until [`MemoryBackend::with_object_history`]. Versions live in a
//! `jmap_object_history_server::HistoryStore` inside the scoped state, so a
//! conditional rollback discards them with the rest. Node ids are
//! `node-N` from a counter that never goes back, so no id of a destroyed
//! node is minted again (§5.1).

use std::collections::HashMap;

use jmap_filenode_types::{FileNode, FileNodeComparator, FileNodeFilter, NodeType};
use jmap_object_history_server::{HistoryRetention, HistoryStore};
use jmap_object_history_types::{HistoryEntry, HistoryObject, ObjectHistoryAccountCapability};
use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError,
    DestroyedQueryChangesParams, DestroyedQueryParams, HistoryGetParams, HistoryGetResult,
    ObjectHistoryBackend, QueryChangesResult, QueryObject, QueryResult, Restored, SetError,
    SetErrorType, SetObject, apply_restore_patch, limit_or_max, resolve_query_start,
};
use jmap_types::{Id, JmapError, PatchObject, State, UTCDate};
use serde_json::Value;

use super::{
    ChangeType, MemoryBackend, MemoryError, MemoryState, check_metadata_filter, check_sort,
    compare_nodes, node_matches_tree,
};
use jmap_server::MemoryCaller;

const FILE_NODE: &str = "FileNode";

fn not_found() -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
}

fn bad_date(e: &jmap_types::ValidationError) -> JmapError {
    JmapError::invalid_arguments(e.to_string())
}

impl MemoryState {
    /// Destroyed nodes in result order: the window, then the FileNode/query
    /// filter and sort over final versions (parent-based conditions see the
    /// current tree), most recently destroyed first when there is no sort
    /// (§4).
    fn destroyed_results<O: QueryObject>(
        &self,
        account: &Id,
        after: Option<&UTCDate>,
        before: Option<&UTCDate>,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
    ) -> Result<Vec<Id>, BackendQueryError<MemoryError>> {
        let (Some(h), Some(store)) = (self.history.as_ref(), self.get_or_err(account)) else {
            return Ok(Vec::new());
        };
        let sort: Vec<FileNodeComparator> = match sort {
            None => Vec::new(),
            Some(s) => serde_json::to_value(s)
                .and_then(serde_json::from_value)
                .map_err(|e| MemoryError::serde("parse sort", &e))?,
        };
        check_sort(&sort).map_err(BackendQueryError::Method)?;
        let filter: Option<FileNodeFilter> = match filter {
            None => None,
            Some(f) => {
                let v = serde_json::to_value(f)
                    .map_err(|e| MemoryError::serde("serialize filter", &e))?;
                check_metadata_filter(&v, self.metadata_info.as_ref())?;
                Some(
                    serde_json::from_value(v)
                        .map_err(|e| MemoryError::serde("parse filter", &e))?,
                )
            }
        };
        let rows = h
            .destroyed(FILE_NODE, account, after, before)
            .map_err(|e| BackendQueryError::Method(bad_date(&e)))?;
        let mut kept: Vec<FileNode> = Vec::new();
        for d in rows {
            let node = serde_json::from_value::<FileNode>(d.snapshot)
                .map_err(|e| MemoryError::serde("history snapshot", &e))?;
            let ok = match &filter {
                None => true,
                Some(f) => node_matches_tree(&node, f, &store.nodes)?,
            };
            if ok {
                kept.push(node);
            }
        }
        // Stable over the store's destruction order; with no sort that
        // order stands (compare_nodes has a default order of its own).
        if !sort.is_empty() {
            kept.sort_by(|a, b| compare_nodes(a, b, &sort));
        }
        Ok(kept.into_iter().map(|n| n.id).collect())
    }
}

impl MemoryState {
    /// Whether `user` sees the object history of `account`: only its owner,
    /// or a caller with no principal, does. Object history §8.2 lets a
    /// caller see only what it "could read now"; this backend does not
    /// keep the rights each version was readable under, so it shows no
    /// history to a sharee.
    fn history_visible(&self, account: &Id, user: Option<&Id>) -> bool {
        user.is_none_or(|u| self.get_or_err(account).is_some_and(|s| s.owner == *u))
    }
}

impl<C> MemoryBackend<C> {
    /// Keep object history for FileNode in every account with `retention`;
    /// `capability` is also what
    /// [`ObjectHistoryBackend::object_history_capability`] advertises.
    ///
    /// # Panics
    ///
    /// When `supportedTypes` names a type other than FileNode.
    #[must_use]
    pub fn with_object_history(
        self,
        capability: ObjectHistoryAccountCapability,
        retention: HistoryRetention,
    ) -> Self {
        for t in &capability.supported_types {
            assert_eq!(t, FILE_NODE, "the memory backend keeps no history for {t}");
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

    /// Discard one version of a node, in any order (§3.1).
    pub fn discard_version_for_test(&self, account: &Id, id: &Id, version: u64) {
        if let Some(h) = self.inner.lock_unscoped().history.as_mut() {
            h.discard_version(FILE_NODE, account, id, version);
        }
    }

    /// Discard every version of a node: the §8.3 administrative purge.
    pub fn purge_object_history(&self, account: &Id, id: &Id) {
        if let Some(h) = self.inner.lock_unscoped().history.as_mut() {
            h.purge(FILE_NODE, account, id);
        }
    }

    /// Set the §8.2 cut for FileNode in `account`. The backend also sets it
    /// whenever an update changes inherited rights (a `shareWith` change or
    /// a move to a parent whose inherited rights differ).
    pub fn set_rights_changed_at_for_test(&self, account: &Id, ms: i64) {
        if let Some(h) = self.inner.lock_unscoped().history.as_mut() {
            h.set_rights_changed(FILE_NODE, account, ms);
        }
    }
}

impl<C: MemoryCaller> ObjectHistoryBackend for MemoryBackend<C> {
    fn object_history_capability(
        &self,
        _caller: &C,
        account_id: &Id,
    ) -> Option<ObjectHistoryAccountCapability> {
        let inner = self.inner.lock_unscoped();
        inner.get_or_err(account_id)?;
        inner.history.as_ref().map(|h| h.capability().clone())
    }

    async fn get_object_history<O: HistoryObject>(
        &self,
        caller: &C,
        account_id: &Id,
        params: HistoryGetParams<'_>,
    ) -> Result<HistoryGetResult<O>, BackendQueryError<MemoryError>> {
        let mut guard = self.inner.lock().await;
        if let Some(h) = guard.history.as_mut() {
            h.prune();
        }
        if !guard.history_visible(account_id, caller.principal()) {
            return Ok(HistoryGetResult::new(
                Vec::new(),
                params.ids.map(<[Id]>::to_vec).unwrap_or_default(),
            ));
        }
        let live: HashMap<Id, Value> = guard
            .get_or_err(account_id)
            .map(|s| {
                s.nodes
                    .iter()
                    .filter_map(|(id, n)| Some((id.clone(), serde_json::to_value(n).ok()?)))
                    .collect()
            })
            .unwrap_or_default();
        let Some(h) = guard.history.as_ref() else {
            return Ok(HistoryGetResult::new(
                Vec::new(),
                params.ids.map(<[Id]>::to_vec).unwrap_or_default(),
            ));
        };
        let (rows, not_found) = h
            .select(
                O::TYPE_NAME,
                account_id,
                params.ids,
                &live,
                params.arguments,
            )
            .map_err(|e| BackendQueryError::Method(bad_date(&e)))?;
        let mut entries = Vec::with_capacity(rows.len());
        for (snapshot, info) in rows {
            let object = serde_json::from_value::<O>(snapshot)
                .map_err(|e| MemoryError::serde("history snapshot", &e))?;
            entries.push(HistoryEntry::new(object, info));
        }
        Ok(HistoryGetResult::new(entries, not_found))
    }

    async fn query_destroyed<O: HistoryObject + QueryObject>(
        &self,
        caller: &C,
        account_id: &Id,
        params: DestroyedQueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        let mut guard = self.inner.lock().await;
        if let Some(h) = guard.history.as_mut() {
            h.prune();
        }
        let q = &params.query;
        let all = if guard.history_visible(account_id, caller.principal()) {
            guard.destroyed_results::<O>(
                account_id,
                params.destroyed_after,
                params.destroyed_before,
                q.filter,
                q.sort,
            )?
        } else {
            Vec::new()
        };
        let start = resolve_query_start(&all, q.position, q.anchor, q.anchor_offset)
            .map_err(BackendQueryError::Method)?;
        let ids: Vec<Id> = all[start..]
            .iter()
            .take(limit_or_max(q.limit))
            .cloned()
            .collect();
        let state = guard.history.as_ref().map_or_else(String::new, |h| {
            h.destroyed_query_state(
                FILE_NODE,
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
        caller: &C,
        account_id: &Id,
        params: DestroyedQueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        let mut guard = self.inner.lock().await;
        if let Some(h) = guard.history.as_mut() {
            h.prune();
        }
        let q = &params.query_changes;
        if !guard.history_visible(account_id, caller.principal()) {
            return Err(BackendChangesError::CannotCalculate);
        }
        let all = guard
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
        let Some(h) = guard.history.as_ref() else {
            return Err(BackendChangesError::CannotCalculate);
        };
        // A state of another kind or window cannot be calculated (decision
        // 25).
        let changed = h
            .destroyed_changes_since(
                FILE_NODE,
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
            FILE_NODE,
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
        caller: &C,
        account_id: &Id,
        id: &Id,
        patch: Option<PatchObject>,
    ) -> Result<Restored<O>, BackendSetError<MemoryError>> {
        let mut guard = self.inner.lock().await;
        if let Some(h) = guard.history.as_mut() {
            h.prune();
        }
        if !guard.history_visible(account_id, caller.principal()) {
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::Forbidden)
                    .with_description("only the account owner may restore a FileNode"),
            )));
        }
        let Some(store) = guard.get_or_err(account_id) else {
            return Err(not_found());
        };
        if store.nodes.contains_key(id) {
            return Err(not_found());
        }
        let Some(Value::Object(final_version)) = guard
            .history
            .as_ref()
            .and_then(|h| h.final_version(FILE_NODE, account_id, id))
            .cloned()
        else {
            return Err(not_found());
        };
        let baseline = apply_restore_patch::<O>(&final_version, patch.as_ref())
            .map_err(BackendSetError::SetError)?;
        let value = Value::Object(baseline.clone());
        let Ok(node) = serde_json::from_value::<FileNode>(value.clone()) else {
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties).with_properties(["id"]),
            )));
        };
        // §5.2: a missing parent fails as an update naming it would, and a
        // sibling with the same name is the Foo/set conflict.
        let store = guard.get_or_err(account_id).ok_or_else(not_found)?;
        if let Some(parent) = &node.parent_id {
            let is_directory = store
                .nodes
                .get(parent)
                .is_some_and(|p| p.node_type == Some(NodeType::Directory));
            if !is_directory {
                return Err(BackendSetError::SetError(Box::new(
                    SetError::new(SetErrorType::InvalidProperties).with_properties(["parentId"]),
                )));
            }
        }
        if store
            .nodes
            .values()
            .any(|n| n.parent_id == node.parent_id && n.name == node.name)
        {
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::AlreadyExists,
            ))));
        }
        let object = serde_json::from_value::<O>(value)
            .map_err(|e| BackendSetError::Other(MemoryError::serde("restored FileNode", &e)))?;
        if let Some(h) = guard.history.as_mut() {
            h.restore(FILE_NODE, account_id, id);
        }
        let metadata = guard.has_metadata();
        let store = guard.get_mut_or_err(account_id).ok_or_else(not_found)?;
        let before = store.nodes.clone();
        store.nodes.insert(id.clone(), node);
        store.log_write(&before, ChangeType::Restored, id, None, metadata, false);
        Ok(Restored::new(object, baseline))
    }
}
