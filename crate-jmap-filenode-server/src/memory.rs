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
//! behind one [`jmap_server::ScopedState`], persistence is not implemented, and a number
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
//! The store is a [`jmap_server::ScopedState`]: a `std::sync::Mutex` behind
//! a gate that a draft-ietf-jmap-conditional-02 FileNode/set scope holds
//! (`MemoryBackend`'s `ConditionalBackend` impl). Async backend methods
//! take `lock().await`, which waits while another task's scope is open;
//! sync setup and seeding helpers take `lock_unscoped()`. With no scope
//! open the gate is free. The `await_holding_lock` clippy lint is enabled
//! module-wide and enforces that no lock guard is held across an
//! `.await`.
//!
//! `ScopedState` takes a poisoned mutex anyway, so a panic while holding
//! the guard (already a bug in this module) does not panic every later
//! call; an atomic scope that panicked is rolled back to its snapshot.
//! The crate's tests never poison the lock. A production backend should
//! implement the trait with its own locking
//! rather than copy this pattern.
//!
//! # Caller context and rights
//!
//! `CallerCtx` is a type implementing [`MemoryCaller`](crate::memory::MemoryCaller):
//! the caller's Principal id ([`Id`](jmap_types::Id), the default), which
//! [`JmapAccounts::principal_id`](jmap_server::JmapAccounts::principal_id)
//! returns, or `()` for a single-user deployment with no identity. Each
//! account has an owner
//! ([`with_account`](crate::memory::MemoryBackend::with_account),
//! [`with_account_owner`](crate::memory::MemoryBackend::with_account_owner)).
//! The owner, and a caller without a principal, "has all rights" on every
//! node (draft-ietf-jmap-filenode-20 §5). Every other user's rights are
//! "the union of the rights granted to the user by the shareWith entries on
//! the node and on all its ancestors"; a File or SymLink takes its
//! parent's. Reads leave out nodes the caller cannot discover, and show a
//! node the caller may not read with only "id, @type, name, parentId,
//! myRights, isSubscribed, and shareWith"; `shareWith` itself only with
//! `mayShare`. Writes re-check the rights under the store lock. Only the
//! owner may create or move a node at the top level
//! (`mayCreateTopLevelFileNode`). `isSubscribed` is kept per user: the
//! stored value is the owner's, and a sharee's starts at the default,
//! `true` (-20 §3.1.2).
//!
//! `FileNode/changes` is per user: a node that becomes discoverable to the
//! caller is reported as created, one that stops being discoverable as
//! destroyed, and one whose rights for the caller changed as updated
//! (-20 §5). Object history is the owner's only.
//!
//! There is no Principal store: `shareWith` ids are not checked.
//!
//! # Tracking
//!
//! Promoted from `tests/common/mod.rs` per Beads issue JMAP-hwdv (epic)
//! and JMAP-hwdv.4 (this crate, mirror of canonical JMAP-hwdv.1 in
//! jmap-mail-server).

#![allow(async_fn_in_trait)]
#![deny(clippy::await_holding_lock)]

mod access;
mod history;

use std::collections::{BTreeSet, HashMap};
use std::marker::PhantomData;

use crate::FileNodeBackend;
use jmap_cid_types::Sha256;
use jmap_filenode_types::{
    FileNode, FileNodeComparator, FileNodeComparatorProperty, FileNodeFilter, FilesRights,
    NodeRole, NodeType,
};
use jmap_metadata_server::{
    ChangedProperties, MetadataCondition, MetadataFilterError, MetadataOnlyChanges, SetKind,
    changed_properties, check_metadata_depth, check_metadata_write, is_metadata_property,
    metadata_path_exists, metadata_text_contains, metadata_text_equals, unicode_casemap_prepare,
};
use jmap_metadata_types::{DataTypeMetadataInfo, MetadataTextMatch};
use jmap_object_history_server::{ChangeEvent, ChangeKind, fold_restore};
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapAccounts, JmapObject, JmapReads, QueryChangesParams, QueryChangesResult, QueryObject,
    QueryParams, QueryResult, SetError, SetErrorType, SetObject,
};
use jmap_server::{
    BeginSetError, ConditionalBackend, DeferredFailure, PatchPathError, ScopedState, SetOp,
    SetScope, SetScopeRequest, apply_patch_object, check_patch_paths, limit_or_max,
    resolve_query_start,
};
use jmap_types::{Filter, Id, JmapError, Operator, State};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Opaque storage-layer error returned by [`MemoryBackend`] operations.
///
/// The inner description is a human-readable string intended for
/// diagnostic logging; it is not a stable wire-format identifier.
///
/// # Forward compatibility
///
/// This type is `#[non_exhaustive]` and uses a named-field shape so
/// future revisions can add structured context (error kind, source
/// reference, account id, etc.) without a breaking change. Outside-
/// crate construction goes through [`MemoryError::new`]; outside-crate
/// reads go through [`MemoryError::description`].
///
/// Mirrors the canonical jmap-mail-server `MemoryError` shape
/// (bd:JMAP-510h.36; canonical was reshaped in commit 2941c50).
#[non_exhaustive]
#[derive(Debug)]
pub struct MemoryError {
    description: String,
}

impl MemoryError {
    /// Construct a [`MemoryError`] from a human-readable description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }

    /// Human-readable description of the underlying failure.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Construct a [`MemoryError`] for a `serde_json` failure during
    /// `context` (the operation that failed).
    ///
    /// Only the error's [`serde_json::error::Category`] is kept. A serde
    /// data error quotes the offending value (`invalid type: string
    /// "alice@example.com", expected u64`), and the `JmapAccounts::Error`
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

// ---------------------------------------------------------------------------
// Caller context
// ---------------------------------------------------------------------------

/// A caller context the memory backend accepts as its
/// [`JmapAccounts::CallerCtx`]: anything that names the caller's Principal,
/// or names none.
///
/// [`JmapAccounts::principal_id`] returns [`MemoryCaller::principal`]. A
/// caller that names the account owner, or names no principal, has all
/// rights; any other has the rights its id is given in `shareWith`
/// (draft-ietf-jmap-filenode-20 §5).
pub trait MemoryCaller: Clone + Send + Sync + 'static {
    /// The caller's Principal id, or `None` for a deployment without
    /// identity, whose every caller acts as the account owner.
    fn principal(&self) -> Option<&Id>;
}

/// The caller is the Principal with this id.
impl MemoryCaller for Id {
    fn principal(&self) -> Option<&Id> {
        Some(self)
    }
}

/// A single-user deployment: every caller acts as the account owner.
impl MemoryCaller for () {
    fn principal(&self) -> Option<&Id> {
        None
    }
}

/// The Principal that owns every account the unit tests register.
#[cfg(test)]
pub(crate) fn owner() -> Id {
    Id::from("owner")
}

// ---------------------------------------------------------------------------
// Change log
// ---------------------------------------------------------------------------

/// Type of a change recorded in the in-memory change log.
///
/// Internal-only — the canonical jmap-mail-server has no equivalent
/// public type (changes are tracked via private internals there). Per
/// workspace AGENTS.md "Canonical Templates" rule, this stays
/// `pub(crate)` so filenode-server matches the canonical shape.
/// Tests do not reference this enum directly (bd:JMAP-510h.35).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChangeType {
    /// Object was created.
    Created,
    /// Object was updated.
    Updated,
    /// Object was destroyed.
    Destroyed,
    /// Object was restored from history (draft-ietf-jmap-object-history
    /// §5.1).
    Restored,
}

/// Per-ID fate tracker for RFC 8620 §5.2 ID deduplication across change log
/// entries in one `/changes` window.
///
/// - created+updated → `Created`
/// - created+destroyed → omitted (the client never knew the object)
/// - updated+destroyed → `Destroyed`
/// - updated+updated → `Updated`
///
/// Mirrors the canonical jmap-mail-server `IdFate` enum.
#[derive(Debug, Clone, Copy)]
enum IdFate {
    Created,
    Updated,
    Destroyed,
}

#[derive(Clone, Debug)]
struct ChangeEntry {
    /// State counter at the time of the change (state *after* the change).
    state: u64,
    change_type: ChangeType,
    id: Id,
    /// For an update, the top-level properties it altered, or `None` when
    /// not known (always `None` for a create or destroy). Feeds
    /// draft-ietf-jmap-metadata-05 §3.3 `ignoreMetadataOnlyChanges` and
    /// `updatedProperties`.
    updated_props: Option<BTreeSet<String>>,
    /// Who `FileNode/changes` reports this entry to.
    audience: Audience,
}

/// The users a change-log entry is reported to (draft-ietf-jmap-filenode-20
/// §5: changes of rights and discoverability are reported "for that
/// user").
#[derive(Clone, Debug)]
enum Audience {
    /// The account owner (and callers without a principal), and these
    /// sharees: those who could discover the node both before and after
    /// the change (after a create, before a destroy).
    Owner(BTreeSet<Id>),
    /// One sharee only: a change of that user's rights, discoverability or
    /// own `isSubscribed`.
    Sharee(Id),
}

impl Audience {
    /// Whether the entry is reported to the sharee `sharee`, or to the
    /// owner when `None`.
    fn includes(&self, sharee: Option<&Id>) -> bool {
        match (self, sharee) {
            (Self::Owner(_), None) => true,
            (Self::Owner(users), Some(u)) => users.contains(u),
            (Self::Sharee(_), None) => false,
            (Self::Sharee(only), Some(u)) => only == u,
        }
    }
}

impl ChangeEntry {
    /// `true` for an update that changed only `metadata` and/or
    /// `privateMetadata` (draft-ietf-jmap-metadata-05 §3.3).
    fn is_metadata_only_update(&self) -> bool {
        matches!(self.change_type, ChangeType::Updated)
            && self.updated_props.as_ref().is_some_and(|props| {
                !props.is_empty() && props.iter().all(|p| is_metadata_property(p))
            })
    }
}

// ---------------------------------------------------------------------------
// Per-account storage
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AccountStore {
    /// The Principal that owns the account and "has all rights" on its
    /// nodes (draft-ietf-jmap-filenode-20 §5).
    owner: Id,
    /// The stored nodes. `isSubscribed` here is the owner's value, and a
    /// stored `myRights` is never shown: reads compute it per caller.
    nodes: HashMap<Id, FileNode>,
    /// A sharee's own `isSubscribed` on a node, by (node, user), once set
    /// (-20 §3.1.2: "This property is stored per user").
    subscriptions: HashMap<(Id, Id), bool>,
    /// Monotonically increasing state counter.  Starts at 0 (no changes yet).
    state: u64,
    change_log: Vec<ChangeEntry>,
    /// Counter used to assign server IDs: "node-1", "node-2", …
    node_counter: u64,
    /// Blob digests reported by [`FileNodeBackend::blob_sha256`]; see
    /// [`MemoryBackend::seed_blob_sha256`].
    blob_sha256: HashMap<Id, Sha256>,
}

impl AccountStore {
    fn new(owner: Id) -> Self {
        Self {
            owner,
            nodes: HashMap::new(),
            subscriptions: HashMap::new(),
            state: 0,
            change_log: Vec::new(),
            node_counter: 0,
            blob_sha256: HashMap::new(),
        }
    }

    /// Allocate the next reference-impl node id.
    ///
    /// The format is `node-N` where N is a monotonically increasing
    /// counter scoped to this `AccountStore`. The format is **observable**
    /// to test fixtures: any caller that seeds a node with id `node-K`
    /// via [`MemoryBackend::seed_node`] would collide on the next
    /// `create_object` if `node_counter` were not synchronised. The
    /// seed path watches for `node-N`-shaped ids and bumps the counter
    /// past the seeded N so the next allocation cannot collide.
    fn next_node_id(&mut self) -> Id {
        self.node_counter += 1;
        Id::from(format!("node-{}", self.node_counter))
    }

    /// If `id` matches the reference-impl format `node-N`, bump
    /// `node_counter` to at least `N` so future [`Self::next_node_id`]
    /// allocations cannot collide with this externally-seeded id.
    ///
    /// Idempotent: replays on the same id are no-ops. Non-matching ids
    /// (UUIDs, arbitrary strings) are ignored — they are guaranteed
    /// not to collide with the `node-N` allocator anyway.
    fn sync_counter_with_seeded_id(&mut self, id: &Id) {
        let s = id.as_ref();
        if let Some(rest) = s.strip_prefix("node-")
            && let Ok(n) = rest.parse::<u64>()
            && n > self.node_counter
        {
            self.node_counter = n;
        }
    }

    fn bump_state(
        &mut self,
        change_type: ChangeType,
        id: Id,
        updated_props: Option<BTreeSet<String>>,
        audience: Audience,
    ) {
        self.state += 1;
        self.change_log.push(ChangeEntry {
            state: self.state,
            change_type,
            id,
            updated_props,
            audience,
        });
    }

    /// The viewer `user` is in this account.
    fn viewer<'a>(&'a self, user: Option<&'a Id>, metadata: bool) -> access::Viewer<'a> {
        access::Viewer {
            user,
            owner: &self.owner,
            metadata,
        }
    }

    /// What `user` can discover in this account now.
    fn view_for(&self, user: Option<&Id>, metadata: bool) -> access::View {
        access::view(&self.nodes, self.viewer(user, metadata))
    }

    /// `user`'s own `isSubscribed` on node `id`, when the user is a sharee
    /// who set it; `None` leaves the stored value, which is the owner's
    /// (or the default `true`).
    fn subscription(&self, id: &Id, user: Option<&Id>) -> Option<bool> {
        let sharee = user.filter(|u| **u != self.owner)?;
        Some(
            self.subscriptions
                .get(&(id.clone(), sharee.clone()))
                .copied()
                .unwrap_or(true),
        )
    }

