# jmap-blob-server — Implementation Plan

draft-ietf-jmap-blobext-03 and RFC 9404 method handlers. Design:
`docs/design/blobext.md` §5–§7; decisions: `docs/design/DECISIONS.md`.
Tracked by bd JMAP-tna1.2 (epic).

## Modules

| Module | Contents | Spec | Bead |
|---|---|---|---|
| `lib.rs` | `register_blob_handlers` (Blob/get, set, lookup, convert; upload with JMAP-tna1.2.11), public `handle_blob_*` | -03 §3 | JMAP-tna1.2.4 (done) |
| `backend.rs` | `BlobBackend: JmapBackend` hooks (design §6) | -03 | JMAP-tna1.2.4 (done) |
| `mode.rs` | `BlobMode::from_using`: blob2 vs RFC 9404 vs both (`invalidArguments`) | -03 §2.1 | JMAP-tna1.2.4 (done) |
| `range.rs` | range clamp, UTF-8 and digest helpers | -03 §5 | JMAP-tna1.2.4 (done) |
| `order.rs` | topological sort of `#creationId` refs, cycle detection | RFC 8620 §5.3 | JMAP-tna1.2.4 (done) |
| `memory/` (feature `memory`) | `MemoryBackend`: store, chunks, reference index, stored digests, touch | design §7 | JMAP-tna1.2.5 |
| `memory/convert.rs` | ustar archive/extract, deferred results, stub converter (`test-helpers`) | -03 §8 | JMAP-tna1.2.6 |
| `get.rs` | `Blob/get` (stub answering `serverFail` until then) | -03 §5 | JMAP-tna1.2.7 |
| `set.rs` | `Blob/set` | -03 §4 | JMAP-tna1.2.8 |
| `lookup.rs` | `Blob/lookup` | -03 §6 | JMAP-tna1.2.9 |
| `convert.rs` | `Blob/convert` | -03 §8 | JMAP-tna1.2.10 |
| `upload.rs` | RFC 9404 `Blob/upload` and 9404-mode get/lookup | RFC 9404 §4 | JMAP-tna1.2.11 |

Later: `register_blob_conditional` (JMAP-tna1.2.12), RFC 8620 §6.3
`Blob/copy` (JMAP-tna1.2.14).

## Out of Scope

- Blob storage, the reverse lookup index and media conversion — backend
  hooks the consumer implements.
- Session capability mounting — the consumer (testjig: JMAP-tna1.2.13).
