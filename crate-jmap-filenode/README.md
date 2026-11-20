# jmap-filenode

JMAP FileNode ([draft-ietf-jmap-filenode-20]) in one dependency: re-exports
[`jmap-filenode-types`] always, [`jmap-filenode-client`] with the `client`
feature and [`jmap-filenode-server`] with the `server` feature (`memory`
adds the server's in-memory reference backend).

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_filenode::types` |
| `client` | `jmap_filenode::client` |
| `server` | `jmap_filenode::server` |
| `memory` | `server`, plus `jmap_filenode::server::memory` |

- `types`: the wire types and capability constants.
- `client`: the sealed `JmapFilenodeExt` extension trait over
  `jmap-base-client`.
- `server`: method handlers for `FileNode/*` over `FileNodeBackend`,
  `register_filenode_handlers`.

## What it's for

FileNode is a filesystem-like tree of files and directories in a JMAP
account. Applications that want the whole family depend on this crate and
pick `client`, `server` or both; workspace crates depend on the component
crates directly. The Chat bridge, `jmap-chat-filenode-server`, is not
re-exported; depend on it directly.

## How to use

```toml
[dependencies]
jmap-filenode = { version = "0.6", features = ["client"] }
```

```rust
use jmap_filenode::types::{FileNode, JMAP_FILENODE_URI, NodeType};

assert_eq!(JMAP_FILENODE_URI, "urn:ietf:params:jmap:filenode");
let dir: FileNode = serde_json::from_str(r#"{"@type": "Directory", "id": "dir1",
    "parentId": null, "name": "Documents", "role": null,
    "created": "2024-06-01T00:00:00Z", "modified": "2025-01-20T14:30:00Z",
    "shareWith": null}"#)
    .unwrap();
assert_eq!(dir.node_type, Some(NodeType::Directory));
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_filenode::server::X` is the same item as `jmap_filenode_server::X`.
The component crates version in lockstep with this one on the shared
`jmap-*` line.

## Gotchas

- `server` alone does not build the reference backend; enable `memory` for
  `server::memory::MemoryBackend` (demonstration quality, not production).
- Depending on this crate and on a component crate at a different version
  gives two copies of each type, which do not unify. Keep them on one
  version line.
- Version 0.6.0 replaces the 0.0.1 name-claim placeholder.

## References

- [draft-ietf-jmap-filenode-20] — JMAP FileNode
- [RFC 8620] — JMAP Core

[draft-ietf-jmap-filenode-20]: https://datatracker.ietf.org/doc/draft-ietf-jmap-filenode/
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-filenode-types`]: https://crates.io/crates/jmap-filenode-types
[`jmap-filenode-client`]: https://crates.io/crates/jmap-filenode-client
[`jmap-filenode-server`]: https://crates.io/crates/jmap-filenode-server
