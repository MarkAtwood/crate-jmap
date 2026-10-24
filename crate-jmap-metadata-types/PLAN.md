# jmap-metadata-types — Implementation Plan

JMAP Object Metadata extension (draft-ietf-jmap-metadata-04) data types.
Types only — no method handlers, no async, no network I/O.

## Crate Family Position

```
jmap-metadata-types  ← this crate
    ├── jmap-metadata-server   pure server-side helpers
    └── jmap-metadata-client   pure client-side helpers
```

## Why this crate exists

The base JMAP spec has no metaprotocol mechanism for a server to declare
what vendor extras it supports or whether they can be filtered.
`draft-ietf-jmap-metadata-04` is the IETF-track answer:

- A capability URI `urn:ietf:params:jmap:metadata` whose account value
  maps each listed data type to a `DataTypeMetadataInfo` (§1.2.1).
- Two properties on each listed data type, `metadata` (shared) and
  `privateMetadata` (per-user), keyed by namespace identifier (§2, §2.1).
- Extensions to the data type's own `/get` (§3.1), `/set` (§3.2),
  `/changes` (§3.3), `/copy` (§3.4), `/queryChanges` (§3.5), and `/query` (§3.6).

draft-02 removed the -01 standalone `Metadata` object
(`Annotation` / `ImapMetadata` / `WebDavMetadata`) and its `Metadata/*`
methods. The -02 rewrite of this crate is bd JMAP-ua37.1.

## What This Crate Covers

| Module | Item(s) | Draft section |
|---|---|---|
| `capability.rs` | `DataTypeMetadataInfo`, `JMAP_METADATA_URI` | §1.2.1 |
| `filter.rs` | `MetadataTextMatch` | §3.6 |
| `namespace.rs` | `is_registered_namespace`, `is_vendor_namespace`, `is_valid_namespace` | §2.1 |

## What Is Out of Scope

- Depth counting, namespace-support checks, filter predicates, and
  `/changes` classification — `jmap-metadata-server`.
- Property-name constants and filter-condition builders —
  `jmap-metadata-client`.
- Typed `metadata` / `privateMetadata` fields, `Property` variants, or
  filter fields on the extension-types data objects. Decision
  (bd JMAP-ua37.4): both properties round-trip through each object's
  `extra` map under the workspace extras-preservation policy.
- Transport and network I/O — no tokio, no reqwest.

## Type Reference

### DataTypeMetadataInfo (§1.2.1)

| Wire field | Rust field | Default |
|---|---|---|
| `namespaces` | `namespaces: Vec<String>` | `[]` |
| `supportsVendorNamespaces` | `supports_vendor_namespaces: bool` | `false` |
| `supportsPrivate` | `supports_private: bool` | `false` |
| `maxDepth` | `max_depth: Option<u64>` | `null` (no server-enforced limit) |

### MetadataTextMatch (§3.6)

| Wire field | Rust field | Notes |
|---|---|---|
| `path` | `path: String` | `<namespace>` or `<namespace>/<key>`, RFC 6901 escaping |
| `value` | `value: String` | substring vs equality decided by the carrying condition |

## Test Oracle Strategy

Tests must use independent oracles — never derive expected values
from the code under test. Acceptable sources:

1. Literal JSON copied from the §6 examples (e.g. §6.1 capability,
   §6.7 query).
2. Hand-written JSON constructed directly from the §1.2.1 and §3.6
   field descriptions.
3. Namespace accept/reject cases taken from the §2.1 grammar.

Tests pinned to a draft example are named `*_draft_02_*`, after the
-02 example they were written from; -04 keeps those examples. When the
draft revs, update or replace them rather than mutating them silently.

## Spec References

- https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-04 — normative
- `~/PROJECT/IETF/specs/rfc8620.txt` — base protocol

## Dependencies

```toml
serde      = { workspace = true }
serde_json = { workspace = true }
# No tokio, no async, no network deps
```

## Type-design constraints

### Extras-preservation policy (JMAP-lbdy)

Neither public struct carries an `extra` field:

- `MetadataTextMatch` is filter algebra, excluded by the workspace
  policy "filter algebra excluded".
- `DataTypeMetadataInfo` is a capability object, consistent with the
  canonical extension-types template treating capabilities as
  Session-shape objects rather than data objects.

### Draft-version risk

`draft-ietf-jmap-metadata-04` is an active IETF Working Group draft
(expires 22 April 2027). All structs are `#[non_exhaustive]` so
adding fields when the draft revs is non-breaking. The -01 → -02
rewrite was a breaking change at every level (types, methods, wire
format); a future rev of similar scope will need the same
coordinated update across all three `jmap-metadata-*` crates.
