//! In-memory reference implementation of [`GraphBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` backs this crate's integration tests and is a readable
//! example for anyone writing a real backend. All state lives in
//! `HashMap`s behind a `std::sync::Mutex`; nothing is persisted.
//!
//! # Caller context and rights
//!
//! `CallerCtx` is the caller's Principal id, which
//! [`JmapBackend::principal_id`] returns, so `createdBy` and `updatedBy` are
//! the caller. Each account is registered with an owner, who "implicitly
//! has all rights" on its Graphs and is the only user who may create one.
//! Every other user has the rights the Graph's `shareWith` gives their id,
//! or none. `isSubscribed` is kept per user: `true` for the owner and
//! `false` for everyone else until they set it (the draft leaves a
//! sharee's initial value server-defined).
//!
//! # Sharing
//!
//! There is no Principal store: `shareWith` ids are not checked, and no
//! ShareNotifications are created.
//!
//! # Destroy
//!
//! Destroying a Graph also destroys its nodes and edges, and destroying a
//! node its edges, each in the same locked step as the destroy.
//!
//! # Query
//!
//! `/query` evaluates the filter and sort with the [`crate::query`]
//! functions over the records the caller may read. `/queryChanges` is not
//! implemented: results say `canCalculateChanges: false` and
//! `/queryChanges` returns `cannotCalculateChanges` (RFC 8620 §5.6).
//!
//! # Concurrency
//!
//! The `await_holding_lock` lint is denied module-wide: no guard is held
//! across an `.await`.

#![deny(clippy::await_holding_lock)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use jmap_graph_types::query::{
    GraphComparator, GraphEdgeComparator, GraphEdgeFilterCondition, GraphFilter,
    GraphNodeComparator, GraphNodeFilterCondition,
};
use jmap_graph_types::{Graph, GraphCapability, GraphEdge, GraphNode, GraphRights};
use jmap_server::{apply_patch_object, resolve_query_start};
use jmap_types::{Id, State};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};

use crate::query::{
    compare_edges, compare_graphs, compare_nodes, edge_matches, graph_matches, node_matches,
};
use crate::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    GraphBackend, JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError,
    SetErrorType, SetObject,
};

/// How one id changed in one state step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Created,
    Updated,
    Destroyed,
}

