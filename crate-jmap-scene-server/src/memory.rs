//! In-memory reference implementation of [`SceneBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` backs this crate's integration tests and is a readable
//! example for anyone writing a real backend. All state lives in
//! `HashMap`s behind a `std::sync::Mutex`; nothing is persisted. Blobs are
//! registered by size only, with `add_blob`.
//!
//! # Caller context
//!
//! `CallerCtx` is the caller's userId (`String`). The creator of an asset
//! or region is its owner; [`SceneBackend::may_modify_asset`] and
//! [`SceneBackend::may_update_region`] allow the owner and the users
//! registered with `add_admin`, [`SceneBackend::may_destroy_region`] the
//! owner only.
//!
//! # Companion capabilities and bindings
//!
//! `advertise_capability` records a capability URI for an account, for
//! [`SceneBackend::has_capability`]. `set_region_binding` records what a
//! binding id references; [`SceneBackend::region_binding`] answers
//! [`BindingTarget::Missing`] for any id not recorded.
//!
//! # Quota
//!
//! `set_storage_quota` sets an account's total asset storage in bytes;
//! [`SceneBackend::create_object`] refuses a `SceneAsset` that would
//! exceed it, and a `SceneRegion` beyond the capability's
//! `maxRegionsPerAccount`, with `overQuota`.
//!
//! # Region destroy
//!
//! Destroying a `SceneRegion` also destroys the stored `SceneObject` and
//! `SceneAvatar` records whose `regionId` names it.
//!
//! # Query
//!
//! `SceneAsset/query` and `SceneRegion/query` evaluate the filter and sort
//! with the [`crate::query`] functions. `/queryChanges` is not implemented: results
//! say `canCalculateChanges: false` and `/queryChanges` returns
//! `cannotCalculateChanges` (RFC 8620 §5.6).
//!
//! # Concurrency
//!
//! The `await_holding_lock` lint is denied module-wide: no guard is held
//! across an `.await`.

#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use jmap_scene_types::query::{
    SceneAssetComparator, SceneAssetFilter, SceneRegionComparator, SceneRegionFilter,
};
use jmap_scene_types::{SceneAsset, SceneAvatar, SceneCapability, SceneObject, SceneRegion};
use jmap_server::{apply_patch_object, resolve_query_start};
use jmap_types::{Id, State};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::query::{asset_matches, compare_assets, compare_regions, region_matches};
use crate::{
    BackendChangesError, BackendQueryError, BackendSetError, BindingTarget, ChangesResult,
    GetObject, JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SceneBackend,
    SetError, SetErrorType, SetObject,
};

/// How one id changed in one state step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Created,
    Updated,
    Destroyed,
}

type Key = (&'static str, String);

#[derive(Default)]
struct Inner {
    /// `(type_name, account_id)` → `id → stored object`.
    objects: HashMap<Key, HashMap<Id, Value>>,
    /// `(type_name, account_id)` → state counter.
    states: HashMap<Key, u64>,
    /// `(type_name, account_id)` → `(state after the change, id, change)`.
    log: HashMap<Key, Vec<(u64, Id, Change)>>,
    /// Registered accounts and their Scene capability object.
    accounts: HashMap<String, SceneCapability>,
    /// `(account_id, blob_id)` → blob size in bytes.
    blobs: HashMap<(String, String), u64>,
    /// Account → total SceneAsset storage limit in bytes.
    storage_quota: HashMap<String, u64>,
    /// Object id → the userId that created it.
    owners: HashMap<Id, String>,
    /// UserIds with admin privileges.
    admins: HashSet<String>,
    /// Account → advertised capability URIs.
    capabilities: HashMap<String, HashSet<String>>,
    /// `(binding property, id)` → what the id references.
    bindings: HashMap<(String, String), BindingTarget>,
}

impl Inner {
    fn objects(&self, type_name: &'static str, account_id: &Id) -> Option<&HashMap<Id, Value>> {
        self.objects
            .get(&(type_name, account_id.as_ref().to_owned()))
    }

    fn objects_mut(&mut self, type_name: &'static str, account_id: &Id) -> &mut HashMap<Id, Value> {
        self.objects
            .entry((type_name, account_id.as_ref().to_owned()))
            .or_default()
    }

    fn state(&self, type_name: &'static str, account_id: &Id) -> u64 {
        self.states
            .get(&(type_name, account_id.as_ref().to_owned()))
            .copied()
            .unwrap_or(0)
    }

    /// Advance the state of `type_name` once and log `changes` under it.
    fn commit(&mut self, type_name: &'static str, account_id: &Id, changes: Vec<(Id, Change)>) {
        let key = (type_name, account_id.as_ref().to_owned());
        let state = self.states.entry(key.clone()).or_insert(0);
        *state += 1;
        let state = *state;
        self.log
            .entry(key)
            .or_default()
            .extend(changes.into_iter().map(|(id, c)| (state, id, c)));
    }

    /// Every stored object of type `O` that decodes.
    fn all<O: JmapObject>(&self, account_id: &Id) -> impl Iterator<Item = O> + '_ {
        self.objects(O::TYPE_NAME, account_id)
            .into_iter()
            .flat_map(HashMap::values)
            .filter_map(|v| O::deserialize(v).ok())
    }
}

