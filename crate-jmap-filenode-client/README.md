# jmap-filenode-client

## What it is

Typed client methods for the JMAP FileNode extension ([draft-ietf-jmap-filenode-13]). Wraps
[`jmap-base-client`] transport with strongly-typed request builders and response types for
all 6 JMAP FileNode method names.

## What it's for

Implements draft-ietf-jmap-filenode method bindings on top of
`jmap-base-client`: `FileNode/get|changes|set|copy|query|queryChanges`.
Sibling of `jmap-mail-client` in the extension-client family — mirrors that
crate's shape. Depends on `jmap-base-client` for transport and session, and
on `jmap-filenode-types` for the wire types.

## How to use

```rust,no_run
use jmap_base_client::{BearerAuth, ClientConfig, JmapClient};
use jmap_filenode_client::{JmapFileNodeExt, FileNodeSetParams, FileNodeOnExists};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
// 1. Build a JmapClient (auth, base URL — see jmap-base-client docs).
let auth = BearerAuth::new("my-token")?;
let client = JmapClient::new_plain(auth, "https://jmap.example.com", ClientConfig::default())?;

// 2. Fetch the session object.
let session = client.fetch_session().await?;

// 3. Bind to a SessionClient for FileNode methods.
let sc = client.with_filenode_session(session);

// 4. List all FileNodes (directories and files).
let nodes = sc.file_node_get(None, None, None).await?;

// 5. Create a directory.
sc.file_node_set(
    Some(serde_json::json!({
        "new1": { "name": "Projects", "parentId": null }
    })),
    None,
    None,
    None,
).await?;

// 6. Create a file node with collision handling.
let params = FileNodeSetParams {
    on_destroy_remove_children: None,
    on_exists: Some(FileNodeOnExists::Rename),
    compare_case_insensitively: Some(true),
};
sc.file_node_set(
    Some(serde_json::json!({
        "new2": { "name": "report.pdf", "parentId": "dir1", "blobId": "blob-xyz" }
    })),
    None,
    None,
    Some(params),
).await?;
# Ok(())
# }
```

Id parameters are typed `&jmap_types::Id` (or `&[jmap_types::Id]` for slices)
to make invalid Ids unrepresentable. State tokens use `&jmap_types::State`.
Construct Ids with `Id::new_validated(s)` to enforce RFC 8620 §1.2 syntax at
the boundary, or with `Id::from(s)` when the value is known-valid (e.g.
already came back from a server response).

## Methods

All `pub async fn` on `SessionClient`:

| Method | Parameters | JMAP method | Returns |
|---|---|---|---|
| `file_node_get` | `ids, properties, fetch_parents` | `FileNode/get` | `GetResponse<FileNode>` |
| `file_node_changes` | `since_state, max_changes` | `FileNode/changes` | `ChangesResponse` |
| `file_node_set` | `create, update, destroy, params` | `FileNode/set` | `SetResponse<FileNode>` |
| `file_node_copy` | `from_account_id, create, on_destroy_remove_children, on_exists, compare_case_insensitively` | `FileNode/copy` | `SetResponse<FileNode>` |
| `file_node_query` | `filter, sort, position, limit, depth` | `FileNode/query` | `QueryResponse` |
| `file_node_query_changes` | `since_query_state, max_changes` | `FileNode/queryChanges` | `QueryChangesResponse` |

**`file_node_get`** — `fetch_parents: Some(true)` asks the server to include all ancestor
nodes of the requested IDs in the response list (§3.2.1).

**`file_node_copy`** — `on_destroy_remove_children: Option<bool>` is the first optional
parameter (added per §3.2.4). The `from_account_id` is a required positional parameter;
all collision-handling options (`on_exists`, `compare_case_insensitively`) follow.

**`file_node_query`** — `depth: Option<u64>` controls recursive descent: `None` or
`Some(0)` means no recursion; `Some(n)` recurses `n` levels into subdirectories.

## Response types

| Type | Used for |
|---|---|
| `GetResponse<FileNode>` | `file_node_get` — typed list of FileNode objects |
| `SetResponse<FileNode>` | `file_node_set`, `file_node_copy` — created/updated/destroyed maps |
| `ChangesResponse` | `file_node_changes` — added, updated, and destroyed ID lists |
| `QueryResponse` | `file_node_query` — ordered list of matching IDs |
| `QueryChangesResponse` | `file_node_query_changes` — added and removed ID lists |

## FileNodeSetParams

Passed as the `params` argument to `file_node_set` to set top-level `/set` arguments:

| Field | Type | Wire name | Description |
|---|---|---|---|
| `on_destroy_remove_children` | `Option<bool>` | `onDestroyRemoveChildren` | Cascade destroy to descendants (default: false) |
| `on_exists` | `Option<FileNodeOnExists>` | `onExists` | Collision policy for name conflicts |
| `compare_case_insensitively` | `Option<bool>` | `compareCaseInsensitively` | Case-fold name comparisons (default: false) |

All fields are `#[serde(skip_serializing_if = "Option::is_none")]` so `None` values are
omitted from the wire request.

## FileNodeOnExists

Controls what happens when a new node's name collides with an existing sibling:

| Variant | Wire value | Behaviour |
|---|---|---|
| `Replace` | `"replace"` | Destroy the existing node and create the new one in its place |
| `Rename` | `"rename"` | Suffix the new node's name (e.g. `"report-1.pdf"`) to avoid the collision |

When `onExists` is absent, the server defaults to returning an `alreadyExists` SetError
with `existingId` pointing to the conflicting node.

## How it works

Each method on `SessionClient` runs the same pipeline:

1. Validate arguments (typed `&Id` / `&[Id]` makes invalid Ids unrepresentable;
   defence-in-depth empty-state guards return `InvalidArgument` before any I/O).
2. Resolve `(api_url, account_id)` from the bound session for
   `urn:ietf:params:jmap:filenode`.
3. Build the method-arguments JSON.
4. Wrap it into a `JmapRequest` via `JmapRequestBuilder` with
   `using = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:filenode"]`.
5. POST it via `jmap_base_client::JmapClient::call`.
6. `extract_response::<T>` finds the typed result for call ID `"r1"`.

The `Jmap*Ext` extension trait (`JmapFileNodeExt`) adds the
`with_filenode_session(session)` accessor to `JmapClient`. The returned
`SessionClient` carries the session and exposes every FileNode method as a
typed `async fn`.

## Gotchas

- `file_node_query` with `depth > 0` sends exactly **one** HTTP request to the JMAP server with the `depth` integer as a JSON field. The O(depth) backend calls are made by the server handler internally as it calls `query_objects` per level against its storage backend — the client has no visibility into this and does not issue multiple HTTP round-trips. Backends can override `FileNodeBackend::query_subtree` to reduce server-side backend calls to a single recursive query.
- No integration tests against a real JMAP server; tests use request-shape oracles and
  serialization checks only.

## References

- **[draft-ietf-jmap-filenode-13]** — JMAP FileNode
- **[RFC 8620]** — JMAP Core

[draft-ietf-jmap-filenode-13]: https://www.ietf.org/archive/id/draft-ietf-jmap-filenode-13.txt
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-base-client`]: ../crate-jmap-base-client
