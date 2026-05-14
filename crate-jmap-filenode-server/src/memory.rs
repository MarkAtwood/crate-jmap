//! In-memory reference implementation of [`FileNodeBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`FileNodeBackend`] trait to study when writing
//!    a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-FileNode dispatcher
//!    with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and a number
//! of draft-ietf-jmap-filenode edge cases are simplified (see source comments).
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
//! use jmap_filenode_server::{memory::MemoryBackend, register_filenode_handlers};
//! use jmap_server::Dispatcher;
//!
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_filenode_handlers(&mut dispatcher, Arc::new(MemoryBackend::new()));
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
//! Promoted from `tests/common/mod.rs` per Beads issue JMAP-hwdv (epic)
//! and JMAP-hwdv.4 (this crate, mirror of canonical JMAP-hwdv.1 in
//! jmap-mail-server).

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::{
    BackendChangesError, BackendSetError, ChangesResult, FileNodeBackend, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};
use jmap_filenode_types::{FileNode, NodeRole, NodeType};
use jmap_types::{Id, State};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Minimal error type for `MemoryBackend`.
#[derive(Debug)]
pub struct MemoryError(pub String);

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "memory backend error: {}", self.0)
    }
}

impl std::error::Error for MemoryError {}

// ---------------------------------------------------------------------------
// Change log
// ---------------------------------------------------------------------------

/// Type of a change recorded in the in-memory change log.
#[derive(Clone, Debug)]
pub enum ChangeType {
    /// Object was created.
    Created,
    /// Object was updated.
    Updated,
    /// Object was destroyed.
    Destroyed,
}

#[derive(Clone, Debug)]
struct ChangeEntry {
    /// State counter at the time of the change (state *after* the change).
    state: u64,
    change_type: ChangeType,
    id: Id,
}

// ---------------------------------------------------------------------------
// Per-account storage
// ---------------------------------------------------------------------------

struct AccountStore {
    nodes: HashMap<Id, FileNode>,
    /// Monotonically increasing state counter.  Starts at 0 (no changes yet).
    state: u64,
    change_log: Vec<ChangeEntry>,
    /// Counter used to assign server IDs: "node-1", "node-2", …
    node_counter: u64,
}

impl AccountStore {
    fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            state: 0,
            change_log: Vec::new(),
            node_counter: 0,
        }
    }

    fn next_node_id(&mut self) -> Id {
        self.node_counter += 1;
        Id::from(format!("node-{}", self.node_counter))
    }

    fn bump_state(&mut self, change_type: ChangeType, id: Id) {
        self.state += 1;
        self.change_log.push(ChangeEntry {
            state: self.state,
            change_type,
            id,
        });
    }
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct MemoryState {
    accounts: HashMap<String, AccountStore>,
}

impl MemoryState {
    fn get_or_err(&self, account_id: &Id) -> Option<&AccountStore> {
        self.accounts.get(account_id.as_ref())
    }

    fn get_mut_or_err(&mut self, account_id: &Id) -> Option<&mut AccountStore> {
        self.accounts.get_mut(account_id.as_ref())
    }
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// Full in-memory backend implementing all [`FileNodeBackend`] methods.
///
/// Cloning is cheap: the clone shares the same `Arc<Mutex<…>>`.
#[derive(Clone)]
pub struct MemoryBackend {
    inner: Arc<Mutex<MemoryState>>,
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBackend {
    /// Create a new, empty `MemoryBackend`.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MemoryState {
                accounts: HashMap::new(),
            })),
        }
    }

    /// Register an account and return `self` for chaining.
    pub fn with_account(self, account_id: &str) -> Self {
        self.inner
            .lock()
            .unwrap()
            .accounts
            .insert(account_id.to_owned(), AccountStore::new());
        self
    }

    /// Add a pre-existing node to an account (used to set up test fixtures).
    ///
    /// The state counter is NOT incremented — this is silent fixture setup.
    pub fn seed_node(&self, account_id: &str, node: FileNode) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(store) = guard.accounts.get_mut(account_id) {
            store.nodes.insert(node.id.clone(), node);
        }
    }
}

// ---------------------------------------------------------------------------
// JmapBackend impl
// ---------------------------------------------------------------------------

