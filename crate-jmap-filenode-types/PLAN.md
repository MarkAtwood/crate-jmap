# jmap-filenode-types — Implementation Plan

JMAP FileNode extension (draft-ietf-jmap-filenode-13) data types. Types only — no
method handlers, no async, no network I/O. This crate sits between `jmap-types`
(shared JMAP base primitives) and `jmap-filenode-server` / `jmap-filenode-client`.

## Crate Family Position

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-filenode-types  ← this crate
            ├── jmap-filenode-server (method handlers)
            └── jmap-filenode-client (extension trait)
```

## What This Crate Covers

Each object type maps to one source module. The normative reference for every
field name, type, and serialization rule is draft-ietf-jmap-filenode-13 §3.

| Module | Type(s) | Draft section |
|---|---|---|
| `filenode.rs` | `FileNode`, `NodeType`, `FilesRights` | §3.1 |
| `query.rs` | `FileNodeFilterCondition`, `FileNodeFilter`, `FileNodeComparator` | §3.2.5 |
| `capability.rs` | `FileNodeCapability` (account-level) | §2.1 |

Generic query types (`Filter<T>`, `FilterOperator<T>`, `Operator`) live in
`jmap-types::query` because they are defined by RFC 8620 §5.5, not this draft.

## What Is Out of Scope

- Method handlers (`FileNode/get`, `FileNode/set`, etc.) — those live in `jmap-filenode-server`
- Transport and network I/O — no tokio, no reqwest
- Partial PATCH semantics — `jmap-filenode-server` applies patches; this crate holds the types
- JMAP Blob Extensions (`draft-gondwana-jmap-blobext`) integration types (`ArchiveEntry`
  extension with `nodeId`/`recurse`, `ExtractRecipe` with `parentNodeId`) — those belong in
  a future `jmap-blobext-types` crate that depends on this one

## Full Type Reference (draft-ietf-jmap-filenode-13 §3.1)

### FileNode

The central object. Represents either a file, a directory, or a symlink in a
hierarchical tree rooted at nodes with `parentId: null`.

| Field | Wire type | Notes |
|---|---|---|
| `id` | `Id` | immutable; server-set |
| `parentId` | `Id\|null` | null = top-level node |
| `nodeType` | `String` | immutable after creation; server-inferred if absent; see `NodeType` |
| `blobId` | `Id\|null` | non-null for file nodes; null for directory and symlink |
| `target` | `String[]\|null` | symlink target as path element array; null for file/directory |
| `size` | `UnsignedInt\|null` | server-set; null for directory/symlink; set from blob on create/update |
| `name` | `String` | Net-Unicode, min 1 char; siblings under same parent must be unique |
| `type` | `String\|null` | IANA media type; non-null for files, null for directory/symlink |
| `created` | `UTCDate` | default: current server time |
| `modified` | `UTCDate\|null` | client-managed; null → server sets to current time |
| `accessed` | `UTCDate\|null` | client-managed; null → server sets to current time |
| `changed` | `UTCDate` | server-set; auto-updated on any property change |
| `executable` | `Boolean` | default: false |
| `isSubscribed` | `Boolean` | per-user; default: true |
| `myRights` | `FilesRights` | server-set; derived ACL for requesting user, including inheritance |
| `shareWith` | `Id[FilesRights]\|null` | map of userId→rights; null if not shared or user lacks mayShare |
| `role` | `String\|null` | special directory role; null for files; see `FileNodeRole` constants |

Note: the wire field name `type` collides with a Rust keyword. Use
`#[serde(rename = "type")]` on the struct field, which must be named `media_type`
in Rust.

### NodeType

The `nodeType` field takes one of three registered values (IANA "JMAP FileNode Types"
registry, §10.4), plus an open catch-all for forward compatibility:

```
"file"      — regular file; blobId must be non-null
"directory" — collection that may contain child nodes; blobId must be null
"symlink"   — symbolic link; target must be non-null, blobId must be null
```

Use a custom serde implementation (not `#[serde(rename_all)]`) to support an
`Other(String)` variant. Clients MUST ignore unrecognised nodeType values per the
general JMAP extensibility principle.

### FilesRights

The ACL object returned in `myRights` and used as the value type in `shareWith`.
All fields are `Boolean`.

