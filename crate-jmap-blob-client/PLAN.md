# jmap-blob-client — Implementation Plan

draft-ietf-jmap-blobext-03 client bindings. Design:
`docs/design/blobext.md` §8. Tracked by bd JMAP-tna1 (epic), bead B12.

| Module | Contents |
|---|---|
| `lib.rs` | `JmapBlobExt` (sealed): `with_blob_session`, re-exports |
| `methods.rs` | `SessionClient`: `blob_set`, `blob_create`, `blob_touch`, `blob_destroy`, `blob_get`, `blob_lookup`, `blob_convert`, `upload_url`, `preferred_digest_algorithm`, `lookup_type_names`, `verify_digest`, `for_account`, `blob_capability` |

`using` is core + blob2 (+ the type capabilities of `Blob/lookup` names),
never `urn:ietf:params:jmap:blob`.

Out of scope: RFC 9404 bindings (`Blob/upload`, 9404-mode get/lookup) —
bd B24, P3.