impl JmapBackend for MemoryBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .accounts
            .contains_key(account_id.as_ref()))
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        // Only FileNode objects are stored.  For any other type, return empty.
        // We detect FileNode by checking O::TYPE_NAME.
        if O::TYPE_NAME != "FileNode" {
            return Ok((vec![], vec![]));
        }

        let guard = self.inner.lock().unwrap();
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => return Ok((vec![], vec![])),
        };

        match ids {
            None => {
                // Return all nodes.
                let mut nodes: Vec<O> = Vec::new();
                for node in store.nodes.values() {
                    let v = serde_json::to_value(node).map_err(|e| MemoryError(e.to_string()))?;
                    let obj: O =
                        serde_json::from_value(v).map_err(|e| MemoryError(e.to_string()))?;
                    nodes.push(obj);
                }
                Ok((nodes, vec![]))
            }
            Some(id_slice) => {
                let mut found: Vec<O> = Vec::new();
                let mut not_found: Vec<Id> = Vec::new();
                for id in id_slice {
                    if let Some(node) = store.nodes.get(id) {
                        let v =
                            serde_json::to_value(node).map_err(|e| MemoryError(e.to_string()))?;
                        let obj: O =
                            serde_json::from_value(v).map_err(|e| MemoryError(e.to_string()))?;
                        found.push(obj);
                    } else {
                        not_found.push(id.clone());
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
        let guard = self.inner.lock().unwrap();
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => return Ok(State::from("0")),
        };
        Ok(State::from(store.state.to_string()))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        let since: u64 = since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::CannotCalculate)?;

        let guard = self.inner.lock().unwrap();
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => {
                return Ok(ChangesResult::new(
                    vec![],
                    vec![],
                    vec![],
                    false,
                    State::from("0"),
                ))
            }
        };

        let current_state = State::from(store.state.to_string());

        // Collect all changes after since_state.
        let all: Vec<&ChangeEntry> = store
            .change_log
            .iter()
            .filter(|e| e.state > since)
            .collect();

        // Apply max_changes limit.
        let has_more_changes = if let Some(max) = max_changes {
            all.len() as u64 > max
        } else {
            false
        };
        let entries: Vec<&ChangeEntry> = if let Some(max) = max_changes {
            all.into_iter().take(max as usize).collect()
        } else {
            all
        };

        let mut created: Vec<Id> = Vec::new();
        let mut updated: Vec<Id> = Vec::new();
        let mut destroyed: Vec<Id> = Vec::new();

        for e in entries {
            match e.change_type {
                ChangeType::Created => created.push(e.id.clone()),
                ChangeType::Updated => updated.push(e.id.clone()),
                ChangeType::Destroyed => destroyed.push(e.id.clone()),
            }
        }

        Ok(ChangesResult::new(
            created,
            updated,
            destroyed,
            has_more_changes,
            current_state,
        ))
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        _position: i64,
    ) -> Result<QueryResult, Self::Error> {
        let guard = self.inner.lock().unwrap();
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => {
                return Ok(QueryResult::new(
                    vec![],
                    0,
                    Some(0),
                    State::from("0"),
                    false,
                ))
            }
        };
        let current_state = State::from(store.state.to_string());

        // Parse the filter as FileNodeFilterCondition via serde round-trip.
        // This avoids the #[non_exhaustive] restriction outside the defining crate.
        let fc: Option<jmap_filenode_types::FileNodeFilterCondition> = if let Some(f) = filter {
            serde_json::to_value(f)
                .ok()
                .and_then(|v| serde_json::from_value(v).ok())
        } else {
            None
        };

        let mut ids: Vec<Id> = Vec::new();
        for node in store.nodes.values() {
            if node_matches_filter(node, fc.as_ref(), &store.nodes)? {
                ids.push(node.id.clone());
            }
        }

        // Sort by id string for determinism in tests.
        ids.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));

        if let Some(lim) = limit {
            ids.truncate(lim as usize);
        }
        let total = ids.len() as u64;

        Ok(QueryResult::new(ids, 0, Some(total), current_state, false))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        since_query_state: &State,
        _filter: Option<&O::Filter>,
        _sort: Option<&[O::Comparator]>,
        _max_changes: Option<u64>,
        _up_to_id: Option<&Id>,
        _collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        let guard = self.inner.lock().unwrap();
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => {
                return Ok(QueryChangesResult::new(
                    since_query_state.clone(),
                    State::from("0"),
                    Some(0),
                    vec![],
                    vec![],
                ))
            }
        };
        let current_state = State::from(store.state.to_string());

        Ok(QueryChangesResult::new(
            since_query_state.clone(),
            current_state,
            Some(0),
            vec![],
            vec![],
        ))
    }
}

