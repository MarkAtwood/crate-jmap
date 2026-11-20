//! Object history for Graph, GraphNode and GraphEdge in the memory backend
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes;
//! docs/design/object-history.md §7; docs/design/DECISIONS.md decision 22).
//! GraphNodeOp keeps none: "When a node is destroyed, its ops are destroyed
//! with it" (graph-edit §Compaction).
//!
//! Off until [`MemoryBackend::with_object_history`]. Versions live in a
//! `jmap_object_history_server::HistoryStore` inside the scoped state, so a
//! conditional rollback discards them with the rest.
//!
//! §8.2: history shows a caller only what it could read now: a version of
//! a Graph, or of a node or edge of a Graph, the caller may read; a
//! destroyed Graph's history, or that of its contents, to the account owner
//! only.

use std::collections::HashMap;

use jmap_graph_types::{Graph, GraphEdge, GraphNode};
use jmap_object_history_server::{HistoryRetention, HistoryStore};
use jmap_object_history_types::{HistoryEntry, HistoryObject, ObjectHistoryAccountCapability};
use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError,
    DestroyedQueryChangesParams, DestroyedQueryParams, HistoryGetParams, HistoryGetResult,
    JmapObject, MemoryCaller, ObjectHistoryBackend, QueryChangesResult, QueryObject, QueryResult,
    Restored, SetError, SetErrorType, SetObject, apply_restore_patch, limit_or_max,
    resolve_query_start,
};
use jmap_types::{Id, JmapError, PatchObject, State};
use serde_json::Value;

use super::{Change, Inner, MemoryBackend, MemoryError, OpLog};

/// The types this backend keeps history for.
const SUPPORTED: &[&str] = &["Graph", "GraphNode", "GraphEdge"];

fn not_found() -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
}

fn invalid(property: &str) -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(
        SetError::new(SetErrorType::InvalidProperties).with_properties([property]),
    ))
}

fn bad_date(e: &jmap_types::ValidationError) -> JmapError {
    JmapError::invalid_arguments(e.to_string())
}

impl Inner {
    /// Record the update of `id` from `previous`.
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

    /// Record the destruction of `id` with `last` as its final state; every
    /// route counts (§3), so the Graph and GraphNode cascades too.
    pub(super) fn record_destroy(&mut self, type_name: &str, account: &Id, id: &Id, last: Value) {
        if let Some(h) = self.history.as_mut() {
            h.record_destroy(type_name, account, id, last);
        }
    }

    fn prune_history(&mut self) {
        if let Some(h) = self.history.as_mut() {
            h.prune();
        }
    }

    /// §8.2: may `caller` see a version `v` of a `type_name` object?
    fn may_see(&self, caller: &Id, account: &Id, type_name: &str, v: &Value) -> bool {
        let graph = if type_name == Graph::TYPE_NAME {
            v.get("id")
        } else {
            v.get("graphId")
        }
        .and_then(Value::as_str)
        .map(Id::from);
        match graph {
            Some(g) if self.get(Graph::TYPE_NAME, account, &g).is_some() => {
                self.may_read(caller, account, &g)
            }
            _ => self.is_owner(caller, account),
        }
    }

    /// Destroyed objects the caller may see, most recently destroyed first
    /// (§4). No filter or sort is evaluated over destroyed graph objects:
    /// any is `unsupportedFilter`/`unsupportedSort` (§4 allows fewer).
    fn destroyed_results<O: QueryObject>(
        &self,
        caller: &Id,
        account: &Id,
        params: (Option<&jmap_types::UTCDate>, Option<&jmap_types::UTCDate>),
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
    ) -> Result<Vec<Id>, BackendQueryError<MemoryError>> {
        let Some(h) = self.history.as_ref() else {
            return Ok(Vec::new());
        };
        if sort.is_some_and(|s| !s.is_empty()) {
            return Err(BackendQueryError::Method(JmapError::unsupported_sort()));
        }
        if filter.is_some() {
            return Err(BackendQueryError::Method(JmapError::unsupported_filter()));
        }
        Ok(h.destroyed(O::TYPE_NAME, account, params.0, params.1)
            .map_err(|e| BackendQueryError::Method(bad_date(&e)))?
            .into_iter()
            .filter(|d| self.may_see(caller, account, O::TYPE_NAME, &d.snapshot))
            .map(|d| d.id)
            .collect())
    }
}

impl<C> MemoryBackend<C> {
    /// Keep object history for the types `capability.supportedTypes` lists
    /// (Graph, GraphNode, GraphEdge) in every account, with `retention`;
    /// `capability` is also what
    /// [`ObjectHistoryBackend::object_history_capability`] advertises.
    ///
    /// # Panics
    ///
    /// When `supportedTypes` names another type.
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
        self.lock().history = Some(HistoryStore::new(capability, retention));
        self
    }

    /// Fix the history clock at `ms` milliseconds since the epoch.
    pub fn set_history_clock_for_test(&self, ms: i64) {
        if let Some(h) = self.lock().history.as_mut() {
            h.set_clock(ms);
        }
    }
}