    /// Record a write to node `id`, made while the nodes were `before`, in
    /// the change log, for every user it concerns.
    ///
    /// draft-ietf-jmap-filenode-20 §5: "The server MUST report every node
    /// whose rights for a user changed as changed in FileNode/changes for
    /// that user, including nodes that have no myRights of their own", and
    /// "When a node stops being discoverable by the user ... the server
    /// reports it as destroyed in FileNode/changes". So, besides the entry
    /// for `id` itself, each sharee named by a `shareWith` before or after
    /// the write gets an entry of its own for each node whose
    /// discoverability or rights for that sharee the write changed: created
    /// when it became discoverable, destroyed when it stopped being, updated
    /// when its rights changed. The entry for `id` reaches the owner and
    /// the sharees who could discover it on both sides of the write.
    /// `owner_only` keeps it from the sharees: a change to the owner's own
    /// `isSubscribed` (-20 §3.1.1: "The server SHOULD report a per-user
    /// change in FileNode/changes only to the users whose value changed").
    fn log_write(
        &mut self,
        before: &HashMap<Id, FileNode>,
        change_type: ChangeType,
        id: &Id,
        updated_props: Option<BTreeSet<String>>,
        metadata: bool,
        owner_only: bool,
    ) {
        let mut users = access::sharees(before);
        users.extend(access::sharees(&self.nodes));
        users.remove(&self.owner);
        let mut main = BTreeSet::new();
        let mut per_user: Vec<(ChangeType, Id, Option<BTreeSet<String>>, Id)> = Vec::new();
        for user in users {
            let was = access::view(before, self.viewer(Some(&user), metadata));
            let now = self.view_for(Some(&user), metadata);
            let reaches = match change_type {
                ChangeType::Created | ChangeType::Restored => now.contains_key(id),
                ChangeType::Destroyed => was.contains_key(id),
                ChangeType::Updated => was.contains_key(id) && now.contains_key(id),
            };
            if reaches && !owner_only {
                main.insert(user.clone());
            }
            let mut ids: BTreeSet<&Id> = was.keys().collect();
            ids.extend(now.keys());
            for node_id in ids {
                // The entry for `id` already tells a user it reaches.
                if node_id == id && reaches {
                    continue;
                }
                let (kind, props) = match (was.get(node_id), now.get(node_id)) {
                    (Some(_), None) => (ChangeType::Destroyed, None),
                    (None, Some(_)) => (ChangeType::Created, None),
                    (Some(b), Some(a)) if b != a => (
                        ChangeType::Updated,
                        rights_props(self.nodes.get(node_id), b, a),
                    ),
                    _ => continue,
                };
                per_user.push((kind, node_id.clone(), props, user.clone()));
            }
        }
        self.bump_state(
            change_type,
            id.clone(),
            updated_props,
            Audience::Owner(main),
        );
        for (kind, node_id, props, user) in per_user {
            self.bump_state(kind, node_id, props, Audience::Sharee(user));
        }
    }
}

