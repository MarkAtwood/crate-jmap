# jmap-filenode-client — Implementation Plan

JMAP FileNode extension (draft-ietf-jmap-filenode-13) method implementations on top
of `jmap-base-client`.

## Crate Family Position

```
jmap-types
    ├── jmap-filenode-types
    │       └── (types used here)
    └── jmap-base-client
            └── jmap-filenode-client  ← this crate
```

## What This Crate Is

An extension layer over `jmap-base-client` that adds typed methods for every JMAP
FileNode operation: `FileNode/get`, `FileNode/set`, `FileNode/changes`,
`FileNode/query`, `FileNode/queryChanges`, `FileNode/copy`.

Consumers call `jmap-base-client::JmapClient::call()` directly or use the typed
helpers defined here. No new HTTP machinery — all network operations go through
`jmap-base-client`.

## What This Crate Is Not

- Not a server-side crate
- Not a standalone HTTP client (no auth, no transport — that's `jmap-base-client`)
- Not handling the Direct HTTP Write endpoint (`PUT`/`PATCH` to `webWriteUrlTemplate`)
  — that is a plain HTTP operation outside the JMAP API envelope; consumers wire it
  up using `reqwest` or similar, reading the URL template from `FileNodeCapability`

## Source Material

Design pattern to follow:
- `~/PROJECT/JMAP/crate-jmap-mail-client/PLAN.md` and `src/` — identical extension
  trait pattern; copy the structure exactly
- `~/PROJECT/crate-jmapchat-client/src/methods/` — how method inputs/outputs are
  structured and how `JmapRequestBuilder` is used

Normative spec: `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-filenode-13.txt`

## Extension Trait Pattern

Cross-crate inherent impls are not valid Rust (orphan rule). To add methods to
`JmapClient` from this crate, use an **extension trait**:

```rust
pub trait JmapFileNodeExt {
    async fn file_node_get(...) -> Result<...>;
}

impl JmapFileNodeExt for JmapClient {
    async fn file_node_get(...) -> Result<...> { ... }
}
```

Callers must bring the trait into scope: `use jmap_filenode_client::JmapFileNodeExt;`

Rust 1.75 AFIT (async fn in trait, via RPITIT) is used — no `async-trait` crate.
This works because we do not need `dyn JmapFileNodeExt`. If dyn dispatch is ever
required, wrap with `async-trait 0.1` at that time.

## Planned Public API

```rust
use jmap_base_client::{ClientError, JmapClient};
use jmap_filenode_types::{FileNode, FileNodeFilterCondition, FileNodeCapability};
use jmap_types::{Id, State};

/// Extension trait adding JMAP FileNode methods to [`JmapClient`].
///
/// Import this trait to use: `use jmap_filenode_client::JmapFileNodeExt;`
pub trait JmapFileNodeExt {
    // ── FileNode/get (§3.2.1) ───────────────────────────────────────────────

    /// Fetch FileNodes by id.
    ///
    /// If `fetch_parents` is true, the response also includes all ancestor nodes
    /// of the requested ids (§3.2.1 `fetchParents` argument).
    async fn file_node_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
        fetch_parents: bool,
    ) -> Result<GetResponse<FileNode>, ClientError>;

    // ── FileNode/changes (§3.2.2) ───────────────────────────────────────────

    async fn file_node_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    // ── FileNode/set (§3.2.3) ───────────────────────────────────────────────

    /// Create, update, and/or destroy FileNodes.
    ///
    /// `req` carries `create`, `update`, `destroy`, and the FileNode-specific
    /// top-level arguments: `onDestroyRemoveChildren`, `onExists`, and
    /// `compareCaseInsensitively`.
    async fn file_node_set(
        &self,
        account_id: &Id,
        req: FileNodeSetRequest,
    ) -> Result<FileNodeSetResponse, ClientError>;

    // ── FileNode/copy (§3.2.4) ──────────────────────────────────────────────

    /// Copy FileNodes from another account.
    ///
    /// `req` carries `fromAccountId`, `create` (map of createId → copy params),
    /// `onDestroyRemoveChildren`, and `onExists`.
    async fn file_node_copy(
        &self,
        account_id: &Id,
        req: FileNodeCopyRequest,
    ) -> Result<FileNodeCopyResponse, ClientError>;

    // ── FileNode/query (§3.2.5) ─────────────────────────────────────────────

    /// Query FileNodes matching a filter.
    ///
    /// `depth` controls recursive descent into subdirectories (§3.2.5):
    /// `None` or `Some(0)` = no recursion, `Some(n)` = recurse n levels.
    async fn file_node_query(
        &self,
        account_id: &Id,
        req: FileNodeQueryRequest,
    ) -> Result<QueryResponse, ClientError>;

    // ── FileNode/queryChanges (§3.2.6) ──────────────────────────────────────

    async fn file_node_query_changes(
        &self,
        account_id: &Id,
        req: FileNodeQueryChangesRequest,
    ) -> Result<QueryChangesResponse, ClientError>;
}

impl JmapFileNodeExt for JmapClient {
    // implementations in filenode.rs
}
```

### Supporting request/response structs

These are thin wrappers that serialize to the correct JMAP wire format. Defined in
`filenode.rs`.

```rust
/// Request body for FileNode/set.
///
/// All fields mirror §3.2.3 exactly.
pub struct FileNodeSetRequest {
    pub if_in_state: Option<State>,
    pub create: Option<HashMap<String, FileNode>>,
    pub update: Option<HashMap<Id, jmap_types::PatchObject>>,  // RFC 8620 §5.3
    pub destroy: Option<Vec<Id>>,
    // FileNode-specific top-level arguments
    pub on_destroy_remove_children: bool,   // default false
    pub on_exists: Option<FileNodeOnExists>, // null | "replace" | "rename"
    pub compare_case_insensitively: bool,   // default false
}

/// The `onExists` policy for name collision handling.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileNodeOnExists {
    Replace,
    Rename,
}

/// Response body for FileNode/set.
pub struct FileNodeSetResponse {
    pub account_id: Id,
    pub old_state: Option<State>,
    pub new_state: State,
    pub created: Option<HashMap<String, FileNode>>,
    pub updated: Option<HashMap<Id, Option<FileNode>>>,
    pub destroyed: Option<Vec<Id>>,
    pub not_created: Option<HashMap<String, SetError>>,
    pub not_updated: Option<HashMap<Id, SetError>>,
    pub not_destroyed: Option<HashMap<Id, SetError>>,
}

/// Request body for FileNode/copy.
pub struct FileNodeCopyRequest {
    pub from_account_id: Id,
    pub if_from_in_state: Option<State>,
    pub if_in_state: Option<State>,
    pub create: HashMap<String, FileNodeCopyCreate>,
    pub on_destroy_remove_children: bool,
    pub on_exists: Option<FileNodeOnExists>,
}

/// One entry in the FileNode/copy `create` map.
pub struct FileNodeCopyCreate {
    pub id: Id,               // source node id in fromAccountId
    pub parent_id: Option<Id>, // destination parentId
    pub name: Option<String>,  // destination name (defaults to source name)
}

/// Response body for FileNode/copy.
pub struct FileNodeCopyResponse {
    pub from_account_id: Id,
    pub account_id: Id,
    pub old_state: Option<State>,
    pub new_state: State,
    pub created: Option<HashMap<String, FileNode>>,
    pub not_created: Option<HashMap<String, SetError>>,
}

/// Request body for FileNode/query.
pub struct FileNodeQueryRequest {
    pub filter: Option<FileNodeFilter>,
    pub sort: Option<Vec<FileNodeComparator>>,
    pub position: Option<i64>,
    pub anchor: Option<Id>,
    pub anchor_offset: Option<i64>,
    pub limit: Option<u64>,
    pub calculate_total: Option<bool>,
    pub depth: Option<u64>, // FileNode-specific (§3.2.5)
}

/// Request body for FileNode/queryChanges.
pub struct FileNodeQueryChangesRequest {
    pub filter: Option<FileNodeFilter>,
    pub sort: Option<Vec<FileNodeComparator>>,
    pub since_query_state: State,
    pub max_changes: Option<u64>,
    pub up_to_id: Option<Id>,
    pub calculate_total: Option<bool>,
    pub depth: Option<u64>,
}
```

## Module Layout

```
src/
  lib.rs      pub trait JmapFileNodeExt; impl JmapFileNodeExt for JmapClient; re-exports
  filenode.rs FileNode/get, FileNode/set, FileNode/changes, FileNode/query,
              FileNode/queryChanges, FileNode/copy request/response types;
              FileNodeOnExists enum
```

Note: all six methods are in `filenode.rs` because the crate has only one object type.
Split if the module grows unwieldy.

## Test Strategy

- All tests use `wiremock` via `jmap-base-client`'s HTTP layer — no live network
- Request serialization tests: construct a typed request, verify JSON matches
  draft-ietf-jmap-filenode-13 examples and field names
- Response deserialization tests: feed spec-grounded JSON, verify typed structs
- The capability example JSON from §2.1.1 is the primary oracle for `FileNodeCapability`
  deserialization

### Key test cases

- `file_node_get` with `fetch_parents: true` serializes `fetchParents: true`
- `file_node_set` with `on_destroy_remove_children: true` serializes correctly
- `file_node_set` with `on_exists: Some(FileNodeOnExists::Replace)` serializes as `"replace"`
- `file_node_set` with `on_exists: None` serializes as `null` (not absent)
- `file_node_set` response with `alreadyExists` SetError (which includes `existingId`) deserializes
- `file_node_copy` request serializes `fromAccountId` and nested create map
- `file_node_query` with `depth: Some(2)` serializes `depth: 2`
- `file_node_query` response with `total` and `ids` deserializes to `QueryResponse`

## Dependencies

```toml
jmap-types           = { path = "../crate-jmap-types" }
jmap-filenode-types  = { path = "../crate-jmap-filenode-types" }
jmap-base-client     = { path = "../crate-jmap-base-client" }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror  = "2"
```

No direct reqwest/tokio dependency — all I/O goes through `jmap-base-client`.