impl<C: MemoryCaller> ObjectHistoryBackend for MemoryBackend<C> {
    fn object_history_capability(
        &self,
        _caller: &C,
        account_id: &Id,
    ) -> Option<ObjectHistoryAccountCapability> {
        let inner = self.lock();
        inner.accounts.get(account_id.as_ref())?;
        inner.history.as_ref().map(|h| h.capability().clone())
    }

    async fn get_object_history<O: HistoryObject>(
        &self,
        caller: &C,
        account_id: &Id,
        params: HistoryGetParams<'_>,
    ) -> Result<HistoryGetResult<O>, BackendQueryError<MemoryError>> {
        let caller = &self.acting(caller, account_id).await;
        let mut inner = self.locked().await;
        inner.prune_history();
        let empty = HashMap::new();
        let live = inner.objects(O::TYPE_NAME, account_id).unwrap_or(&empty);
        let Some(h) = inner.history.as_ref() else {
            return Ok(HistoryGetResult::new(
                Vec::new(),
                params.ids.map(<[Id]>::to_vec).unwrap_or_default(),
            ));
        };
        let (rows, mut not_found) = h
            .select(O::TYPE_NAME, account_id, params.ids, live, params.arguments)
            .map_err(|e| BackendQueryError::Method(bad_date(&e)))?;
        let mut entries = Vec::with_capacity(rows.len());
        for (snapshot, info) in rows {
            if !inner.may_see(caller, account_id, O::TYPE_NAME, &snapshot) {
                if let Some(id) = snapshot.get("id").and_then(Value::as_str).map(Id::from)
                    && !not_found.contains(&id)
                {
                    not_found.push(id);
                }
                continue;
            }
            let object = serde_json::from_value::<O>(snapshot)
                .map_err(|e| BackendQueryError::Other(MemoryError::new(e.to_string())))?;
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
        let caller = &self.acting(caller, account_id).await;
        let mut inner = self.locked().await;
        inner.prune_history();
        let q = &params.query;
        let all = inner.destroyed_results::<O>(
            caller,
            account_id,
            (params.destroyed_after, params.destroyed_before),
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
        caller: &C,
        account_id: &Id,
        params: DestroyedQueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        let caller = &self.acting(caller, account_id).await;
        let mut inner = self.locked().await;
        inner.prune_history();
        let q = &params.query_changes;
        let all = inner
            .destroyed_results::<O>(
                caller,
                account_id,
                (params.destroyed_after, params.destroyed_before),
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
        caller: &C,
        account_id: &Id,
        id: &Id,
        patch: Option<PatchObject>,
    ) -> Result<Restored<O>, BackendSetError<MemoryError>> {
        let caller = &self.acting(caller, account_id).await;
        let mut inner = self.locked().await;
        inner.prune_history();
        if inner.get(O::TYPE_NAME, account_id, id).is_some() {
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
        let fin = Value::Object(final_version.clone());
        if !inner.may_see(caller, account_id, O::TYPE_NAME, &fin) {
            return Err(not_found());
        }
        let baseline = apply_restore_patch::<O>(&final_version, patch.as_ref())
            .map_err(BackendSetError::SetError)?;
        let value = Value::Object(baseline.clone());
        if O::TYPE_NAME != Graph::TYPE_NAME {
            // A node or edge needs its Graph, writable by the caller.
            let Some(graph) = value.get("graphId").and_then(Value::as_str).map(Id::from) else {
                return Err(invalid("graphId"));
            };
            if inner.get(Graph::TYPE_NAME, account_id, &graph).is_none() {
                return Err(invalid("graphId"));
            }
            inner.require_write(caller, account_id, &graph, not_found)?;
        } else if !inner.is_owner(caller, account_id) {
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::Forbidden,
            ))));
        }
        if O::TYPE_NAME == GraphEdge::TYPE_NAME {
            // graph-00's integrity rule: both endpoints are current nodes.
            for end in ["fromId", "toId"] {
                let node = value.get(end).and_then(Value::as_str).map(Id::from);
                if !node.is_some_and(|n| inner.get(GraphNode::TYPE_NAME, account_id, &n).is_some())
                {
                    return Err(invalid(end));
                }
            }
        }
        let object = serde_json::from_value::<O>(value.clone()).map_err(|_| invalid("id"))?;
        if let Some(h) = inner.history.as_mut() {
            h.restore(O::TYPE_NAME, account_id, id);
        }
        if O::TYPE_NAME == GraphNode::TYPE_NAME {
            // Decision 22: "When a node is destroyed, its ops are destroyed
            // with it" (graph-edit §Compaction), so the node comes back at
            // its final snapshot with an empty op log whose head is that
            // snapshot's sequence number.
            let seq = value
                .get("snapshotSeq")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            inner.op_logs.insert(
                (account_id.as_ref().to_owned(), id.clone()),
                OpLog {
                    head: seq,
                    compacted_through: seq,
                    held: Default::default(),
                },
            );
        }
        let type_name: &'static str = O::TYPE_NAME;
        inner
            .objects_mut(type_name, account_id)
            .insert(id.clone(), value);
        inner.commit(type_name, account_id, vec![(id.clone(), Change::Restored)]);
        Ok(Restored::new(object, baseline))
    }
}
