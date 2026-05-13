# jmap-filenode-server — Implementation Plan

JMAP FileNode extension (draft-ietf-jmap-filenode-13) method handlers. Plugs into
`jmap-server`'s `Dispatcher`. Backend-agnostic: defines a `FileNodeBackend` trait;
consumers provide the implementation.

## Crate Family Position

```
jmap-types
    ├── jmap-server              dispatcher
    └── jmap-filenode-types      data types
            └── jmap-filenode-server  ← this crate
```

## What This Crate Is

Method handler implementations for every JMAP FileNode method defined in
draft-ietf-jmap-filenode-13: `FileNode/get`, `FileNode/changes`, `FileNode/set`,
`FileNode/copy`, `FileNode/query`, `FileNode/queryChanges`.

Defines a `FileNodeBackend` trait that the application implements. The crate handles
all JMAP protocol semantics (tree integrity, name uniqueness, circular reference
prevention, partial success, onDestroyRemoveChildren cascading, onExists collision
handling, fetchParents expansion). The backend handles storage.

## What This Crate Is Not

- Not a full JMAP server
- Not coupled to any specific storage (SQLite, PostgreSQL, in-memory)
- Not handling auth — caller's responsibility before `Dispatcher::dispatch()`
- Not axum-specific — any `http`-based framework works
- Not handling Direct HTTP Write (PUT/PATCH to `webWriteUrlTemplate`) — that is
  an HTTP endpoint separate from the JMAP API endpoint; server consumers wire it
  up using backend methods if they choose to support it

## Source Material

### Normative

`~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-filenode-13.txt` — read the
relevant section before implementing each handler. Wire field names, error codes,
and behavioral requirements come from the spec, not from memory.

### Backend trait pattern — copy this

`~/PROJECT/crate-jmap/crate-jmap-mail-server/src/backend.rs`

The `MailBackend` trait (which itself follows `StorageBackend` from
`~/PROJECT/crate-jmapchat-server/jmapchat-server/src/backend.rs`) is the exact
pattern to follow for `FileNodeBackend`. Same AFIT pattern, same
`BackendChangesError`/`BackendSetError` error types, same
`ChangesResult`/`QueryResult` structs.

The key difference: `FileNodeBackend` extends `JmapBackend` (the supertrait from
`jmap-server`) for the read-side operations, then adds write and FileNode-specific
methods — exactly as `MailBackend` does.

## Capability URI

`urn:ietf:params:jmap:filenode` (§2.1, §10.1)

This string is the key used in both the session-level `capabilities` object (value
must be an empty object `{}`) and the per-account `accountCapabilities` object
(value is `FileNodeCapability` from `jmap-filenode-types`).

```rust
pub const CAPABILITY_URI: &str = "urn:ietf:params:jmap:filenode";
```

## RFC Method Coverage

| Method | Draft § | Handler notes |
|---|---|---|
| `FileNode/get` | §3.2.1 | standard get + `fetchParents` expansion |
| `FileNode/changes` | §3.2.2 | standard changes |
| `FileNode/set` | §3.2.3 | standard set + tree integrity + name collision + `onDestroyRemoveChildren` + `onExists` + `compareCaseInsensitively` |
| `FileNode/copy` | §3.2.4 | cross-account copy; inherits `onDestroyRemoveChildren` and `onExists` |
| `FileNode/query` | §3.2.5 | standard query + `depth` recursion argument |
| `FileNode/queryChanges` | §3.2.6 | standard queryChanges |

Total: 6 method registrations.

## FileNodeBackend Trait

Follows the `MailBackend` pattern from `crate-jmap-mail-server/src/backend.rs`.
Read-side operations inherited from `JmapBackend` supertrait. Write and
FileNode-specific operations defined here.

