# jmap-metadata-server — Implementation Plan

Pure server-side helpers for the JMAP Object Metadata extension
([draft-ietf-jmap-metadata-02](https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-02)).

## Crate Family Position

```
jmap-metadata-types       data types
    └── jmap-metadata-server  ← this crate
```

## What This Crate Is

Stateless functions over `serde_json` values and `jmap-metadata-types`
structs that extension servers call when implementing the draft-02
extensions to their own methods (§3). draft-02 removed the -01
standalone `Metadata` object and its five `Metadata/*` methods, so the
-01 `MetadataBackend` trait, `register_metadata_handlers`, and
`MemoryBackend` were deleted in the -02 rewrite (bd JMAP-ua37.2).

## What This Crate Is Not

- Not a set of JMAP method handlers.
- Not coupled to any storage; holds no state.
- Not handling auth or per-viewer `privateMetadata` filtering — that
  needs caller identity and lives in each extension server's backend.
- Not wired into the workspace's extension servers yet — bd JMAP-s4kp.

## Capability URI

`urn:ietf:params:jmap:metadata` (§1.2.1), re-exported as
`JMAP_METADATA_URI`. Session-level value: empty object. Account-level
value: `{ "dataTypes": { "<Type>": DataTypeMetadataInfo, … } }`.

## Helper Coverage

| Helper | Draft § | Contract |
|---|---|---|
| `value_depth` | §2.1 | longest path of nested objects; scalars, empty arrays/objects, and flat objects are depth 1; arrays add no depth, objects inside them do |
| `exceeds_max_depth` | §2.1, §3.2 rule 4 | `value_depth(v) > max_depth` → caller rejects with `invalidProperties` |
| `is_namespace_supported` | §3.1, §3.2 rule 3, §3.5 | registered name listed in `namespaces`, or vendor domain name with `supportsVendorNamespaces: true` |
| `is_metadata_property` | §3.3 | `"metadata"` or `"privateMetadata"` |
| `is_metadata_only_change` | §3.3 | non-empty and every changed property is a metadata property |
| `metadata_path_exists` | §3.5 | namespace-only path requires a non-`{}` value |
| `metadata_text_contains` | §3.5 | case-insensitive substring; non-string or missing → no match |
| `metadata_text_equals` | §3.5 | case-sensitive byte equality; non-string or missing → no match |

## Caller responsibilities

The helpers are building blocks; the extension server owns:

1. `/set` rule order (§3.2): `forbidden` → private-unsupported →
   namespace-unsupported (except §2.1.1 removal) → `maxDepth`.
2. `null` metadata on create → `invalidProperties` (§3.2).
3. `overQuota` (§3.2.1, §5).
4. Per-viewer `privateMetadata` visibility and state advancement
   (§2.2, §4).
5. `updatedProperties` and `ignoreMetadataOnlyChanges` response
   shaping (§3.3).
6. `/get` subselectors (§3.1).
7. Unsupported-namespace filter conditions match nothing; any
   `privateMetadata*` condition when `supportsPrivate` is false →
   `unsupportedFilter` (§3.5).

## Module Layout

```
src/
  lib.rs        re-exports; JMAP_METADATA_URI re-export
  validate.rs   value_depth, exceeds_max_depth, is_namespace_supported
  changes.rs    METADATA_PROPERTY, PRIVATE_METADATA_PROPERTY,
                is_metadata_property, is_metadata_only_change
  filter.rs     metadata_path_exists, metadata_text_contains,
                metadata_text_equals
```

## Test Strategy

Inline `#[cfg(test)]` unit tests in each module. Oracles are the
draft's own statements and examples (e.g. the §2.1 depth examples,
the §6.7 query example), hand-written as JSON literals — never
derived from the code under test.

## Dependencies

```toml
jmap-metadata-types = { workspace = true }
serde_json          = { workspace = true }
```