type Key = (&'static str, String);

/// A registered account.
struct Account {
    owner: Id,
    capability: GraphCapability,
}

#[derive(Default)]
struct Inner {
    /// `(type_name, account_id)` → `id → stored object`.
    objects: HashMap<Key, HashMap<Id, Value>>,
    /// `(type_name, account_id)` → state counter.
    states: HashMap<Key, u64>,
    /// `(type_name, account_id)` → `(state after the change, id, change)`.
    log: HashMap<Key, Vec<(u64, Id, Change)>>,
    accounts: HashMap<String, Account>,
    /// `(graph id, user)` → that user's `isSubscribed`.
    subscriptions: HashMap<(Id, Id), bool>,
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

    fn get(&self, type_name: &'static str, account_id: &Id, id: &Id) -> Option<&Value> {
        self.objects(type_name, account_id).and_then(|m| m.get(id))
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

    fn is_owner(&self, caller: &Id, account_id: &Id) -> bool {
        self.accounts
            .get(account_id.as_ref())
            .is_some_and(|a| a.owner == *caller)
    }

    /// The caller's rights on Graph `graph_id`, or `None` if it is not
    /// stored: all rights for the account owner, otherwise
    /// `shareWith[caller]` or none.
    fn rights(&self, caller: &Id, account_id: &Id, graph_id: &Id) -> Option<GraphRights> {
        let graph = self.get(Graph::TYPE_NAME, account_id, graph_id)?;
        if self.is_owner(caller, account_id) {
            let mut all = GraphRights::default();
            all.may_read = true;
            all.may_write = true;
            all.may_admin = true;
            return Some(all);
        }
        Some(
            graph
                .get("shareWith")
                .and_then(|s| s.get(caller.as_ref()))
                .and_then(|r| GraphRights::deserialize(r).ok())
                .unwrap_or_default(),
        )
    }

    /// The stored Graph `v` as the caller sees it: its `myRights` and the
    /// caller's own `isSubscribed`.
    fn view(&self, caller: &Id, account_id: &Id, v: &Value) -> Value {
        let mut v = v.clone();
        let id = Id::from(v["id"].as_str().unwrap_or_default());
        let rights = self.rights(caller, account_id, &id).unwrap_or_default();
        v["myRights"] = json!(rights);
        v["isSubscribed"] = json!(self
            .subscriptions
            .get(&(id, caller.clone()))
            .copied()
            .unwrap_or_else(|| self.is_owner(caller, account_id)));
        v
    }

    /// The `mayWrite` check on the Graph of an item, under the lock:
    /// `missing` when the Graph does not exist or the caller lacks
    /// `mayRead`, `forbidden` when the caller lacks `mayWrite`.
    #[allow(clippy::result_large_err)]
    fn require_write(
        &self,
        caller: &Id,
        account_id: &Id,
        graph_id: &Id,
        missing: impl FnOnce() -> BackendSetError<MemoryError>,
    ) -> Result<(), BackendSetError<MemoryError>> {
        match self.rights(caller, account_id, graph_id) {
            Some(r) if r.may_write => Ok(()),
            Some(r) if r.may_read => Err(set_error(SetErrorType::Forbidden, "mayWrite required")),
            _ => Err(missing()),
        }
    }

    /// The properties of `which` in edge `v` that do not name a GraphNode
    /// of Graph `graph_id`.
    fn bad_endpoints(
        &self,
        account_id: &Id,
        graph_id: &Id,
        v: &Value,
        which: &[&'static str],
    ) -> Vec<&'static str> {
        which
            .iter()
            .copied()
            .filter(|k| {
                v.get(*k)
                    .and_then(Value::as_str)
                    .and_then(|id| self.get(GraphNode::TYPE_NAME, account_id, &Id::from(id)))
                    .and_then(|n| n.get("graphId"))
                    .and_then(Value::as_str)
                    != Some(graph_id.as_ref())
            })
            .collect()
    }

    /// Remove every stored object of `type_name` that `doomed` selects and
    /// log each as destroyed, in one state step.
    fn destroy_where(
        &mut self,
        type_name: &'static str,
        account_id: &Id,
        doomed: impl Fn(&Value) -> bool,
    ) {
        let map = self.objects_mut(type_name, account_id);
        let gone: Vec<Id> = map
            .iter()
            .filter(|(_, v)| doomed(v))
            .map(|(k, _)| k.clone())
            .collect();
        if gone.is_empty() {
            return;
        }
        for k in &gone {
            map.remove(k);
        }
        self.commit(
            type_name,
            account_id,
            gone.into_iter().map(|k| (k, Change::Destroyed)).collect(),
        );
    }

    /// Every stored object of type `O` that decodes, Graphs as the caller
    /// sees them.
    fn all<O: JmapObject>(&self, caller: &Id, account_id: &Id) -> Vec<O> {
        self.objects(O::TYPE_NAME, account_id)
            .into_iter()
            .flat_map(HashMap::values)
            .filter_map(|v| {
                if O::TYPE_NAME == Graph::TYPE_NAME {
                    O::deserialize(&self.view(caller, account_id, v)).ok()
                } else {
                    O::deserialize(v).ok()
                }
            })
            .collect()
    }

    /// `true` if the caller may read Graph `graph_id`.
    fn may_read(&self, caller: &Id, account_id: &Id, graph_id: &Id) -> bool {
        self.rights(caller, account_id, graph_id)
            .is_some_and(|r| r.may_read)
    }
}

/// A fully in-memory implementation of [`GraphBackend`].
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

    /// Register `account_id`, owned by Principal `owner`, with the Graph
    /// capability object it advertises.
    #[doc(hidden)]
    pub fn register_account(&self, account_id: &Id, owner: &Id, capability: GraphCapability) {
        self.lock().accounts.insert(
            account_id.as_ref().to_owned(),
            Account {
                owner: owner.clone(),
                capability,
            },
        );
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

fn set_error<E>(t: SetErrorType, description: &str) -> BackendSetError<E> {
    BackendSetError::SetError(SetError::new(t).with_description(description))
}

fn not_found<E>() -> BackendSetError<E> {
    BackendSetError::SetError(SetError::new(SetErrorType::NotFound))
}

fn invalid<E>(properties: &[&str], description: &str) -> BackendSetError<E> {
    BackendSetError::SetError(
        SetError::new(SetErrorType::InvalidProperties)
            .with_description(description)
            .with_properties(properties.iter().copied()),
    )
}

fn other<E: std::fmt::Display>(e: E) -> BackendSetError<MemoryError> {
    BackendSetError::Other(MemoryError::new(e.to_string()))
}

fn decode<O: JmapObject>(v: &Value) -> Result<O, MemoryError> {
    O::deserialize(v).map_err(|e| MemoryError::new(format!("decode {}: {e}", O::TYPE_NAME)))
}

/// Re-decode `v`, a value of a generic `QueryObject` associated type, as
/// the concrete type `T` it is once `O::TYPE_NAME` is known.
fn recode<T: DeserializeOwned>(v: &impl Serialize) -> Result<T, BackendQueryError<MemoryError>> {
    serde_json::to_value(v)
        .and_then(T::deserialize)
        .map_err(|e| BackendQueryError::Other(MemoryError::new(e.to_string())))
}

/// The ids of `list` that `keep` accepts, ordered by `cmp`.
fn sorted_ids<T>(
    list: Vec<T>,
    keep: impl Fn(&T) -> bool,
    cmp: impl Fn(&T, &T) -> std::cmp::Ordering,
    id: impl Fn(T) -> Id,
) -> Vec<Id> {
    let mut list: Vec<T> = list.into_iter().filter(|t| keep(t)).collect();
    list.sort_by(|a, b| cmp(a, b));
    list.into_iter().map(id).collect()
}

/// The `graphId` of stored item `v`.
fn graph_of(v: &Value) -> Id {
    Id::from(v.get("graphId").and_then(Value::as_str).unwrap_or_default())
}

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;
    type CallerCtx = Id;

    fn principal_id(caller: &Id) -> Option<&Id> {
        Some(caller)
    }

    async fn account_exists(&self, _caller: &Id, account_id: &Id) -> Result<bool, MemoryError> {
        Ok(self.lock().accounts.contains_key(account_id.as_ref()))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &Id,
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        let inner = self.lock();
        let shown = |v: &Value| {
            if O::TYPE_NAME == Graph::TYPE_NAME {
                decode(&inner.view(caller, account_id, v))
            } else {
                decode(v)
            }
        };
        let map = inner.objects(O::TYPE_NAME, account_id);
        match ids {
            None => {
                let list = map
                    .into_iter()
                    .flat_map(HashMap::values)
                    .map(shown)
                    .collect::<Result<_, _>>()?;
                Ok((list, Vec::new()))
            }
            Some(ids) => {
                let mut found = Vec::new();
                let mut missing = Vec::new();
                for id in ids {
                    match map.and_then(|m| m.get(id)) {
                        Some(v) => found.push(shown(v)?),
                        None => missing.push(id.clone()),
                    }
                }
                Ok((found, missing))
            }
        }
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &Id,
        account_id: &Id,
    ) -> Result<State, MemoryError> {
        Ok(State::from(
            self.lock().state(O::TYPE_NAME, account_id).to_string(),
        ))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &Id,
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
        caller: &Id,
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
        let ids = if O::TYPE_NAME == Graph::TYPE_NAME {
            let filter: Option<GraphFilter> = filter.map(recode).transpose()?;
            let sort: Vec<GraphComparator> = recode(&sort)?;
            sorted_ids(
                inner.all::<Graph>(caller, account_id),
                |g| g.my_rights.may_read && filter.as_ref().is_none_or(|f| graph_matches(f, g)),
                |a, b| compare_graphs(a, b, &sort),
                |g| g.id,
            )
        } else if O::TYPE_NAME == GraphNode::TYPE_NAME {
            let filter: Option<GraphNodeFilterCondition> = filter.map(recode).transpose()?;
            let sort: Vec<GraphNodeComparator> = recode(&sort)?;
            sorted_ids(
                inner.all::<GraphNode>(caller, account_id),
                |n| {
                    filter.as_ref().is_some_and(|f| node_matches(f, n))
                        && inner.may_read(caller, account_id, &n.graph_id)
                },
                |a, b| compare_nodes(a, b, &sort),
                |n| n.id,
            )
        } else if O::TYPE_NAME == GraphEdge::TYPE_NAME {
            let filter: Option<GraphEdgeFilterCondition> = filter.map(recode).transpose()?;
            let sort: Vec<GraphEdgeComparator> = recode(&sort)?;
            sorted_ids(
                inner.all::<GraphEdge>(caller, account_id),
                |e| {
                    filter.as_ref().is_some_and(|f| edge_matches(f, e))
                        && inner.may_read(caller, account_id, &e.graph_id)
                },
                |a, b| compare_edges(a, b, &sort),
                |e| e.id,
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
        _caller: &Id,
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

impl GraphBackend for MemoryBackend {
    async fn graph_capability(
        &self,
        _caller: &Id,
        account_id: &Id,
    ) -> Result<GraphCapability, MemoryError> {
        self.lock()
            .accounts
            .get(account_id.as_ref())
            .map(|a| a.capability.clone())
            .ok_or_else(|| MemoryError::new("account not registered"))
    }

    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Id,
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<MemoryError>> {
        let mut value = serde_json::to_value(&obj).map_err(other)?;
        let mut inner = self.lock();
        let id = Id::from(ulid::Ulid::new().to_string());
        value["id"] = json!(id);
        if O::TYPE_NAME == Graph::TYPE_NAME {
            if !inner.is_owner(caller, account_id) {
                return Err(set_error(
                    SetErrorType::Forbidden,
                    "only the account owner may create a Graph",
                ));
            }
            let subscribed = value["isSubscribed"].as_bool().unwrap_or(true);
            inner
                .subscriptions
                .insert((id.clone(), caller.clone()), subscribed);
        } else {
            // The rights and the edge endpoints, re-checked under the lock.
            let graph_id = graph_of(&value);
            inner.require_write(caller, account_id, &graph_id, || {
                invalid(
                    &["graphId"],
                    "graphId does not identify a Graph the user may read",
                )
            })?;
            if O::TYPE_NAME == GraphEdge::TYPE_NAME {
                let bad = inner.bad_endpoints(account_id, &graph_id, &value, &["fromId", "toId"]);
                if !bad.is_empty() {
                    return Err(invalid(&bad, "not a GraphNode of the edge's Graph"));
                }
            }
        }
        inner
            .objects_mut(O::TYPE_NAME, account_id)
            .insert(id.clone(), value.clone());
        inner.commit(
            O::TYPE_NAME,
            account_id,
            vec![(id.clone(), Change::Created)],
        );
        let shown = if O::TYPE_NAME == Graph::TYPE_NAME {
            inner.view(caller, account_id, &value)
        } else {
            value
        };
        Ok((id, decode(&shown).map_err(BackendSetError::Other)?))
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Id,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let mut current = inner
            .get(O::TYPE_NAME, account_id, id)
            .cloned()
            .ok_or_else(not_found)?;
        let Value::Object(mut patch) = serde_json::to_value(&patch).map_err(other)? else {
            return Err(set_error(SetErrorType::InvalidPatch, "not a PatchObject"));
        };
        let top = |k: &String| k.split('/').next().unwrap_or(k).to_owned();
        let keys: Vec<String> = patch.keys().map(top).collect();

        let mut subscribed = None;
        if O::TYPE_NAME == Graph::TYPE_NAME {
            // §Graph/set rights, re-checked under the lock.
            let r = inner
                .rights(caller, account_id, id)
                .filter(|r| r.may_read)
                .ok_or_else(not_found)?;
            if keys.iter().any(|k| k == "shareWith") && !r.may_admin {
                return Err(set_error(SetErrorType::Forbidden, "mayAdmin required"));
            }
            if keys
                .iter()
                .any(|k| !["isSubscribed", "shareWith", "updated"].contains(&k.as_str()))
                && !r.may_write
            {
                return Err(set_error(SetErrorType::Forbidden, "mayWrite required"));
            }
            // `isSubscribed` is the caller's own; `null` resets it.
            subscribed = patch.remove("isSubscribed");
        } else {
            inner.require_write(caller, account_id, &graph_of(&current), not_found)?;
        }

        let Value::Object(base) = &mut current else {
            return Err(BackendSetError::Other(MemoryError::new(
                "stored object is not a JSON object",
            )));
        };
        apply_patch_object(base, &patch)
            .map_err(|e| set_error(SetErrorType::InvalidPatch, &e.to_string()))?;
        if O::deserialize(&current).is_err() {
            return Err(set_error(
                SetErrorType::InvalidProperties,
                &format!("patched {} is invalid", O::TYPE_NAME),
            ));
        }
        if O::TYPE_NAME == GraphEdge::TYPE_NAME {
            let moved: Vec<&'static str> = ["fromId", "toId"]
                .into_iter()
                .filter(|k| keys.iter().any(|p| p == k))
                .collect();
            let bad = inner.bad_endpoints(account_id, &graph_of(&current), &current, &moved);
            if !bad.is_empty() {
                return Err(invalid(&bad, "not a GraphNode of the edge's Graph"));
            }
        }
        match subscribed {
            Some(Value::Bool(b)) => {
                inner.subscriptions.insert((id.clone(), caller.clone()), b);
            }
            Some(_) => {
                inner.subscriptions.remove(&(id.clone(), caller.clone()));
            }
            None => {}
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
        caller: &Id,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<MemoryError>> {
        let mut inner = self.lock();
        let current = inner
            .get(O::TYPE_NAME, account_id, id)
            .cloned()
            .ok_or_else(not_found)?;
        if O::TYPE_NAME == Graph::TYPE_NAME {
            let r = inner
                .rights(caller, account_id, id)
                .filter(|r| r.may_read)
                .ok_or_else(not_found)?;
            if !r.may_admin {
                return Err(set_error(SetErrorType::Forbidden, "mayAdmin required"));
            }
        } else {
            inner.require_write(caller, account_id, &graph_of(&current), not_found)?;
        }
        inner.destroy_where(O::TYPE_NAME, account_id, |v| v["id"] == json!(id));
        let id_str = Some(id.as_ref());
        if O::TYPE_NAME == Graph::TYPE_NAME {
            // "the server MUST also destroy every GraphNode and GraphEdge
            // in it".
            for contained in [GraphNode::TYPE_NAME, GraphEdge::TYPE_NAME] {
                inner.destroy_where(contained, account_id, |v| {
                    v.get("graphId").and_then(Value::as_str) == id_str
                });
            }
            inner.subscriptions.retain(|(g, _), _| g != id);
        } else if O::TYPE_NAME == GraphNode::TYPE_NAME {
            // "the server MUST also destroy every GraphEdge whose `fromId`
            // or `toId` is that node".
            inner.destroy_where(GraphEdge::TYPE_NAME, account_id, |v| {
                ["fromId", "toId"]
                    .iter()
                    .any(|k| v.get(*k).and_then(Value::as_str) == id_str)
            });
        }
        Ok(())
    }

    fn supports_type<O: JmapObject>(&self, _caller: &Id, _account_id: &Id) -> bool {
        [Graph::TYPE_NAME, GraphNode::TYPE_NAME, GraphEdge::TYPE_NAME].contains(&O::TYPE_NAME)
    }
}