/// The properties that change for a user whose rights on `node` went from
/// `before` to `after`: `myRights` on a Directory, with `shareWith` when
/// `mayShare` changed (-20 §3.1.2: `shareWith` "is null if the user
/// requesting the object does not have myRights.mayShare"). `None`
/// (unknown) when `mayRead` changed, which shows or hides most properties
/// (-20 §5), and for a node without `myRights` of its own.
fn rights_props(
    node: Option<&FileNode>,
    before: &access::Sight,
    after: &access::Sight,
) -> Option<BTreeSet<String>> {
    if before.readable() != after.readable()
        || node.is_none_or(|n| n.node_type != Some(NodeType::Directory))
    {
        return None;
    }
    let mut props = BTreeSet::from(["myRights".to_owned()]);
    if before.rights.may_share != after.rights.may_share {
        props.insert("shareWith".to_owned());
    }
    Some(props)
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct MemoryState {
    accounts: HashMap<String, AccountStore>,
    /// The draft-ietf-jmap-metadata-05 §1.2.1 DataTypeMetadataInfo of
    /// FileNode, set by [`MemoryBackend::with_metadata_info`]. `None`:
    /// FileNode does not have the metadata properties.
    metadata_info: Option<DataTypeMetadataInfo>,
    /// The `maxFileNodeDepth` of every account, set by
    /// [`MemoryBackend::with_max_file_node_depth`]. `None`: no limit.
    max_file_node_depth: Option<u64>,
    /// Object history, once [`MemoryBackend::with_object_history`] turns it
    /// on.
    history: Option<jmap_object_history_server::HistoryStore>,
}

impl MemoryState {
    fn get_or_err(&self, account_id: &Id) -> Option<&AccountStore> {
        self.accounts.get(account_id.as_ref())
    }

    fn get_mut_or_err(&mut self, account_id: &Id) -> Option<&mut AccountStore> {
        self.accounts.get_mut(account_id.as_ref())
    }

    /// Whether FileNode has the draft-ietf-jmap-metadata-05 properties.
    fn has_metadata(&self) -> bool {
        self.metadata_info.is_some()
    }

    /// The nodes of `account_id` that `user` can discover, each as `user`
    /// receives it (see [`access::shown`]); empty for an unknown account.
    fn shown_nodes(&self, account_id: &Id, user: Option<&Id>) -> HashMap<Id, FileNode> {
        let Some(store) = self.get_or_err(account_id) else {
            return HashMap::new();
        };
        store
            .view_for(user, self.has_metadata())
            .into_iter()
            .filter_map(|(id, sight)| {
                let node = store.nodes.get(&id)?;
                let shown = access::shown(node, &sight, store.subscription(&id, user));
                Some((id, shown))
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// MemoryBackend
// ---------------------------------------------------------------------------

/// Full in-memory backend implementing all [`FileNodeBackend`] methods.
///
/// Cloning is cheap and **shared-state**; see the [`Clone`] impl.
///
/// `C` is the [`JmapAccounts::CallerCtx`]: [`Id`], the caller's Principal
/// id, by default, or `()` for a deployment without identity (see the
/// module documentation, "Caller context and rights").
#[non_exhaustive]
pub struct MemoryBackend<C = Id> {
    inner: ScopedState<MemoryState>,
    caller: PhantomData<fn() -> C>,
}

/// Manual `Clone` impl, not `#[derive(Clone)]`: cloning is **shared-state**.
///
/// A clone copies no data. The original and the clone hold `Arc::clone`s
/// of the same `Mutex`-guarded store and observe each other's mutations, so
/// one handle can seed or inspect state while another serves requests. This
/// impl must never become a deep copy; for independent state construct a
/// fresh [`MemoryBackend::new`] instead of calling `.clone()`.
impl<C> Clone for MemoryBackend<C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            caller: PhantomData,
        }
    }
}

/// An empty backend: [`MemoryBackend::new`] for any caller context, such
/// as `MemoryBackend::<()>::default()` for a deployment without identity.
impl<C> Default for MemoryBackend<C> {
    fn default() -> Self {
        Self {
            inner: ScopedState::new(MemoryState {
                accounts: HashMap::new(),
                metadata_info: None,
                max_file_node_depth: None,
                history: None,
            }),
            caller: PhantomData,
        }
    }
}

impl MemoryBackend {
    /// Create a new, empty `MemoryBackend` whose callers are Principal ids.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<C> MemoryBackend<C> {
    /// Register an account owned by the Principal whose id is
    /// `account_id`, and return `self` for chaining.
    ///
    /// The same as [`Self::with_account_owner`] with `account_id` as the
    /// owner.
    ///
    /// # Panics
    ///
    /// In debug builds, if `account_id` is the empty string (RFC 8620
    /// §1.2, bd:JMAP-dx0s).
    #[must_use]
    pub fn with_account(self, account_id: &Id) -> Self {
        self.with_account_owner(account_id, account_id)
    }

    /// Register an account owned by Principal `owner`, and return `self`
    /// for chaining.
    ///
    /// The owner "has all rights" on every node of the account, may create
    /// nodes at the top level, and can discover every node
    /// (draft-ietf-jmap-filenode-20 §5); so does a caller with no
    /// principal. Other callers have the rights `shareWith` gives their
    /// id.
    ///
    /// **Idempotent on collision.** If the account already exists, the
    /// existing per-account state (nodes, state counter, change log)
    /// is preserved and this call is a no-op. To deliberately erase
    /// an account, drop the `MemoryBackend` and construct a new one,
    /// or use a future `reset_account` API.
    ///
    /// Rationale (bd:JMAP-510h.41): a re-registration in a long-lived
    /// process (e.g. a SIGHUP-driven config reload) must not silently
    /// drop the account's nodes. `HashMap::insert` would have done
    /// exactly that.
    ///
    /// # Panics
    ///
    /// In debug builds, if `account_id` is the empty string (RFC 8620
    /// §1.2, bd:JMAP-dx0s).
    #[must_use]
    pub fn with_account_owner(self, account_id: &Id, owner: &Id) -> Self {
        debug_assert!(
            !account_id.as_ref().is_empty(),
            "with_account: account_id must not be the empty string \
             (RFC 8620 §1.2, bd:JMAP-dx0s)"
        );
        self.inner
            .lock_unscoped()
            .accounts
            .entry(account_id.as_ref().to_owned())
            .or_insert_with(|| AccountStore::new(owner.clone()));
        self
    }

    /// Give `data_type` the draft-ietf-jmap-metadata-05 `metadata`
    /// property, configured by `info` (the DataTypeMetadataInfo of -05
    /// §1.2.1).
    ///
    /// Without this call FileNode has no metadata: `FileNode/set` rejects a
    /// write to `metadata` or `privateMetadata` with `invalidProperties`
    /// and `FileNode/query` rejects a metadata filter condition with
    /// `unsupportedFilter`. With it, `FileNode/set` validates metadata
    /// writes (-05 §3.2, §4.1), `FileNode/query` evaluates the -05 §3.6
    /// conditions, and `FileNode/changes` honours
    /// `ignoreMetadataOnlyChanges` and reports `updatedProperties` (-05
    /// §3.3). Advertising the capability in the session is the server's
    /// job: list `info` under `"FileNode"` in the account's
    /// `urn:ietf:params:jmap:metadata` `dataTypes`.
    ///
    /// -05 §4.1 names FileNode among the types with `myRights`, so an
    /// update that writes `metadata` needs `myRights.maySetMetadata`: on
    /// the node itself, or for a `File` or `SymLink`, which has no
    /// `myRights`, on its parent (draft-ietf-jmap-filenode-20 §5). The
    /// account owner holds it; a sharee holds it when a `shareWith` entry
    /// on the node or an ancestor grants `maySetMetadata`.
    /// A metadata-only change leaves `changed` alone, as -05 §2.2 requires.
    ///
    /// # Panics
    ///
    /// Panics if `data_type` is not `"FileNode"`, the one type this backend
    /// stores, or if `info.supports_private` is true: this backend does not
    /// keep `privateMetadata` per user (-05 §2).
    #[must_use]
    pub fn with_metadata_info(self, data_type: &str, info: DataTypeMetadataInfo) -> Self {
        assert!(
            data_type == FileNode::TYPE_NAME,
            "MemoryBackend supports metadata only on FileNode, not {data_type:?}"
        );
        assert!(
            !info.supports_private,
            "MemoryBackend does not support privateMetadata (supportsPrivate must be false)"
        );
        self.inner.lock_unscoped().metadata_info = Some(info);
        self
    }

    /// Give every account the draft-ietf-jmap-filenode-20 `maxFileNodeDepth`
    /// `max`, which [`FileNodeBackend::max_file_node_depth`] reports and the
    /// handlers enforce. Without this call there is no limit (`null`).
    #[must_use]
    pub fn with_max_file_node_depth(self, max: u64) -> Self {
        self.inner.lock_unscoped().max_file_node_depth = Some(max);
        self
    }

    /// Add a pre-existing node to an account (used to set up test fixtures).
    ///
    /// The state counter is NOT incremented — this is silent fixture setup.
    ///
    /// If the seeded `node.id` matches the reference-impl format `node-N`,
    /// the per-account id counter is synchronised so future
    /// [`FileNodeBackend::create_object`] allocations cannot collide with
    /// this fixture id (bd:JMAP-510h.61). Seeded ids in other shapes
    /// (UUIDs, hand-picked strings without the `node-` prefix or with a
    /// non-numeric suffix) are accepted unchanged and cannot collide
    /// with the allocator.
    ///
    /// # Caller-managed invariants
    ///
    /// `seed_node` is the test-fixture API — invariant checks the
    /// production [`FileNodeBackend`] write path enforces are **NOT**
    /// re-applied here. Test authors using `seed_node` are responsible
    /// for the following (bd:JMAP-510h.42):
    ///
    /// - **Dangling `parent_id` is allowed**: a seeded node may
    ///   reference a `parent_id` that has not been seeded. Some
    ///   handler tests deliberately exploit this to construct
    ///   "children-without-parent" states that exercise the
    ///   `nodeHasChildren` guard, the cycle-detection fallback, and
    ///   the `get_ancestors` short-circuit. Tests that rely on a
    ///   complete tree must seed the full chain themselves.
    /// - **Duplicate ids overwrite**: seeding the same id twice keeps
    ///   the second value (HashMap::insert semantics).
    /// - **No cycle check**: seeding A with parent=B and B with
    ///   parent=A produces a 2-cycle. [`FileNodeBackend::get_ancestors`]
    ///   uses a visited-set fallback so the walk terminates, but the
    ///   resulting ancestor chain is "everything in the cycle is its
    ///   own ancestor" — a wrong-shape chain. Avoid in tests that
    ///   need a correct ancestor sequence.
    ///
    /// # Panics
    ///
    /// Panics if `account_id` is not registered (call
    /// [`Self::with_account`] first). Previously this was a silent
    /// no-op — a test that calls `seed_node` before `with_account`
    /// would produce a passing-but-empty backend; the panic surfaces
    /// the setup mistake at the seed call site rather than as a
    /// confusing "fixture vanished" later (bd:JMAP-510h.42).
    pub fn seed_node(&self, account_id: &Id, node: FileNode) {
        let mut guard = self.inner.lock_unscoped();
        let store = guard
            .accounts
            .get_mut(account_id.as_ref())
            .unwrap_or_else(|| {
                panic!(
                    "seed_node: account_id {account_id:?} is not registered. \
                 Call MemoryBackend::with_account({account_id:?}) before seeding. \
                 Silently dropping the seed would produce a passing-but-empty \
                 backend and a hard-to-debug test failure later."
                )
            });
        store.sync_counter_with_seeded_id(&node.id);
        store.nodes.insert(node.id.clone(), node);
    }

    /// Seed a pre-existing object into the store without bumping the
    /// state counter. Test-fixture setup only. Mirrors
    /// `jmap_mail_server::memory::MemoryBackend::seed_object`; the
    /// object is stored through [`Self::seed_node`], whose
    /// caller-managed invariants apply.
    ///
    /// `type_name` is the `TYPE_NAME` of the one type this backend
    /// stores, `"FileNode"`.
    ///
    /// # Panics
    ///
    /// If `type_name` is not `"FileNode"`, `value` is not a JSON object
    /// whose `id` string equals `id`, `value` does not deserialize as a
    /// [`FileNode`], or `account_id` is not registered (see
    /// [`Self::seed_node`]).
    pub fn seed_object(
        &self,
        account_id: &Id,
        type_name: &'static str,
        id: &Id,
        value: serde_json::Value,
    ) {
        const KNOWN_TYPES: &[&str] = &[FileNode::TYPE_NAME];
        assert!(
            KNOWN_TYPES.contains(&type_name),
            "seed_object: type_name {type_name:?} is not one of the known \
             TYPE_NAME values {KNOWN_TYPES:?}"
        );
        let obj = value.as_object().unwrap_or_else(|| {
            panic!("seed_object: value must be a JSON object with an `id` field, got {value:?}")
        });
        let value_id = obj
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| {
                panic!(
                    "seed_object: value must contain an `id` string field; got value = {value:?}"
                )
            });
        assert_eq!(
            value_id,
            id.as_ref(),
            "seed_object: value's `id` field {value_id:?} does not match the \
             `id` argument {id:?}"
        );
        let node: FileNode = serde_json::from_value(value)
            .unwrap_or_else(|e| panic!("seed_object: value is not a FileNode: {e}"));
        self.seed_node(account_id, node);
    }

    /// Record the SHA-256 digest of blob `blob_id` in an account, as a
    /// server advertising `urn:ietf:params:jmap:cid` would know it
    /// (draft-atwood-jmap-cid-00 §FileNode Extension).
    ///
    /// [`FileNodeBackend::blob_sha256`] returns `Some(digest)` for a seeded
    /// blob and `None` (capability not advertised) for any other, so tests
    /// can exercise both sides without a blob store.
    ///
    /// # Panics
    ///
    /// Panics if `account_id` is not registered, as [`Self::seed_node`]
    /// does.
    pub fn seed_blob_sha256(&self, account_id: &str, blob_id: &str, digest: Sha256) {
        let mut guard = self.inner.lock_unscoped();
        let store = guard.accounts.get_mut(account_id).unwrap_or_else(|| {
            panic!(
                "seed_blob_sha256: account_id {account_id:?} is not registered. \
                 Call MemoryBackend::with_account({account_id:?}) first."
            )
        });
        store.blob_sha256.insert(Id::from(blob_id), digest);
    }
}

// ---------------------------------------------------------------------------
// JmapBackend impl
// ---------------------------------------------------------------------------

impl<C: MemoryCaller> JmapAccounts for MemoryBackend<C> {
    type Error = MemoryError;
    type CallerCtx = C;

    /// [`MemoryCaller::principal`].
    fn principal_id(caller: &C) -> Option<&Id> {
        caller.principal()
    }

    /// The info set by [`MemoryBackend::with_metadata_info`] for FileNode,
    /// the same for every account; `None` for any other type.
    fn metadata_info<O: JmapObject>(
        &self,
        _caller: &C,
        _account_id: &Id,
    ) -> Option<DataTypeMetadataInfo> {
        if O::TYPE_NAME != FileNode::TYPE_NAME {
            return None;
        }
        self.inner.lock_unscoped().metadata_info.clone()
    }

    async fn account_exists(&self, _caller: &C, account_id: &Id) -> Result<bool, Self::Error> {
        Ok(self
            .inner
            .lock()
            .await
            .accounts
            .contains_key(account_id.as_ref()))
    }
}

/// `node` as an `O`, by serde round-trip; `context` names the step for the
/// error.
fn decode_node<O: JmapObject>(node: &FileNode, context: &str) -> Result<O, MemoryError> {
    let v = serde_json::to_value(node)
        .map_err(|e| MemoryError::serde(format_args!("{context}: serialize FileNode"), &e))?;
    serde_json::from_value(v)
        .map_err(|e| MemoryError::serde(format_args!("{context}: deserialize"), &e))
}

impl<C: MemoryCaller> JmapReads for MemoryBackend<C> {
    /// draft-ietf-jmap-filenode-20 §5: "Nodes which are not "discoverable"
    /// MUST be reported in notFound if fetched with FileNode/get". Each
    /// node comes as the caller sees it: `myRights` its own rights, and
    /// without `mayRead` only the properties -20 §5 lists.
    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        caller: &C,
        account_id: &Id,
        ids: Option<&[Id]>,
        _properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        // Only FileNode objects are stored.  For any other type, return empty.
        // We detect FileNode by checking O::TYPE_NAME.
        if O::TYPE_NAME != "FileNode" {
            return Ok((vec![], vec![]));
        }

        let guard = self.inner.lock().await;
        let shown = guard.shown_nodes(account_id, caller.principal());

        match ids {
            None => {
                let mut nodes: Vec<O> = Vec::new();
                for node in shown.values() {
                    nodes.push(decode_node(node, "get_objects all")?);
                }
                Ok((nodes, vec![]))
            }
            Some(id_slice) => {
                let mut found: Vec<O> = Vec::new();
                let mut not_found: Vec<Id> = Vec::new();
                for id in id_slice {
                    if let Some(node) = shown.get(id) {
                        found.push(decode_node(node, "get_objects by-ids")?);
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
        _caller: &C,
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        let guard = self.inner.lock().await;
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => return Ok(State::from("0")),
        };
        Ok(State::from(store.state.to_string()))
    }

    /// The changes since `sinceState` as the caller sees them
    /// (draft-ietf-jmap-filenode-20 §5): a node that became discoverable to
    /// the caller is created, one that stopped being discoverable is
    /// destroyed, and one whose rights for the caller changed is updated.
    /// Changes to nodes the caller could not discover are left out.
    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        caller: &C,
        account_id: &Id,
        params: jmap_server::ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        let since_state = params.since_state;
        let max_changes = params.max_changes;
        // draft-ietf-jmap-metadata-05 §3: jmap_server::handle_changes has
        // already folded a `using` without the capability into this flag.
        let mode = if params.ignore_metadata_only_changes {
            MetadataOnlyChanges::Ignore
        } else {
            MetadataOnlyChanges::Report
        };
        let since: u64 = since_state
            .as_ref()
            .parse()
            .map_err(|_| BackendChangesError::CannotCalculate)?;

        let guard = self.inner.lock().await;
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => {
                return Ok(ChangesResult::new(
                    vec![],
                    vec![],
                    vec![],
                    false,
                    State::from("0"),
                ));
            }
        };

        let current_state = State::from(store.state.to_string());

        // Collect all changes after since_state. Under Ignore a
        // metadata-only update entry is dropped here: each entry names one
        // id, so an id stays in `updated` exactly when one of its updates
        // since sinceState touched more than metadata (-05 §3.3: "exclude
        // from the updated array any id whose only changes since
        // sinceState ... are confined to metadata and/or privateMetadata"),
        // and the dropped entries do not count toward maxChanges ("Excluded
        // ids do not count toward maxChanges"). Entries for other users,
        // and for nodes the caller could not discover, are dropped too.
        let sharee = caller.principal().filter(|u| **u != store.owner);
        let all: Vec<&ChangeEntry> = store
            .change_log
            .iter()
            .filter(|e| e.state > since)
            .filter(|e| e.audience.includes(sharee))
            .filter(|e| !(mode == MetadataOnlyChanges::Ignore && e.is_metadata_only_update()))
            .collect();

        // RFC 8620 §5.2: over maxChanges, return an intermediate state
        // with hasMoreChanges=true. newState is then the state of the last
        // included entry, so the next call resumes at the first cut-off one.
        let limit = limit_or_max(max_changes);
        let has_more_changes = all.len() > limit;
        let entries = &all[..all.len().min(limit)];
        let new_state = match entries.last() {
            Some(e) if has_more_changes => State::from(e.state.to_string()),
            _ => current_state,
        };

        // RFC 8620 §5.2 ID deduplication across the window, using the
        // canonical jmap-mail-server `IdFate` rules. `order` keeps first-seen
        // order so the response lists are deterministic.
        let mut fates: HashMap<Id, IdFate> = HashMap::new();
        let mut order: Vec<Id> = Vec::new();
        let mut window_props = ChangedProperties::new();
        for e in entries {
            let fate = match (e.change_type, fates.get(&e.id)) {
                (ChangeType::Created, _) => IdFate::Created,
                (ChangeType::Updated, Some(&f @ (IdFate::Created | IdFate::Destroyed))) => f,
                (ChangeType::Updated, _) => IdFate::Updated,
                // "If a record has been created AND destroyed since the old
                // state, the server SHOULD remove the id from the response
                // entirely." (RFC 8620 §5.2)
                (ChangeType::Destroyed, Some(IdFate::Created)) => {
                    fates.remove(&e.id);
                    continue;
                }
                (ChangeType::Destroyed, _) => IdFate::Destroyed,
                // Settled below from the id's whole window.
                (ChangeType::Restored, _) => IdFate::Updated,
            };
            if e.change_type == ChangeType::Updated {
                window_props.record(e.id.as_ref(), e.updated_props.as_ref());
            }
            if fates.insert(e.id.clone(), fate).is_none() {
                order.push(e.id.clone());
            }
        }

        // Object history -04 §5.1: an id restored in the window is
        // classified by whether it existed at sinceState and exists now,
        // "exactly as if the object had never been destroyed between them".
        let mut events: HashMap<Id, Vec<ChangeEvent>> = HashMap::new();
        for e in entries {
            let event = match e.change_type {
                ChangeType::Created => ChangeEvent::Created,
                ChangeType::Updated => ChangeEvent::Updated,
                ChangeType::Destroyed => ChangeEvent::Destroyed,
                ChangeType::Restored => ChangeEvent::Restored,
            };
            events.entry(e.id.clone()).or_default().push(event);
        }
        for (id, list) in &events {
            if !list.contains(&ChangeEvent::Restored) {
                continue;
            }
            let fate = match fold_restore(list) {
                Some(ChangeKind::Created) => IdFate::Created,
                Some(ChangeKind::Destroyed) => IdFate::Destroyed,
                Some(_) => IdFate::Updated,
                None => {
                    fates.remove(id);
                    continue;
                }
            };
            if fates.insert(id.clone(), fate).is_none() {
                order.push(id.clone());
            }
        }

        let mut created: Vec<Id> = Vec::new();
        let mut updated: Vec<Id> = Vec::new();
        let mut destroyed: Vec<Id> = Vec::new();
        // `fates.remove` emits each id once and skips ids omitted above.
        for id in order {
            match fates.remove(&id) {
                Some(IdFate::Created) => created.push(id),
                Some(IdFate::Updated) => updated.push(id),
                Some(IdFate::Destroyed) => destroyed.push(id),
                None => {}
            }
        }

        // -05 §3.3 `updatedProperties`; FileNode has none of the RFC 8621
        // §2.2 count properties that only Mailbox/changes may list.
        let updated_properties = window_props.updated_properties(&updated, mode, &[]);
        Ok(
            ChangesResult::new(created, updated, destroyed, has_more_changes, new_state)
                .with_updated_properties(updated_properties),
        )
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        caller: &C,
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        let QueryParams {
            filter,
            sort,
            limit,
            position,
            anchor,
            anchor_offset,
            ..
        } = params;
        // RFC 8620 §5.5: sort as asked, or reject what cannot be applied with
        // `unsupportedSort` — never silently ignore it (bd:JMAP-yt1w.27).
        // Same serde round-trip as the filter below.
        let sort: Vec<FileNodeComparator> = match sort {
            None => Vec::new(),
            Some(s) => serde_json::to_value(s)
                .and_then(serde_json::from_value)
                .map_err(|e| MemoryError::serde("parse sort", &e))?,
        };
        check_sort(&sort).map_err(BackendQueryError::Method)?;
        let guard = self.inner.lock().await;
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => {
                return Ok(QueryResult::new(
                    vec![],
                    0,
                    Some(0),
                    State::from("0"),
                    false,
                ));
            }
        };
        let current_state = State::from(store.state.to_string());

        // Parse the filter as FileNodeFilter via serde round-trip.
        // This avoids the #[non_exhaustive] restriction outside the defining
        // crate. Serde errors are propagated as MemoryError rather than
        // silently dropped — silent-drop on filter parse degenerates into
        // an unfiltered query, which is a query-correctness bug
        // (workspace AGENTS.md "Filter algebra exclusion §1"; bd:JMAP-510h.53).
        //
        // draft-ietf-jmap-metadata-05 §3.6: [`check_metadata_filter`]
        // validates the `metadata*` conditions first, so a malformed path is
        // `invalidArguments` and a condition while FileNode is not opted in
        // ([`MemoryBackend::with_metadata_info`]) is `unsupportedFilter`.
        let fc: Option<FileNodeFilter> = match filter {
            None => None,
            Some(f) => {
                let v = serde_json::to_value(f)
                    .map_err(|e| MemoryError::serde("serialize filter", &e))?;
                check_metadata_filter(&v, guard.metadata_info.as_ref())?;
                Some(
                    serde_json::from_value(v)
                        .map_err(|e| MemoryError::serde("parse filter", &e))?,
                )
            }
        };

        // draft-ietf-jmap-filenode-20 §5: undiscoverable nodes "MUST NOT be
        // returned in response to FileNode/query". The filter and sort run
        // over the nodes as the caller sees them, so for a node the caller
        // can discover but not read "a condition on a property that
        // FileNode/get returns as null for that node ... does not match the
        // node", and such a node sorts as lacking the property (-20 §3.2.5).
        let shown = guard.shown_nodes(account_id, caller.principal());
        let mut matched: Vec<&FileNode> = Vec::new();
        for node in shown.values() {
            let matches = match &fc {
                None => true,
                Some(f) => node_matches_tree(node, f, &shown)?,
            };
            if matches {
                matched.push(node);
            }
        }

        // Id breaks ties so pages stay stable across calls.
        matched.sort_by(|a, b| {
            compare_nodes(a, b, &sort).then_with(|| a.id.as_ref().cmp(b.id.as_ref()))
        });
        let ids: Vec<Id> = matched.into_iter().map(|n| n.id.clone()).collect();

        let total = ids.len() as u64;
        // RFC 8620 §5.5 position / anchor / anchorOffset.
        let start = resolve_query_start(&ids, position, anchor, anchor_offset)
            .map_err(BackendQueryError::Method)?;
        let ids: Vec<Id> = ids[start..]
            .iter()
            .take(limit_or_max(limit))
            .cloned()
            .collect();

        Ok(QueryResult::new(
            ids,
            start as u64,
            Some(total),
            current_state,
            false,
        ))
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &C,
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        let QueryChangesParams {
            since_query_state, ..
        } = params;
        let guard = self.inner.lock().await;
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => {
                return Ok(QueryChangesResult::new(
                    since_query_state.clone(),
                    State::from("0"),
                    Some(0),
                    vec![],
                    vec![],
                ));
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

/// Whether a change to `property` updates `changed`.
///
/// draft-ietf-jmap-filenode-20 §3.1.1: the server updates `changed`
/// "whenever such a property is modified", one "that has the same value for
/// every user", and "A change to a per-user property (isSubscribed, or
/// privateMetadata ...) ... need not update changed". -20 lists `metadata`
/// among the properties that do, but draft-ietf-jmap-metadata-05 §2.2 says
/// "Changes to metadata or privateMetadata alone MUST NOT update
/// server-maintained modification timestamps of the related object, such
/// as the changed property of FileNode ... This overrides any requirement
/// of those specifications", so `metadata` does not either.
fn updates_changed(property: &str) -> bool {
    !matches!(property, "changed" | "isSubscribed") && !is_metadata_property(property)
}

/// Every descendant of `id` among `nodes`.
fn descendant_ids(nodes: &HashMap<Id, FileNode>, id: &Id) -> Vec<Id> {
    let mut children: HashMap<&Id, Vec<&Id>> = HashMap::new();
    for node in nodes.values() {
        if let Some(parent) = node.parent_id.as_ref() {
            children.entry(parent).or_default().push(&node.id);
        }
    }
    let mut seen = std::collections::HashSet::from([id]);
    let mut pending = vec![id];
    let mut found = Vec::new();
    while let Some(current) = pending.pop() {
        for child in children.get(current).into_iter().flatten() {
            if seen.insert(child) {
                found.push((*child).clone());
                pending.push(child);
            }
        }
    }
    found
}

/// The ids of `parent` and its ancestors among `nodes` that carry a
/// non-empty `shareWith`, nearest first: the sources of the rights a child
/// of `parent` inherits. A cycle in a malformed tree ends the walk.
fn sharing_ancestors(nodes: &HashMap<Id, FileNode>, parent: Option<&Id>) -> Vec<Id> {
    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();
    let mut current = parent;
    while let Some(id) = current {
        if !seen.insert(id) {
            break;
        }
        let Some(node) = nodes.get(id) else { break };
        if node.share_with.as_ref().is_some_and(|s| !s.is_empty()) {
            found.push(id.clone());
        }
        current = node.parent_id.as_ref();
    }
    found
}

/// The `forbidden` SetError for destroying, renaming or moving a Directory
/// with a role (draft-ietf-jmap-filenode-20 §5).
fn role_forbidden() -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(
        SetError::new(SetErrorType::Forbidden)
            .with_description("a Directory with a role cannot be destroyed, renamed or moved"),
    ))
}

/// Rebuild `obj` with `id` as its `id` property, for a `/copy` that
/// stores a record read from another account under a new id.
fn with_id<O: SetObject>(obj: &O, id: &Id) -> Result<O, BackendSetError<MemoryError>> {
    let mut val = serde_json::to_value(obj)
        .map_err(|e| BackendSetError::Other(MemoryError::serde("serialize copied object", &e)))?;
    if let Some(map) = val.as_object_mut() {
        map.insert(
            "id".to_owned(),
            serde_json::Value::String(id.as_ref().to_owned()),
        );
    }
    serde_json::from_value(val)
        .map_err(|e| BackendSetError::Other(MemoryError::serde("deserialize copied object", &e)))
}

/// A `forbidden` SetError: a write "would violate an ACL or other
/// permissions policy" (RFC 8620 §5.3).
fn forbidden(description: &str) -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(
        SetError::new(SetErrorType::Forbidden).with_description(description),
    ))
}

