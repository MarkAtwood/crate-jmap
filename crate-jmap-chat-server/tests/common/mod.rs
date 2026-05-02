//! Shared test infrastructure — MemoryBackend in-memory ChatBackend implementation.
//!
//! Each integration test binary includes this module with `mod common;`.
//! Dead-code warnings are suppressed because not all items are used in every binary.
#![allow(dead_code)]
#![allow(async_fn_in_trait)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use jmap_chat_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, ChatBackend, GetObject,
    JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType,
    SetObject,
};
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// A change log entry for one state transition.
#[derive(Clone)]
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
    objects: HashMap<(String, String), HashMap<Id, serde_json::Value>>,
    /// `(type_name, account_id)` → current state counter
    states: HashMap<(String, String), u64>,
    /// `(type_name, account_id)` → ordered change entries
    change_log: HashMap<(String, String), Vec<ChangeEntry>>,
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
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// A fully in-memory implementation of [`ChatBackend`].
///
/// Stores objects as serialized JSON; each mutation bumps a monotonic state
/// counter and records a change log entry. Used as both the integration-test
/// harness and the canonical example for backend implementors.
#[derive(Clone, Default)]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate a server-assigned id for a new object.
    fn next_id(inner: &mut Inner, type_name: &str, account_id: &str) -> Id {
        let n = inner
            .objects_ref(type_name, account_id)
            .map_or(0, |m| m.len());
        Id::from(format!("{}{}", type_name.to_ascii_lowercase(), n + 1).as_str())
    }
}

/// A simple string error for MemoryBackend failures.
#[derive(Debug, Clone)]
pub struct MemoryError(pub String);

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MemoryError {}

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[O::Property]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        let inner = self.inner.lock().unwrap();
        let map = inner.objects_ref(O::TYPE_NAME, account_id.as_ref());

        match ids {
            None => {
                // Return all objects.
                let mut list = Vec::new();
                if let Some(m) = map {
                    for val in m.values() {
                        match serde_json::from_value::<O>(val.clone()) {
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
                        Some(val) => match serde_json::from_value::<O>(val.clone()) {
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
        Ok(State::from(n.to_string().as_str()))
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

        // Collect entries with new_state > since_n.
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
            State::from(current_state.to_string().as_str()),
        ))
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
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
        // Reuse get_changes to determine which ids were added/removed/updated.
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

        // Build the current ordered id list for position assignment.
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

impl ChatBackend for MemoryBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        let mut inner = self.inner.lock().unwrap();
        let server_id = Self::next_id(&mut inner, O::TYPE_NAME, account_id.as_ref());

        // Serialize, set "id" to the server-assigned id, then deserialize back.
        let mut val = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;
        val["id"] = serde_json::Value::String(server_id.as_ref().to_owned());
        let stored_obj: O = serde_json::from_value(val.clone())
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

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

        // Apply JSON Merge Patch (RFC 7396): merge patch fields into current value.
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
// FaultyBackend — always returns errors, for negative-path testing
// ---------------------------------------------------------------------------

/// A backend that always returns errors. Used to test the handler's error paths.
#[derive(Clone, Default)]
pub struct FaultyBackend;

impl JmapBackend for FaultyBackend {
    type Error = MemoryError;

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _account_id: &Id,
        _ids: Option<&[Id]>,
        _properties: Option<&[O::Property]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        Err(MemoryError("storage unavailable".to_owned()))
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _account_id: &Id,
    ) -> Result<State, Self::Error> {
        Err(MemoryError("storage unavailable".to_owned()))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _account_id: &Id,
        _since_state: &State,
        _max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        Err(BackendChangesError::Other(MemoryError(
            "storage unavailable".to_owned(),
        )))
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _account_id: &Id,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        _limit: Option<u64>,
        _position: i64,
    ) -> Result<QueryResult, Self::Error> {
        Err(MemoryError("storage unavailable".to_owned()))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _account_id: &Id,
        _since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        _max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        Err(BackendChangesError::Other(MemoryError(
            "storage unavailable".to_owned(),
        )))
    }
}

impl ChatBackend for FaultyBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _account_id: &Id,
        _create_id: &str,
        _obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError(
            "storage unavailable".to_owned(),
        )))
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _account_id: &Id,
        _id: &Id,
        _patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError(
            "storage unavailable".to_owned(),
        )))
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _account_id: &Id,
        _id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        Err(BackendSetError::Other(MemoryError(
            "storage unavailable".to_owned(),
        )))
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// JSON Merge Patch (RFC 7396)
// ---------------------------------------------------------------------------

/// Apply a JSON Merge Patch to `target` in-place.
fn json_merge_patch(target: &mut serde_json::Value, patch: serde_json::Value) {
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
                    json_merge_patch(entry, patch_val);
                }
            }
        }
        other => *target = other,
    }
}