| Field | Meaning |
|---|---|
| `mayRead` | May read properties and blob content |
| `mayAddChildren` | May create child nodes in this directory |
| `mayRename` | May rename or move this node |
| `mayDelete` | May destroy this node |
| `mayModifyContent` | May update blobId, type, target, modified, accessed, executable |
| `mayShare` | May change shareWith (see RFC 9670 JMAP Sharing) |

### FileNodeRole constants

Known roles from the IANA "JMAP FileNode Roles" registry (§10.5):

```
"root"      — base of a filesystem; should have parentId null
"home"      — user's home directory
"temp"      — temporary space; may be cleaned up automatically
"trash"     — deleted data
"documents" — document storage
"downloads" — downloaded files
"music"     — audio files
"pictures"  — photos and images
"videos"    — video files
```

Expose as `pub const &str` values in `mod role`, not as an enum — the registry is
extensible and unknown roles must pass through without error. Clients MUST ignore
unrecognised role values (§3.1).

### FileNodeCapability (account-level, §2.1)

The value of the `urn:ietf:params:jmap:filenode` key in `accountCapabilities`.
This struct lives here (not in jmap-filenode-server) because clients also need to
read session capabilities.

| Field | Wire type | Notes |
|---|---|---|
| `maxFileNodeDepth` | `UnsignedInt\|null` | max hierarchy depth; null = no limit |
| `maxSizeFileNodeName` | `UnsignedInt` | max name length in UTF-8 octets; MUST be ≥ 100 |
| `forbiddenNameChars` | `String\|null` | characters forbidden in names; null = no restriction |
| `forbiddenNodeNames` | `String[]\|null` | complete names forbidden (case-insensitive); null = no restriction |
| `fileNodeQuerySortOptions` | `String[]` | supported sort properties for FileNode/query |
| `mayCreateTopLevelFileNode` | `Boolean` | user may create nodes with null parentId |
| `webTrashUrl` | `String\|null` | web URL for trash folder; null if unavailable |
| `caseInsensitiveNames` | `Boolean` | server treats names as case-insensitive for collision checks |
| `webUrlTemplate` | `String\|null` | URI Template (level 1) with `{id}` for viewing a node on the web |
| `webWriteUrlTemplate` | `String\|null` | URI Template (level 1) with `{id}` for direct HTTP writes |

### FileNodeFilterCondition and FileNodeComparator (§3.2.5)

Query filter conditions for `FileNode/query`. All fields are optional; a node
matches a condition only if every provided field matches.

| Filter field | Wire type | Semantics |
|---|---|---|
| `isTopLevel` | `Boolean` | must have null parentId |
| `parentId` | `Id` | exact match on parentId |
| `ancestorId` | `Id` | any ancestor has this id |
| `descendantId` | `Id` | node is an ancestor of this id (inverse of ancestorId) |
| `nodeType` | `String` | exact match on nodeType |
| `role` | `String` | exact match on role |
| `hasAnyRole` | `Boolean` | true = role is non-null; false = role is null |
| `blobId` | `Id` | exact match on blobId |
| `isExecutable` | `Boolean` | exact match on executable flag |
| `createdBefore` | `UTCDate` | created < this date |
| `createdAfter` | `UTCDate` | created ≥ this date |
| `modifiedBefore` | `UTCDate` | modified < this date |
| `modifiedAfter` | `UTCDate` | modified ≥ this date |
| `accessedBefore` | `UTCDate` | accessed < this date |
| `accessedAfter` | `UTCDate` | accessed ≥ this date |
| `minSize` | `UnsignedInt` | size ≥ this value |
| `maxSize` | `UnsignedInt` | size < this value |
| `name` | `String` | exact byte match on name |
| `nameMatch` | `String` | glob match on name (case-insensitive; `*`, `?`, `[abc]`, `[!abc]`) |
| `type` | `String` | exact byte match on type (media type) |
| `typeMatch` | `String` | glob match on type using same glob syntax as nameMatch |
| `body` | `String` | full-text search in blob content |
| `text` | `String` | equivalent to body OR nameMatch OR typeMatch |

Note: the filter field `type` collides with a Rust keyword. Use
`#[serde(rename = "type")]` on the struct field, naming it `media_type` in Rust.

