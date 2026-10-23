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
//! Destroying a Graph also destroys its nodes, edges and ops, and
//! destroying a node its edges and ops, each in the same locked step as the
//! destroy.
//!
//! # Live editing
//!
//! `register_edit` advertises `urn:ietf:params:jmap:graph:edit` for an
//! account. A GraphNodeOp create is sequenced under the lock: the next
//! `seq` of its node, after re-checking `mayWrite`, the node's
//! `editFormat` and `baseSeq`. A GraphNode update re-checks the
//! §GraphNode Extensions rules under the lock. `compact_ops` is the
//! compaction policy: front-only, never past `snapshotSeq`, never an op
//! younger than `opRetention`. `set_projected_graph` marks an account's
//! projected Graph. Cursor events are recorded, read back with
//! `published_cursors`; `block` makes one Principal blocked by another,
//! for [`GraphBackend::is_sender_blocked`], and `advertise` records a
//! companion capability for [`GraphBackend::has_capability`]. No op events
//! are recorded:
//! there is no WebSocket transport.
//!
//! # Query
//!
//! `/query` evaluates the filter and sort with the [`crate::query`]
//! functions over the records the caller may read. `/queryChanges` is not
//! implemented: results say `canCalculateChanges: false` and
//! `/queryChanges` returns `cannotCalculateChanges` (RFC 8620 §5.6).
//!
//! `register_search` advertises `urn:ietf:params:jmap:graph:search` for an
//! account. A GraphNode or GraphEdge `where` is evaluated with a
//! [`crate::search::GraphView`] of the filter's Graph, which holds every
//! node and edge of a Graph the caller may read: the projected Graph's
//! contents are not narrowed per user. No language is implemented, so an
//! `expression` is `unsupportedFilter` even when `languages` lists it.
//!
//! # Concurrency
//!
//! The `await_holding_lock` lint is denied module-wide: no guard is held
//! across an `.await`.

#![deny(clippy::await_holding_lock)]

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use jmap_graph_types::{CandidateKind, Expression, GraphSearchCapability};
use jmap_graph_types::{
    Graph, GraphCapability, GraphEdge, GraphEditCapability, GraphNode, GraphNodeOp, GraphRights,
};
use jmap_graph_types::{
    GraphComparator, GraphEdgeComparator, GraphEdgeFilterCondition, GraphFilter,
    GraphNodeComparator, GraphNodeFilterCondition,
};
use jmap_graph_types::{GraphCursorEvent, GraphNodeOpFilterCondition};
use jmap_server::{apply_patch_object, limit_or_max, resolve_query_start};
use jmap_types::{Id, JmapError, State};
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::edit::check_node_edit;
use crate::query::{
    compare_edges, compare_graphs, compare_nodes, edge_matches, graph_matches, node_matches,
};
use crate::search::GraphView;
use crate::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, EditLog, GetObject,
    GraphBackend, JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject,
    QueryParams, QueryResult, SetError, SetErrorType, SetObject,
};

/// How one id changed in one state step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Created,
    Updated,
    Destroyed,
}

/// `type_name` → `account_id` → `V`. Nested rather than tuple-keyed so a
/// lookup borrows the `&str` account id instead of allocating a `String`.
type Keyed<V> = HashMap<&'static str, HashMap<String, V>>;

/// A registered account.
struct Account {
    owner: Id,
    capability: GraphCapability,
    edit: Option<GraphEditCapability>,
    search: Option<GraphSearchCapability>,
    projected: Option<Id>,
}

/// One node's edit log: its head, how far it is compacted, and the
/// `(seq, op id, sequenced at)` of each op still held, oldest first.
#[derive(Default)]
struct OpLog {
    head: u64,
    compacted_through: u64,
    held: VecDeque<(u64, Id, SystemTime)>,
}

#[derive(Default)]
struct Inner {
    /// `type_name` → `account_id` → `id → stored object`.
    objects: Keyed<HashMap<Id, Value>>,
    /// `type_name` → `account_id` → state counter.
    states: Keyed<u64>,
    /// `type_name` → `account_id` → `(state after the change, id, change)`.
    log: Keyed<Vec<(u64, Id, Change)>>,
    accounts: HashMap<String, Account>,
    /// `(graph id, user)` → that user's `isSubscribed`.
    subscriptions: HashMap<(Id, Id), bool>,
    /// `(account id, node id)` → the node's edit log.
    op_logs: HashMap<(String, Id), OpLog>,
    /// Cursor events [`GraphBackend::publish_cursor`] received, in order.
    cursors: Vec<GraphCursorEvent>,
    /// `(blocking Principal, blocked Principal)` pairs.
    blocked: HashSet<(Id, Id)>,
    /// `(account id, capability URI)` pairs advertised as companions.
    advertised: HashSet<(Id, String)>,
}