/// A fully in-memory implementation of [`SceneBackend`].
///
/// `Clone` is a cheap `Arc` clone: every clone shares the same state.
#[derive(Clone, Default)]
pub struct MemoryBackend {
    inner: Arc<Mutex<Inner>>,
}

impl MemoryBackend {
    /// An empty backend with no accounts.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A poisoned lock means a test already panicked mid-mutation.
        self.inner.lock().expect("MemoryBackend lock poisoned")
    }

    /// Register `account_id` with the Scene capability object it
    /// advertises.
    #[doc(hidden)]
    pub fn register_account(&self, account_id: &Id, capability: SceneCapability) {
        self.lock()
            .accounts
            .insert(account_id.as_ref().to_owned(), capability);
    }

    /// Record an uploaded blob `blob_id` of `size` bytes in `account_id`.
    #[doc(hidden)]
    pub fn add_blob(&self, account_id: &Id, blob_id: &str, size: u64) {
        self.lock()
            .blobs
            .insert((account_id.as_ref().to_owned(), blob_id.to_owned()), size);
    }

    /// Limit the total `size` of `account_id`'s SceneAssets to `bytes`.
    #[doc(hidden)]
    pub fn set_storage_quota(&self, account_id: &Id, bytes: u64) {
        self.lock()
            .storage_quota
            .insert(account_id.as_ref().to_owned(), bytes);
    }

    /// Give `user` admin privileges.
    #[doc(hidden)]
    pub fn add_admin(&self, user: &str) {
        self.lock().admins.insert(user.to_owned());
    }

    /// Advertise capability `uri` for `account_id`.
    #[doc(hidden)]
    pub fn advertise_capability(&self, account_id: &Id, uri: &str) {
        self.lock()
            .capabilities
            .entry(account_id.as_ref().to_owned())
            .or_default()
            .insert(uri.to_owned());
    }

    /// Record that binding `property` set to `id` references `target`.
    #[doc(hidden)]
    pub fn set_region_binding(&self, property: &str, id: &str, target: BindingTarget) {
        self.lock()
            .bindings
            .insert((property.to_owned(), id.to_owned()), target);
    }

    /// Store `value` as object `id` of `type_name`, owned by `owner`,
    /// without validation or a state change, to seed states the methods
    /// cannot reach.
    #[doc(hidden)]
    pub fn insert_object_for_test(
        &self,
        type_name: &'static str,
        account_id: &Id,
        id: &str,
        owner: &str,
        value: Value,
    ) {
        let mut inner = self.lock();
        inner
            .objects_mut(type_name, account_id)
            .insert(Id::from(id), value);
        inner.owners.insert(Id::from(id), owner.to_owned());
    }
}

/// Opaque storage-layer error returned by [`MemoryBackend`].
#[non_exhaustive]
#[derive(Debug)]
pub struct MemoryError {
    description: String,
}

impl MemoryError {
    /// A [`MemoryError`] with a human-readable description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }

