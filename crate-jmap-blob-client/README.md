# jmap-blob-client

Typed JMAP Blob Management ([draft-ietf-jmap-blobext-03]) calls over
`jmap-base-client`: `Blob/set`, `Blob/get`, `Blob/lookup` and
`Blob/convert`.

**Status: scaffold.** The crate has no public items yet; the bindings land
in follow-up work (see `PLAN.md`).

## What it is

A sealed extension trait on `jmap_base_client::JmapClient` that binds a
Session and returns a `SessionClient` with one method per `Blob/*` call.

## What it's for

Clients that create blobs from pieces of other blobs, read ranges and
digests, find which objects reference a blob, or convert blobs on the
server.

## How to use

Nothing to use yet.

## How it works

Mirrors the canonical `jmap-mail-client`. Requests carry `core` and
`urn:ietf:params:jmap:blob2` in `using`, plus the type capabilities that
`Blob/lookup` needs.

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
