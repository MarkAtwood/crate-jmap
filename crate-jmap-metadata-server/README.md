# jmap-metadata-server

## What it is

Server-side helpers for the JMAP Object Metadata
extension
([draft-ietf-jmap-metadata-04](https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-04)).
No method handlers, no backend trait, no dispatcher registration, no
`memory` feature.

## What it's for

draft-04 has no standalone `Metadata` object and no `Metadata/*`
methods. Each data type a server lists in the capability's `dataTypes`
gains `metadata` and `privateMetadata` properties, served by that
type's own `Foo/get`, `Foo/set`, `Foo/changes`, `Foo/queryChanges`,
and `Foo/query` (§3). Extension servers (`jmap-mail-server`,
`jmap-calendars-server`, …) call these helpers from their own handlers
and backends to implement those extensions. Wiring them into the
workspace's extension servers is tracked by bd JMAP-s4kp.

| Module | Item | Draft § |
|---|---|---|
| `validate` | `value_depth`, `exceeds_max_depth` | §2.1 (depth), §3.2 rule 4 |
| `validate` | `is_namespace_supported` | §3.1, §3.2 rule 3, §3.6 |
| `validate` | `check_metadata_write`, `SetKind`, `InvalidMetadataWrite` | §3.2 rules 2–3, §2.1.1 migration case |
| `validate` | `check_metadata_depth` | §2.1 (object value, depth), §3.2 rule 4 |
| `changes` | `METADATA_PROPERTY`, `PRIVATE_METADATA_PROPERTY`, `is_metadata_property`, `is_metadata_only_change` | §3.3 (`ignoreMetadataOnlyChanges`, `updatedProperties`) |
| `changes` | `changed_properties`, `ChangedProperties`, `MetadataOnlyChanges` | §3, §3.3 per-id change tracking over a `/changes` window |
| `filter` | `MetadataCondition`, `MetadataFilterError` | §3.6 decode, validate, and evaluate the six conditions of one `FilterCondition` |
| `filter` | `metadata_path_exists` | §3.6 `metadataExists` / `privateMetadataExists` |
| `filter` | `metadata_text_contains` | §3.6 `metadataTextContains` / `privateMetadataTextContains` |
| `filter` | `metadata_text_equals` | §3.6 `metadataTextEquals` / `privateMetadataTextEquals` |

`JMAP_METADATA_URI` is re-exported from `jmap-metadata-types`.

## How to use

```rust
use jmap_metadata_server::{exceeds_max_depth, is_metadata_only_change, metadata_path_exists};
use serde_json::json;

// /set: reject a namespace value deeper than the capability's maxDepth
// with invalidProperties (§2.1, §3.2 rule 4).
assert!(exceeds_max_depth(&json!({"a": {"b": "c"}}), 1));

// /query: evaluate a metadataExists condition against an object's
// metadata map (§3.6).
let metadata = json!({"photography": {"album": "sunset"}});
assert!(metadata_path_exists(metadata.as_object().unwrap(), "photography/album"));

// /changes with ignoreMetadataOnlyChanges: drop ids whose only changed
// properties are metadata / privateMetadata (§3.3).
assert!(is_metadata_only_change(&["metadata", "privateMetadata"]));
assert!(!is_metadata_only_change(&["metadata", "subject"]));
```

## What the caller still owns

The helpers hold no server state. The extension server remains responsible
for everything that needs state or identity:

- **`/set` rule 1** (§3.2, §4): `notFound` when the user cannot read
  the object, then `forbidden` for a `metadata` write without
  `maySetMetadata`. Then call `check_metadata_write` (rules 2–3), apply
  the write (`invalidPatch` for patch errors that depend on the stored
  value), and call `check_metadata_depth` (rule 4). The first matching
  rule determines the SetError.
- **Quota** (§3.2.1, §5): `overQuota`.
- **Per-viewer `privateMetadata`** (§2.2, §4): each user sees only
  their own private metadata, and a private write advances the state
  string only for the writer. This needs caller identity via
  `JmapBackend::principal_id`; a backend returning `None` cannot honor
  it, and per §2.2 must not advertise the capability with
  `supportsPrivate: true`.
- **`updatedProperties`** (§3.3): computed per viewer. Record only the
  changes the viewer can see in `ChangedProperties`.
- **`/get` subselectors** (§3.1): `metadata/<namespace>` paths, union
  semantics, silent omission of unsupported namespaces.

## Migration from 0.1 (draft-01)

The 0.1 crate exposed `MetadataBackend`, `register_metadata_handlers`,
`memory::MemoryBackend`, and five `Metadata/*` handlers. All are
removed in 0.2. Integrate the helpers into each listed data type's own
handlers instead.

## Crate family

```
jmap-metadata-types      DataTypeMetadataInfo, MetadataTextMatch,
                         namespace validation
    └── jmap-metadata-server  ← this crate
```

## References

- **[draft-ietf-jmap-metadata-04]** — JMAP Object Metadata (normative
  for depth counting, namespace support, filter semantics, and
  `/changes` extensions)
- **[RFC 8620]** — JMAP Core (`/get`, `/set`, `/changes`, `/query`,
  SetError)

[draft-ietf-jmap-metadata-04]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-04
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