    /// Human-readable description of the failure.
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

fn not_found<E>() -> BackendSetError<E> {
    BackendSetError::SetError(SetError::new(SetErrorType::NotFound))
}

fn decode<O: JmapObject>(v: &Value) -> Result<O, MemoryError> {
    O::deserialize(v).map_err(|e| MemoryError::new(format!("decode {}: {e}", O::TYPE_NAME)))
}

fn recode_as<T: DeserializeOwned>(v: &Value) -> Result<T, BackendQueryError<MemoryError>> {
    T::deserialize(v).map_err(|e| BackendQueryError::Other(MemoryError::new(e.to_string())))
}

/// Re-decode `v`, a value of a generic `QueryObject` associated type, as
/// the concrete type `T` it is once `O::TYPE_NAME` is known.
fn recode<T: DeserializeOwned>(v: &impl Serialize) -> Result<T, BackendQueryError<MemoryError>> {
    let v = serde_json::to_value(v)
        .map_err(|e| BackendQueryError::Other(MemoryError::new(e.to_string())))?;
    recode_as(&v)
}

/// The ids of `list` that `keep` accepts, ordered by `cmp`.
fn sorted_ids<T>(
    list: impl Iterator<Item = T>,
    keep: impl Fn(&T) -> bool,
    cmp: impl Fn(&T, &T) -> std::cmp::Ordering,
    id: impl Fn(T) -> Id,
) -> Vec<Id> {
    let mut list: Vec<T> = list.filter(|t| keep(t)).collect();
    list.sort_by(|a, b| cmp(a, b));
    list.into_iter().map(id).collect()
}

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;
    type CallerCtx = String;

