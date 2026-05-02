# jmap-filenode-client — Implementation Plan

JMAP FileNode method implementations on top of `jmap-base-client`.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-filenode-13.txt`

## Crate Family Position

```
jmap-types
    └── jmap-base-client
            └── jmap-filenode-client  ← this crate
```

## What This Crate Is

Extension trait `JmapFileNodeExt` over `jmap_base_client::JmapClient` that adds typed
methods for every JMAP FileNode operation.

## Planned Public API

```rust
pub trait JmapFileNodeExt {
    async fn file_node_get(&self, account_id: &Id, ids: Option<&[Id]>, props: &[&str])
        -> Result<GetResponse<FileNode>, ClientError>;
    async fn file_node_set(&self, account_id: &Id, req: SetRequest<FileNode>)
        -> Result<SetResponse<FileNode>, ClientError>;
    async fn file_node_query(&self, account_id: &Id, req: FileNodeQueryRequest)
        -> Result<QueryResponse, ClientError>;
    async fn file_node_copy(&self, from_account_id: &Id, to_account_id: &Id, req: CopyRequest<FileNode>)
        -> Result<CopyResponse<FileNode>, ClientError>;
    // ... all FileNode methods
}
```

## Pattern to Follow

`~/PROJECT/JMAP/crate-jmap-mail-client/` — identical extension trait pattern.
