# jmap-filenode-server

## What it is

JMAP FileNode ([draft-ietf-jmap-filenode-16]) method handlers and the
`FileNodeBackend` trait. Plugs into [`jmap-server`]'s `Dispatcher`. Implements
all 6 FileNode method names. Storage-agnostic — consumers implement the
`FileNodeBackend` trait for their own data layer.

## What it's for

Implements draft-ietf-jmap-filenode — tree-structured FileNode objects with
cycle detection, `onExists` collision handling, and `onDestroyRemoveChildren`
cascade — so consumers can wire JMAP FileNode method dispatch into their HTTP
transport. Sibling to `jmap-mail-server` (the canonical extension-server
template) and the other `jmap-*-server` crates; all of them inter-depend
through the workspace `JmapHandler` trait in `jmap-server`. The consumer
supplies a `FileNodeBackend` impl (storage), a `CallerCtx` type (auth
identity), and wires the dispatcher into HTTP / SSE / WebSocket transport
themselves.

## How to use

```rust,ignore
use std::sync::Arc;
use jmap_filenode_server::{FileNodeBackend, register_filenode_handlers};
use jmap_server::Dispatcher;

// 1. Implement FileNodeBackend for your storage layer. The impl must also
//    satisfy the JmapBackend supertrait (account_exists, get_objects,
//    get_state, get_changes, query_objects, query_changes) and declare
//    `type Error` and `type CallerCtx`. See the FileNodeBackend trait
//    section below for the full surface.

// 2. Construct a dispatcher parameterised on your CallerCtx type, then
//    register all 6 FileNode methods in one call.
let mut dispatcher: Dispatcher<MyCallerCtx> = Dispatcher::new();
register_filenode_handlers(&mut dispatcher, Arc::new(my_backend));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, caller_ctx, session_state).await;
```

`Dispatcher<C>` requires `B::CallerCtx = C`; `register_filenode_handlers`
enforces the match at the type level. For a single-user dev server use
`Dispatcher<()>` with a backend whose `CallerCtx = ()`. For
multi-tenant production, define a typed caller context (e.g. a
principal-id newtype) and parameterise both sides on that type.