    async fn account_exists(&self, _caller: &String, account_id: &Id) -> Result<bool, MemoryError> {
        Ok(self.lock().accounts.contains_key(account_id.as_ref()))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        let inner = self.lock();
        let map = inner.objects(O::TYPE_NAME, account_id);
        match ids {
            None => {
                let list = map
                    .into_iter()
                    .flat_map(HashMap::values)
                    .map(decode)
                    .collect::<Result<_, _>>()?;
                Ok((list, Vec::new()))
            }
            Some(ids) => {
                let mut found = Vec::new();
                let mut missing = Vec::new();
                for id in ids {
                    match map.and_then(|m| m.get(id)) {
                        Some(v) => found.push(decode(v)?),
                        None => missing.push(id.clone()),
                    }
                }
                Ok((found, missing))
            }
        }
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
    ) -> Result<State, MemoryError> {
        Ok(State::from(
            self.lock().state(O::TYPE_NAME, account_id).to_string(),
        ))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<MemoryError>> {
        let inner = self.lock();
        let since: u64 = since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::CannotCalculate)?;
        let current = inner.state(O::TYPE_NAME, account_id);
        if since > current {
            return Err(BackendChangesError::CannotCalculate);
        }

        // RFC 8620 §5.2: each id appears once, in the bucket of its net
        // change since `since_state`. A create followed by an update stays
        // created; a later destroy or re-create overrides.
        let mut net: Vec<(Id, Change)> = Vec::new();
        for (_, id, change) in inner
            .log
            .get(&(O::TYPE_NAME, account_id.as_ref().to_owned()))
            .into_iter()
            .flatten()
            .filter(|(s, _, _)| *s > since)
        {
            match net.iter_mut().find(|(seen, _)| seen == id) {
                Some((_, prev)) if *prev == Change::Created && *change == Change::Updated => {}
                Some((_, prev)) => *prev = *change,
                None => net.push((id.clone(), *change)),
            }
        }
        if max_changes.is_some_and(|max| net.len() as u64 > max) {
            return Err(BackendChangesError::TooManyChanges {
                limit: max_changes.unwrap_or_default(),
            });
        }
        let bucket = |want: Change| -> Vec<Id> {
            net.iter()
                .filter(|(_, c)| *c == want)
                .map(|(id, _)| id.clone())
                .collect()
        };
        Ok(ChangesResult::new(
            bucket(Change::Created),
            bucket(Change::Updated),
            bucket(Change::Destroyed),
            false,
            State::from(current.to_string()),
        ))
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
        anchor: Option<&Id>,
        anchor_offset: i64,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        let sort = sort.unwrap_or_default();
        let inner = self.lock();
        let ids = if O::TYPE_NAME == SceneAsset::TYPE_NAME {
            let filter: Option<SceneAssetFilter> = filter.map(recode).transpose()?;
            let sort: Vec<SceneAssetComparator> = recode(&sort)?;
            sorted_ids(
                inner.all::<SceneAsset>(account_id),
                |a| filter.as_ref().is_none_or(|f| asset_matches(f, a)),
                |a, b| compare_assets(a, b, &sort),
                |a| a.id,
            )
        } else if O::TYPE_NAME == SceneRegion::TYPE_NAME {
            let filter: Option<SceneRegionFilter> = filter.map(recode).transpose()?;
            let sort: Vec<SceneRegionComparator> = recode(&sort)?;
            sorted_ids(
                inner.all::<SceneRegion>(account_id),
                |r| filter.as_ref().is_none_or(|f| region_matches(f, r)),
                |a, b| compare_regions(a, b, &sort),
                |r| r.id,
            )
        } else {
            return Err(BackendQueryError::Other(MemoryError::new(format!(
                "query of {} is not implemented",
                O::TYPE_NAME
            ))));
        };

        let total = ids.len() as u64;
        let start = resolve_query_start(&ids, position, anchor, anchor_offset)
            .map_err(BackendQueryError::Method)?;
        let page = ids
            .into_iter()
            .skip(start)
            .take(limit.map_or(usize::MAX, |n| n as usize))
            .collect();
        Ok(QueryResult::new(
            page,
            start as u64,
            Some(total),
            State::from(inner.state(O::TYPE_NAME, account_id).to_string()),
            false,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &String,
        _account_id: &Id,
        _since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        _max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        Err(BackendChangesError::CannotCalculate)
    }
}

impl SceneBackend for MemoryBackend {
    async fn scene_capability(
        &self,
        _caller: &String,
        account_id: &Id,
    ) -> Result<SceneCapability, MemoryError> {
        self.lock()
            .accounts
            .get(account_id.as_ref())
            .cloned()
            .ok_or_else(|| MemoryError::new("account not registered"))
    }

    async fn blob_size(
        &self,
        _caller: &String,
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<Option<u64>, MemoryError> {
        Ok(self
            .lock()
            .blobs
            .get(&(account_id.as_ref().to_owned(), blob_id.as_ref().to_owned()))
            .copied())
    }

    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &String,
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<MemoryError>> {
        let mut value = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError::new(e.to_string())))?;
        let mut inner = self.lock();
        // "Account asset storage quota exceeded", checked under the lock.
        if O::TYPE_NAME == SceneAsset::TYPE_NAME {
            if let Some(&quota) = inner.storage_quota.get(account_id.as_ref()) {
                let used: u64 = inner.all::<SceneAsset>(account_id).map(|a| a.size).sum();
                let size = value.get("size").and_then(Value::as_u64).unwrap_or(0);
                if used.saturating_add(size) > quota {
                    return Err(BackendSetError::SetError(
                        SetError::new(SetErrorType::OverQuota)
                            .with_description("account asset storage quota exceeded"),
                    ));
                }
            }
        }
        // "`overQuota` when `maxRegionsPerAccount` would be exceeded",
        // counted under the lock.
        if O::TYPE_NAME == SceneRegion::TYPE_NAME {
            let max = inner
                .accounts
                .get(account_id.as_ref())
                .and_then(|c| c.max_regions_per_account);
            let held = inner.all::<SceneRegion>(account_id).count() as u64;
            if max.is_some_and(|max| held >= max) {
                return Err(BackendSetError::SetError(
                    SetError::new(SetErrorType::OverQuota)
                        .with_description("maxRegionsPerAccount reached"),
                ));
            }
        }
        let id = Id::from(ulid::Ulid::new().to_string());
        value["id"] = json!(id);
        let stored: O = decode(&value).map_err(BackendSetError::Other)?;
        inner
            .objects_mut(O::TYPE_NAME, account_id)
            .insert(id.clone(), value);
        inner.owners.insert(id.clone(), caller.clone());
        inner.commit(
            O::TYPE_NAME,
            account_id,
            vec![(id.clone(), Change::Created)],
        );
        Ok((id, stored))
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let mut current = inner
            .objects(O::TYPE_NAME, account_id)
            .and_then(|m| m.get(id))
            .cloned()
            .ok_or_else(not_found)?;

        let patch = serde_json::to_value(&patch)
            .map_err(|e| BackendSetError::Other(MemoryError::new(e.to_string())))?;
        let (Value::Object(patch), Value::Object(base)) = (patch, &mut current) else {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::InvalidPatch,
            )));
        };
        apply_patch_object(base, &patch).map_err(|e| {
            BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()),
            )
        })?;
        if O::deserialize(&current).is_err() {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_description(format!("patched {} is invalid", O::TYPE_NAME)),
            ));
        }
        inner
            .objects_mut(O::TYPE_NAME, account_id)
            .insert(id.clone(), current);
        inner.commit(
            O::TYPE_NAME,
            account_id,
            vec![(id.clone(), Change::Updated)],
        );
        Ok(None)
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        inner
            .objects_mut(O::TYPE_NAME, account_id)
            .remove(id)
            .ok_or_else(not_found)?;
        inner.owners.remove(id);
        inner.commit(
            O::TYPE_NAME,
            account_id,
            vec![(id.clone(), Change::Destroyed)],
        );
        // "removes the SceneRegion and all contained SceneObject and
        // SceneAvatar records", in the same locked step.
        if O::TYPE_NAME == SceneRegion::TYPE_NAME {
            for contained in [SceneObject::TYPE_NAME, SceneAvatar::TYPE_NAME] {
                let map = inner.objects_mut(contained, account_id);
                let gone: Vec<Id> = map
                    .iter()
                    .filter(|(_, v)| v.get("regionId").and_then(Value::as_str) == Some(id.as_ref()))
                    .map(|(k, _)| k.clone())
                    .collect();
                if gone.is_empty() {
                    continue;
                }
                for k in &gone {
                    map.remove(k);
                }
                for k in &gone {
                    inner.owners.remove(k);
                }
                inner.commit(
                    contained,
                    account_id,
                    gone.into_iter().map(|k| (k, Change::Destroyed)).collect(),
                );
            }
        }
        Ok(())
    }

    async fn may_modify_asset(
        &self,
        caller: &String,
        _account_id: &Id,
        asset: &SceneAsset,
    ) -> Result<bool, MemoryError> {
        // "Only the asset owner or an admin may modify asset metadata."
        let inner = self.lock();
        Ok(inner.admins.contains(caller) || inner.owners.get(&asset.id) == Some(caller))
    }

    fn has_capability(&self, _caller: &String, account_id: &Id, uri: &str) -> bool {
        self.lock()
            .capabilities
            .get(account_id.as_ref())
            .is_some_and(|c| c.contains(uri))
    }

    async fn region_binding(
        &self,
        _caller: &String,
        _account_id: &Id,
        property: &str,
        id: &Id,
    ) -> Result<BindingTarget, MemoryError> {
        Ok(self
            .lock()
            .bindings
            .get(&(property.to_owned(), id.as_ref().to_owned()))
            .copied()
            .unwrap_or(BindingTarget::Missing))
    }

    async fn may_update_region(
        &self,
        caller: &String,
        _account_id: &Id,
        region: &SceneRegion,
    ) -> Result<bool, MemoryError> {
        // "Only the owner or an admin may modify region properties."
        let inner = self.lock();
        Ok(inner.admins.contains(caller) || inner.owners.get(&region.id) == Some(caller))
    }

    async fn may_destroy_region(
        &self,
        caller: &String,
        _account_id: &Id,
        region: &SceneRegion,
    ) -> Result<bool, MemoryError> {
        // "Only the region owner may destroy a region."
        Ok(self.lock().owners.get(&region.id) == Some(caller))
    }

    fn supports_type<O: JmapObject>(&self, _caller: &String, _account_id: &Id) -> bool {
        O::TYPE_NAME == SceneAsset::TYPE_NAME || O::TYPE_NAME == SceneRegion::TYPE_NAME
    }
}
