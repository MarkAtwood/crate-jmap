# jmap-filenode-server

JMAP FileNode ([draft-ietf-jmap-filenode-13]) method handlers for Rust. Plugs into
[`jmap-server`]'s `Dispatcher`. Implements all 6 FileNode method names.
Storage-agnostic — consumers implement the `FileNodeBackend` trait for their own data layer.

## Usage

```rust
use std::sync::Arc;
use jmap_filenode_server::{FileNodeBackend, register_filenode_handlers};
use jmap_server::Dispatcher;

// 1. Implement FileNodeBackend for your storage layer (see trait section below).
struct MyBackend { /* db pool, tree store, etc. */ }
impl FileNodeBackend for MyBackend { /* ... */ }

// 2. Wire all 6 FileNode methods into a Dispatcher in one call.
let mut dispatcher: Dispatcher<()> = Dispatcher::new();
register_filenode_handlers(&mut dispatcher, Arc::new(MyBackend { /* ... */ }));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, (), session_state).await;
```

After `register_filenode_handlers` returns, the dispatcher handles every method name listed in
the [Registered methods](#registered-methods) section below. The same `Arc<MyBackend>` can
be shared with other parts of your application.

## Registered methods

All 6 method names from draft-ietf-jmap-filenode-13 §3.2 are registered:

| Object | Methods |
|---|---|
| `FileNode` | `get`, `changes`, `set`, `copy`, `query`, `queryChanges` |

## FileNodeBackend trait

Implement this trait to connect the handlers to your storage system. The read-side methods
(`get_objects`, `get_state`, `get_changes`, `query_objects`, `query_changes`) are defined on
the `JmapBackend` supertrait (from `jmap-server`). `FileNodeBackend` adds write operations
and the four FileNode-specific structural queries required for tree management.

```rust
pub trait FileNodeBackend: JmapBackend {
    // --- Write operations ---

    /// Create a new FileNode.
    /// Returns (assigned_id, created_object). create_id is the client-side
    /// creation key from the /set request.
    fn create_object<O: SetObject + Send + Sync>(
        &self, account_id: &Id, create_id: &str, obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    /// Apply a partial update (patch) to an existing FileNode.
    /// Returns Some(updated_object) if the backend modified server-set fields
    /// beyond the patch (RFC 8620 §5.3 echo); None if applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self, account_id: &Id, id: &Id, patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy a FileNode by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self, account_id: &Id, id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns true if this account supports the given JMAP object type.
    fn supports_type<O: JmapObject>(&self) -> bool;

    // --- FileNode-specific structural queries (no defaults) ---

    /// Returns the ancestor chain of the given nodes from immediate parent to root.
    ///
    /// Used for: (1) cycle detection when moving a node; (2) fetchParents
    /// expansion in FileNode/get.
    fn get_ancestors(
        &self, account_id: &Id, ids: &[Id],
    ) -> impl Future<Output = Result<Vec<FileNode>, Self::Error>> + Send;

    /// Returns all IDs that are descendants of the given node (children,
    /// grandchildren, etc.).
    ///
    /// Used for: (1) cycle detection — if proposed new parentId is in the
    /// descendant set, the move would create a cycle; (2) nodeHasChildren guard
    /// — if result is non-empty the node has children.
    fn get_descendant_ids(
        &self, account_id: &Id, id: &Id,
    ) -> impl Future<Output = Result<Vec<Id>, Self::Error>> + Send;

    /// Returns true if blob_id exists in account_id's blob store.
    ///
    /// Used by FileNode/set to validate blobId before creating a file node.
    fn blob_exists(
        &self, account_id: &Id, blob_id: &Id,
    ) -> impl Future<Output = bool> + Send;

    /// Returns the id of any sibling node that already has the given name,
    /// or None if the name is unique within that parent.
    ///
    /// parent_id is None for the root level. case_insensitive controls the
    /// comparison. Used by FileNode/set to enforce the alreadyExists constraint.
    fn find_sibling_by_name(
        &self,
        account_id: &Id,
        parent_id: Option<&Id>,
        name: &str,
        case_insensitive: bool,
    ) -> impl Future<Output = Result<Option<Id>, Self::Error>> + Send;
}
```

`BackendSetError<E>` is an enum over two variants:

- `BackendSetError::SetError(SetError)` — a semantic RFC 8620 SetError
  (`notFound`, `invalidProperties`, `forbidden`, `alreadyExists`, `nodeHasChildren`, etc.)
- `BackendSetError::Other(E)` — a storage-layer error that becomes a `serverFail` response

## How it works

### Registration

`register_filenode_handlers` uses `ClosureHandlerWithCtx` (provided by
`jmap-server`) to wrap each handler function and `Arc<B>` into a
`JmapHandler<C>` and registers it with the dispatcher. The dispatcher's
`CallerCtx` value is forwarded into each closure as `_ctx`; the standard
`handle_*` handler bodies receive `(Arc<B>, call_id, args)` only. One
`Arc::clone` per method name; no heap allocation per request.

### FileNode/set create — nodeType inference and validation

When creating a FileNode, the handler infers `nodeType` from the creation object if not
explicitly set: if `blobId` is present the type is `"file"`, if `target` is present the
type is `"symlink"`, otherwise the type is `"directory"`. After inference, the handler
validates consistency (`blobId` must be null for directories and symlinks; `target` must be
null for files and directories). For file nodes, `blob_exists` is called to confirm the
referenced blob is accessible.

### FileNode/set — onExists collision handling

When a new node's name collides with an existing sibling (detected via `find_sibling_by_name`):

- `onExists` absent or `null` — the create fails with `alreadyExists`, and the response
  includes `existingId` pointing to the conflicting node.
- `onExists: "replace"` — the existing node is destroyed (subject to children guard), then
  the new node is created.
- `onExists: "rename"` — the handler iterates through suffixed names (`name-1`, `name-2`, …)
  up to 100 attempts until a unique name is found. Beyond 100 attempts, `serverFail` is returned.

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

`register_filenode_handlers` registers each method as a `ClosureHandlerWithCtx` that
forwards the dispatcher's `CallerCtx` value into the closure as `_ctx`. The standard
`handle_*` handler bodies ignore `_ctx` and receive only `(Arc<B>, call_id, args)`;
the value is still available for backends that register handlers individually via
`ClosureHandlerWithCtx`.

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
    └── jmap-filenode-types      domain types (FileNode, NodeType, FilesRights, etc.)
            └── jmap-filenode-server  ← this crate
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will remain that way
until the family is published to crates.io.

## Known Limitations

- `get_ancestors`, `get_descendant_ids`, `find_sibling_by_name`, and `blob_exists` have no
  default implementations; backends must implement all four.
- The `FileNode/query` depth expansion calls `FileNodeBackend::query_subtree` once. The **default** `query_subtree` implementation calls `query_objects` with a `parentId` filter once per depth level (O(depth) backend calls). Backends with a nested-sets model, closure table, or recursive CTE should override `query_subtree` with a single bulk query.
- The `onExists: "rename"` suffix loop is capped at 100 attempts; beyond that,
  `serverFail` is returned.
- No storage backend ships with this crate. A tree-backed `MockBackend` exists in the
  `test_support` module inside `src/lib.rs` for unit testing only; it is not suitable for
  production use.

## References

- **[draft-ietf-jmap-filenode-13]** — JMAP FileNode (normative for all method semantics)
- **[RFC 8620]** — JMAP Core (request format, SetError, ResultReference, `/set` response shape)

[draft-ietf-jmap-filenode-13]: https://www.ietf.org/archive/id/draft-ietf-jmap-filenode-13.txt
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-server`]: ../crate-jmap-server

## License

MIT OR Apache-2.0