/// The `notFound` SetError, also for a node the caller cannot discover
/// (draft-ietf-jmap-filenode-20 §5).
fn not_found() -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
}

/// A test of one right of a `FilesRights`.
type RightTest = fn(&FilesRights) -> bool;

/// The right of draft-ietf-jmap-filenode-20 §3.1.2 that changing
/// `property` takes, or `None` when it takes none checked here.
///
/// - `mayRename`: "rename or move this node, or change its role";
/// - `mayModifyContent`: "the content-related properties of this node
///   (blobId, mediaType, target, created, modified, accessed,
///   executable)", with the server-set `size` and `sha256` that follow
///   `blobId`;
/// - `mayShare`: "change the sharing of this node", `shareWith`.
///
/// `isSubscribed` "requires no rights beyond being able to discover the
/// node", and `metadata` takes `maySetMetadata`, checked with the other
/// draft-ietf-jmap-metadata-05 rules.
fn needed_right(property: &str) -> Option<(&'static str, RightTest)> {
    match property {
        "name" | "parentId" | "role" => Some(("mayRename", |r| r.may_rename)),
        "blobId" | "size" | "sha256" | "mediaType" | "target" | "created" | "modified"
        | "accessed" | "executable" => Some(("mayModifyContent", |r| r.may_modify_content)),
        "shareWith" => Some(("mayShare", |r| r.may_share)),
        _ => None,
    }
}

/// Check that a node may be placed under `parent` by a create or move.
///
/// draft-ietf-jmap-filenode-20 §5: "Moving a node also requires
/// mayAddChildren on the new parent, or mayCreateTopLevelFileNode to move
/// it to the top level", and §2.1: without `mayCreateTopLevelFileNode`
/// "creating a node with a null parentId, or moving one there, fails with
/// "forbidden"". In this backend only the account owner may create at the
/// top level. A parent the caller cannot discover is reported as missing,
/// so the answer does not reveal it.
fn check_parent(
    view: &access::View,
    sharee: Option<&Id>,
    parent: Option<&Id>,
) -> Result<(), BackendSetError<MemoryError>> {
    if sharee.is_none() {
        return Ok(());
    }
    match parent {
        None => Err(forbidden(
            "a node at the top level requires mayCreateTopLevelFileNode",
        )),
        Some(p) => match view.get(p) {
            None => Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(["parentId"])
                    .with_description("the parent does not exist"),
            ))),
            Some(s) if !s.rights.may_add_children => Err(forbidden(
                "adding a child requires mayAddChildren on the parent",
            )),
            Some(_) => Ok(()),
        },
    }
}

/// -20 §3.1.2 `shareWith`: "The owner of the node MUST NOT be in this
/// set."
fn check_share_with(node: &FileNode, owner: &Id) -> Result<(), BackendSetError<MemoryError>> {
    if node
        .share_with
        .as_ref()
        .is_some_and(|s| s.contains_key(owner))
    {
        return Err(BackendSetError::SetError(Box::new(
            SetError::new(SetErrorType::InvalidProperties)
                .with_properties(["shareWith"])
                .with_description("the owner of the node must not be in shareWith"),
        )));
    }
    Ok(())
}

/// What the viewer of `view` sees of `parent`, the rights a new child of
/// it inherits (-20 §5).
fn node_parent_sight(view: &access::View, parent: Option<&Id>) -> Option<access::Sight> {
    parent.and_then(|p| view.get(p)).cloned()
}

/// The source of a `/copy`: the account it is read from and its id there.
struct CopySource<'a> {
    account_id: &'a Id,
    id: &'a Id,
}

impl<C: MemoryCaller> MemoryBackend<C> {
    /// Store the FileNode `build` returns for a new id in `account_id`: the
    /// body of [`FileNodeBackend::create_object`] and
    /// [`FileNodeBackend::copy_object`]. `supplied` is the `/copy` create
    /// object whose metadata is validated (-05 §3.4); `None` validates the
    /// stored node itself, as a `FileNode/set` create does.
    ///
    /// The caller's rights are checked under the same lock as the insert
    /// (draft-ietf-jmap-filenode-20 §5): `mayAddChildren` on the parent,
    /// or the owner for a top-level node; `mayShare`, inherited from the
    /// parent, to give the node a `shareWith`; and for a copy, "mayRead on
    /// it" (-20 §3.2.4) in `source`.
    async fn store_created<O: SetObject>(
        &self,
        caller: &C,
        account_id: &Id,
        build: impl FnOnce(&Id) -> Result<O, BackendSetError<MemoryError>>,
        supplied: Option<&serde_json::Value>,
        source: Option<CopySource<'_>>,
    ) -> Result<(Id, O), BackendSetError<MemoryError>> {
        // Only FileNode is supported.
        if O::TYPE_NAME != "FileNode" {
            return Err(BackendSetError::SetError(Box::new(SetError::new(
                SetErrorType::InvalidProperties,
            ))));
        }

        let user = caller.principal();
        let mut guard = self.inner.lock().await;
        let metadata_info = guard.metadata_info.clone();
        let metadata = metadata_info.is_some();
        if let Some(source) = source {
            let from = guard.get_or_err(source.account_id).ok_or_else(not_found)?;
            match from.view_for(user, metadata).get(source.id) {
                None => return Err(not_found()),
                Some(sight) if !sight.readable() => {
                    return Err(forbidden("copying a node requires mayRead on it"));
                }
                Some(_) => {}
            }
        }
        let store = guard.get_mut_or_err(account_id).ok_or_else(|| {
            BackendSetError::SetError(Box::new(SetError::new(SetErrorType::InvalidProperties)))
        })?;
        let new_id = store.next_node_id();
        let obj = build(&new_id)?;
        // Store as a typed FileNode: serialize the built O and read it back.
        let mut v = serde_json::to_value(&obj).map_err(|e| {
            BackendSetError::Other(MemoryError::serde(
                format_args!("create_object: serialize {}", O::TYPE_NAME),
                &e,
            ))
        })?;
        if let Some(obj) = v.as_object_mut() {
            let supplied = supplied.and_then(serde_json::Value::as_object);
            check_metadata_create(metadata_info.as_ref(), supplied.unwrap_or(obj), obj)?;
            // draft-ietf-jmap-filenode-20 §3.1.1 `changed` is server-set;
            // §3.2.4 gives a copy "a new changed value".
            obj.insert(
                "changed".to_owned(),
                serde_json::json!(jmap_server::now_utc_string()),
            );
        }
        let mut node: FileNode = serde_json::from_value(v).map_err(|e| {
            BackendSetError::Other(MemoryError::serde(
                "create_object: deserialize FileNode",
                &e,
            ))
        })?;

        let sharee = user.filter(|u| **u != store.owner).cloned();
        let view = store.view_for(user, metadata);
        check_parent(&view, sharee.as_ref(), node.parent_id.as_ref())?;
        check_share_with(&node, &store.owner)?;
        // The new node's rights are first those it inherits from its
        // parent, so giving it a shareWith takes mayShare there.
        let inherited = node
            .parent_id
            .as_ref()
            .and_then(|p| view.get(p))
            .map(|s| s.rights.may_share);
        if sharee.is_some()
            && node.share_with.as_ref().is_some_and(|s| !s.is_empty())
            && inherited != Some(true)
        {
            return Err(forbidden("setting shareWith requires mayShare"));
        }

        // `myRights` is computed per caller, never stored; a sharee's
        // `isSubscribed` is the sharee's own (-20 §3.1.2).
        node.my_rights = None;
        let subscribed = match sharee.as_ref() {
            Some(u) => node.is_subscribed.take().map(|v| (u.clone(), v)),
            None => None,
        };
        let before = store.nodes.clone();
        store.nodes.insert(new_id.clone(), node);
        if let Some((u, v)) = subscribed {
            store.subscriptions.insert((new_id.clone(), u), v);
        }
        store.log_write(&before, ChangeType::Created, &new_id, None, metadata, false);

        // A sharee with mayAddChildren but not mayRead on the parent cannot
        // discover the node it created (-20 §5); it is shown with the
        // rights it inherits, so without the values it cannot read.
        let sight = store
            .view_for(user, metadata)
            .remove(&new_id)
            .or_else(|| {
                node_parent_sight(
                    &view,
                    store.nodes.get(&new_id).and_then(|n| n.parent_id.as_ref()),
                )
            })
            .unwrap_or_else(|| access::Sight {
                rights: FilesRights::default(),
            });
        let shown = store
            .nodes
            .get(&new_id)
            .map(|node| access::shown(node, &sight, store.subscription(&new_id, user)))
            .ok_or_else(|| {
                BackendSetError::Other(MemoryError::new("create_object: the node was not stored"))
            })?;
        let obj: O = decode_node(&shown, "create_object").map_err(BackendSetError::Other)?;
        Ok((new_id, obj))
    }
}

// ---------------------------------------------------------------------------
// FileNodeBackend impl
// ---------------------------------------------------------------------------

impl<C: MemoryCaller> FileNodeBackend for MemoryBackend<C> {
    async fn create_object<O, F>(
        &self,
        caller: &C,
        account_id: &Id,
        _create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<Self::Error>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        self.store_created(caller, account_id, |id| Ok(build(id)), None, None)
            .await
    }