// ---------------------------------------------------------------------------
// FileNodeBackend impl
// ---------------------------------------------------------------------------

impl FileNodeBackend for MemoryBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        _create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        // Only FileNode is supported.
        if O::TYPE_NAME != "FileNode" {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::InvalidProperties,
            )));
        }

        // Serialize obj → FileNode → assign id → deserialize back to O.
        let v = serde_json::to_value(&obj)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

        let mut node: FileNode = serde_json::from_value(v)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

        let new_id = {
            let mut guard = self.inner.lock().unwrap();
            let store = guard.get_mut_or_err(account_id).ok_or_else(|| {
                BackendSetError::SetError(SetError::new(SetErrorType::InvalidProperties))
            })?;
            let new_id = store.next_node_id();
            node.id = new_id.clone();
            store.nodes.insert(new_id.clone(), node.clone());
            store.bump_state(ChangeType::Created, new_id.clone());
            new_id
        };

        let result_v = serde_json::to_value(&node)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;
        let result_obj: O = serde_json::from_value(result_v)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

        Ok((new_id, result_obj))
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        // Only FileNode is supported.
        if O::TYPE_NAME != "FileNode" {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )));
        }

        // Serialize patch as a JSON Value for merge-patching.
        //
        // LIMITATION: Only top-level key patches are supported.  Paths using "/"
        // syntax (e.g. "keywords/flag1") are ignored — the key is merged as-is at
        // the top level of the serialized node.  This is sufficient for the current
        // integration tests and simpler than a full RFC 7396 implementation.
        let patch_val = serde_json::to_value(&patch)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

        let mut guard = self.inner.lock().unwrap();
        let store = guard
            .get_mut_or_err(account_id)
            .ok_or_else(|| BackendSetError::SetError(SetError::new(SetErrorType::NotFound)))?;

        let node = store
            .nodes
            .get(id)
            .ok_or_else(|| BackendSetError::SetError(SetError::new(SetErrorType::NotFound)))?
            .clone();

        // Serialize the stored node to JSON, apply the patch as a JSON merge,
        // then deserialize back to FileNode.
        let mut node_val = serde_json::to_value(&node)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

        if let (Some(obj), Some(patch_obj)) = (node_val.as_object_mut(), patch_val.as_object()) {
            for (k, v) in patch_obj {
                obj.insert(k.clone(), v.clone());
            }
        }

        let updated_node: FileNode = serde_json::from_value(node_val)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

        store.nodes.insert(id.clone(), updated_node.clone());
        store.bump_state(ChangeType::Updated, id.clone());

        let result_v = serde_json::to_value(&updated_node)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;
        let result_obj: O = serde_json::from_value(result_v)
            .map_err(|e| BackendSetError::Other(MemoryError(e.to_string())))?;

        Ok(Some(result_obj))
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        let mut guard = self.inner.lock().unwrap();
        let store = guard
            .get_mut_or_err(account_id)
            .ok_or_else(|| BackendSetError::SetError(SetError::new(SetErrorType::NotFound)))?;

        if store.nodes.remove(id).is_none() {
            return Err(BackendSetError::SetError(SetError::new(
                SetErrorType::NotFound,
            )));
        }
        store.bump_state(ChangeType::Destroyed, id.clone());
        Ok(())
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        true
    }

    async fn get_ancestors(
        &self,
        _caller: &(),
        account_id: &Id,
        ids: &[Id],
    ) -> Result<Vec<FileNode>, Self::Error> {
        let guard = self.inner.lock().unwrap();
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => return Ok(vec![]),
        };

        // Walk parent_id links upward from each id, collecting ancestors.
        // Deduplicate by Id (Hash + Eq) to avoid cycles or double-insertion.
        // Consistent with the FileNodeBackend::query_subtree default impl in
        // src/backend.rs, which also keys visited-sets by Id directly.
        let mut visited: std::collections::HashSet<Id> = ids.iter().cloned().collect();
        let mut ancestors: Vec<FileNode> = Vec::new();

        // Start from the immediate parents of the requested ids.
        let mut frontier: Vec<Id> = ids
            .iter()
            .filter_map(|id| store.nodes.get(id).and_then(|n| n.parent_id.clone()))
            .collect();

        loop {
            if frontier.is_empty() {
                break;
            }
            let mut next_frontier: Vec<Id> = Vec::new();
            for parent_id in frontier.drain(..) {
                if !visited.insert(parent_id.clone()) {
                    continue;
                }
                if let Some(node) = store.nodes.get(&parent_id) {
                    ancestors.push(node.clone());
                    if let Some(ref grandparent_id) = node.parent_id {
                        next_frontier.push(grandparent_id.clone());
                    }
                }
            }
            frontier = next_frontier;
        }

        Ok(ancestors)
    }

    async fn get_descendant_ids(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<Vec<Id>, Self::Error> {
        let guard = self.inner.lock().unwrap();
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => return Ok(vec![]),
        };

        // BFS over all nodes, following nodes where parent_id == current frontier.
        // Visited-set keys by Id (Hash + Eq) — consistent with the
        // FileNodeBackend::query_subtree default impl in src/backend.rs.
        let mut result: Vec<Id> = Vec::new();
        let mut frontier: Vec<Id> = vec![id.clone()];
        let mut visited: std::collections::HashSet<Id> = std::iter::once(id.clone()).collect();

        loop {
            if frontier.is_empty() {
                break;
            }
            let mut next_frontier: Vec<Id> = Vec::new();
            for current_id in &frontier {
                for node in store.nodes.values() {
                    if node.parent_id.as_ref() == Some(current_id)
                        && visited.insert(node.id.clone())
                    {
                        result.push(node.id.clone());
                        next_frontier.push(node.id.clone());
                    }
                }
            }
            frontier = next_frontier;
        }

        Ok(result)
    }

    async fn blob_exists(
        &self,
        _caller: &(),
        _account_id: &Id,
        _blob_id: &Id,
    ) -> Result<bool, Self::Error> {
        // In the test environment, all blobs are assumed to exist.
        Ok(true)
    }

    /// Case-folding algorithm: `str::to_lowercase` (Unicode simple lowercase,
    /// locale-independent). This is the reference test-and-demo behaviour; per
    /// the [`FileNodeBackend::find_sibling_by_name`] trait doc, the algorithm
    /// is implementation-defined and the workspace does not standardise it.
    /// Production backends will typically need to match the underlying
    /// storage layer's folding rules.
    ///
    /// [`FileNodeBackend::find_sibling_by_name`]: crate::backend::FileNodeBackend::find_sibling_by_name
    async fn find_sibling_by_name(
        &self,
        _caller: &(),
        account_id: &Id,
        parent_id: Option<&Id>,
        name: &str,
        folding: crate::backend::CaseFolding,
    ) -> Result<Option<Id>, Self::Error> {
        let guard = self.inner.lock().unwrap();
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => return Ok(None),
        };

        let case_insensitive = matches!(folding, crate::backend::CaseFolding::Insensitive);
        let search_name = if case_insensitive {
            name.to_lowercase()
        } else {
            name.to_owned()
        };

        for node in store.nodes.values() {
            // Check parent_id matches.
            let parent_matches = match (parent_id, &node.parent_id) {
                (None, None) => true,
                (Some(pid), Some(nid)) => pid == nid,
                _ => false,
            };
            if !parent_matches {
                continue;
            }
            // Check name matches.
            let node_name = if case_insensitive {
                node.name.to_lowercase()
            } else {
                node.name.clone()
            };
            if node_name == search_name {
                return Ok(Some(node.id.clone()));
            }
        }

        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Filter matching helper
