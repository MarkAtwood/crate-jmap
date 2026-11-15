# jmap-blob-types — Implementation Plan

draft-ietf-jmap-blobext-03 (`urn:ietf:params:jmap:blob2`) and RFC 9404
(`urn:ietf:params:jmap:blob`) data types. Types only — no method handlers,
no async, no network I/O. Design: `docs/design/blobext.md` §4; decisions:
`docs/design/DECISIONS.md`. Tracked by bd JMAP-tna1.2 (epic).

## Crate Family Position

```
jmap-types (RFC 8620 wire primitives, data_types registry)
    └── jmap-blob-types  ← this crate
            ├── jmap-blob-server (method handlers, BlobBackend)
            └── jmap-blob-client (typed bindings)
```

## Planned Modules

| Module | Contents | Spec | Bead |
|---|---|---|---|
| `capability.rs` | `JMAP_BLOB2_URI`, `JMAP_BLOB_URI`, `BlobCapability`, `BlobAccountCapability` (+ `rfc9404()` view) | -03 §2.1; RFC 9404 §3.1 | JMAP-tna1.2.2 |
| `digest.rs` | `DigestAlgorithm` (RFC 9530 names), built-in digesters (sha, sha-256, sha-512) | -03 §4, §5 | JMAP-tna1.2.2 |
| `blob_ref.rs` | `BlobRef` (literal id or `#creationId`) | RFC 8620 §5.3 | JMAP-tna1.2.2 |
| `source.rs` | `DataSourceObject` (custom serde: exactly one data key) | -03 §4 | JMAP-tna1.2.2 |
| `set.rs` | `Blob/set` create/update/response types, `BlobObject` | -03 §4 | JMAP-tna1.2.2 |
| `get.rs` | `Blob/get` request/response types, `ImageData`, chunks | -03 §5 | JMAP-tna1.2.2 |
| `convert.rs` | `Blob/convert` recipes, `ArchiveEntry`, enums, `compress_level_range` | -03 §8 | JMAP-tna1.2.3 |
| `lookup.rs` | `Blob/lookup` request/response types | -03 §6 | JMAP-tna1.2.3 |
| `rfc9404.rs` | RFC 9404 `Blob/upload` types | RFC 9404 §4.1 | JMAP-tna1.2.11 |

The JMAP Data Types registry is `jmap_types::data_types`, not this crate
(decision 14).

## Out of Scope

- Method handlers, the backend trait, storage and conversion —
  `jmap-blob-server`.
- The umbrella `jmap-blob` crate — bd B22.
