# jmap-blob-types

Serde-annotated Rust types for JMAP Blob Management:
[draft-ietf-jmap-blobext-03] (`urn:ietf:params:jmap:blob2`) and [RFC 9404]
(`urn:ietf:params:jmap:blob`). Types only — no method handlers, no async,
no network I/O.

**Status: scaffold.** The crate has no public items yet; the wire types land
in follow-up work (see `PLAN.md`).

## What it is

The data types for `Blob/set`, `Blob/get`, `Blob/lookup` and `Blob/convert`
(blobext-03), and `Blob/upload` (RFC 9404): the capability objects, digest
algorithms, data source objects, conversion recipes and their responses.

## What it's for

Consumed by `jmap-blob-server` (method handlers and the `BlobBackend`
trait) and `jmap-blob-client` (typed method bindings).

## How to use

Nothing to use yet. Once the types land, decode a `Blob/get` or `Blob/set`
response with `serde_json` as with any other `jmap-*-types` crate.

## How it works

The types will follow the workspace extension-types template
(`jmap-mail-types`): camelCase wire names quoted from the drafts,
`#[non_exhaustive]` structs with an `extra` map for unknown fields, and
`BTreeMap` for wire maps.

## Gotchas

- blobext-03 and RFC 9404 use different capability URIs; a client MUST NOT
  put both in one request's `using`.

## Crate family

```
jmap-types (RFC 8620 wire primitives)
    └── jmap-blob-types  ← this crate
            ├── jmap-blob-server (method handlers)
            └── jmap-blob-client (client extension trait)
```

## References

- **[draft-ietf-jmap-blobext-03]** — JMAP Blob Management Extension
- **[RFC 9404]** — JMAP Blob Management Extension
- **[RFC 8620]** — JMAP Core

[draft-ietf-jmap-blobext-03]: https://datatracker.ietf.org/doc/draft-ietf-jmap-blobext/03/
[RFC 9404]: https://www.rfc-editor.org/rfc/rfc9404
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