// ---------------------------------------------------------------------------

/// Returns `Ok(true)` if `node` satisfies the filter condition `fc`,
/// `Ok(false)` if it does not match, and `Err(MemoryError(...))` if the
/// filter condition references a field that the reference implementation
/// does not support.
///
/// Implemented fields (all simple equality / inequality semantics):
/// - `parentId` — exact match on parent_id
/// - `isTopLevel` — parent_id is null
/// - `ancestorId` — `node` has the given id somewhere on its parent chain
/// - `descendantId` — `node` is an ancestor of the node with the given id
/// - `nodeType` — exact match on the node_type string
/// - `role` — exact match on the role string
/// - `hasAnyRole` — node has a non-null role (or null role if false)
/// - `blobId` — exact match on blob_id
/// - `type` (media type) — exact match on media_type
/// - `name` — exact byte match on name
/// - `nameMatch` / `typeMatch` — case-insensitive glob match (`*`, `?`)
/// - `isExecutable` — exact match on the executable flag
/// - `minSize` / `maxSize` — size bounds (`size >= minSize` / `size < maxSize`)
/// - `createdBefore` / `createdAfter`
/// - `modifiedBefore` / `modifiedAfter`
/// - `accessedBefore` / `accessedAfter`
///
/// Explicitly NOT supported (returns Err so a test that uses one of these
/// gets a clear backend error rather than a silent match-all that would
/// look like a passing query):
/// - `body` (full-text search in blob content)
/// - `text` (full-text search OR name OR type)
///
/// Reasoning: bd:JMAP-510h.9 documented the silent match-all default as a
/// double bug — wrong test oracle and footgun reference for downstream
/// backends to copy. Explicit Err on the genuinely-unsupported conditions
/// closes the footgun while implementing the cheap equality fields
/// keeps the reference impl useful for the common cases.
fn node_matches_filter(
    node: &FileNode,
    fc: Option<&jmap_filenode_types::FileNodeFilterCondition>,
    nodes: &HashMap<Id, FileNode>,
) -> Result<bool, MemoryError> {
    let fc = match fc {
        Some(f) => f,
        None => return Ok(true), // No filter — all nodes match.
    };

    if let Some(ref pid) = fc.parent_id {
        if node.parent_id.as_ref() != Some(pid) {
            return Ok(false);
        }
    }

    if let Some(is_top) = fc.is_top_level {
        let top = node.parent_id.is_none();
        if top != is_top {
            return Ok(false);
        }
    }

    if let Some(ref aid) = fc.ancestor_id {
        // Walk parent chain from `node` upward looking for aid.
        let mut cur = node.parent_id.as_ref();
        let mut found = false;
        let mut depth = 0u32;
        while let Some(pid) = cur {
            if pid == aid {
                found = true;
                break;
            }
            // Cycle safeguard: a malformed tree must not loop forever.
            depth += 1;
            if depth > 10_000 {
                return Err(MemoryError(
                    "node_matches_filter: ancestor chain exceeds 10_000 depth, suspected cycle"
                        .to_owned(),
                ));
            }
            cur = nodes.get(pid).and_then(|n| n.parent_id.as_ref());
        }
        if !found {
            return Ok(false);
        }
    }

    if let Some(ref did) = fc.descendant_id {
        // Node matches if its id appears on the parent chain of the
        // node identified by `did`.
        let target = match nodes.get(did) {
            Some(t) => t,
            None => return Ok(false),
        };
        let mut cur = target.parent_id.as_ref();
        let mut found = false;
        let mut depth = 0u32;
        while let Some(pid) = cur {
            if pid == &node.id {
                found = true;
                break;
            }
            depth += 1;
            if depth > 10_000 {
                return Err(MemoryError(
                    "node_matches_filter: descendant chain exceeds 10_000 depth, suspected cycle"
                        .to_owned(),
                ));
            }
            cur = nodes.get(pid).and_then(|n| n.parent_id.as_ref());
        }
        if !found {
            return Ok(false);
        }
    }

    if let Some(ref nt) = fc.node_type {
        let node_nt_str = node.node_type.as_ref().map(NodeType::to_wire_str);
        if node_nt_str != Some(nt.as_str()) {
            return Ok(false);
        }
    }

    if let Some(ref r) = fc.role {
        let node_role_str = node.role.as_ref().map(NodeRole::to_wire_str);
        if node_role_str != Some(r.as_str()) {
            return Ok(false);
        }
    }

    if let Some(has_role) = fc.has_any_role {
        let has = node.role.is_some();
        if has != has_role {
            return Ok(false);
        }
    }

    if let Some(ref bid) = fc.blob_id {
        if node.blob_id.as_ref() != Some(bid) {
            return Ok(false);
        }
    }

    if let Some(ref mt) = fc.media_type {
        if node.media_type.as_deref() != Some(mt.as_str()) {
            return Ok(false);
        }
    }

    if let Some(ref name) = fc.name {
        if node.name != *name {
            return Ok(false);
        }
    }

    if let Some(ref pattern) = fc.name_match {
        if !glob_match_case_insensitive(pattern, &node.name) {
            return Ok(false);
        }
    }

    if let Some(ref pattern) = fc.type_match {
        let mt = node.media_type.as_deref().unwrap_or("");
        if !glob_match_case_insensitive(pattern, mt) {
            return Ok(false);
        }
    }

    if let Some(is_exec) = fc.is_executable {
        if node.executable.unwrap_or(false) != is_exec {
            return Ok(false);
        }
    }

    if let Some(min) = fc.min_size {
        if node.size.unwrap_or(0) < min {
            return Ok(false);
        }
    }

    if let Some(max) = fc.max_size {
        if node.size.unwrap_or(0) >= max {
            return Ok(false);
        }
    }

    if let Some(ref before) = fc.created_before {
        if !date_strictly_before(node.created.as_ref(), before) {
            return Ok(false);
        }
    }
    if let Some(ref after) = fc.created_after {
        if !date_on_or_after(node.created.as_ref(), after) {
            return Ok(false);
        }
    }
    if let Some(ref before) = fc.modified_before {
        if !date_strictly_before(node.modified.as_ref(), before) {
            return Ok(false);
        }
    }
    if let Some(ref after) = fc.modified_after {
        if !date_on_or_after(node.modified.as_ref(), after) {
            return Ok(false);
        }
    }
    if let Some(ref before) = fc.accessed_before {
        if !date_strictly_before(node.accessed.as_ref(), before) {
            return Ok(false);
        }
    }
    if let Some(ref after) = fc.accessed_after {
        if !date_on_or_after(node.accessed.as_ref(), after) {
            return Ok(false);
        }
    }

    if fc.body.is_some() {
        return Err(MemoryError(
            "node_matches_filter: 'body' (full-text search) is not supported by the reference \
             MemoryBackend"
                .to_owned(),
        ));
    }
    if fc.text.is_some() {
        return Err(MemoryError(
            "node_matches_filter: 'text' (full-text search) is not supported by the reference \
             MemoryBackend"
                .to_owned(),
        ));
    }

    Ok(true)
}