impl Inner {
    fn objects(&self, type_name: &'static str, account_id: &Id) -> Option<&HashMap<Id, Value>> {
        self.objects
            .get(type_name)
            .and_then(|m| m.get(account_id.as_ref()))
    }

    fn objects_mut(&mut self, type_name: &'static str, account_id: &Id) -> &mut HashMap<Id, Value> {
        self.objects
            .entry(type_name)
            .or_default()
            .entry(account_id.as_ref().to_owned())
            .or_default()
    }

    fn get(&self, type_name: &'static str, account_id: &Id, id: &Id) -> Option<&Value> {
        self.objects(type_name, account_id).and_then(|m| m.get(id))
    }

    fn state(&self, type_name: &'static str, account_id: &Id) -> u64 {
        self.states
            .get(type_name)
            .and_then(|m| m.get(account_id.as_ref()))
            .copied()
            .unwrap_or(0)
    }

    /// Advance the state of `type_name` once and log `changes` under it.
    fn commit(&mut self, type_name: &'static str, account_id: &Id, changes: Vec<(Id, Change)>) {
        let state = self
            .states
            .entry(type_name)
            .or_default()
            .entry(account_id.as_ref().to_owned())
            .or_insert(0);
        *state += 1;
        let state = *state;
        self.log
            .entry(type_name)
            .or_default()
            .entry(account_id.as_ref().to_owned())
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
        v["isSubscribed"] = json!(
            self.subscriptions
                .get(&(id, caller.clone()))
                .copied()
                .unwrap_or_else(|| self.is_owner(caller, account_id))
        );
        v
    }

    /// The `mayWrite` check on the Graph of an item, under the lock:
    /// `missing` when the Graph does not exist or the caller lacks
    /// `mayRead`, `forbidden` when the caller lacks `mayWrite`.
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

    fn edit(&self, account_id: &Id) -> Option<&GraphEditCapability> {
        self.accounts
            .get(account_id.as_ref())
            .and_then(|a| a.edit.as_ref())
    }

    fn log(&self, account_id: &Id, node_id: &Id) -> EditLog {
        self.op_logs
            .get(&(account_id.as_ref().to_owned(), node_id.clone()))
            .map_or_else(EditLog::default, |l| {
                EditLog::new(l.head, l.compacted_through)
            })
    }

    /// Drop the edit logs of nodes no longer stored, after a cascade.
    fn forget_logs(&mut self, account_id: &Id) {
        let nodes: HashSet<Id> = self
            .objects(GraphNode::TYPE_NAME, account_id)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        self.op_logs
            .retain(|(a, n), _| a != account_id.as_ref() || nodes.contains(n));
    }