```rust
/// Storage backend for JMAP FileNode method handlers.
///
/// Read-side operations (`get_objects`, `get_state`, `get_changes`,
/// `query_objects`, `query_changes`) are inherited from [`JmapBackend`].
///
/// This trait is not object-safe by design (generic methods via AFIT).
/// Use `Arc<impl FileNodeBackend>` when sharing across tasks.
///
/// Implementor invariants:
/// 1. State monotonicity: `get_state` returns a different token after every
///    successful mutation. Token does not change on failure.
/// 2. Initial state: `"0"` is the valid initial state sentinel.
/// 3. Name uniqueness: the backend enforces the sibling name uniqueness
///    constraint at the storage layer. The handler also validates before
///    calling the backend, but the backend is the final authority.
/// 4. Blob lifetime: a blob referenced by a FileNode MUST NOT be garbage-
///    collected while the FileNode exists. The backend is responsible.
/// 5. Partial set success: per-object failures do not roll back other objects
///    in the same /set call (RFC 8620 §5.3).
#[allow(async_fn_in_trait)]
pub trait FileNodeBackend: JmapBackend {
    // ── Write operations (same pattern as MailBackend) ──────────────────────

    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    // ── FileNode-specific ────────────────────────────────────────────────────

    /// Fetch all ancestor FileNodes of the given ids, in no particular order.
    ///
    /// Used by `FileNode/get` when `fetchParents: true` (§3.2.1). The handler
    /// deduplicates results before returning. Returns only ancestors, not the
    /// requested nodes themselves (the handler already has those).
    ///
    /// Backends that store nodes in a flat table should traverse parentId links
    /// iteratively. Backends with adjacency lists or nested sets can do this
    /// more efficiently.
    fn get_ancestors(
        &self,
        account_id: &Id,
        ids: &[Id],
    ) -> impl Future<Output = Result<Vec<FileNode>, Self::Error>> + Send;

    /// Fetch all descendant FileNode ids of the given node, recursively.
    ///
    /// Used by the handler when `onDestroyRemoveChildren: true` to collect all
    /// nodes that must be destroyed. Also used for circular reference detection:
    /// if the proposed new parentId is in the descendant set of the node being
    /// moved, the move must be rejected with `invalidProperties`.
    ///
    /// The return value includes the ids of all descendants at all depths, but
    /// NOT the id of the node itself. The backend may return them in any order;
    /// the handler will destroy them leaf-first (or rely on the backend to do
    /// so atomically).
    fn get_descendant_ids(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<Vec<Id>, Self::Error>> + Send;

    /// Return true if `blob_id` exists in `account_id`'s blob store.
    ///
    /// Used by the handler when creating or updating a file node to verify
    /// that the provided blobId refers to an actual stored blob. A FileNode
    /// that references a non-existent blob must be rejected with
    /// `invalidProperties`.
    ///
    /// There is no default — a silently wrong default would cause every
    /// missing-blob error to be misreported.
    fn blob_exists(
        &self,
        account_id: &Id,
        blob_id: &Id,
    ) -> impl Future<Output = bool> + Send;

    /// Copy a FileNode from one account to another (§3.2.4).
    ///
    /// The handler handles the RFC 8620 response structure (`created`,
    /// `notCreated`). The backend handles the actual duplication. For file
    /// nodes, the backend must ensure the blob is accessible in the
    /// destination account (copy or share the blob reference as appropriate
    /// for the storage system).
    ///
    /// `onDestroyRemoveChildren` and `onExists` are top-level arguments on
    /// FileNode/copy (§3.2.4) with the same semantics as FileNode/set. The
    /// handler resolves collision policy before calling the backend; the backend
    /// receives the resolved destination parentId and name.
    fn copy_file_node(
        &self,
        from_account_id: &Id,
        node_id: &Id,
        to_account_id: &Id,
        to_parent_id: Option<&Id>,
        name: &str,
    ) -> impl Future<Output = Result<(Id, FileNode), BackendSetError<Self::Error>>> + Send;

    /// Return true if this backend supports the given JMAP object type.
    ///
    /// FileNodeBackend only handles FileNode, so this is primarily a hook for
    /// the session capability builder to confirm support. Backends that support
    /// all types unconditionally can return `true` always.
    fn supports_type<O: JmapObject>(&self) -> bool;
}

/// Register all JMAP FileNode handlers with a jmap-server Dispatcher.
///
/// After calling this, the dispatcher handles all 6 FileNode method names.
/// Wrap `backend` in `Arc` before passing — it is cloned into each handler.
pub fn register_filenode_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: FileNodeBackend + 'static,
    C: Clone + Send + 'static;

pub use backend::{
    BackendChangesError, BackendSetError,
    ChangesResult, QueryResult, QueryChangesResult, AddedItem,
};

pub const CAPABILITY_URI: &str = "urn:ietf:params:jmap:filenode";
```

## Key Design Decisions

### 1. FileNodeBackend extends JmapBackend (not a standalone trait)

Same structure as `MailBackend`. The supertrait `JmapBackend` (from `jmap-server`)
provides `get_objects`, `get_state`, `get_changes`, `query_objects`, `query_changes`.
Only write operations and FileNode-specific operations are defined here. Implementors
who already implement `MailBackend` or `ChatBackend` will find the contract familiar.

### 2. fetchParents is handler logic + one backend call

`FileNode/get` with `fetchParents: true` (§3.2.1) must return all ancestor nodes.
The handler fetches the requested nodes via `get_objects`, then calls `get_ancestors`
once with all requested IDs, deduplicates across both result sets, and returns the
union. The backend provides `get_ancestors` because the traversal is storage-specific
(flat table vs adjacency list vs nested set). The handler does the deduplication.

### 3. Tree integrity validation is split between handler and backend

The handler performs these checks before calling the backend:

- **Circular reference prevention**: before moving a node (updating `parentId`),
  the handler calls `get_descendant_ids` for that node. If the proposed new
  `parentId` is in the descendant set, the move is rejected with
  `invalidProperties` (§3.2.3: "attempt to move a node to a parent for which
  this node is also an ancestor is an error").
- **parentId existence and type**: before creating or moving a node, the handler
  verifies (via `get_objects<FileNode>`) that the proposed `parentId` exists and
  has `nodeType: "directory"`. Only directories may have children. A file or
  symlink as a proposed parent is `invalidProperties`.
- **blobId validation**: before creating or updating a file node, the handler
  calls `blob_exists`. A missing blobId is `invalidProperties`.
- **nodeType/blobId/target consistency**: handler enforces:
  - file: blobId non-null, target null
  - directory: blobId null, target null
  - symlink: blobId null, target non-null

The backend enforces name uniqueness atomically (it has the transaction). The
handler also checks name uniqueness via `query_objects` (filter by parentId and
name), but the backend is the final authority to prevent TOCTOU races.

### 4. onDestroyRemoveChildren — handler orchestrates, backend has get_descendant_ids

RFC §3.2.3: when `onDestroyRemoveChildren: true`, all child nodes must also be
destroyed and their IDs included in the `destroyed` response list.

The handler calls `get_descendant_ids` to get all descendant IDs, then calls
`destroy_object<FileNode>` for each (leaf-first ordering is not required — the
backend's transaction must handle it), and finally calls `destroy_object<FileNode>`
for the top node.

When `onDestroyRemoveChildren: false` (default): if the destroy set does not include
all children, the destroy must fail with `nodeHasChildren` (§10.2). The handler checks
this by calling `query_objects` (filter: parentId = this node's id) and verifying the
result is empty, or that all returned child IDs are also in the current destroy set.
Per §3.2.3, the server MUST NOT return `nodeHasChildren` if all children are being
destroyed in the same operation — the handler must perform this set membership check
before calling the backend.

### 5. onExists collision handling is handler logic

`onExists` (§3.2.3) controls behavior when a create or update would produce a name
collision with an existing sibling:

- `null` (default): reject with `alreadyExists` SetError; the SetError object MUST
  include an `existingId` property with the ID of the conflicting node.
- `"replace"`: destroy the existing node first. If that node is a directory with
  children and `onDestroyRemoveChildren` is false, respond with `nodeHasChildren`.
  The destroyed ID must appear in the `destroyed` response list.
- `"rename"`: the server chooses a non-colliding name and returns the new name in
  the `created` or `updated` response for that ID.

`compareCaseInsensitively: true` (§3.2.3) widens the collision check to be
case-insensitive for this request only.

The handler resolves the collision policy by querying existing siblings before
calling the backend. The backend enforces the final uniqueness constraint atomically.

### 6. FileNode/copy shares top-level arguments with FileNode/set

§3.2.4 explicitly states that `FileNode/copy` has the same additional top-level
arguments as `FileNode/set`: `onDestroyRemoveChildren` and `onExists`, with the
same semantics. The handler applies the same collision and destruction logic as
FileNode/set before calling `copy_file_node`.

Cross-account copy: when copying from `fromAccountId` to the current account,
the blob referenced by the source file node must be accessible in the destination.
The backend handles this — it may share a blob reference, copy the blob content,
or return `BackendSetError::Forbidden` if the source blob is inaccessible.

### 7. depth argument in FileNode/query

§3.2.5 adds a `depth: UnsignedInt|null` argument to `FileNode/query`. When absent,
null, or zero, no recursion — the query returns only direct matches. When non-zero,
the query also returns nodes that are descendants (up to `depth` levels) of any node
matching the filter.

This is passed through to the backend via `query_objects` as part of the filter
context. Backends that cannot support recursive queries must return results for
depth=0 only and the handler must detect depth>0 with a `cannotCalculateChanges`-
style error, or the backend may implement the recursion in the storage layer.
The exact mechanism (extra field on the query filter vs. a separate backend method)
is an implementation decision — document in backend.rs when implemented.

### 8. nodeType inference on create is handler logic

§3.2.3: if `nodeType` is absent on create, the handler infers it:
- `"file"` if blobId is non-null
- `"symlink"` if target is non-null
- `"directory"` otherwise

The handler sets the inferred nodeType before calling `create_object` so the backend
always receives an explicit nodeType.

### 9. size is server-set on create and on blobId update

§3.2.3: when `blobId` is provided, the server MUST set `size` from the blob. If the
client provides `size`, it MUST match the blob size (the handler should verify this;
if it cannot without fetching the blob, the backend must verify and return
`invalidProperties` if there is a mismatch). If the client updates `blobId` without
providing `size`, the backend sets `size` from the new blob and returns it in the
`updated` response map. The handler must include this in the response.

### 10. changed is server-set — never in patches

§3.2.3: `changed` is updated automatically by the server whenever any property is
modified. Clients cannot set it. The handler must strip `changed` from incoming
patches and the backend must set it on every mutation.

### 11. register_filenode_handlers is the entry point

One function registers all 6 method handlers with the caller's
`jmap-server::Dispatcher<C>`. The backend is wrapped in `Arc<B>` and cloned into
each handler closure — same pattern as `jmap-mail-server`.

### 12. Custom error type for nodeHasChildren

The spec (§10.2) defines `nodeHasChildren` as a JMAP-registered set error code.
This must be a named variant in the `SetErrorType` extension or a custom string.
The handler maps `BackendSetError::NodeHasChildren` to the wire value
`"nodeHasChildren"`. The `alreadyExists` error (with `existingId` property) is a
SetError with extra fields — use a newtype or an error struct that carries the
`existing_id: Id` field for JSON serialization.

## Module Layout

```
src/
  lib.rs        re-exports; register_filenode_handlers; CAPABILITY_URI const
  backend.rs    FileNodeBackend trait; custom error types (NodeHasChildren,
                AlreadyExistsError with existingId); re-exports from jmap-server
  filenode.rs   FileNode/get (with fetchParents), FileNode/changes,
                FileNode/set (with tree integrity, onDestroyRemoveChildren,
                onExists, compareCaseInsensitively, nodeType inference),
                FileNode/copy, FileNode/query (with depth), FileNode/queryChanges
```

Note: all six methods are in `filenode.rs` because the FileNode crate has only one
object type. Split into separate files if the module grows unwieldy.

## Test Strategy

A `MemoryBackend` in `tests/common/mod.rs` provides an in-memory `HashMap`-based
implementation of `FileNodeBackend`. It must correctly implement:
- `get_ancestors` (walk parentId links in the HashMap)
- `get_descendant_ids` (BFS/DFS over parentId index)
- `blob_exists` (check a HashSet of "uploaded" blob IDs)
- `copy_file_node` (duplicate the node, share the blob reference)

This serves as both the test harness and the canonical example for implementors.

```
tests/
  common/
    mod.rs          MemoryBackend implementation
  filenode_tests.rs
```

### Test cases to include (all with spec-grounded JSON oracles)

- `FileNode/get`: fetch file node; fetch directory node; fetch symlink node
- `FileNode/get` with `fetchParents: true`: returns ancestors; deduplicates shared ancestors
- `FileNode/get`: non-existent id returns notFound (not a 500)
- `FileNode/set` create: file node with valid blobId; directory node; symlink node
- `FileNode/set` create: infers `nodeType` from blobId / target / neither
- `FileNode/set` create: missing blobId for file → `invalidProperties`
- `FileNode/set` create: blobId non-null for directory → `invalidProperties`
- `FileNode/set` create: blobId non-null for symlink → `invalidProperties`
- `FileNode/set` create: target non-null for file → `invalidProperties`
- `FileNode/set` create: target non-null for directory → `invalidProperties`
- `FileNode/set` update: move node to new parent; verify tree integrity check
- `FileNode/set` update: attempt to move a directory into its own subtree → `invalidProperties`
- `FileNode/set` destroy: `onDestroyRemoveChildren: false`, children present → `nodeHasChildren`
- `FileNode/set` destroy: `onDestroyRemoveChildren: false`, all children in same destroy set → succeeds
- `FileNode/set` destroy: `onDestroyRemoveChildren: true` → all descendant IDs in `destroyed`
- `FileNode/set` create: name collision + `onExists: null` → `alreadyExists` with `existingId`
- `FileNode/set` create: name collision + `onExists: "replace"` → existing destroyed, new created
- `FileNode/set` create: name collision + `onExists: "rename"` → new name in `created` response
- `FileNode/set` create: name collision + `onExists: "replace"` but target is non-empty directory + `onDestroyRemoveChildren: false` → `nodeHasChildren`
- `FileNode/set` create: `compareCaseInsensitively: true` treats "Foo" and "foo" as colliding
- `FileNode/copy`: node copied to new account; source unchanged
- `FileNode/query`: filter by `parentId`; filter by `ancestorId`; filter by `nodeType`
- `FileNode/query`: `depth: 2` recursion returns nodes at depth 1 and 2 under matched folders
- `FileNode/changes`: returns changed IDs since state; maps to `cannotCalculateChanges`
- `FileNode/queryChanges`: returns added/removed deltas since queryState

## Dependencies

```toml
jmap-types         = { path = "../crate-jmap-types" }
jmap-filenode-types = { path = "../crate-jmap-filenode-types" }
jmap-server        = { path = "../crate-jmap-server" }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror  = "2"
tokio      = { version = "1", features = ["rt"] }
```

No MIME parsing libraries. No HTTP client. No database drivers.