/// Case-insensitive glob matcher.
///
/// Supports `*` (matches any sequence including empty) and `?` (matches
/// any single character). Iterative algorithm with O(len(pattern) *
/// len(text)) worst-case; sufficient for reference-impl correctness.
/// `[...]` character classes are not supported.
fn glob_match_case_insensitive(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().flat_map(char::to_lowercase).collect();
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    glob_match_inner(&p, 0, &t, 0)
}

fn glob_match_inner(p: &[char], pi: usize, t: &[char], ti: usize) -> bool {
    if pi == p.len() {
        return ti == t.len();
    }
    match p[pi] {
        '*' => {
            // Try matching zero, one, two, ... chars against the rest.
            for skip in ti..=t.len() {
                if glob_match_inner(p, pi + 1, t, skip) {
                    return true;
                }
            }
            false
        }
        '?' => ti < t.len() && glob_match_inner(p, pi + 1, t, ti + 1),
        c => ti < t.len() && t[ti] == c && glob_match_inner(p, pi + 1, t, ti + 1),
    }
}

fn date_strictly_before(
    node_date: Option<&jmap_types::UTCDate>,
    bound: &jmap_types::UTCDate,
) -> bool {
    match node_date {
        Some(d) => d.as_ref() < bound.as_ref(),
        None => false,
    }
}

fn date_on_or_after(node_date: Option<&jmap_types::UTCDate>, bound: &jmap_types::UTCDate) -> bool {
    match node_date {
        Some(d) => d.as_ref() >= bound.as_ref(),
        None => false,
    }
}