    /// Every node and edge of Graph `graph_id`, or none if the caller may
    /// not read it.
    fn graph_content(
        &self,
        caller: &Id,
        account_id: &Id,
        graph_id: &Id,
    ) -> (Vec<GraphNode>, Vec<GraphEdge>) {
        if !self.may_read(caller, account_id, graph_id) {
            return (Vec::new(), Vec::new());
        }
        let mut nodes = self.all::<GraphNode>(caller, account_id);
        nodes.retain(|n| n.graph_id == *graph_id);
        let mut edges = self.all::<GraphEdge>(caller, account_id);
        edges.retain(|e| e.graph_id == *graph_id);
        (nodes, edges)
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
#[non_exhaustive]
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
                edit: None,
                search: None,
                projected: None,
            },
        );
    }

    /// Advertise `urn:ietf:params:jmap:graph:edit` for the registered
    /// `account_id` with `capability`.
    #[doc(hidden)]
    pub fn register_edit(&self, account_id: &Id, capability: GraphEditCapability) {
        if let Some(a) = self.lock().accounts.get_mut(account_id.as_ref()) {
            a.edit = Some(capability);
        }
    }

    /// Advertise `urn:ietf:params:jmap:graph:search` for the registered
    /// `account_id` with `capability`.
    #[doc(hidden)]
    pub fn register_search(&self, account_id: &Id, capability: GraphSearchCapability) {
        if let Some(a) = self.lock().accounts.get_mut(account_id.as_ref()) {
            a.search = Some(capability);
        }
    }

    /// Mark `graph_id` as the registered `account_id`'s projected Graph.
    #[doc(hidden)]
    pub fn set_projected_graph(&self, account_id: &Id, graph_id: &Id) {
        if let Some(a) = self.lock().accounts.get_mut(account_id.as_ref()) {
            a.projected = Some(graph_id.clone());
        }
    }

    /// Advertise the companion capability `uri` (e.g.
    /// `urn:ietf:params:jmap:chat`) on `account_id`.
    #[doc(hidden)]
    pub fn advertise(&self, account_id: &Id, uri: &str) {
        self.lock()
            .advertised
            .insert((account_id.clone(), uri.to_owned()));
    }

    /// Make Principal `blocked` blocked by Principal `blocker`.
    #[doc(hidden)]
    pub fn block(&self, blocker: &Id, blocked: &Id) {
        self.lock()
            .blocked
            .insert((blocker.clone(), blocked.clone()));
    }

    /// The cursor events [`GraphBackend::publish_cursor`] received, in
    /// order.
    #[doc(hidden)]
    pub fn published_cursors(&self) -> Vec<GraphCursorEvent> {
        self.lock().cursors.clone()
    }

    /// Compact node `node_id`'s edit log through `through`
    /// (draft-atwood-jmap-graph-edit-00 §Compaction and History) and return
    /// the highest `seq` now compacted.
    ///
    /// Destroys held ops from the front only, none past the node's
    /// `snapshotSeq` ("The server MUST NOT destroy an op whose `seq` is
    /// greater than `snapshotSeq`") and none sequenced less than
    /// `opRetention` seconds ago, each a destroyed GraphNodeOp change.
    #[doc(hidden)]
    pub fn compact_ops(&self, account_id: &Id, node_id: &Id, through: u64) -> u64 {
        let mut inner = self.lock();
        let snapshot = inner
            .get(GraphNode::TYPE_NAME, account_id, node_id)
            .and_then(|n| n.get("snapshotSeq"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let retention = inner
            .edit(account_id)
            .and_then(|c| c.op_retention)
            .map(Duration::from_secs);
        let limit = through.min(snapshot);
        let now = SystemTime::now();
        let key = (account_id.as_ref().to_owned(), node_id.clone());
        let Some(log) = inner.op_logs.get_mut(&key) else {
            return 0;
        };
        let mut gone = Vec::new();
        while let Some((seq, id, at)) = log.held.front() {
            let young = retention.is_some_and(|r| now.duration_since(*at).unwrap_or_default() < r);
            if *seq > limit || young {
                break;
            }
            log.compacted_through = *seq;
            gone.push((id.clone(), Change::Destroyed));
            log.held.pop_front();
        }
        let compacted = log.compacted_through;
        if !gone.is_empty() {
            let ops = inner.objects_mut(GraphNodeOp::TYPE_NAME, account_id);
            for (id, _) in &gone {
                ops.remove(id);
            }
            inner.commit(GraphNodeOp::TYPE_NAME, account_id, gone);
        }
        compacted
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

fn set_error<E>(t: SetErrorType, description: &str) -> BackendSetError<E> {
    BackendSetError::SetError(Box::new(SetError::new(t).with_description(description)))
}

fn not_found<E>() -> BackendSetError<E> {
    BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
}

fn invalid<E>(properties: &[&str], description: &str) -> BackendSetError<E> {
    BackendSetError::SetError(Box::new(
        SetError::new(SetErrorType::InvalidProperties)
            .with_description(description)
            .with_properties(properties.iter().copied()),
    ))
}

fn other(e: serde_json::Error) -> BackendSetError<MemoryError> {
    BackendSetError::Other(MemoryError::serde("serialize", &e))
}

fn decode<O: JmapObject>(v: &Value) -> Result<O, MemoryError> {
    O::deserialize(v).map_err(|e| MemoryError::serde(format_args!("decode {}", O::TYPE_NAME), &e))
}

/// Re-decode `v`, a value of a generic `QueryObject` associated type, as
/// the concrete type `T` it is once `O::TYPE_NAME` is known.
fn recode<T: DeserializeOwned>(v: &impl Serialize) -> Result<T, BackendQueryError<MemoryError>> {
    serde_json::to_value(v)
        .and_then(T::deserialize)
        .map_err(|e| BackendQueryError::Other(MemoryError::serde("recode query argument", &e)))
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

/// The `candidates`, ids of objects of `kind` that match the
/// draft-atwood-jmap-graph-00 filter, for which `where_` is true in a
/// [`GraphView`] of the Graph's visible `nodes` and `edges`. An
/// `expression` is `unsupportedFilter`: no language is implemented.
fn search_hits<'a>(
    where_: Option<&Value>,
    expression: Option<&String>,
    kind: CandidateKind,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    candidates: impl Iterator<Item = &'a Id>,
) -> Result<HashSet<Id>, BackendQueryError<MemoryError>> {
    if expression.is_some() {
        return Err(BackendQueryError::Method(JmapError::unsupported_filter()));
    }
    let Some(w) = where_ else {
        return Ok(candidates.cloned().collect());
    };
    let expr = Expression::parse(w, kind).map_err(|e| BackendQueryError::Method(e.into()))?;
    let view = GraphView::new(nodes, edges);
    Ok(candidates
        .filter(|id| match kind {
            CandidateKind::Node => view.node_matches(&expr, id),
            CandidateKind::Edge => view.edge_matches(&expr, id),
        })
        .cloned()
        .collect())
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

    // ponytail: the change log is per account, not per caller, so a
    // caller without `mayRead` on a Graph still sees the ids of it and of
    // its nodes, edges and ops, and a `shareWith` change surfaces nothing
    // as created or destroyed (the `GraphBackend` # Rights SHOULD).
    // Upgrade path: record each change's Graph and each user's `mayRead`
    // transitions in the log and filter the net changes by them.
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
            .get(O::TYPE_NAME)
            .and_then(|m| m.get(account_id.as_ref()))
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
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        let QueryParams {
            filter,
            sort,
            limit,
            position,
            anchor,
            anchor_offset,
            ..
        } = params;
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
            let Some(f) = filter else {
                return Err(BackendQueryError::Method(JmapError::unsupported_filter()));
            };
            let (nodes, edges) = inner.graph_content(caller, account_id, &f.graph_id);
            let hits = search_hits(
                f.where_.as_ref(),
                f.expression.as_ref(),
                CandidateKind::Node,
                &nodes,
                &edges,
                nodes.iter().filter(|n| node_matches(&f, n)).map(|n| &n.id),
            )?;
            sorted_ids(
                nodes,
                |n| hits.contains(&n.id),
                |a, b| compare_nodes(a, b, &sort),
                |n| n.id,
            )
        } else if O::TYPE_NAME == GraphEdge::TYPE_NAME {
            let filter: Option<GraphEdgeFilterCondition> = filter.map(recode).transpose()?;
            let sort: Vec<GraphEdgeComparator> = recode(&sort)?;
            let Some(f) = filter else {
                return Err(BackendQueryError::Method(JmapError::unsupported_filter()));
            };
            let (nodes, edges) = inner.graph_content(caller, account_id, &f.graph_id);
            let hits = search_hits(
                f.where_.as_ref(),
                f.expression.as_ref(),
                CandidateKind::Edge,
                &nodes,
                &edges,
                edges.iter().filter(|e| edge_matches(&f, e)).map(|e| &e.id),
            )?;
            sorted_ids(
                edges,
                |e| hits.contains(&e.id),
                |a, b| compare_edges(a, b, &sort),
                |e| e.id,
            )
        } else if O::TYPE_NAME == GraphNodeOp::TYPE_NAME {
            let filter: Option<GraphNodeOpFilterCondition> = filter.map(recode).transpose()?;
            sorted_ids(
                inner.all::<GraphNodeOp>(caller, account_id),
                |o| {
                    filter
                        .as_ref()
                        .is_some_and(|f| o.node_id == f.node_id && o.seq > f.after_seq.unwrap_or(0))
                        && inner.may_read(caller, account_id, &o.graph_id)
                },
                |a, b| a.seq.cmp(&b.seq),
                |o| o.id,
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
            .take(limit_or_max(limit))
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
        _params: QueryChangesParams<'_, O>,
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

    async fn create_object<O, F>(
        &self,
        caller: &Id,
        account_id: &Id,
        _create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<MemoryError>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        let id = Id::from(ulid::Ulid::new().to_string());
        let mut value = serde_json::to_value(build(&id)).map_err(other)?;
        let mut inner = self.lock();
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
        } else if O::TYPE_NAME == GraphNodeOp::TYPE_NAME {
            sequence(&mut inner, caller, account_id, &id, &mut value)?;
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

        let old = current.clone();
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
        if O::TYPE_NAME == GraphNode::TYPE_NAME && inner.edit(account_id).is_some() {
            // §GraphNode Extensions and §Snapshots, re-checked under the
            // lock so concurrent snapshot writes cannot regress.
            let projected = inner
                .accounts
                .get(account_id.as_ref())
                .and_then(|a| a.projected.as_ref())
                .is_some_and(|g| *g == graph_of(&current));
            check_node_edit(
                old.as_object(),
                current.as_object().unwrap_or(&serde_json::Map::new()),
                keys.iter().any(|k| k == "snapshotSeq"),
                inner.log(account_id, id).head,
                projected,
            )
            .map_err(BackendSetError::SetError)?;
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
            // "when a Graph is destroyed, so are the ops of all its nodes".
            for contained in [
                GraphNode::TYPE_NAME,
                GraphEdge::TYPE_NAME,
                GraphNodeOp::TYPE_NAME,
            ] {
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
            // "When a node is destroyed, its ops are destroyed with it".
            inner.destroy_where(GraphNodeOp::TYPE_NAME, account_id, |v| {
                v.get("nodeId").and_then(Value::as_str) == id_str
            });
        }
        inner.forget_logs(account_id);
        Ok(())
    }

    async fn graph_edit_capability(
        &self,
        _caller: &Id,
        account_id: &Id,
    ) -> Result<Option<GraphEditCapability>, MemoryError> {
        Ok(self.lock().edit(account_id).cloned())
    }

    async fn graph_search_capability(
        &self,
        _caller: &Id,
        account_id: &Id,
    ) -> Result<Option<GraphSearchCapability>, MemoryError> {
        Ok(self
            .lock()
            .accounts
            .get(account_id.as_ref())
            .and_then(|a| a.search.clone()))
    }

    async fn edit_log(
        &self,
        _caller: &Id,
        account_id: &Id,
        node_id: &Id,
    ) -> Result<EditLog, MemoryError> {
        Ok(self.lock().log(account_id, node_id))
    }

    async fn projected_graph_id(
        &self,
        _caller: &Id,
        account_id: &Id,
    ) -> Result<Option<Id>, MemoryError> {
        Ok(self
            .lock()
            .accounts
            .get(account_id.as_ref())
            .and_then(|a| a.projected.clone()))
    }

    async fn publish_cursor(&self, _caller: &Id, _account_id: &Id, event: &GraphCursorEvent) {
        self.lock().cursors.push(event.clone());
    }

    fn has_capability(&self, _caller: &Id, account_id: &Id, uri: &str) -> bool {
        self.lock()
            .advertised
            .contains(&(account_id.clone(), uri.to_owned()))
    }

    async fn is_sender_blocked(&self, caller: &Id, sender_id: &Id) -> Result<bool, MemoryError> {
        Ok(self
            .lock()
            .blocked
            .contains(&(caller.clone(), sender_id.clone())))
    }

    fn supports_type<O: JmapObject>(&self, _caller: &Id, account_id: &Id) -> bool {
        [Graph::TYPE_NAME, GraphNode::TYPE_NAME, GraphEdge::TYPE_NAME].contains(&O::TYPE_NAME)
            || (O::TYPE_NAME == GraphNodeOp::TYPE_NAME && self.lock().edit(account_id).is_some())
    }
}

/// Sequence GraphNodeOp `value`, whose id is `id`, under the lock: re-check
/// that its node is one the caller may read with a non-null `editFormat`
/// (`invalidProperties` naming `nodeId`), `mayWrite` (`forbidden`) and
/// `baseSeq` against the head (`invalidProperties` naming `baseSeq`), then
/// give it the node's Graph and "the next `seq` of the node".
fn sequence(
    inner: &mut Inner,
    caller: &Id,
    account_id: &Id,
    id: &Id,
    value: &mut Value,
) -> Result<(), BackendSetError<MemoryError>> {
    let bad_node = || {
        invalid(
            &["nodeId"],
            "nodeId does not identify a node the user may read whose editFormat is not null",
        )
    };
    let node_id = Id::from(value["nodeId"].as_str().unwrap_or_default());
    let node = inner
        .get(GraphNode::TYPE_NAME, account_id, &node_id)
        .ok_or_else(bad_node)?;
    let graph_id = graph_of(node);
    let editable = node.get("editFormat").is_some_and(Value::is_string);
    inner.require_write(caller, account_id, &graph_id, bad_node)?;
    if !editable {
        return Err(bad_node());
    }
    let log = inner
        .op_logs
        .entry((account_id.as_ref().to_owned(), node_id))
        .or_default();
    if value["baseSeq"].as_u64().is_none_or(|b| b > log.head) {
        return Err(invalid(
            &["baseSeq"],
            "baseSeq is greater than the head of the edit log",
        ));
    }
    log.head += 1;
    log.held
        .push_back((log.head, id.clone(), SystemTime::now()));
    value["seq"] = json!(log.head);
    value["graphId"] = json!(graph_id);
    Ok(())
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