    async fn copy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &C,
        from_account_id: &Id,
        to_account_id: &Id,
        obj: O,
        create: &serde_json::Value,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        // `obj.id` is still the source node's id in `from_account_id`.
        let source_id = serde_json::to_value(&obj)
            .ok()
            .and_then(|v| {
                v.get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(Id::from)
            })
            .ok_or_else(not_found)?;
        let source = CopySource {
            account_id: from_account_id,
            id: &source_id,
        };
        // -05 §3.4: the create object's metadata follows the Foo/set create
        // rules; the copied values are not revalidated.
        self.store_created(
            caller,
            to_account_id,
            |id| with_id(&obj, id),
            Some(create),
            Some(source),
        )
        .await
    }

    /// The rights of draft-ietf-jmap-filenode-20 §5 are re-checked under
    /// the store lock: a node the caller cannot discover is `notFound`;
    /// each changed property takes its right (see `needed_right`), a move
    /// also `mayAddChildren` on the new parent; and `isSubscribed` from a
    /// sharee changes only that sharee's value.
    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &C,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        // Only FileNode is supported.
        if O::TYPE_NAME != "FileNode" {
            return Err(not_found());
        }

        // Serialize patch as a JSON Value for JMAP-style path patching.
        //
        // RFC 8620 §5.3 PatchObject: keys may contain "/" separators naming a
        // path into nested objects (e.g. `"shareWith/alice"`). Null values
        // remove the target key; non-null values overwrite or create it.
        // A pointer whose leading parts do not exist is invalidPatch.
        let mut patch_val = serde_json::to_value(&patch).map_err(|e| {
            BackendSetError::Other(MemoryError::serde("update_object: serialize patch", &e))
        })?;
        let invalid_patch = |description: String| {
            BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidPatch).with_description(description),
            ))
        };
        // draft-ietf-jmap-metadata-05 §3.2: "Patch errors that can be
        // determined from the request alone (for example, two paths where
        // one is a prefix of the other) MUST be reported with
        // "invalidPatch" before rule 1 is evaluated."
        if let serde_json::Value::Object(patch_map) = &patch_val {
            check_patch_paths(patch_map)
                .map_err(|e: PatchPathError| invalid_patch(e.to_string()))?;
        }

        let user = caller.principal();
        let mut guard = self.inner.lock().await;
        let metadata_info = guard.metadata_info.clone();
        let metadata = metadata_info.is_some();
        // -05 §3.2 rule 1: "If the user cannot read the related object,
        // reject with "notFound"" comes before "forbidden"; -20 §5 makes a
        // node the caller cannot discover unreadable in that sense.
        let store = guard.get_mut_or_err(account_id).ok_or_else(not_found)?;
        let sharee = user.filter(|u| **u != store.owner).cloned();
        let view = store.view_for(user, metadata);
        let sight = view.get(id).ok_or_else(not_found)?.clone();
        let node = store.nodes.get(id).ok_or_else(not_found)?.clone();

        // -20 §3.1.2: a sharee's isSubscribed "changes only the current
        // user's value, and requires no rights beyond being able to
        // discover the node". Take it out of the patch of the stored node.
        let mut own_subscription = None;
        if let (Some(_), Some(map)) = (sharee.as_ref(), patch_val.as_object_mut())
            && let Some(v) = map.remove("isSubscribed")
        {
            // null restores the default, true.
            own_subscription = Some(v.as_bool().unwrap_or(true));
        }

        // Serialize the stored node to JSON, apply the patch as a JMAP patch,
        // then deserialize back to FileNode.
        let mut node_val = serde_json::to_value(&node).map_err(|e| {
            BackendSetError::Other(MemoryError::serde(
                "update_object: serialize stored FileNode",
                &e,
            ))
        })?;

        // -05 §4.1 `maySetMetadata`, which for a File or SymLink is its
        // parent's (draft-ietf-jmap-filenode-20 §5); `sight` already holds
        // the parent's rights for those.
        let may_set_metadata = sight.rights.may_set_metadata;

        let mut updated_props = None;
        if let (Some(obj), Some(patch_obj)) = (node_val.as_object_mut(), patch_val.as_object()) {
            // -05 §3.2 rules 1 to 3 run before the patch is applied.
            let info = check_metadata_set(
                metadata_info.as_ref(),
                SetKind::Update,
                patch_obj,
                Some(obj),
                may_set_metadata,
            )?;
            // -05 §2 `metadata` has "default: {}", so a
            // `metadata/<namespace>` pointer finds its parent.
            if info.is_some() {
                obj.entry("metadata")
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            }
            let before = obj.clone();
            // The value-dependent patch errors (missing parent, path through
            // an array or a Binary value) land here, after rule 3 and before rule 4 (-05 §3.2).
            apply_patch_object(obj, patch_obj).map_err(|e| invalid_patch(e.to_string()))?;
            if let Some(info) = info {
                // -05 §3.2: "In an update, null is equivalent to {}"; the
                // patch removed the property instead.
                for property in ["metadata", "privateMetadata"] {
                    if patch_obj
                        .get(property)
                        .is_some_and(serde_json::Value::is_null)
                    {
                        obj.insert(
                            property.to_owned(),
                            serde_json::Value::Object(serde_json::Map::new()),
                        );
                    }
                }
                // -05 §3.2 rule 4 runs on the value as it would be stored.
                check_metadata_depth(patch_obj, obj, info)
                    .map_err(|e| invalid_metadata_property(e.property(), e.description()))?;
            }
            let mut props = changed_properties(&before, obj);
            if props.iter().any(|p| updates_changed(p)) {
                obj.insert(
                    "changed".to_owned(),
                    serde_json::json!(jmap_server::now_utc_string()),
                );
                // -05 §3.3 `updatedProperties` names it too.
                props.insert("changed".to_owned());
            }
            updated_props = Some(props);
        }

        // A patched value of the wrong type is the client's error, not ours.
        let updated_node: FileNode = serde_json::from_value(node_val).map_err(|e| {
            BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties).with_description(e.to_string()),
            ))
        })?;

        // draft-ietf-jmap-filenode-20 §5: "A Directory with a role cannot
        // be destroyed, renamed or moved", re-verified under the lock.
        if node.role.is_some()
            && (updated_node.name != node.name || updated_node.parent_id != node.parent_id)
        {
            return Err(role_forbidden());
        }

        // -20 §5: "To rename or move a node, or change its role, the server
        // checks mayRename ... in the rights of the node itself; for a node
        // with no myRights, these are its parent's rights ... Without these
        // rights, the operation fails with "forbidden"."
        for property in updated_props.iter().flatten() {
            if let Some((right, holds)) = needed_right(property)
                && !holds(&sight.rights)
            {
                return Err(forbidden(&format!("changing {property} requires {right}")));
            }
        }
        if updated_node.parent_id != node.parent_id {
            check_parent(&view, sharee.as_ref(), updated_node.parent_id.as_ref())?;
        }
        check_share_with(&updated_node, &store.owner)?;

        // The history rights cut (object history §8.2): a change to the
        // shareWith in force over this node or its descendants.
        let inherited_rights_changed = updated_node.parent_id != node.parent_id
            && sharing_ancestors(&store.nodes, node.parent_id.as_ref())
                != sharing_ancestors(&store.nodes, updated_node.parent_id.as_ref());
        let share_with_changed = inherited_rights_changed
            || updated_props
                .as_ref()
                .is_some_and(|props| props.contains("shareWith"));
        // -20 §3.1.1: "The server SHOULD report a per-user change in
        // FileNode/changes only to the users whose value changed": an
        // update of the owner's isSubscribed alone is the owner's.
        let owner_only = updated_props
            .as_ref()
            .is_some_and(|p| !p.is_empty() && p.iter().all(|p| p == "isSubscribed"));
        let only_own_subscription =
            own_subscription.is_some() && updated_props.as_ref().is_none_or(BTreeSet::is_empty);

        let before = store.nodes.clone();
        store.nodes.insert(id.clone(), updated_node.clone());
        if !only_own_subscription {
            store.log_write(
                &before,
                ChangeType::Updated,
                id,
                updated_props,
                metadata,
                owner_only,
            );
        }
        if let (Some(u), Some(v)) = (sharee.as_ref(), own_subscription) {
            let key = (id.clone(), u.clone());
            if store.subscriptions.get(&key).copied().unwrap_or(true) != v || only_own_subscription
            {
                store.subscriptions.insert(key, v);
                store.bump_state(
                    ChangeType::Updated,
                    id.clone(),
                    Some(BTreeSet::from(["isSubscribed".to_owned()])),
                    Audience::Sharee(u.clone()),
                );
            }
        }
        let shown = store
            .view_for(user, metadata)
            .get(id)
            .map(|sight| access::shown(&updated_node, sight, store.subscription(id, user)));
        // Object history: the replaced version, and the §8.2 cut when
        // inherited rights changed (conservatively, for every FileNode of
        // the account).
        if let Some(h) = guard.history.as_mut() {
            if let Ok(previous) = serde_json::to_value(&node) {
                h.record_replace("FileNode", account_id, id, previous);
            }
            if share_with_changed {
                let now = h.now_ms();
                h.set_rights_changed("FileNode", account_id, now);
            }
        }

        // A node the update made undiscoverable to the caller is reported
        // without its new values.
        let Some(shown) = shown else {
            return Ok(None);
        };
        let result_obj: O = decode_node(&shown, "update_object").map_err(BackendSetError::Other)?;
        Ok(Some(result_obj))
    }

    /// draft-ietf-jmap-filenode-20 §5, under the store lock: a node the
    /// caller cannot discover is `notFound`; destroying needs `mayDelete`
    /// "in the rights of the node itself; for a node with no myRights,
    /// these are its parent's rights"; and "A destroy that would remove
    /// any of [the nodes the user cannot discover] ... MUST be rejected
    /// with "forbidden"", so a node with an undiscoverable descendant stays.
    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &C,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        let user = caller.principal();
        let mut guard = self.inner.lock().await;
        let metadata = guard.has_metadata();
        let store = guard.get_mut_or_err(account_id).ok_or_else(not_found)?;
        let view = store.view_for(user, metadata);
        let Some(sight) = view.get(id) else {
            return Err(not_found());
        };
        match store.nodes.get(id) {
            None => return Err(not_found()),
            // draft-ietf-jmap-filenode-20 §5, as in update_object.
            Some(node) if node.role.is_some() => return Err(role_forbidden()),
            Some(_) => {}
        }
        if !sight.rights.may_delete {
            return Err(forbidden("destroying a node requires mayDelete"));
        }
        if descendant_ids(&store.nodes, id)
            .iter()
            .any(|d| !view.contains_key(d))
        {
            return Err(forbidden(
                "destroying this node would remove nodes you cannot discover",
            ));
        }
        let before = store.nodes.clone();
        let removed = store.nodes.remove(id);
        store.subscriptions.retain(|(node, _), _| node != id);
        store.log_write(&before, ChangeType::Destroyed, id, None, metadata, false);
        if let (Some(h), Some(node)) = (guard.history.as_mut(), removed)
            && let Ok(last) = serde_json::to_value(&node)
        {
            h.record_destroy("FileNode", account_id, id, last);
        }
        Ok(())
    }

    /// As [`FileNodeBackend::destroy_object`] would answer about the
    /// caller's rights: the node is discoverable and the caller holds
    /// `mayDelete` on it (draft-ietf-jmap-filenode-20 §5).
    async fn may_destroy(
        &self,
        caller: &C,
        account_id: &Id,
        node: &FileNode,
    ) -> Result<bool, Self::Error> {
        let guard = self.inner.lock().await;
        let metadata = guard.has_metadata();
        Ok(guard.get_or_err(account_id).is_some_and(|store| {
            store
                .view_for(caller.principal(), metadata)
                .get(&node.id)
                .is_some_and(|s| s.rights.may_delete)
        }))
    }

    fn supports_type<O: JmapObject>(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
    ) -> bool {
        true
    }

    /// Each ancestor as the caller sees it. The ancestors of a node the
    /// caller can discover are discoverable too (-20 §5: "a sharee seeing
    /// the full path to anything that they have access to").
    async fn get_ancestors(
        &self,
        caller: &C,
        account_id: &Id,
        ids: &[Id],
    ) -> Result<Vec<FileNode>, Self::Error> {
        let guard = self.inner.lock().await;
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => return Ok(vec![]),
        };
        let shown = guard.shown_nodes(account_id, caller.principal());

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

        while !frontier.is_empty() {
            let mut next_frontier: Vec<Id> = Vec::new();
            for parent_id in frontier.drain(..) {
                if !visited.insert(parent_id.clone()) {
                    continue;
                }
                if let Some(node) = store.nodes.get(&parent_id) {
                    if let Some(seen) = shown.get(&parent_id) {
                        ancestors.push(seen.clone());
                    }
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
        _caller: &C,
        account_id: &Id,
        id: &Id,
    ) -> Result<Vec<Id>, Self::Error> {
        let guard = self.inner.lock().await;
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => return Ok(vec![]),
        };

        // bd:JMAP-510h.17 — build a parent_id → children index in a single
        // O(N) pass, then BFS over that index. Previous impl scanned all
        // nodes per frontier id (O(N*D) where N=total nodes, D=tree depth).
        // The reference impl is documented as guidance for production
        // backend authors; an O(N*D) tree traversal copied verbatim would
        // become a real perf bug.
        let mut children_by_parent: std::collections::HashMap<&Id, Vec<&Id>> =
            std::collections::HashMap::new();
        for node in store.nodes.values() {
            if let Some(parent_id) = node.parent_id.as_ref() {
                children_by_parent
                    .entry(parent_id)
                    .or_default()
                    .push(&node.id);
            }
        }

        // BFS via the index. Visited-set keys by Id (Hash + Eq) —
        // consistent with the FileNodeBackend::query_subtree default impl
        // in src/backend.rs. Visited-set guards against cycles introduced
        // by malformed fixtures (see seed_node's caller-managed-invariants
        // rustdoc, bd:JMAP-510h.42).
        let mut result: Vec<Id> = Vec::new();
        let mut frontier: Vec<&Id> = vec![id];
        let mut visited: std::collections::HashSet<Id> = std::iter::once(id.clone()).collect();

        while !frontier.is_empty() {
            let mut next_frontier: Vec<&Id> = Vec::new();
            for current_id in &frontier {
                if let Some(children) = children_by_parent.get(*current_id) {
                    for child_id in children {
                        if visited.insert((*child_id).clone()) {
                            result.push((*child_id).clone());
                            next_frontier.push(*child_id);
                        }
                    }
                }
            }
            frontier = next_frontier;
        }

        Ok(result)
    }

    async fn blob_exists(
        &self,
        _caller: &C,
        _account_id: &Id,
        _blob_id: &Id,
    ) -> Result<bool, Self::Error> {
        // In the test environment, all blobs are assumed to exist.
        Ok(true)
    }

    async fn blob_sha256(
        &self,
        _caller: &C,
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<Option<Sha256>, Self::Error> {
        Ok(self
            .inner
            .lock()
            .await
            .get_or_err(account_id)
            .and_then(|store| store.blob_sha256.get(blob_id).cloned()))
    }

    /// `true` once any blob digest has been seeded for the account via
    /// [`MemoryBackend::seed_blob_sha256`], the same signal that makes
    /// [`FileNodeBackend::blob_sha256`] report the capability.
    async fn cid_advertised(&self, _caller: &C, account_id: &Id) -> Result<bool, Self::Error> {
        Ok(self
            .inner
            .lock()
            .await
            .get_or_err(account_id)
            .is_some_and(|store| !store.blob_sha256.is_empty()))
    }

    /// The limit set by [`MemoryBackend::with_max_file_node_depth`].
    fn max_file_node_depth(&self, _caller: &C, _account_id: &Id) -> Option<u64> {
        self.inner.lock_unscoped().max_file_node_depth
    }

    /// [`crate::CaseFolding::Insensitive`] compares under
    /// `i;unicode-casemap` (draft-ietf-jmap-filenode-20 §1.1) with
    /// [`unicode_casemap_prepare`].
    async fn find_sibling_by_name(
        &self,
        _caller: &C,
        account_id: &Id,
        parent_id: Option<&Id>,
        name: &str,
        folding: crate::CaseFolding,
    ) -> Result<Option<Id>, Self::Error> {
        let guard = self.inner.lock().await;
        let store = match guard.get_or_err(account_id) {
            Some(s) => s,
            None => return Ok(None),
        };
        let search_name = fold_name(name, folding);
        Ok(store
            .nodes
            .values()
            .find(|node| {
                node.parent_id.as_ref() == parent_id
                    && fold_name(&node.name, folding) == search_name
            })
            .map(|node| node.id.clone()))
    }

    /// One pass over the parent's children, then the smallest free suffix.
    /// There is no attempt cap: at most `siblings + 1` candidates are tried,
    /// so this always returns `Some`. Folding matches
    /// [`Self::find_sibling_by_name`].
    async fn find_unique_name(
        &self,
        _caller: &C,
        account_id: &Id,
        parent_id: Option<&Id>,
        base_name: &str,
        folding: crate::CaseFolding,
    ) -> Result<Option<String>, Self::Error> {
        let fold = |s: &str| fold_name(s, folding).into_owned();
        let guard = self.inner.lock().await;
        let taken: std::collections::HashSet<String> = guard
            .get_or_err(account_id)
            .map(|store| {
                store
                    .nodes
                    .values()
                    .filter(|node| node.parent_id.as_ref() == parent_id)
                    .map(|node| fold(&node.name))
                    .collect()
            })
            .unwrap_or_default();
        Ok((1_u64..)
            .map(|n| format!("{base_name}-{n}"))
            .find(|candidate| !taken.contains(&fold(candidate))))
    }
}

/// `name` as compared under `folding`: unchanged for
/// [`crate::CaseFolding::Sensitive`], and for every other mode its RFC 5051
/// §2 `i;unicode-casemap` form (draft-ietf-jmap-filenode-20 §1.1).
fn fold_name(name: &str, folding: crate::CaseFolding) -> std::borrow::Cow<'_, str> {
    match folding {
        crate::CaseFolding::Sensitive => std::borrow::Cow::Borrowed(name),
        _ => std::borrow::Cow::Owned(unicode_casemap_prepare(name)),
    }
}

