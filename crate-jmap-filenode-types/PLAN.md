# jmap-filenode-types — Implementation Plan

Data types for the JMAP FileNode extension.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-filenode-13.txt`

## Crate Family Position

```
jmap-types
    └── jmap-filenode-types  ← this crate
            ├── jmap-filenode-server
            └── jmap-filenode-client
```

## What This Crate Is

Serde-serializable data types for JMAP FileNode:
- `FileNode` — hierarchical file/folder tree node stored via JMAP blob machinery
- Enables file storage, sharing, and synchronization over JMAP

No async, no I/O. Depends only on `jmap-types` and `serde`.

## Key Types (draft-ietf-jmap-filenode-13)

- `FileNode` — `id`, `parentId`, `name`, `mediaType`, `blobId`, `size`, `cid`,
  `isFolder`, `children`, `shareWith`, `myRights`, `created`, `modified`
- `FileNodeRights` — `mayRead`, `mayWrite`, `mayAdmin`, `mayDelete`, `mayCreateChild`
- `FileNodeShare` — `mayRead`, `mayWrite`, `mayAdmin`

## Source Material

draft-ietf-jmap-filenode-13 is a mature draft. The JMAP Chat spec also references FileNode
for file attachments; see `~/PROJECT/jmap-chat-spec/draft-atwood-jmap-chat-filenode-00.md`
for the Chat-specific binding.

Note: `jmap-chat-types` may already define some FileNode-adjacent types for Chat attachments.
Check before duplicating.
