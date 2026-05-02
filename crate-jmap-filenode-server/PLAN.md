# jmap-filenode-server — Implementation Plan

Backend-agnostic JMAP FileNode method handlers.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-filenode-13.txt`

## Crate Family Position

```
jmap-types
    └── jmap-filenode-types
            └── jmap-filenode-server  ← this crate
```

## What This Crate Is

Method handlers for JMAP FileNode, plugged into `jmap-server::Dispatcher` via
`register_filenode_handlers(dispatcher, backend)`.

## Methods (draft-ietf-jmap-filenode-13)

- `FileNode/get` — fetch file/folder nodes by id
- `FileNode/set` — create, update, delete file/folder nodes
- `FileNode/changes` — incremental sync
- `FileNode/query` — list nodes matching a filter (e.g. children of a folder)
- `FileNode/queryChanges` — incremental query results
- `FileNode/copy` — copy a node (possibly cross-account)

## Backend Trait

```rust
pub trait FileNodeBackend: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    async fn get_objects<O>(...) -> Result<...>;
    async fn set_objects<O>(...) -> Result<...>;
    // etc.
}
```

## Module Layout

```
src/
  lib.rs        FileNodeBackend trait, register_filenode_handlers
  filenode.rs   FileNode/get, set, changes, query, queryChanges, copy
  backend.rs    FileNodeBackend trait, error types
  helpers.rs    shared utilities
```

## Pattern to Follow

`~/PROJECT/JMAP/crate-jmap-mail-server/` — identical handler structure.

## Chat Integration Note

The JMAP Chat spec (`draft-atwood-jmap-chat-filenode-00.md`) extends FileNode for Chat
attachments. If implementing both, the Chat binding crate should depend on this crate,
not duplicate the core FileNode types.
