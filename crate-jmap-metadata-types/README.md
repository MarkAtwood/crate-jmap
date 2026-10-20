# jmap-metadata-types

Serde-annotated Rust types for the JMAP Object Metadata extension
([draft-ietf-jmap-metadata-02]). Types only — no method handlers,
no async, no network I/O.

## What it is

| Item | Description |
|---|---|
| `DataTypeMetadataInfo` | Per-data-type entry of the account capability's `dataTypes` map (§1.2.1): `namespaces`, `supportsVendorNamespaces`, `supportsPrivate`, `maxDepth` |
| `MetadataTextMatch` | `{ path, value }` value of the four `*TextContains` / `*TextEquals` filter conditions (§3.5) |
| `is_registered_namespace` | Registered-name syntax check: US-ASCII letters, digits, `-`, `_`, no dot (§2.1) |
| `is_vendor_namespace` | Vendor domain-name structural check: at least one dot, non-empty printable-ASCII labels; no DNS validation (§2.1) |
| `is_valid_namespace` | Either of the above (§2.1) |
| `JMAP_METADATA_URI` | `"urn:ietf:params:jmap:metadata"` (§1.2.1, §8.1) |

## What it's for

Shared vocabulary for `jmap-metadata-server` (server-side helpers) and
`jmap-metadata-client` (client-side helpers). This crate is the
foundation of the workspace-recommended IETF-track escape hatch for
vendor data that needs to be queryable: the workspace
extras-preservation policy excludes filter algebra (silent-drop is a
query-correctness bug), so vendors who need filterable data put it in
a namespace under `metadata` / `privateMetadata` rather than in a
vendor-extended filter condition.

## Why this crate exists

`jmap-types` and the extension-types crates apply an
[extras-preservation policy](../AGENTS.md) that captures vendor
fields in a flatten `extra` map on every wire-format Deserialize
struct. That policy explicitly **excludes filter and comparator
algebra**: silently round-tripping an unknown filter clause through
the client gives the wrong record set back with no error signal.

`draft-ietf-jmap-metadata-02` is the IETF-track answer. Each data type
a server lists in the capability's `dataTypes` gains two properties,
`metadata` (shared) and `privateMetadata` (per-user), whose values are
objects keyed by namespace identifier (§2). Clients read and write them
through the data type's own `/get` and `/set`, and search them through
six `metadata*` / `privateMetadata*` conditions on the data type's own
`/query` filter (§3). There is no standalone `Metadata` object and no
`Metadata/*` method.

## Spec coverage

**draft-ietf-jmap-metadata-02 sections implemented:**

- §1.2.1 — `DataTypeMetadataInfo` and `JMAP_METADATA_URI`
- §2.1 — namespace identifier validation
- §3.5 — `MetadataTextMatch`

Not in this crate:

- Depth counting (§2.1), namespace-support checks (§3.1, §3.2), filter
  predicates (§3.5), and `/changes` classification (§3.3) — pure
  helpers in `jmap-metadata-server`.
- Filter-condition builders and property-name constants —
  `jmap-metadata-client`.
- Typed `metadata` / `privateMetadata` fields on the extension-types
  data objects. By decision (bd JMAP-ua37.4) both properties
  round-trip through each object's `extra` map instead.

## How to use

```rust
use jmap_metadata_types::{DataTypeMetadataInfo, is_registered_namespace};

// The "FileNode" entry of the §6.1 capability example.
let info: DataTypeMetadataInfo = serde_json::from_str(r#"{
    "namespaces": ["photography"],
    "supportsVendorNamespaces": false,
    "supportsPrivate": false,
    "maxDepth": 3
}"#).unwrap();

assert_eq!(info.namespaces, vec!["photography"]);
assert!(!info.supports_vendor_namespaces);
assert_eq!(info.max_depth, Some(3));
assert!(is_registered_namespace("photography"));
```

## How it works

`DataTypeMetadataInfo` uses `#[serde(rename_all = "camelCase")]`, with
`#[serde(default)]` on the three fields the draft gives defaults
(`namespaces`, `supportsVendorNamespaces`, `supportsPrivate`).
`maxDepth` is `Option<u64>`; `None` is the draft's `null` (no
server-enforced limit).

The namespace predicates check syntax only. Whether a namespace is
*supported* on a data type depends on the capability and lives in
`jmap_metadata_server::is_namespace_supported`.

## Gotchas

Tests are named with `_draft_02_` where they pin a specific draft
example. When the draft revs, expect to update or replace these tests
alongside the wire-format changes. The `#[non_exhaustive]` derive on
every struct keeps additive spec evolution non-breaking, but a
renamed or removed wire field is still a breaking change in the next
0.x bump.

`MetadataTextMatch` and `DataTypeMetadataInfo` carry no `extra` field:
the first is filter algebra and the second is a capability object, both
outside the extras-preservation policy (see the workspace
[AGENTS.md](../AGENTS.md)).

## References

- **[draft-ietf-jmap-metadata-02]** — JMAP Object Metadata (normative for all type definitions)
- **[RFC 8620]** — JMAP Core (capabilities object, `/get`, `/set`, `/query`)

[draft-ietf-jmap-metadata-02]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-02
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
