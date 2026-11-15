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

## Modules

| Module | Contents | Spec | Bead |
|---|---|---|---|
| `capability.rs` | `JMAP_BLOB2_URI`, `JMAP_BLOB_URI`, `BlobCapability`, `BlobAccountCapability` (+ `rfc9404()` view) | -03 §2.1; RFC 9404 §3.1 | JMAP-tna1.2.2 (done) |
| `digest.rs` | `DigestAlgorithm` (RFC 9530 names), built-in digesters (sha, sha-256, sha-512) | -03 §2.1; RFC 9530 §7.2 | JMAP-tna1.2.2 (done) |
| `blob_ref.rs` | `BlobRef` (literal id or `#creationId`) | RFC 8620 §5.3 | JMAP-tna1.2.2 (done) |
| `source.rs` | `DataSourceObject` (custom serde: `digest:*` keys; the exactly-one-data-key rule is the server's check) | -03 §3 | JMAP-tna1.2.2 (done) |
| `set.rs` | `Blob/set` create/update/response types, `BlobObject` | -03 §4, §7 | JMAP-tna1.2.2 (done) |
| `errors.rs` | `UNKNOWN_DATA_TYPE`, `UNKNOWN_FORMAT`, `BLOB_HAS_REFERENCE`, `CONVERSION_FAILED` | -03 §11.3, §11.4 | JMAP-tna1.2.2 (done) |
| `get.rs` | `Blob/get` request/response types, `ImageData`, chunks | -03 §5 | JMAP-tna1.2.2 (done) |
| `convert.rs` | `BlobConvertArgs`, `Blob/convert` recipes, `ArchiveEntry`, enums, `compress_level_range` | -03 §8 | JMAP-tna1.2.3 (done) |
| `lookup.rs` | `Blob/lookup` request/response types | -03 §6 | JMAP-tna1.2.3 (done) |
| `rfc9404.rs` | RFC 9404 `Blob/upload` types | RFC 9404 §4.1 | JMAP-tna1.2.11 |

The JMAP Data Types registry is `jmap_types::data_types`, not this crate
(decision 14).

## Out of Scope

- Method handlers, the backend trait, storage and conversion —
  `jmap-blob-server`.
- The umbrella `jmap-blob` crate — bd B22.

`BlobObject: SetObject` (for the conditional opt-in) comes with
JMAP-tna1.2.12.
