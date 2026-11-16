# jmap-blob-client

Typed JMAP Blob Management ([draft-ietf-jmap-blobext-03]) calls over
`jmap-base-client`: `Blob/set`, `Blob/get`, `Blob/lookup` and
`Blob/convert`.

## What it is

A sealed extension trait on `jmap_base_client::JmapClient` that binds a
Session and returns a `SessionClient` with one method per `Blob/*` call.

## What it's for

Clients that create blobs from pieces of other blobs, read ranges and
digests, find which objects reference a blob, or convert blobs on the
server.

## How to use

```rust,ignore
use jmap_blob_client::{JmapBlobExt, BlobGetInput};

let session = client.fetch_session().await?;
let blobs = client.with_blob_session(session);
let created = blobs.blob_create(create_map).await?;           // Blob/set
let got = blobs.blob_get(ids, BlobGetInput::default()).await?; // Blob/get
let refs = blobs.blob_lookup(ids, vec!["Email".into()], &[]).await?;
let out = blobs.blob_convert(recipes).await?;                  // Blob/convert
```

| Method | Call |
|---|---|
| `blob_set(BlobSetInput)`, `blob_create`, `blob_touch(ids, expires)`, `blob_destroy` | `Blob/set` (§4, §7) |
| `blob_get(ids, BlobGetInput)` | `Blob/get` (§5) |
| `blob_lookup(ids, type_names, extra_using)` | `Blob/lookup` (§6) |
| `blob_convert(create)` | `Blob/convert` (§8) |
| `blob_account_id`, `for_account`, `blob_capability`, `upload_url`, `preferred_digest_algorithm`, `lookup_type_names`, `SessionClient::verify_digest` | session and capability helpers |

A conditional `Blob/set` (draft-ietf-jmap-conditional-02) goes through
`jmap_base_client::SetCall`.

## How it works

Mirrors the canonical `jmap-mail-client`. Requests carry `core` and
`urn:ietf:params:jmap:blob2` in `using`, plus the type capabilities that
`Blob/lookup` needs: each type name's capability from the JMAP Data Types
registry (`jmap_types::data_types`). A registered name whose capability the
Session lacks, and empty `ids` or `type_names`, are refused before sending
with `InvalidArgument`. `upload_url` prefers the account capability's
`uploadUrl` and falls back to the Session's.

## Gotchas

- This crate speaks blobext-03 (`blob2`) only. RFC 9404 bindings are later
  work.

## Crate family

```
jmap-base-client
    └── jmap-blob-client  ← this crate
jmap-blob-types           data types
```

## References

- **[draft-ietf-jmap-blobext-03]** — JMAP Blob Management Extension
- **[RFC 8620]** — JMAP Core

[draft-ietf-jmap-blobext-03]: https://datatracker.ietf.org/doc/draft-ietf-jmap-blobext/03/
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
