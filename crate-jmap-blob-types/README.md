# jmap-blob-types

Serde-annotated Rust types for JMAP Blob Management:
[draft-ietf-jmap-blobext-03] (`urn:ietf:params:jmap:blob2`) and the
capability of [RFC 9404] (`urn:ietf:params:jmap:blob`). Types only — no
method handlers, no async, no network I/O.

## What it is

| Type | Module | Source |
|---|---|---|
| `JMAP_BLOB2_URI`, `JMAP_BLOB_URI`, `BlobCapability`, `BlobAccountCapability` (`check`, `rfc9404`), `Rfc9404AccountCapability`, `CapabilityError` | `capability` | -03 §2.1; RFC 9404 §3.1 |
| `DigestAlgorithm`, `Digest` (built-in sha, sha-256, sha-512) | `digest` | -03 §2.1; RFC 9530 §7.2 |
| `BlobRef` (blobId or `#creationId`) | `blob_ref` | RFC 8620 §5.3 |
| `DataSourceObject` | `source` | -03 §3 |
| `BlobSetArgs`, `BlobCreateObject`, `BlobObject`, `BlobUpdated`, `BlobSetResponse` | `set` | -03 §4, §7 |
| `BlobGetArgs`, `BlobProperty`, `DataSourceProperty`, `BlobGetItem`, `BlobGetResponse`, `ImageData`, `ImageGps` | `get` | -03 §5 |
| `BlobLookupArgs`, `BlobInfo`, `BlobLookupResponse` | `lookup` | -03 §6 |
| `BlobConvertArgs`, `ConvertRequest`, `Recipe` and its seven recipes, `ArchiveEntry`, `EntryType`, `CompressionMethod`, `ColorSpace`, `ConvertedBlob`, `BlobConvertResponse`, `compress_level_range` | `convert` | -03 §8 |
| `BlobUploadArgs`, `UploadObject`, `BlobUploadCallResponse` | `rfc9404` | RFC 9404 §4.1 |
| `errors::{UNKNOWN_DATA_TYPE, UNKNOWN_FORMAT, BLOB_HAS_REFERENCE, CONVERSION_FAILED}` | `errors` | -03 §11.3, §11.4 |

## What it's for

Consumed by `jmap-blob-server` (method handlers and the `BlobBackend`
trait) and `jmap-blob-client` (typed method bindings). Blob/set builds
blobs from inline data and ranges of other blobs, Blob/get reads ranges,
digests and chunk structure, Blob/lookup finds the objects referencing a
blob, and Blob/convert transforms blobs on the server.

## How to use

```rust
use jmap_blob_types::{BlobSetArgs, DigestAlgorithm};

let args: BlobSetArgs = serde_json::from_str(r#"{
    "accountId": "abc",
    "create": {
        "b1": {
            "data": [{"data:asText": "Hello, world!"}],
            "type": "text/plain",
            "digest:sha-256": "MV9b23bQeMQ7isAGTkoBZGErH853yGk0W/yUx1iU7dM="
        }
    }
}"#).unwrap();

let b1 = &args.create.unwrap()["b1"];
let mut d = DigestAlgorithm::Sha256.builtin_digester().unwrap();
d.update(b"Hello, world!");
assert_eq!(d.finish(), b1.digests[&DigestAlgorithm::Sha256]);
```

## How it works

- Wire names are quoted from the drafts. Structs are `#[non_exhaustive]`
  with a constructor and keep unknown members in `extra`; wire maps are
  `BTreeMap`.
- `digest:<alg>` members (in DataSourceObject, BlobCreateObject and
  Blob/get list entries) are collected into a
  `BTreeMap<DigestAlgorithm, String>` by custom serde. `DigestAlgorithm`
  compares, orders and hashes by its wire string, so map keys serialize
  in wire order and `Other("sha")` equals `Sha`.
- Tri-state properties (`expires`, `data:asText`, `imageData`) are
  `Option<Option<T>>`: absent, `null`, or a value.
- `oldState` / `newState` of Blob/set are always `null` (blobs have no
  state); empty `/set` and `/convert` result maps are written as `null`.

## Gotchas

- A request MUST NOT list both `urn:ietf:params:jmap:blob` and
  `urn:ietf:params:jmap:blob2` in `using` (-03 §2.1).
- `DataSourceObject` does not enforce "exactly one of data:asText,
  data:asBase64, blobId": the server reports a violation as
  `invalidProperties` `["data"]`, which a parse error could not.
- `ConvertRequest` does fail to parse without exactly one recipe; a server
  that answers per entry checks `Recipe::KEYS` on the raw object first.
- `Blob/get` `ids` and every recipe blobId may be `#creationId` references
  (`BlobRef`); response ids are plain `Id`s.
- The JMAP Data Types registry used by Blob/lookup is
  `jmap_types::data_types`, not this crate.

## Crate family

```
jmap-types (RFC 8620 wire primitives, data_types registry)
    └── jmap-blob-types  ← this crate
            ├── jmap-blob-server (method handlers)
            └── jmap-blob-client (client extension trait)
```

## References

- **[draft-ietf-jmap-blobext-03]** — JMAP Blob Management Extension
- **[RFC 9404]** — JMAP Blob Management Extension
- **[RFC 9530]** — Digest Fields (algorithm names)
- **[RFC 8620]** — JMAP Core

[draft-ietf-jmap-blobext-03]: https://datatracker.ietf.org/doc/draft-ietf-jmap-blobext/03/
[RFC 9404]: https://www.rfc-editor.org/rfc/rfc9404
[RFC 9530]: https://www.rfc-editor.org/rfc/rfc9530
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