// ---------------------------------------------------------------------------
// Filter matching helper
// ---------------------------------------------------------------------------

/// Evaluate an RFC 8620 §5.5 filter tree over [`node_matches_filter`]:
/// "AND": all of the conditions must match, "OR": at least one, "NOT":
/// none.
///
/// Every child is evaluated, without short-circuiting, so an unsupported
/// condition anywhere in the tree is an `Err` regardless of its position.
/// An operator or filter variant this version does not know is
/// `unsupportedFilter` too, rather than a guessed match (RFC 8620 §5.5).
fn node_matches_tree(
    node: &FileNode,
    filter: &FileNodeFilter,
    nodes: &HashMap<Id, FileNode>,
) -> Result<bool, BackendQueryError<MemoryError>> {
    let op = match filter {
        Filter::Condition(c) => return node_matches_filter(node, Some(c), nodes),
        Filter::Operator(op) => op,
        _ => return Err(BackendQueryError::Method(JmapError::unsupported_filter())),
    };
    let mut any = false;
    let mut all = true;
    for child in &op.conditions {
        let m = node_matches_tree(node, child, nodes)?;
        any |= m;
        all &= m;
    }
    match op.operator {
        Operator::And => Ok(all),
        Operator::Or => Ok(any),
        Operator::Not => Ok(!any),
        _ => Err(BackendQueryError::Method(JmapError::unsupported_filter())),
    }
}

/// Returns `Ok(true)` if `node` satisfies the filter condition `fc`,
/// `Ok(false)` if it does not match, and `unsupportedFilter` if the
/// filter condition references a field that the reference implementation
/// does not support.
///
/// Implemented fields (all simple equality / inequality semantics):
/// - `parentId` — exact match on parent_id
/// - `isTopLevel` — parent_id is null
/// - `ancestorId` — `node` has the given id somewhere on its parent chain
/// - `descendantId` — `node` is an ancestor of the node with the given id
/// - `nodeType` — exact match on the `@type` string
/// - `role` — exact match on the role string
/// - `hasAnyRole` — node has a non-null role (or null role if false)
/// - `blobId` — exact match on blob_id
/// - `mediaType` — exact match on media_type
/// - `name` — exact byte match on name
/// - `nameMatch` / `mediaTypeMatch` — glob match (`*`, `?`, `[...]`),
///   case-insensitive under `i;unicode-casemap`
/// - `isExecutable` — exact match on the executable flag
/// - `minSize` / `maxSize` — size bounds (`size >= minSize` / `size < maxSize`)
/// - `createdBefore` / `createdAfter`
/// - `modifiedBefore` / `modifiedAfter`
/// - `accessedBefore` / `accessedAfter`
///
/// draft-ietf-jmap-filenode-20 §3.2.5: "The blobId, isExecutable, minSize,
/// maxSize, mediaType, mediaTypeMatch, and body conditions match only nodes
/// whose @type is "File". The role and hasAnyRole conditions match only
/// nodes whose @type is "Directory". This applies to both true and false
/// values."
///
/// Explicitly NOT supported (answered with `unsupportedFilter`, RFC 8620
/// §5.5, rather than a silent match-all that would look like a passing
/// query):
/// - `body` (full-text search in blob content)
/// - `text` (full-text search OR name OR media type)
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
) -> Result<bool, BackendQueryError<MemoryError>> {
    let fc = match fc {
        Some(f) => f,
        None => return Ok(true), // No filter — all nodes match.
    };

    // -20 §3.2.5: conditions on a type-specific property match only nodes
    // of that @type. `body` is left to the unsupportedFilter check below.
    let file_only = fc.blob_id.is_some()
        || fc.is_executable.is_some()
        || fc.min_size.is_some()
        || fc.max_size.is_some()
        || fc.media_type.is_some()
        || fc.media_type_match.is_some();
    if file_only && node.node_type != Some(NodeType::File) {
        return Ok(false);
    }
    let directory_only = fc.role.is_some() || fc.has_any_role.is_some();
    if directory_only && node.node_type != Some(NodeType::Directory) {
        return Ok(false);
    }

    if let Some(ref pid) = fc.parent_id
        && node.parent_id.as_ref() != Some(pid)
    {
        return Ok(false);
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
                return Err(BackendQueryError::Other(MemoryError::new(
                    "node_matches_filter: ancestor chain exceeds 10_000 depth, suspected cycle",
                )));
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
                return Err(BackendQueryError::Other(MemoryError::new(
                    "node_matches_filter: descendant chain exceeds 10_000 depth, suspected cycle",
                )));
            }
            cur = nodes.get(pid).and_then(|n| n.parent_id.as_ref());
        }
        if !found {
            return Ok(false);
        }
    }

    if let Some(ref nt) = fc.node_type {
        let node_nt_str = node.node_type.as_ref().map(NodeType::to_wire_str);
        if node_nt_str != Some(nt.to_wire_str()) {
            return Ok(false);
        }
    }

    if let Some(ref r) = fc.role {
        let node_role_str = node.role.as_ref().map(NodeRole::to_wire_str);
        if node_role_str != Some(r.to_wire_str()) {
            return Ok(false);
        }
    }

    if let Some(ref sid) = fc.space_id
        && node.space_id.as_ref() != Some(sid)
    {
        return Ok(false);
    }

    if let Some(has_role) = fc.has_any_role {
        let has = node.role.is_some();
        if has != has_role {
            return Ok(false);
        }
    }

    if let Some(ref bid) = fc.blob_id
        && node.blob_id.as_ref() != Some(bid)
    {
        return Ok(false);
    }

    if let Some(ref mt) = fc.media_type
        && node.media_type.as_deref() != Some(mt.as_str())
    {
        return Ok(false);
    }

    if let Some(ref name) = fc.name
        && node.name != *name
    {
        return Ok(false);
    }

    if let Some(ref pattern) = fc.name_match
        && !crate::glob::glob_match(pattern, &node.name)
    {
        return Ok(false);
    }

    if let Some(ref pattern) = fc.media_type_match {
        let mt = node.media_type.as_deref().unwrap_or("");
        if !crate::glob::glob_match(pattern, mt) {
            return Ok(false);
        }
    }

    if let Some(is_exec) = fc.is_executable
        && node.executable.unwrap_or(false) != is_exec
    {
        return Ok(false);
    }

    if let Some(min) = fc.min_size
        && node.size.unwrap_or(0) < min
    {
        return Ok(false);
    }

    if let Some(max) = fc.max_size
        && node.size.unwrap_or(0) >= max
    {
        return Ok(false);
    }

    if let Some(ref before) = fc.created_before
        && !date_strictly_before(node.created.as_ref(), before)
    {
        return Ok(false);
    }
    if let Some(ref after) = fc.created_after
        && !date_on_or_after(node.created.as_ref(), after)
    {
        return Ok(false);
    }
    if let Some(ref before) = fc.modified_before
        && !date_strictly_before(node.modified.as_ref(), before)
    {
        return Ok(false);
    }
    if let Some(ref after) = fc.modified_after
        && !date_on_or_after(node.modified.as_ref(), after)
    {
        return Ok(false);
    }
    if let Some(ref before) = fc.accessed_before
        && !date_strictly_before(node.accessed.as_ref(), before)
    {
        return Ok(false);
    }
    if let Some(ref after) = fc.accessed_after
        && !date_on_or_after(node.accessed.as_ref(), after)
    {
        return Ok(false);
    }

    // Full-text search is not implemented here. RFC 8620 §5.5: "A server
    // MUST reject the call with an unsupportedFilter error if it cannot
    // process the given filter."
    if fc.body.is_some() || fc.text.is_some() {
        return Err(BackendQueryError::Method(JmapError::unsupported_filter()));
    }

    // draft-ietf-jmap-metadata-05 §3.6 shared-metadata conditions, already
    // validated by `check_metadata_filter`. The `privateMetadata*` ones
    // never reach this point: with `supportsPrivate` false it rejects them.
    Ok(metadata_matches(
        node.metadata.as_ref(),
        fc.metadata_exists.as_deref(),
        fc.metadata_text_contains.as_ref(),
        fc.metadata_text_equals.as_ref(),
    ))
}

// ---------------------------------------------------------------------------
// draft-ietf-jmap-metadata-05 helpers
// ---------------------------------------------------------------------------

fn invalid_metadata_property(property: &str, description: &str) -> BackendSetError<MemoryError> {
    BackendSetError::SetError(Box::new(
        SetError::new(SetErrorType::InvalidProperties)
            .with_properties([property])
            .with_description(description),
    ))
}

/// draft-ietf-jmap-metadata-05 §3.2 rules 1 to 3 for a metadata write on a
/// FileNode, before the write is applied. Returns the DataTypeMetadataInfo
/// for the rule 4 check after applying, or `None` when `entry` writes no
/// metadata.
///
/// `entry` is the create object or update patch and `stored` the node
/// before an update (`None` on create). `may_set_metadata` is the
/// caller's `maySetMetadata` right on the node (`None` on create, where
/// the node has no rights yet). Rule 1's `notFound` is the caller's: the
/// update path reports a node the caller cannot discover as `notFound`
/// before this check.
fn check_metadata_set<'a>(
    info: Option<&'a DataTypeMetadataInfo>,
    kind: SetKind,
    entry: &serde_json::Map<String, serde_json::Value>,
    stored: Option<&serde_json::Map<String, serde_json::Value>>,
    may_set_metadata: Option<bool>,
) -> Result<Option<&'a DataTypeMetadataInfo>, BackendSetError<MemoryError>> {
    let Some(first) = entry.keys().find(|k| crate::helpers::is_metadata_key(k)) else {
        return Ok(None);
    };
    // -05 §1.2.1: an unlisted type "does not gain the metadata or
    // privateMetadata properties".
    let Some(info) = info else {
        return Err(invalid_metadata_property(
            first,
            "metadata is not supported on this data type",
        ));
    };
    // Rule 1 / -05 §4.1: "A write to metadata without maySetMetadata MUST
    // be rejected with a "forbidden" SetError". An absent right allows
    // the write (fail-open, JMAP-bghk).
    let writes_shared = entry.keys().any(|k| {
        crate::helpers::is_metadata_key(k)
            && k.split_once('/').map_or(k.as_str(), |(root, _)| root) == "metadata"
    });
    if writes_shared && may_set_metadata == Some(false) {
        return Err(BackendSetError::SetError(Box::new(
            SetError::new(SetErrorType::Forbidden)
                .with_description("maySetMetadata is required to write metadata"),
        )));
    }
    check_metadata_write(kind, entry, stored, info)
        .map_err(|e| invalid_metadata_property(e.property(), e.description()))?;
    Ok(Some(info))
}

