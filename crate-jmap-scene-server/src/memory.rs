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
//! owner only. [`SceneBackend::may_modify_object`] allows the object's
//! owner, its region's owner and the admins; a created object's `ownerId`
//! is the caller.
//!
//! # Avatars
//!
//! [`SceneBackend::caller_user_id`] is the caller.
//! [`SceneBackend::may_enter_region`] refuses an `"invite"` region to
//! everyone but its owner, the admins and the users given access with
//! `grant_region_access`; other policies admit everyone (there are no
//! Spaces here). [`SceneBackend::may_eject_avatar`] allows the avatar's
//! region owner and the admins. Each region's `activeAvatarCount` is
//! recomputed from its avatars with `leftAt` `null` whenever an avatar
//! enters or leaves.
//!
//! # Visibility
//!
//! [`SceneBackend::may_access_region`] applies the `"invite"` rule of
//! [`SceneBackend::may_enter_region`] and admits everyone to a region that
//! is not stored. `hide_object` takes an object out of one user's
//! visibility set, for [`SceneBackend::is_object_visible`]; `block` makes
//! one user blocked by another, for [`SceneBackend::is_user_blocked`].
//! `reserve_display_name` adds a name to the list
//! [`SceneBackend::reserved_display_names`] returns for every account.
//! `/query` omits the regions, objects and avatars those hooks hide from
//! the caller; `activeAvatarCount` stays the true count.
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
//! exceed it, a `SceneRegion` beyond the capability's
//! `maxRegionsPerAccount`, and a `SceneObject` beyond its
//! `maxObjectsPerRegion`, with `overQuota`.
//!
//! # Region destroy
//!
//! Destroying a `SceneRegion` also destroys the stored `SceneObject` and
//! `SceneAvatar` records whose `regionId` names it. Destroying a
//! `SceneObject` also destroys its descendants, in the same state step.
//!
//! # Query
//!
//! `SceneAsset/query`, `SceneRegion/query`, `SceneObject/query` and
//! `SceneAvatar/query` evaluate the filter and sort
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
    SceneAssetComparator, SceneAssetFilter, SceneAvatarComparator, SceneAvatarFilter,
    SceneObjectComparator, SceneObjectFilter, SceneRegionComparator, SceneRegionFilter,
};
use jmap_scene_types::{
    AccessPolicy, SceneAsset, SceneAvatar, SceneCapability, SceneObject, SceneRegion,
};
use jmap_server::{apply_patch_object, resolve_query_start};
use jmap_types::{Id, State, UTCDate};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::query::{
    asset_matches, avatar_matches, compare_assets, compare_avatars, compare_objects,
    compare_regions, object_matches, region_local_position, region_matches,
};
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
    /// `(region id, userId)` pairs granted access to an `"invite"` region.
    grants: HashSet<(String, String)>,
    /// `(userId, object id)` pairs outside that user's visibility set.
    hidden: HashSet<(String, Id)>,
    /// `(blocking userId, blocked userId)` pairs.
    blocked: HashSet<(String, String)>,
    /// Reserved avatar display names.
    reserved_names: Vec<String>,
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

    /// Set each of `regions`' `activeAvatarCount` to its number of avatars
    /// with `leftAt` `null`, and advance the SceneRegion state once if any
    /// count changed.
    fn recount(&mut self, account_id: &Id, regions: &[Id]) {
        let mut changed = Vec::new();
        for region in regions {
            let active = self
                .all::<SceneAvatar>(account_id)
                .filter(|a| a.region_id == *region && a.left_at.is_none())
                .count();
            let Some(stored) = self
                .objects_mut(SceneRegion::TYPE_NAME, account_id)
                .get_mut(region)
            else {
                continue;
            };
            if stored.get("activeAvatarCount") != Some(&json!(active)) {
                stored["activeAvatarCount"] = json!(active);
                changed.push((region.clone(), Change::Updated));
            }
        }
        if !changed.is_empty() {
            self.commit(SceneRegion::TYPE_NAME, account_id, changed);
        }
    }

    /// §Entering a Region for `avatar`, under the lock: the steps of
    /// [`SceneBackend::create_object`] for a `SceneAvatar`. Returns the
    /// trait's error type unboxed, as `create_object` does.
    #[allow(clippy::result_large_err)]
    fn enter(
        &mut self,
        account_id: &Id,
        avatar: SceneAvatar,
    ) -> Result<Value, BackendSetError<MemoryError>> {
        let mine: Vec<SceneAvatar> = self
            .all::<SceneAvatar>(account_id)
            .filter(|a| a.user_id == avatar.user_id)
            .collect();
        let here = mine.iter().find(|a| a.region_id == avatar.region_id);
        if let Some(current) = here.filter(|a| a.left_at.is_none()) {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::AlreadyExists)
                    .with_description("the user already has an active avatar in this region")
                    .with_existing_id(current.id.clone()),
            ));
        }
        let max = self
            .accounts
            .get(account_id.as_ref())
            .and_then(|c| c.max_avatars_per_region);
        let present = self
            .all::<SceneAvatar>(account_id)
            .filter(|a| a.region_id == avatar.region_id && a.left_at.is_none())
            .count() as u64;
        if max.is_some_and(|max| present >= max) {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::OverQuota)
                    .with_description("maxAvatarsPerRegion reached"),
            ));
        }

        let mut changes = Vec::new();
        let mut regions = vec![avatar.region_id.clone()];
        let avatars = self.objects_mut(SceneAvatar::TYPE_NAME, account_id);
        // "the server MUST set `leftAt` on the previous avatar before
        // creating the new one".
        for previous in mine.iter().filter(|a| a.left_at.is_none()) {
            if let Some(v) = avatars.get_mut(&previous.id) {
                v["leftAt"] = json!(avatar.joined_at);
                changes.push((previous.id.clone(), Change::Updated));
                regions.push(previous.region_id.clone());
            }
        }
        // §Reconnecting: "update the existing SceneAvatar record rather
        // than creating a new one".
        let value = match here {
            Some(departed) => {
                let v = avatars.get_mut(&departed.id).ok_or_else(not_found)?;
                v["leftAt"] = Value::Null;
                v["joinedAt"] = json!(avatar.joined_at);
                v["position"] = json!(avatar.position);
                changes.push((departed.id.clone(), Change::Updated));
                v.clone()
            }
            None => {
                let id = Id::from(ulid::Ulid::new().to_string());
                let mut v = serde_json::to_value(&avatar)
                    .map_err(|e| BackendSetError::Other(MemoryError::new(e.to_string())))?;
                v["id"] = json!(id);
                avatars.insert(id.clone(), v.clone());
                changes.push((id, Change::Created));
                v
            }
        };
        self.commit(SceneAvatar::TYPE_NAME, account_id, changes);
        self.recount(account_id, &regions);
        Ok(value)
    }

    /// `true` if `caller` is an admin, the owner of `region_id` or granted
    /// access to it: "Only users explicitly granted access may enter."
    fn invited(&self, caller: &str, region_id: &Id) -> bool {
        self.admins.contains(caller)
            || self.owners.get(region_id).is_some_and(|o| o == caller)
            || self
                .grants
                .contains(&(region_id.as_ref().to_owned(), caller.to_owned()))
    }

    /// [`SceneBackend::may_access_region`]: the `"invite"` rule; a region
    /// that is not stored admits everyone.
    fn may_access(&self, caller: &str, account_id: &Id, region_id: &Id) -> bool {
        self.objects(SceneRegion::TYPE_NAME, account_id)
            .and_then(|m| m.get(region_id))
            .and_then(|v| decode::<SceneRegion>(v).ok())
            .is_none_or(|r| {
                r.access_policy != AccessPolicy::Invite || self.invited(caller, region_id)
            })
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

    /// Give `user` access to the `"invite"` region `region_id`.
    #[doc(hidden)]
    pub fn grant_region_access(&self, region_id: &str, user: &str) {
        self.lock()
            .grants
            .insert((region_id.to_owned(), user.to_owned()));
    }

    /// Take object `object_id` out of `user`'s visibility set, for
    /// [`SceneBackend::is_object_visible`].
    #[doc(hidden)]
    pub fn hide_object(&self, user: &str, object_id: &str) {
        self.lock()
            .hidden
            .insert((user.to_owned(), Id::from(object_id)));
    }

    /// Make `user` blocked by `blocker`, for
    /// [`SceneBackend::is_user_blocked`].
    #[doc(hidden)]
    pub fn block(&self, blocker: &str, user: &str) {
        self.lock()
            .blocked
            .insert((blocker.to_owned(), user.to_owned()));
    }

    /// Reserve avatar display name `name`, for
    /// [`SceneBackend::reserved_display_names`].
    #[doc(hidden)]
    pub fn reserve_display_name(&self, name: &str) {
        self.lock().reserved_names.push(name.to_owned());
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
        caller: &String,
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
                |r| {
                    inner.may_access(caller, account_id, &r.id)
                        && filter.as_ref().is_none_or(|f| region_matches(f, r))
                },
                |a, b| compare_regions(a, b, &sort),
                |r| r.id,
            )
        } else if O::TYPE_NAME == SceneAvatar::TYPE_NAME {
            let filter: Option<SceneAvatarFilter> = filter.map(recode).transpose()?;
            let sort: Vec<SceneAvatarComparator> = recode(&sort)?;
            sorted_ids(
                inner.all::<SceneAvatar>(account_id),
                |a| {
                    inner.may_access(caller, account_id, &a.region_id)
                        && !inner.blocked.contains(&(caller.clone(), a.user_id.clone()))
                        && filter.as_ref().is_none_or(|f| avatar_matches(f, a))
                },
                |a, b| compare_avatars(a, b, &sort),
                |a| a.id,
            )
        } else if O::TYPE_NAME == SceneObject::TYPE_NAME {
            let filter: Option<SceneObjectFilter> = filter.map(recode).transpose()?;
            let sort: Vec<SceneObjectComparator> = recode(&sort)?;
            let all: HashMap<Id, SceneObject> = inner
                .all::<SceneObject>(account_id)
                .map(|o| (o.id.clone(), o))
                .collect();
            sorted_ids(
                all.values(),
                |o| {
                    inner.may_access(caller, account_id, &o.region_id)
                        && !inner.hidden.contains(&(caller.clone(), o.id.clone()))
                        && filter.as_ref().is_none_or(|f| {
                            object_matches(f, o, region_local_position(o, |id| all.get(id)))
                        })
                },
                |a, b| compare_objects(a, b, &sort),
                |o| o.id.clone(),
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
        if O::TYPE_NAME == SceneAvatar::TYPE_NAME {
            let avatar: SceneAvatar = decode(&value).map_err(BackendSetError::Other)?;
            let stored = inner.enter(account_id, avatar)?;
            let id = Id::from(stored["id"].as_str().unwrap_or_default());
            return Ok((id, decode(&stored).map_err(BackendSetError::Other)?));
        }
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
        // "`overQuota` when `maxObjectsPerRegion` would be exceeded",
        // counted under the lock; "The userId of the user who created this
        // object".
        if O::TYPE_NAME == SceneObject::TYPE_NAME {
            let max = inner
                .accounts
                .get(account_id.as_ref())
                .and_then(|c| c.max_objects_per_region);
            let region = value.get("regionId").and_then(Value::as_str);
            let held = inner
                .all::<SceneObject>(account_id)
                .filter(|o| Some(o.region_id.as_ref()) == region)
                .count() as u64;
            if max.is_some_and(|max| held >= max) {
                return Err(BackendSetError::SetError(
                    SetError::new(SetErrorType::OverQuota)
                        .with_description("maxObjectsPerRegion reached"),
                ));
            }
            value["ownerId"] = json!(caller);
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
        // "double-leave is invalid", re-checked under the lock.
        let leaving = O::TYPE_NAME == SceneAvatar::TYPE_NAME && patch.contains_key("leftAt");
        if leaving {
            let stored = inner
                .objects(O::TYPE_NAME, account_id)
                .and_then(|m| m.get(id))
                .and_then(|v| v.get("leftAt"))
                .and_then(|l| serde_json::from_value::<Option<UTCDate>>(l.clone()).ok())
                .flatten();
            if stored.is_some() {
                return Err(BackendSetError::SetError(
                    SetError::new(SetErrorType::InvalidProperties)
                        .with_description("the avatar has already left")
                        .with_properties(["leftAt"]),
                ));
            }
        }
        let region = current
            .get("regionId")
            .and_then(Value::as_str)
            .map(Id::from);
        inner
            .objects_mut(O::TYPE_NAME, account_id)
            .insert(id.clone(), current);
        inner.commit(
            O::TYPE_NAME,
            account_id,
            vec![(id.clone(), Change::Updated)],
        );
        if let Some(region) = region.filter(|_| leaving) {
            inner.recount(account_id, &[region]);
        }
        Ok(None)
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &String,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let map = inner.objects_mut(O::TYPE_NAME, account_id);
        if !map.contains_key(id) {
            return Err(not_found());
        }
        // "removes the SceneObject and all its children (objects whose
        // `parentId` references this object, recursively)".
        let mut gone = vec![id.clone()];
        if O::TYPE_NAME == SceneObject::TYPE_NAME {
            let mut i = 0;
            while let Some(parent) = gone.get(i).cloned() {
                let children: Vec<Id> = map
                    .iter()
                    .filter(|(k, v)| {
                        v.get("parentId").and_then(Value::as_str) == Some(parent.as_ref())
                            && !gone.contains(k)
                    })
                    .map(|(k, _)| k.clone())
                    .collect();
                gone.extend(children);
                i += 1;
            }
        }
        for k in &gone {
            map.remove(k);
        }
        for k in &gone {
            inner.owners.remove(k);
        }
        inner.commit(
            O::TYPE_NAME,
            account_id,
            gone.into_iter().map(|k| (k, Change::Destroyed)).collect(),
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

    async fn may_modify_object(
        &self,
        caller: &String,
        _account_id: &Id,
        object: &SceneObject,
    ) -> Result<bool, MemoryError> {
        // "Only the object's `ownerId`, the region owner, or a
        // deployment-admin may modify another user's object."
        let inner = self.lock();
        Ok(inner.admins.contains(caller)
            || object.owner_id.as_ref() == Some(caller)
            || inner.owners.get(&object.region_id) == Some(caller))
    }

    async fn caller_user_id(
        &self,
        caller: &String,
        _account_id: &Id,
    ) -> Result<String, MemoryError> {
        Ok(caller.clone())
    }

    async fn may_enter_region(
        &self,
        caller: &String,
        _account_id: &Id,
        region: &SceneRegion,
    ) -> Result<bool, MemoryError> {
        // "Only users explicitly granted access may enter."
        Ok(region.access_policy != AccessPolicy::Invite || self.lock().invited(caller, &region.id))
    }

    async fn may_access_region(
        &self,
        caller: &String,
        account_id: &Id,
        region_id: &Id,
    ) -> Result<bool, MemoryError> {
        Ok(self.lock().may_access(caller, account_id, region_id))
    }

    async fn is_object_visible(
        &self,
        caller: &String,
        _account_id: &Id,
        object: &SceneObject,
    ) -> Result<bool, MemoryError> {
        Ok(!self
            .lock()
            .hidden
            .contains(&(caller.clone(), object.id.clone())))
    }

    async fn is_user_blocked(
        &self,
        caller: &String,
        _account_id: &Id,
        user_id: &str,
    ) -> Result<bool, MemoryError> {
        Ok(self
            .lock()
            .blocked
            .contains(&(caller.clone(), user_id.to_owned())))
    }

    async fn reserved_display_names(
        &self,
        _caller: &String,
        _account_id: &Id,
    ) -> Result<Vec<String>, MemoryError> {
        Ok(self.lock().reserved_names.clone())
    }

    async fn may_eject_avatar(
        &self,
        caller: &String,
        _account_id: &Id,
        avatar: &SceneAvatar,
    ) -> Result<bool, MemoryError> {
        // "Region owners and administrators MAY eject avatars".
        let inner = self.lock();
        Ok(inner.admins.contains(caller) || inner.owners.get(&avatar.region_id) == Some(caller))
    }

    fn supports_type<O: JmapObject>(&self, _caller: &String, _account_id: &Id) -> bool {
        [
            SceneAsset::TYPE_NAME,
            SceneRegion::TYPE_NAME,
            SceneObject::TYPE_NAME,
            SceneAvatar::TYPE_NAME,
        ]
        .contains(&O::TYPE_NAME)
    }
}
