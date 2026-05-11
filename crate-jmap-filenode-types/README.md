# jmap-filenode-types

Serde-annotated Rust types for the JMAP FileNode extension ([draft-ietf-jmap-filenode-13]).
Types only — no method handlers, no async, no network I/O.

## What

| Type | Description |
|---|---|
| `FileNode` | The core file-system node object (§3.1) — represents a file, directory, or symlink |
| `NodeType` | Enum: `File`, `Directory`, `Symlink`, `Other(String)` |
| `NodeRole` | Well-known directory roles: `Root`, `Home`, `Temp`, `Trash`, `Documents`, `Downloads`, `Music`, `Pictures`, `Videos`, `Other(String)` |
| `FilesRights` | Per-user access rights on a FileNode (`mayRead`, `mayWrite`, `mayAdmin`, etc.) |
| `FileNodeFilterCondition` | Filter arguments for `FileNode/query` |
| `FileNodeCapability` | Account-level capability object for the FileNode extension |
| `FileNodeProperty` | Enum of legal `FileNode` property names for `properties` arrays |

Capability URI constant:

| Constant | Value |
|---|---|
| `JMAP_FILENODE_URI` | `"urn:ietf:params:jmap:filenode"` |

## Filter extensibility

Filter types in this crate — `FileNodeFilterCondition` and the generic
`Filter<T>` / `Operator` re-exported from `jmap-types` — are **intentionally
not extensible** via vendor "extras" fields. A filter clause the server does
not understand silently breaks query correctness: the client gets the wrong
set of records back with no error signal. So these types deliberately have no
`extra` catch-all field.

Vendors who need to filter on custom fields have two options:

- **IETF-track (recommended).** Use the JMAP Object Metadata extension
  (`draft-ietf-jmap-metadata`, capability URI `urn:ietf:params:jmap:metadata`),
  which defines a `Metadata` / `Annotation` companion object keyed by
  `(relatedType, relatedId)` with capability-declared schema (`metadataTypes`
  / `maxDepth`) and a `Metadata/query` `textMatch` filter. This is the
  workspace's recommended path for vendor data that needs to be queryable.
  Implemented in [`jmap-metadata-types`](../crate-jmap-metadata-types),
  [`jmap-metadata-server`](../crate-jmap-metadata-server), and
  [`jmap-metadata-client`](../crate-jmap-metadata-client) (bd JMAP-06zp).
- **Pre-IETF escape.** If you cannot wait for the metadata draft, escape the
  filter tree to `serde_json::Value` or fork the `FileNodeFilterCondition`
  type. See
  [`crate-jmap-calendars-types/PLAN.md`](../crate-jmap-calendars-types/PLAN.md)
  for the hybrid sloppy-value pattern.

This policy is part of the workspace extras-preservation policy documented in
the workspace [`AGENTS.md`](../AGENTS.md); the filter-algebra exclusion
decision is bd JMAP-lbdy.

## Spec coverage

**draft-ietf-jmap-filenode-13 sections implemented:**

- §3.1 — `FileNode` object: `id`, `parentId`, `blobId`, `target`, `size`, `name`,
  `type` (serialized as `"type"`), `mediaType`, `created`, `modified`, `shareWith`,
  `myRights`, `role`
- §3.1 (IANA registries §10.4, §10.5) — `NodeType` and `NodeRole` registered values
- §3.2.3 — `FileNodeFilterCondition` query filter
- §1.6 — `FileNodeCapability` and `JMAP_FILENODE_URI`

## Usage

```rust
use jmap_filenode_types::{FileNode, NodeType};

// Deserialize a file node.
let file_node: FileNode = serde_json::from_str(r#"{
    "id": "node1",
    "parentId": "dir1",
    "blobId": "blob-abc123",
    "target": null,
    "size": 8192,
    "name": "report.pdf",
    "type": "file",
    "mediaType": "application/pdf",
    "created": "2025-01-15T10:00:00Z",
    "modified": "2025-01-20T14:30:00Z",
    "shareWith": null
}"#)?;
assert_eq!(file_node.node_type, Some(NodeType::File));

// Deserialize a directory node.
let dir_node: FileNode = serde_json::from_str(r#"{
    "id": "dir1",
    "parentId": null,
    "blobId": null,
    "target": null,
    "size": null,
    "name": "Documents",
    "type": "directory",
    "mediaType": null,
    "created": "2024-06-01T00:00:00Z",
    "modified": "2025-01-20T14:30:00Z",
    "shareWith": null
}"#)?;
assert_eq!(dir_node.node_type, Some(NodeType::Directory));
# Ok::<(), serde_json::Error>(())
```

## How it works

All structs carry `#[serde(rename_all = "camelCase")]` to produce camelCase JSON field names
as required by the JMAP wire format. Two fields require explicit `#[serde(rename)]`
annotations because their wire names clash with Rust keywords or conventions:

- `node_type` is serialized as `"type"` — `type` is a reserved keyword in Rust, so the
  struct field is named `node_type` with `#[serde(rename = "type")]`.
- `media_type` in `FileNodeFilterCondition` is serialized as `"mediaType"` — covered by the
  top-level `rename_all = "camelCase"`.

`NodeType` and `NodeRole` are string-backed enums with an `Other(String)` fallback variant
so that unrecognised values from the IANA registry (added after this crate was written)
round-trip without data loss.

## Known Limitations

- `ArchiveEntry` integration types for the JMAP Blob Extensions draft
  (`RFC 9404) are not implemented.

## References

- **[draft-ietf-jmap-filenode-13]** — JMAP FileNode (normative for all type definitions)
- **[RFC 8620]** — JMAP Core (Id, State, SetError, request/response shape)

[draft-ietf-jmap-filenode-13]: https://www.ietf.org/archive/id/draft-ietf-jmap-filenode-13.txt
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620

## License

MIT OR Apache-2.0