Sort comparator `property` values (§3.2.5, in addition to standard RFC 8620 properties):
- `"name"` — alphabetical
- `"type"` — directories first, then by media type
- `"size"` — ascending byte count
- `"created"` — ascending creation time
- `"modified"` — ascending modification time
- `"nodeType"` — directories, then symlinks, then files
- `"tree"` — depth-first recursive by name (like `find` output with `depth` parameter)

## Key Design Decisions

### nodeType inference on create

The spec (§3.1) says: if `nodeType` is absent on create, the server infers it —
`"file"` if `blobId` is non-null, `"symlink"` if `target` is non-null, `"directory"`
otherwise. This is server-side logic in `jmap-filenode-server`; this crate simply
uses `Option<NodeType>` for the field so absence is representable during
deserialization of create requests.

### type field naming

The wire field is literally `"type"`. In Rust this requires `#[serde(rename = "type")]`
and a field name of `media_type` (or similar). Document this explicitly so maintainers
do not inadvertently rename the serde attribute.

### modified and accessed semantics

Both are client-managed (§3.1, §8.1). The server does NOT automatically update them
on change — that is the client's responsibility. Setting either to `null` in an update
signals the server to set it to the current time. This is different from `changed`
(§3.1), which the server automatically updates on every mutation and is never settable
by clients. These are separate fields with different mutability semantics; `Option`
alone does not capture the null-means-reset-to-now semantic. Document this in field
doc comments.

### shareWith value type

`shareWith` is `Id[FilesRights]` — a JSON object whose keys are JMAP `Id`s and
whose values are `FilesRights` objects. In Rust: `Option<HashMap<Id, FilesRights>>`.
The `null` case (user lacks mayShare or node is not shared) maps to `None`.

### Blob lifetime guarantee

A blob referenced by a FileNode MUST NOT be garbage-collected by the server while
the FileNode exists (§3.1). This is an invariant the server backend must enforce;
the type crate simply documents it on the `blob_id` field.

### Filter union — untagged enum with Operator first

`FileNodeFilter` uses the same `#[serde(untagged)]` pattern as `jmap-mail-types`:
`Operator` variant before `Condition`. Serde tries variants in declaration order;
`FilterOperator` requires an `"operator"` key and fails fast when absent.
`FileNodeFilterCondition` must NOT use `#[serde(deny_unknown_fields)]`.

## Module Layout

```
src/
  lib.rs          re-exports
  filenode.rs     FileNode, NodeType, FilesRights, role constants module
  query.rs        FileNodeFilterCondition, FileNodeFilter, FileNodeComparator
  capability.rs   FileNodeCapability (account-level capability struct)
```

## Test Oracle Strategy

Tests must use independent oracles — never derive expected values from the code
under test. Acceptable sources:

1. Hand-written JSON fixtures constructed directly from draft-ietf-jmap-filenode-13
   field descriptions (committed in `tests/fixtures/`).
2. Literal JSON from the capability example in §2.1.1 (copy-pasted from the draft).
3. Known wire values verified against the draft text.

All tests are `#[test]` (no tokio). Roundtrip tests (`serialize → deserialize`)
verify serde consistency but are not a substitute for spec-grounded oracle tests.

Key cases to cover:
- `FileNode` with `nodeType: "file"`, non-null `blobId`, null `target`
- `FileNode` with `nodeType: "directory"`, null `blobId`, null `target`, null `size`
- `FileNode` with `nodeType: "symlink"`, null `blobId`, non-null `target`
- `FileNode` with `nodeType: "unknown-future-type"` deserializes as `NodeType::Other`
- `shareWith` absent vs null vs populated map
- `role` with a known value, an unknown value (preserved as string), and null
- `FileNodeCapability` from the §2.1.1 example JSON
- `FileNodeFilterCondition` with the `type` field (verifying rename works)
- `FileNodeFilter` as both a bare condition and as an `operator` compound

## Spec References

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-filenode-13.txt` — normative
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base protocol (Id, UTCDate,
  Filter, Comparator, State)

## Dependencies

```toml
jmap-types = { path = "../crate-jmap-types" }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
# No tokio, no async, no network deps
```