For a runnable, type-checked variant of this example, see the
[crate-level documentation](https://docs.rs/jmap-filenode-server) — the
doctest in `lib.rs` is verified by `cargo test --doc`.

After `register_filenode_handlers` returns, the dispatcher handles every method name listed in
the [Registered methods](#registered-methods) section below. The same `Arc<MyBackend>` can
be shared with other parts of your application.

## Registered methods

All 6 method names from draft-ietf-jmap-filenode-16 §3.2 are registered:

| Object | Methods |
|---|---|
| `FileNode` | `get`, `changes`, `set`, `copy`, `query`, `queryChanges` |

## FileNodeBackend trait

Implement this trait to connect the handlers to your storage system. The read-side methods
(`get_objects`, `get_state`, `get_changes`, `query_objects`, `query_changes`) are defined on
the `JmapBackend` supertrait (from `jmap-server`). `FileNodeBackend` adds write operations
and the four FileNode-specific structural queries required for tree management.

```rust,ignore
pub trait FileNodeBackend: JmapBackend {
    // --- Write operations ---

    /// Create a new FileNode. `build` receives the backend-assigned id and
    /// returns the object to store. Returns (assigned_id, created_object).
    /// create_id is the client-side creation key from the /set request.
    fn create_object<O, F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send;

    /// Apply a partial update (patch) to an existing FileNode.
    /// Returns Some(updated_object) if the backend modified server-set fields
    /// beyond the patch (RFC 8620 §5.3 echo); None if applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy a FileNode by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns `true` if this backend supports JMAP object type `O` for
    /// `account_id`, as seen by `caller`.
    fn supports_type<O: JmapObject>(&self, caller: &Self::CallerCtx, account_id: &Id) -> bool;

    // --- FileNode-specific structural queries (no defaults) ---

    /// Returns the ancestor chain of the given nodes from immediate parent to root.
    ///
    /// Used for: (1) cycle detection when moving a node; (2) fetchParents
    /// expansion in FileNode/get.
    fn get_ancestors(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        ids: &[Id],
    ) -> impl Future<Output = Result<Vec<FileNode>, Self::Error>> + Send;

    /// Returns all IDs that are descendants of the given node (children,
    /// grandchildren, etc.).
    ///
    /// Used for: (1) cycle detection — if proposed new parentId is in the
    /// descendant set, the move would create a cycle; (2) nodeHasChildren guard
    /// — if result is non-empty the node has children.
    fn get_descendant_ids(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<Vec<Id>, Self::Error>> + Send;

    /// Returns true if blob_id exists in account_id's blob store.
    ///
    /// Used by FileNode/set to validate blobId before creating a file node.
    fn blob_exists(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;

    /// Returns the id of any sibling node that already has the given name,
    /// or None if the name is unique within that parent.
    ///
    /// parent_id is None for the root level. folding selects the name
    /// comparison. Used by FileNode/set to enforce the alreadyExists constraint.
    fn find_sibling_by_name(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        parent_id: Option<&Id>,
        name: &str,
        folding: CaseFolding,
    ) -> impl Future<Output = Result<Option<Id>, Self::Error>> + Send;

    // Methods with defaults, each taking `caller: &Self::CallerCtx` first:
    // blob_sha256, cid_advertised, files_rights, query_subtree.
}
```

`BackendSetError<E>` is an enum over two variants:

- `BackendSetError::SetError(SetError)` — a semantic RFC 8620 SetError
  (`notFound`, `invalidProperties`, `forbidden`, `alreadyExists`, `nodeHasChildren`, etc.)
- `BackendSetError::Other(E)` — a storage-layer error that becomes a `serverFail` response

## Permission enforcement responsibilities

The handlers in this crate do **no** permission checking. Per the
workspace AGENTS.md "Caller identity (foundation seam)" rule, backends
are canonical for authorization: the handler computes the candidate
mutation and the backend's `/set` impl is the canonical point of
enforcement. Caller identity is read via
`JmapBackend::principal_id` on the `caller: &Self::CallerCtx`
parameter every backend method receives. The full contract is the
"Permission enforcement (backend canonical)" rustdoc on
`FileNodeBackend`; `files_rights` only reports rights.

The reference `memory::MemoryBackend` does **not** implement any of
the checks below — it returns `None` from `principal_id` and stores
whatever the handler hands it. Production backends MUST implement each
of the following or they will silently accept JMAP requests that
draft-ietf-jmap-filenode-16 mandates the server to reject. ACL denials
are `forbidden` SetErrors.

Rights are the `FilesRights` in `myRights` (§3.1.1), derived from the
`shareWith` ACLs on a node and its ancestors: "if mayRead is set on a
node, all its child nodes are also readable" (§5).

### FileNode/set and FileNode/copy (§2.1, §3.1.1, §3.2.3, §3.2.4)

1. **Create requires `mayAddChildren` on the parent**, or the
   account's `mayCreateTopLevelFileNode` (§2.1) when `parentId` is
   null.

2. **Update requires the right for each property changed:**
   `mayRename` to change `name` or `parentId` (plus `mayAddChildren`
   on a new parent); `mayModifyContent` to change `blobId`,
   `mediaType`, `target`, `modified`, `accessed` or `executable`;
   `mayShare` to change `shareWith`.

3. **Destroy requires `mayDelete`**, including on the existing node
   that an `onExists: "replace"` create overwrites.

4. **`FileNode/copy`** requires the source to be discoverable in
   `fromAccountId`; the copy is a create in `accountId` and follows
   check 1.

### Reads (§5)

5. **Discoverability.** "Nodes which are not "discoverable" MUST
   return notFound errors if fetched with FileNode/get and MUST NOT be
   returned in response to FileNode/query." `get_objects` and
   `query_objects` leave them out.

### Structural invariants are not ACLs

Cycle prevention on `parentId` changes, name uniqueness under a
parent, blob existence and the `nodeHasChildren` guard (§3.2.3) are
pre-checked by the handler and MUST be re-verified atomically by the
backend, as the `create_object`, `update_object` and `destroy_object`
rustdoc lists. They apply to every caller, so a backend enforces them
even when `principal_id` returns `None`.

### Caller-identity-less backends

The reference `MemoryBackend` returns `None` from `principal_id` and
is intentionally permissive (fail-open, bd:JMAP-bghk) — it is suitable
for single-user dev servers, fixtures, and the workspace's own
integration tests, but NOT for any deployment where more than one
principal can reach the JMAP dispatcher. A deployment that needs ACL
enforcement MUST override `JmapBackend::principal_id` and implement
each of the checks above; the workspace ships no production-grade
reference impl.

## How it works

### Registration

`register_filenode_handlers` uses `ClosureHandler` (provided by
`jmap-server`) to wrap each handler function and `Arc<B>` into a
`JmapHandler<C>` and registers it with the dispatcher. The dispatcher's
`CallerCtx` value is forwarded into each closure as `_ctx`; the standard
`handle_*` handler bodies receive `(Arc<B>, call_id, args)` only. One
`Arc::clone` per method name; no heap allocation per request.

### FileNode/set create — @type validation

Every creation object MUST include `@type` (`"File"`, `"Directory"`, or `"SymLink"`); the
handler does not infer it (draft-ietf-jmap-filenode-16 §3.1.1). A create with `@type`
absent or null fails with `invalidProperties` and `properties: ["@type"]`, as does a create
whose `@type` is not one of those three values. The handler then validates the
type-specific properties (§3.1.2–§3.1.4): a `"File"` requires `blobId` and must not set
`target`; a `"Directory"` must not set `blobId` or `target`; a `"SymLink"` requires a
non-empty `target` and must not set `blobId`. A violation is `invalidProperties` naming the
offending property. For `"File"` nodes, `blob_exists` is called to confirm the referenced
blob is accessible. `mediaType` (formerly `type` in -14) is a `"File"` property.

```json
{ "@type": "File", "name": "report.pdf", "parentId": "d1",
  "blobId": "b1", "mediaType": "application/pdf" }
```

### FileNode/set — `sha256` (draft-atwood-jmap-cid-00)

When the server advertises `urn:ietf:params:jmap:cid`, override
`FileNodeBackend::blob_sha256` to return the stored blob's digest. A `"File"` create
and an update that sets `blobId` store that digest as the node's server-set `sha256`
(a client-supplied value is dropped on create; an update patch that sets `sha256` to
anything but its current value fails with `invalidProperties`, per RFC 8620 §5.3),
and an update echoes it in `updated`. The
default returns `Ok(None)`, meaning the capability is not advertised: the node gets
no `sha256`. Return `Err` when the capability is advertised but the digest cannot be
computed; that create or update fails with `serverFail`.

### FileNode/set — onExists collision handling

When a new node's name collides with an existing sibling (detected via `find_sibling_by_name`):

- `onExists` absent or `null` — the create fails with `alreadyExists`, and the response
  includes `existingId` pointing to the conflicting node.
- `onExists: "replace"` — the existing node is destroyed (subject to children guard), then
  the new node is created.
- `onExists: "rename"` — the handler iterates through suffixed names (`name-1`, `name-2`, …)
  up to 100 attempts until a unique name is found. Beyond 100 attempts, `serverFail` is returned.
- `onExists: "newest"` — the handler compares the `modified` timestamp of the incoming node
  with the existing node. If the incoming timestamp is strictly later, proceeds as with
  `"replace"`. Otherwise, rejects with `alreadyExists`.

### FileNode/set destroy — nodeHasChildren guard and cascade

With `onDestroyRemoveChildren: false` (the default), `get_descendant_ids` is called before
each destroy. If the result is non-empty, the destroy is rejected with `nodeHasChildren` —
**unless** all children are also present in the same destroy request (RFC §3.2.3 §5.3), in
which case the destroy proceeds. With `onDestroyRemoveChildren: true`, all descendants are
destroyed first; all destroyed IDs (node and all descendants) appear in the `/set` response
`destroyed` list.

### FileNode/set update — cycle detection

When a `FileNode/set update` changes `parentId`, the handler calls `get_descendant_ids` on
the node being moved. If the proposed new `parentId` is contained in the descendant set,
the move would create a cycle and is rejected with `invalidProperties: ["parentId"]`.

### FileNode/get — fetchParents expansion

When the request includes `fetchParents: true`, the handler calls `get_ancestors` for each
requested node ID and appends the results (deduplicated by ID) to the response `list`.

### FileNode/query — depth parameter

When `depth > 0`, the handler calls `FileNodeBackend::query_subtree` with the initial
query result IDs as roots and the requested depth. IDs are deduplicated across levels before
returning. `depth: 0` or absent returns only direct matches of the base filter. The default
`query_subtree` implementation calls `query_objects` with a `parentId` filter once per level
(O(depth) backend calls); backends can override it with a single recursive query.

### FileNode/copy — cross-account copy

`FileNode/copy` copies nodes from `fromAccountId` into the authenticated account. Supports
`onExists`, `compareCaseInsensitively`, and `onDestroyRemoveChildren` with the same
semantics as `FileNode/set`.

## CallerCtx

`register_filenode_handlers` registers each method as a `ClosureHandler` that
forwards the dispatcher's `CallerCtx` value into the closure as `_ctx`. The standard
`handle_*` handler bodies ignore `_ctx` and receive only `(Arc<B>, call_id, args)`;
the value is still available for backends that register handlers individually via
`ClosureHandler`.

If you need per-request context — auth identity, tenant id, rate-limit token —
inside one of the standard `handle_*` functions, implement `JmapHandler<C>` directly
and register with `dispatcher.register(method_name, Arc::new(your_handler))`.

## Capability URI

Include this in your Session object's `capabilities` map:

```rust
pub use jmap_filenode_types::JMAP_FILENODE_URI;
// = "urn:ietf:params:jmap:filenode"
```

## Crate family

```
jmap-types
    ├── jmap-server              Dispatcher this plugs into
    ├── jmap-cid-types           Sha256 digest type (draft-atwood-jmap-cid-00)
    └── jmap-filenode-types      domain types (FileNode, NodeType, FilesRights, etc.)
            └── jmap-filenode-server  ← this crate
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will remain that way
until the family is published to crates.io.

## Gotchas

- `get_ancestors`, `get_descendant_ids`, `find_sibling_by_name`, and `blob_exists` have no
  default implementations; backends must implement all four.
- The `FileNode/query` depth expansion calls `FileNodeBackend::query_subtree` once. The **default** `query_subtree` implementation calls `query_objects` with a `parentId` filter once per depth level (O(depth) backend calls). Backends with a nested-sets model, closure table, or recursive CTE should override `query_subtree` with a single bulk query.
- The `onExists: "rename"` suffix loop is capped at 100 attempts; beyond that,
  `serverFail` is returned.
- No production storage backend ships with this crate. A tree-backed reference
  implementation `MemoryBackend` lives in `src/memory.rs` and is gated behind
  `feature = "memory"` — it is intended for tests, examples, and contributor
  smoke-testing only, and is explicitly not suitable for production use. Unit
  tests under `#[cfg(test)]` use this same `MemoryBackend` plus a thin
  fault-injection wrapper (`FaultyBackend`, kept inside `mod tests` for unit
  tests that cannot import from `tests/`) — mirroring the canonical
  `jmap-mail-server` test harness layout.

## References

- **[draft-ietf-jmap-filenode-16]** — JMAP FileNode (normative for all method semantics)
- **[RFC 8620]** — JMAP Core (request format, SetError, ResultReference, `/set` response shape)

[draft-ietf-jmap-filenode-16]: https://datatracker.ietf.org/doc/draft-ietf-jmap-filenode/16/
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-server`]: ../crate-jmap-server