/// draft-ietf-jmap-metadata-05 §3.2 checks for a created FileNode `obj`:
/// rules 1 to 3 on the metadata properties of `supplied`, the create
/// object, then rule 4 on the values it supplied as stored in `obj`. A
/// `FileNode/set` create passes `obj` as both; a `/copy` passes its create
/// object, so the copied values are not revalidated (-05 §3.4: "maxDepth
/// applies only to values supplied in the create object").
fn check_metadata_create(
    info: Option<&DataTypeMetadataInfo>,
    supplied: &serde_json::Map<String, serde_json::Value>,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), BackendSetError<MemoryError>> {
    let entry: serde_json::Map<String, serde_json::Value> = supplied
        .iter()
        .filter(|(k, _)| is_metadata_property(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if let Some(info) = check_metadata_set(info, SetKind::Create, &entry, None, None)? {
        check_metadata_depth(&entry, obj, info)
            .map_err(|e| invalid_metadata_property(e.property(), e.description()))?;
    }
    Ok(())
}

/// The six draft-ietf-jmap-metadata-05 §3.6 FilterCondition fields.
const METADATA_CONDITION_KEYS: [&str; 6] = [
    "metadataExists",
    "privateMetadataExists",
    "metadataTextContains",
    "privateMetadataTextContains",
    "metadataTextEquals",
    "privateMetadataTextEquals",
];

/// Validate the draft-ietf-jmap-metadata-05 §3.6 conditions on every leaf
/// of `filter`, a FilterCondition or FilterOperator tree (RFC 8620 §5.5),
/// before any node is matched.
///
/// Without `info` FileNode has no metadata (-05 §1.2.1: it "does not gain
/// the metadata or privateMetadata properties"), so a metadata leaf is
/// `unsupportedFilter` (RFC 8620 §5.5: "The filter is syntactically valid,
/// but the server cannot process it"). Otherwise
/// [`MetadataCondition::take_from`] applies -05 §3.6: a malformed path is
/// `invalidArguments`, and a `privateMetadata*` condition is
/// `unsupportedFilter` because `supportsPrivate` is false.
fn check_metadata_filter(
    filter: &serde_json::Value,
    info: Option<&DataTypeMetadataInfo>,
) -> Result<(), BackendQueryError<MemoryError>> {
    let unsupported = |description: String| {
        let mut err = JmapError::unsupported_filter();
        err.description = Some(description);
        BackendQueryError::Method(err)
    };
    if filter.get("operator").is_some() {
        return filter
            .get("conditions")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .try_for_each(|f| check_metadata_filter(f, info));
    }
    let Some(leaf) = filter.as_object() else {
        return Ok(());
    };
    if !METADATA_CONDITION_KEYS
        .iter()
        .any(|k| leaf.get(*k).is_some_and(|v| !v.is_null()))
    {
        return Ok(());
    }
    let Some(info) = info else {
        return Err(unsupported(
            "metadata filter conditions are not supported".to_owned(),
        ));
    };
    MetadataCondition::take_from(&mut leaf.clone(), info)
        .map(drop)
        .map_err(|e| match e {
            MetadataFilterError::InvalidArguments(d) => {
                BackendQueryError::Method(JmapError::invalid_arguments(d))
            }
            other => unsupported(other.to_string()),
        })
}

/// Match the draft-ietf-jmap-metadata-05 §3.6 shared-metadata conditions of
/// one filter leaf against a node's `metadata` (absent means `{}`, -05 §2).
fn metadata_matches(
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    exists: Option<&str>,
    contains: Option<&MetadataTextMatch>,
    equals: Option<&MetadataTextMatch>,
) -> bool {
    let empty = serde_json::Map::new();
    let metadata = metadata.unwrap_or(&empty);
    exists.is_none_or(|p| metadata_path_exists(metadata, p))
        && contains.is_none_or(|m| metadata_text_contains(metadata, m))
        && equals.is_none_or(|m| metadata_text_equals(metadata, m))
}

/// Reject a `FileNode/query` sort [`compare_nodes`] cannot apply.
///
/// RFC 8620 §5.5 `unsupportedSort`: "The sort is syntactically valid, but
/// includes a property the server does not support sorting on, or a
/// collation method it does not recognise." `tree` is rejected because
/// draft-ietf-jmap-filenode-20 §3.2.5 defines no descending tree order;
/// strings compare byte-wise, so `i;octet` (RFC 4790 §9.3) is the only
/// collation accepted.
fn check_sort(sort: &[FileNodeComparator]) -> Result<(), JmapError> {
    let supported = |c: &FileNodeComparator| {
        matches!(
            c.property,
            FileNodeComparatorProperty::Name
                | FileNodeComparatorProperty::MediaType
                | FileNodeComparatorProperty::Size
                | FileNodeComparatorProperty::Created
                | FileNodeComparatorProperty::Modified
                | FileNodeComparatorProperty::NodeType
        ) && c.collation.as_deref().is_none_or(|k| k == "i;octet")
    };
    if sort.iter().all(supported) {
        Ok(())
    } else {
        Err(JmapError::unsupported_sort())
    }
}

/// Order two nodes by `sort` (already passed through [`check_sort`]).
///
/// draft-ietf-jmap-filenode-20 §3.2.5: a node lacking the property sorts
/// before one that has it when ascending, and two lacking it fall through
/// to the next Comparator — exactly `Option`'s `None < Some` ordering.
/// `nodeType` puts Directories first, then symlinks, then files, then
/// other `@type` values by value. Dates compare as strings, matching the
/// `createdBefore`-style filters in this file.
fn compare_nodes(a: &FileNode, b: &FileNode, sort: &[FileNodeComparator]) -> std::cmp::Ordering {
    fn type_rank(n: &FileNode) -> Option<(u8, &str)> {
        n.node_type.as_ref().map(|t| match t {
            NodeType::Directory => (0, ""),
            NodeType::Symlink => (1, ""),
            NodeType::File => (2, ""),
            other => (3, other.to_wire_str()),
        })
    }
    sort.iter()
        .map(|c| {
            let ord = match c.property {
                FileNodeComparatorProperty::Name => a.name.cmp(&b.name),
                FileNodeComparatorProperty::MediaType => a.media_type.cmp(&b.media_type),
                FileNodeComparatorProperty::Size => a.size.cmp(&b.size),
                FileNodeComparatorProperty::Created => {
                    let d = |n: &FileNode| n.created.as_ref().map(|d| d.as_ref().to_owned());
                    d(a).cmp(&d(b))
                }
                FileNodeComparatorProperty::Modified => {
                    let d = |n: &FileNode| n.modified.as_ref().map(|d| d.as_ref().to_owned());
                    d(a).cmp(&d(b))
                }
                FileNodeComparatorProperty::NodeType => type_rank(a).cmp(&type_rank(b)),
                _ => std::cmp::Ordering::Equal,
            };
            if c.is_ascending { ord } else { ord.reverse() }
        })
        .find(|o| o.is_ne())
        .unwrap_or(std::cmp::Ordering::Equal)
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

// ---------------------------------------------------------------------------
// ConditionalBackend (draft-ietf-jmap-conditional-02)
// ---------------------------------------------------------------------------

/// Conditional FileNode/set (bd JMAP-y1yu.8): the whole store is one
/// [`ScopedState`], so a scope isolates every account, and an atomic
/// rollback restores nodes, state strings, change logs and the id counter
/// together. Commit checks the tree constraints the handler leaves to it
/// under `atomic` against the final state (`final_state_failures`).
impl<C: MemoryCaller> ConditionalBackend for MemoryBackend<C> {
    async fn begin_set(
        &self,
        _caller: &C,
        request: SetScopeRequest<'_>,
    ) -> Result<Box<dyn SetScope>, BeginSetError<MemoryError>> {
        Ok(self.inner.open(request, final_state_failures).await)
    }
}

/// The changes an atomic scope made that break a FileNode tree constraint
/// in the state it would commit (conditional-02 §4.2;
/// draft-ietf-jmap-filenode-20 §3.1.1, §3.2.3):
///
/// - two siblings with one name (`alreadyExists`, with `existingId`);
///   names compare exactly, because the handler keeps its own checks for a
///   case-insensitive request;
/// - a `parentId` chain that loops (`invalidProperties`);
/// - a node whose parent the scope destroyed (`nodeHasChildren`, on that
///   destroy), or whose parent does not exist (`invalidProperties`).
///
/// Only nodes the scope created or changed are reported, at most once
/// each.
fn final_state_failures(before: &MemoryState, after: &MemoryState) -> Vec<DeferredFailure> {
    let mut failures: Vec<DeferredFailure> = Vec::new();
    for (account, store) in &after.accounts {
        let old = before.accounts.get(account).map(|s| &s.nodes);
        let nodes = &store.nodes;
        for (id, node) in nodes {
            let op = match old.and_then(|o| o.get(id)) {
                None => SetOp::Create(id.clone()),
                Some(prev) if prev != node => SetOp::Update(id.clone()),
                Some(_) => continue,
            };
            let clash = nodes.iter().find(|(other, n)| {
                *other != id && n.parent_id == node.parent_id && n.name == node.name
            });
            let failure = if let Some((existing, _)) = clash {
                DeferredFailure::new(
                    op,
                    SetError::new(SetErrorType::AlreadyExists).with_existing_id(existing.clone()),
                )
            } else if parent_chain_loops(nodes, id) {
                DeferredFailure::new(
                    op,
                    SetError::new(SetErrorType::InvalidProperties).with_properties(["parentId"]),
                )
            } else if let Some(parent) = node.parent_id.as_ref().filter(|p| !nodes.contains_key(*p))
            {
                if old.is_some_and(|o| o.contains_key(parent)) {
                    DeferredFailure::new(
                        SetOp::Destroy(parent.clone()),
                        SetError::new(SetErrorType::from("nodeHasChildren")),
                    )
                } else {
                    DeferredFailure::new(
                        op,
                        SetError::new(SetErrorType::InvalidProperties)
                            .with_properties(["parentId"]),
                    )
                }
            } else {
                continue;
            };
            if !failures.iter().any(|f| f.op == failure.op) {
                failures.push(failure);
            }
        }
    }
    failures
}

/// Whether following `parentId` from `start` comes back to `start`.
fn parent_chain_loops(nodes: &HashMap<Id, FileNode>, start: &Id) -> bool {
    let mut at = start;
    // A chain without a loop through `start` ends within `len` steps.
    for _ in 0..nodes.len() {
        let Some((parent, _)) = nodes
            .get(at)
            .and_then(|n| n.parent_id.as_ref())
            .and_then(|p| nodes.get_key_value(p))
        else {
            return false;
        };
        if parent == start {
            return true;
        }
        at = parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a minimal FileNode by serde round-trip for fixture use.
    fn make_filenode(id: &str, name: &str) -> FileNode {
        serde_json::from_value(json!({
            "id": id,
            "name": name,
            "parentId": null,
            "role": null,
            "@type": "Directory",
            "target": null,
            "blobId": null,
            "size": null,
            "shareWith": null,
            "myRights": null,
            "created": "1970-01-01T00:00:00Z",
            "modified": "1970-01-01T00:00:00Z",
            "accessed": null
        }))
        .expect("fixture FileNode must deserialize")
    }

    /// Oracle: the wire strings `"home"` / `"File"` in hand-written JSON
    /// (draft-ietf-jmap-filenode-20 §3.2.5: `nodeType` and `role` are exact
    /// matches). A hand-built `NodeRole::Other("home")` carries the same
    /// wire string as `NodeRole::Home`, so it must match a Home node too.
    #[test]
    fn node_matches_filter_node_type_and_role() {
        let mut home: FileNode = make_filenode("h", "home");
        home.role = serde_json::from_value(json!("home")).expect("role");
        let file: FileNode = serde_json::from_value(json!({
            "id": "f", "name": "a.txt", "parentId": null, "role": null,
            "@type": "File", "blobId": "b1", "size": 0,
            "created": "1970-01-01T00:00:00Z", "modified": "1970-01-01T00:00:00Z"
        }))
        .expect("fixture File node must deserialize");
        let nodes = HashMap::new();
        let fc = |v| -> jmap_filenode_types::FileNodeFilterCondition {
            serde_json::from_value(v).expect("filter condition")
        };
        let matches = |n: &FileNode, c: &jmap_filenode_types::FileNodeFilterCondition| {
            node_matches_filter(n, Some(c), &nodes).expect("no cycle")
        };

        let by_role = fc(json!({ "role": "home" }));
        assert!(matches(&home, &by_role));
        assert!(!matches(&file, &by_role), "null role must not match");

        let by_type = fc(json!({ "nodeType": "File" }));
        assert!(matches(&file, &by_type));
        assert!(!matches(&home, &by_type), "Directory must not match File");

        let mut other_home = fc(json!({}));
        other_home.role = Some(NodeRole::Other("home".into()));
        assert!(matches(&home, &other_home));
        let mut other_file = fc(json!({}));
        other_file.node_type = Some(NodeType::Other("File".into()));
        assert!(matches(&file, &other_file));
    }

    /// Oracle: draft-ietf-jmap-filenode-20 §3.2.5, "The blobId,
    /// isExecutable, minSize, maxSize, mediaType, mediaTypeMatch, and body
    /// conditions match only nodes whose @type is "File". The role and
    /// hasAnyRole conditions match only nodes whose @type is "Directory".
    /// This applies to both true and false values."
    #[test]
    fn node_matches_filter_type_specific_conditions() {
        let dir = make_filenode("d", "dir");
        let file: FileNode = serde_json::from_value(json!({
            "id": "f", "name": "a.txt", "parentId": null, "@type": "File",
            "blobId": "b1", "size": 3, "type": "text/plain",
            "created": "1970-01-01T00:00:00Z", "modified": "1970-01-01T00:00:00Z"
        }))
        .expect("fixture File node must deserialize");
        let link: FileNode = serde_json::from_value(json!({
            "id": "l", "name": "l", "parentId": null, "@type": "SymLink",
            "target": ["a.txt"],
            "created": "1970-01-01T00:00:00Z", "modified": "1970-01-01T00:00:00Z"
        }))
        .expect("fixture SymLink node must deserialize");
        let nodes = HashMap::new();
        let matching = |c: serde_json::Value| -> Vec<&str> {
            let c: jmap_filenode_types::FileNodeFilterCondition =
                serde_json::from_value(c).expect("filter condition");
            [&dir, &file, &link]
                .into_iter()
                .filter(|n| node_matches_filter(n, Some(&c), &nodes).expect("no cycle"))
                .map(|n| n.id.as_ref())
                .collect()
        };

        for file_only in [
            json!({ "mediaTypeMatch": "*" }),
            json!({ "isExecutable": false }),
            json!({ "minSize": 0 }),
            json!({ "maxSize": 100 }),
        ] {
            assert_eq!(matching(file_only.clone()), ["f"], "{file_only}");
        }
        assert_eq!(matching(json!({ "hasAnyRole": false })), ["d"]);
    }

    /// Oracle: with_account on an existing account is a no-op — pre-existing
    /// nodes, state counter, and change log all survive (bd:JMAP-510h.41).
    /// The original `HashMap::insert` semantics would have replaced the
    /// account with a fresh empty `AccountStore`, silently dropping all
    /// pre-existing data.
    #[tokio::test]
    async fn with_account_preserves_existing_account_state() {
        let backend = MemoryBackend::new().with_account_owner(&Id::from("acc1"), &owner());
        backend.seed_node(&Id::from("acc1"), make_filenode("n1", "first"));

        // Confirm the seed landed.
        let (nodes, _missing) = backend
            .get_objects::<FileNode>(&owner(), &Id::from("acc1"), None, None)
            .await
            .expect("get_objects must succeed");
        assert_eq!(nodes.len(), 1, "seed must be observable before re-register");

        // Re-register the same account id. Pre-fix this destroyed
        // the pre-existing state; post-fix it must be a no-op.
        let backend = backend.with_account_owner(&Id::from("acc1"), &owner());

        let (nodes, _missing) = backend
            .get_objects::<FileNode>(&owner(), &Id::from("acc1"), None, None)
            .await
            .expect("get_objects must succeed after re-register");
        assert_eq!(
            nodes.len(),
            1,
            "pre-existing seed MUST survive a second with_account on the same id"
        );
        assert_eq!(
            nodes[0].id.as_ref(),
            "n1",
            "the surviving node must be the originally-seeded one"
        );
    }

    /// Oracle: a `node-N`-shaped seeded id bumps the per-account
    /// id-counter past N so the next `create_object` cannot allocate
    /// a colliding id. Regression guard for bd:JMAP-510h.61, which
    /// noted that the previous shape silently overwrote the seeded
    /// fixture on the first create call.
    #[tokio::test]
    async fn seed_node_with_collision_shaped_id_bumps_counter() {
        let backend = MemoryBackend::new().with_account_owner(&Id::from("acc1"), &owner());

        // Seed a fixture using the natural-looking `node-3` shape that a
        // contributor reading the public API (without peeking inside
        // memory.rs) would reach for. The bug being guarded is: the
        // counter starts at 0, next_node_id() emits "node-1" on the
        // first call, the HashMap insert silently overwrites "node-1"
        // — but our seed is "node-3", which is not yet at risk.
        // The real bug bites with a seed of "node-1" or any value <=
        // the current counter. We test both shapes here to lock in
        // both behaviours.
        let seed_1 = make_filenode("node-1", "fixture-root-1");
        let seed_3 = make_filenode("node-3", "fixture-root-3");
        backend.seed_node(&Id::from("acc1"), seed_1);
        backend.seed_node(&Id::from("acc1"), seed_3);

        // Create one new node. Before the fix this allocated "node-1"
        // and overwrote the fixture; after the fix it must allocate
        // "node-4" (next-after-the-highest-seed) so both fixtures
        // survive AND the create succeeds with a fresh id.
        let new_node: FileNode = serde_json::from_value(json!({
            "id": jmap_server::CREATE_ID_PLACEHOLDER,
            "name": "freshly-created",
            "parentId": null,
            "role": null,
            "@type": "Directory",
            "target": null,
            "blobId": null,
            "size": null,
            "shareWith": null,
            "myRights": null,
            "created": "2026-05-17T00:00:00Z",
            "modified": "2026-05-17T00:00:00Z",
            "accessed": null
        }))
        .expect("fresh FileNode must deserialize");
        let (allocated_id, _) = backend
            .create_object(&owner(), &Id::from("acc1"), "c1", |id: &Id| {
                let mut node = new_node;
                node.id = id.clone();
                node
            })
            .await
            .expect("create_object must succeed");
        assert_eq!(
            allocated_id.as_ref(),
            "node-4",
            "next_node_id must allocate after the highest seeded node-N: got {}",
            allocated_id.as_ref()
        );

        // Verify both fixtures survived.
        let (nodes, _missing) = backend
            .get_objects::<FileNode>(
                &owner(),
                &Id::from("acc1"),
                Some(&[Id::from("node-1"), Id::from("node-3"), Id::from("node-4")]),
                None,
            )
            .await
            .expect("get_objects must succeed");
        assert_eq!(
            nodes.len(),
            3,
            "all three nodes (2 seeds + 1 create) must be present: got {} ({:?})",
            nodes.len(),
            nodes.iter().map(|n| n.id.as_ref()).collect::<Vec<_>>()
        );
        let names: std::collections::HashSet<_> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(
            names.contains("fixture-root-1"),
            "fixture-root-1 (seed at node-1) must survive: got {names:?}"
        );
        assert!(
            names.contains("fixture-root-3"),
            "fixture-root-3 (seed at node-3) must survive: got {names:?}"
        );
        assert!(
            names.contains("freshly-created"),
            "the new node must be present: got {names:?}"
        );
    }

    /// Oracle: seed_node on an unknown account panics with an
    /// actionable message rather than silently dropping the seed.
    /// Regression guard for bd:JMAP-510h.42 — the previous shape
    /// produced a passing-but-empty backend when a test forgot to
    /// register the account first.
    #[tokio::test]
    #[should_panic(expected = "is not registered")]
    async fn seed_node_on_unknown_account_panics() {
        let backend = MemoryBackend::new(); // no with_account
        backend.seed_node(
            &Id::from("forgotten-acc"),
            make_filenode("node-1", "fixture"),
        );
    }

    /// Oracle: a seed with a non-matching id (UUID, arbitrary string)
    /// does NOT bump the counter — the `node-N` allocator is
    /// guaranteed not to collide with non-matching seeds.
    #[tokio::test]
    async fn seed_node_with_non_matching_id_leaves_counter_alone() {
        let backend = MemoryBackend::new().with_account_owner(&Id::from("acc1"), &owner());
        backend.seed_node(
            &Id::from("acc1"),
            make_filenode("550e8400-e29b-41d4-a716-446655440000", "uuid-seeded"),
        );
        backend.seed_node(
            &Id::from("acc1"),
            make_filenode("hand-picked", "named-seeded"),
        );
        backend.seed_node(
            &Id::from("acc1"),
            make_filenode("node-not-a-number", "almost-but-no"),
        );

        let new_node: FileNode = serde_json::from_value(json!({
            "id": jmap_server::CREATE_ID_PLACEHOLDER,
            "name": "first-create",
            "parentId": null,
            "role": null,
            "@type": "Directory",
            "target": null,
            "blobId": null,
            "size": null,
            "shareWith": null,
            "myRights": null,
            "created": "2026-05-17T00:00:00Z",
            "modified": "2026-05-17T00:00:00Z",
            "accessed": null
        }))
        .expect("fresh FileNode must deserialize");
        let (allocated_id, _) = backend
            .create_object(&owner(), &Id::from("acc1"), "c1", |id: &Id| {
                let mut node = new_node;
                node.id = id.clone();
                node
            })
            .await
            .expect("create_object must succeed");
        assert_eq!(
            allocated_id.as_ref(),
            "node-1",
            "non-matching seeds must not bump the counter; first create stays at node-1: got {}",
            allocated_id.as_ref()
        );
    }

    /// Oracle: `update_object` honors RFC 8620 §5.3 path-style PatchObject
    /// keys (`shareWith/<userId>`). The previous naive top-level-merge
    /// silently no-op'd path patches AND polluted the stored node with a
    /// literal `"shareWith/alice"` field via the workspace extras-
    /// preservation flatten. Regression guard for bd:JMAP-510h.23.
    #[tokio::test]
    async fn update_object_applies_path_style_share_with_patch() {
        use jmap_types::PatchObject;

        let backend = MemoryBackend::new().with_account_owner(&Id::from("acc1"), &owner());

        // Seed a Directory with shareWith already populated (one entry,
        // "existing"); draft-ietf-jmap-filenode-20 §3.1.2 defines shareWith
        // only on nodes whose @type permits children.
        // This makes the path-style mutations exercise both add (new userId)
        // and remove (existing userId → null) branches without first having
        // to materialize shareWith from null. The null-parent gap is a
        // separate concern.
        let node: FileNode = serde_json::from_value(json!({
            "id": "n1",
            "name": "shared-doc",
            "parentId": null,
            "role": null,
            "@type": "Directory",
            "shareWith": {
                "existing": {
                    "mayRead": true,
                    "mayAddChildren": false,
                    "mayRename": false,
                    "mayDelete": false,
                    "mayModifyContent": false,
                    "mayShare": false
                }
            },
            "myRights": null,
            "created": "2026-01-01T00:00:00Z",
            "modified": "2026-01-01T00:00:00Z",
            "accessed": null
        }))
        .expect("seed FileNode must deserialize");
        backend.seed_node(&Id::from("acc1"), node);

        // Patch: add "alice" via path-style; touch unrelated top-level
        // "name" to verify flat patches still work in the new helper.
        let mut patch_map = serde_json::Map::new();
        patch_map.insert("name".to_owned(), json!("renamed-shared-doc"));
        patch_map.insert(
            "shareWith/alice".to_owned(),
            json!({
                "mayRead": true,
                "mayAddChildren": false,
                "mayRename": false,
                "mayDelete": false,
                "mayModifyContent": false,
                "mayShare": false
            }),
        );
        let patch = PatchObject::from_map(patch_map);
        backend
            .update_object::<FileNode>(&owner(), &Id::from("acc1"), &Id::from("n1"), patch)
            .await
            .expect("first patch must succeed");

        // Read back via the typed API and assert shareWith has both entries
        // AND the top-level name was updated.
        let (nodes, _missing) = backend
            .get_objects::<FileNode>(&owner(), &Id::from("acc1"), Some(&[Id::from("n1")]), None)
            .await
            .expect("get_objects must succeed");
        assert_eq!(nodes.len(), 1, "n1 must be returned");
        assert_eq!(
            nodes[0].name, "renamed-shared-doc",
            "flat top-level patch must still apply"
        );
        let sw = nodes[0]
            .share_with
            .as_ref()
            .expect("shareWith must be Some after add");
        assert_eq!(
            sw.len(),
            2,
            "shareWith must contain both existing + alice after path-style add: {sw:?}"
        );
        assert!(
            sw.contains_key(&Id::from("alice")),
            "alice must be present after path-style add: {sw:?}"
        );
        assert!(
            sw.contains_key(&Id::from("existing")),
            "existing must survive path-style add of unrelated key: {sw:?}"
        );

        // Verify the extras catch-all stayed empty (no `shareWith/alice`
        // literal field leaked through the naive top-level merge).
        assert!(
            nodes[0].extra.is_empty(),
            "no path-key literal must land in extras: {:?}",
            nodes[0].extra
        );

        // Second patch: remove "existing" via path-style null.
        let mut remove_map = serde_json::Map::new();
        remove_map.insert("shareWith/existing".to_owned(), serde_json::Value::Null);
        let remove_patch = PatchObject::from_map(remove_map);
        backend
            .update_object::<FileNode>(&owner(), &Id::from("acc1"), &Id::from("n1"), remove_patch)
            .await
            .expect("remove patch must succeed");

        let (nodes, _missing) = backend
            .get_objects::<FileNode>(&owner(), &Id::from("acc1"), Some(&[Id::from("n1")]), None)
            .await
            .expect("get_objects must succeed");
        let sw = nodes[0]
            .share_with
            .as_ref()
            .expect("shareWith must remain Some after partial remove");
        assert_eq!(
            sw.len(),
            1,
            "shareWith must shrink to a single entry after path-style remove: {sw:?}"
        );
        assert!(
            sw.contains_key(&Id::from("alice")),
            "alice must survive removal of existing: {sw:?}"
        );
        assert!(
            !sw.contains_key(&Id::from("existing")),
            "existing must be gone after path-style null patch: {sw:?}"
        );
    }
}

#[cfg(test)]
mod memory_error_tests {
    use super::MemoryError;

    /// The `JmapAccounts::Error` contract: "Implementations MUST NOT
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

#[cfg(test)]
mod clone_tests {
    use super::{MemoryBackend, owner};
    use jmap_types::Id;

    /// Oracle (bd:JMAP-q7k6): `MemoryBackend::clone` is a shared-state
    /// clone, never a deep copy, so a clone and its original hold the
    /// same store while a fresh instance does not.
    #[test]
    fn clone_shares_store() {
        let a = MemoryBackend::new();
        let clone = a.clone();
        let fresh = MemoryBackend::new();
        let a = a.with_account_owner(&Id::from("acc"), &owner());
        let known = |b: &MemoryBackend| b.inner.lock_unscoped().accounts.contains_key("acc");
        assert!(known(&clone), "a clone sees the original's writes");
        assert!(!known(&fresh), "a fresh backend does not");
        drop(a);
    }
}

#[cfg(test)]
mod fixture_helper_tests {
    use super::*;
    use serde_json::json;

    /// Hand-written draft-ietf-jmap-filenode Directory node.
    fn dir_json(id: &str) -> serde_json::Value {
        json!({
            "id": id, "name": id, "parentId": null, "role": null,
            "@type": "Directory", "target": null, "blobId": null, "size": null,
            "shareWith": null, "myRights": null,
            "created": "1970-01-01T00:00:00Z", "modified": "1970-01-01T00:00:00Z",
            "accessed": null
        })
    }

    /// A node seeded through `seed_object` is readable and leaves the
    /// state at its initial value.
    #[tokio::test]
    async fn seed_object_is_readable_without_state_change() {
        let acc = Id::from("acc1");
        let backend = MemoryBackend::new().with_account_owner(&acc, &owner());
        let before = backend.get_state::<FileNode>(&owner(), &acc).await.unwrap();
        backend.seed_object(&acc, "FileNode", &Id::from("d1"), dir_json("d1"));
        let (found, not_found) = backend
            .get_objects::<FileNode>(&owner(), &acc, Some(&[Id::from("d1")]), None)
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, Id::from("d1"));
        assert!(not_found.is_empty());
        assert_eq!(
            backend.get_state::<FileNode>(&owner(), &acc).await.unwrap(),
            before
        );
    }

    #[test]
    #[should_panic(expected = "not one of the known")]
    fn seed_object_rejects_unknown_type_name() {
        let acc = Id::from("acc1");
        MemoryBackend::new()
            .with_account_owner(&acc, &owner())
            .seed_object(&acc, "Filenode", &Id::from("d1"), dir_json("d1"));
    }

    #[test]
    #[should_panic(expected = "does not match the `id` argument")]
    fn seed_object_rejects_id_mismatch() {
        let acc = Id::from("acc1");
        MemoryBackend::new()
            .with_account_owner(&acc, &owner())
            .seed_object(&acc, "FileNode", &Id::from("d1"), dir_json("d2"));
    }

    /// `seed_object` stores through `seed_node`, which refuses an
    /// unregistered account (bd:JMAP-510h.42).
    #[test]
    #[should_panic(expected = "is not registered")]
    fn seed_object_rejects_unregistered_account() {
        MemoryBackend::new().seed_object(
            &Id::from("acc1"),
            "FileNode",
            &Id::from("d1"),
            dir_json("d1"),
        );
    }
}

#[cfg(all(test, debug_assertions))]
mod empty_account_id_tests {
    use super::{Id, MemoryBackend, owner};

    /// Oracle: RFC 8620 §1.2, an Id "is a String of at least 1 and a
    /// maximum of 255 octets in size" (bd:JMAP-dx0s).
    #[test]
    #[should_panic(expected = "account_id must not be the empty string")]
    fn with_account_rejects_empty_id() {
        let _ = MemoryBackend::new().with_account_owner(&Id::from(""), &owner());
    }
}

#[cfg(test)]
mod sort_tests {
    use super::*;
    use serde_json::json;

    /// Oracle: draft-ietf-jmap-filenode-20 §3.2.5 — "Directories sort
    /// first, then symlinks, then files, then nodes of any other @type
    /// ordered by the @type value", and "nodes without the property sort
    /// before nodes that have it when isAscending is true. Nodes that lack
    /// the property are ordered among themselves as if that Comparator
    /// were absent". Each expected order is read off the hand-written
    /// fixtures below, not computed.
    #[test]
    fn compare_nodes_follows_draft_sort_rules() {
        let node = |v: serde_json::Value| -> FileNode {
            serde_json::from_value(v).expect("fixture FileNode must deserialize")
        };
        let dir = node(json!({"id": "d", "name": "m-dir", "@type": "Directory",
            "parentId": null, "created": "2020-01-03T00:00:00Z"}));
        let link = node(json!({"id": "l", "name": "z-link", "@type": "SymLink",
            "parentId": null, "target": ["x"], "created": "2020-01-01T00:00:00Z"}));
        let big = node(
            json!({"id": "b", "name": "a-big", "@type": "File", "parentId": null,
            "blobId": "b1", "size": 300, "mediaType": "text/plain",
            "created": "2020-01-02T00:00:00Z"}),
        );
        let small = node(
            json!({"id": "s", "name": "b-small", "@type": "File", "parentId": null,
            "blobId": "b2", "size": 10, "mediaType": "image/png",
            "created": "2020-01-04T00:00:00Z"}),
        );
        let odd = node(
            json!({"id": "o", "name": "c-odd", "@type": "Gadget", "parentId": null,
            "created": "2020-01-05T00:00:00Z"}),
        );
        let all = [&dir, &link, &big, &small, &odd];

        let order = |sort: serde_json::Value| -> Vec<&str> {
            let sort: Vec<FileNodeComparator> =
                serde_json::from_value(sort).expect("fixture sort must deserialize");
            check_sort(&sort).expect("sort must be supported");
            let mut v = all.to_vec();
            v.sort_by(|a, b| {
                compare_nodes(a, b, &sort).then_with(|| a.id.as_ref().cmp(b.id.as_ref()))
            });
            v.iter().map(|n| n.id.as_ref()).collect()
        };

        assert_eq!(
            order(json!([{"property": "name"}])),
            ["b", "s", "o", "d", "l"]
        );
        assert_eq!(
            order(json!([{"property": "name", "isAscending": false}])),
            ["l", "d", "o", "s", "b"]
        );
        assert_eq!(
            order(json!([{"property": "nodeType"}])),
            ["d", "l", "b", "s", "o"]
        );
        assert_eq!(
            order(json!([{"property": "created"}])),
            ["l", "b", "d", "s", "o"]
        );
        // d, l, o have no size: they sort first, then by the next
        // Comparator (name descending: o "c-odd", l "z-link", d "m-dir").
        assert_eq!(
            order(json!([{"property": "size"}, {"property": "name", "isAscending": false}])),
            ["l", "d", "o", "s", "b"]
        );
        assert_eq!(
            order(json!([{"property": "mediaType", "isAscending": false}])),
            ["b", "s", "d", "l", "o"]
        );
    }
}
