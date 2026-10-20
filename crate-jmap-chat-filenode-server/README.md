# jmap-chat-filenode-server

The Chat binding for `jmap-filenode-server`: the rules of
draft-atwood-jmap-chat-filenode-00 that apply when one server offers
`urn:ietf:params:jmap:chat:filenode` with `urn:ietf:params:jmap:chat` and
`urn:ietf:params:jmap:filenode`. `jmap-filenode-server` and
`jmap-chat-server` do not depend on each other; this crate depends on both.

## What it is

| Item | Spec |
|---|---|
| `check_root_create` | chat-filenode §Creating the File Tree Root: `spaceId` required, Space exists and the caller is a member (`forbidden`), one root per Space (`alreadyExists` with `existingId`), `manage_space` required (`forbidden`, §Permission Mapping) |
| `check_space_quota` | chat-filenode §Account-Level Capability Object `maxSpaceStorageBytes`: a create that would take the Space file tree over the limit is `overQuota` |
| `create_space_file_tree` | chat-filenode §Account-Level Capability Object `autoCreateFileTree`: insert the root when a Space is created |
| `resolve_filenode_attachment` | chat-filenode §filenodeId: an Attachment linked by `filenodeId` must name a file in the channel's Space file tree (`invalidProperties` naming `filenodeId`), the sender needs read access (`forbidden`), and absent `blobId`, `filename`, `contentType`, `size` and `sha256` are filled from the FileNode |
| `destroy_space_file_tree` | chat-filenode §Space Deletion: destroy a destroyed Space's file tree root and every node within it, with no grace period |
| `RootCheckError` | A Chat or FileNode storage failure during a check |
| `TreeDestroyError` | A FileNode storage failure while destroying a Space file tree |

## What it's for

A deployment that implements both `FileNodeBackend` and `ChatBackend`
calls these functions from its trait methods:

| Backend method | Call |
|---|---|
| `FileNodeBackend::create_object::<FileNode>`, before the insert | `check_root_create` |
| The same, when advertising `maxSpaceStorageBytes` | `check_space_quota` |
| `ChatBackend::create_object::<Space>`, after the insert, when advertising `autoCreateFileTree: true` | `create_space_file_tree` |
| `ChatBackend::filenode_attachment` | `resolve_filenode_attachment` |
| `ChatBackend::on_space_destroyed` | `destroy_space_file_tree` |

## How to use

```rust,ignore
// Inside FileNodeBackend::create_object, for a FileNode create:
check_root_create(&self.chat, &(), &self.files, &(), account_id, &node, &self.everyone)
    .await
    .map_err(|e| BackendSetError::Other(e.into()))?
    .map_err(BackendSetError::SetError)?;
self.files.create_object(caller, account_id, create_id, node).await
```

Any node whose `role` is not `chatspace` passes
unchecked, so the call can run before every FileNode insert.

## How it works

The Space is read from the Chat backend and the existing root is found
with the `FileNode/query` `role` + `spaceId` filter the draft requires.
The `manage_space` check unions the member's SpaceRole permissions with
the caller-supplied `@everyone` permissions; ChannelPermission overrides
do not apply to file tree access.

## Gotchas

- The one-root-per-Space check is a read before the insert. Re-check it
  in the same transaction as the insert (for example a unique index on
  `spaceId` for root nodes), as `FileNodeBackend::create_object` already
  requires for name uniqueness.
- `check_root_create` takes the `@everyone` role's permissions as an
  argument: the draft says every member holds that role but puts its
  permissions on no wire object. Controller principals that bypass
  permission resolution are deployment-defined; skip the call for them.
- `check_space_quota` sums the stored `size` of every node in the tree
  plus the new node's `size`: call it after setting the server-set `size`
  from the blob, and re-check under the insert's transaction. It reads the
  whole tree per create; a large deployment keeps a per-Space total.
- `create_space_file_tree` names the root after the Space id; members
  with `manage_space` may rename it.
- `resolve_filenode_attachment` rejects a supplied `blobId`, `size` or
  `sha256` that differs from the FileNode's, and keeps a supplied
  `filename` or `contentType`.
- `destroy_space_file_tree` offers no grace period (the draft's MAY):
  the tree goes with the Space. Each node is destroyed through
  `FileNodeBackend::destroy_object`, descendants first, so
  `FileNode/changes` and push see the change. It is not atomic; on an
  error, call it again to destroy the rest.
- The requesting user is the owner of `account_id`, and the Space and the
  FileNode live in that same account.

## References

- [draft-atwood-jmap-chat-filenode-00] — JMAP Chat FileNode integration
- [draft-atwood-jmap-chat-00] — JMAP Chat
- [draft-ietf-jmap-filenode] — JMAP FileNode
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-chat-filenode-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-chat-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-ietf-jmap-filenode]: https://datatracker.ietf.org/doc/draft-ietf-jmap-filenode/
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
