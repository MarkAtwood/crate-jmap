# jmap-blob-server

JMAP Blob Management method handlers for the `jmap-server` dispatcher:
`Blob/get`, `Blob/set`, `Blob/lookup` and `Blob/convert`
([draft-ietf-jmap-blobext-03]), and `Blob/upload` ([RFC 9404]).

**Status: scaffold.** The crate has no public items yet; the backend trait
and handlers land in follow-up work (see `PLAN.md`).

## What it is

Backend-agnostic handlers over a `BlobBackend` trait, plus an in-memory
reference backend behind the `memory` feature (`test-helpers` adds test
knobs).

## What it's for

A JMAP server that stores blobs plugs its storage into `BlobBackend` and
calls `register_blob_handlers` on its `Dispatcher`. The crates own wire
validation, method semantics and error mapping; blob storage, the reverse
lookup index and media conversion are backend hooks.

## How to use

Nothing to use yet.

## How it works

Handlers will follow the canonical `jmap-mail-server` template: one module
per method, RPITIT `+ Send` backend hooks, and `memory::MemoryBackend` as a
non-production reference implementation.

## Gotchas

- A request that lists both `urn:ietf:params:jmap:blob` and
  `urn:ietf:params:jmap:blob2` in `using` gets `invalidArguments` from every
  `Blob/*` method (blobext-03 §2.1).

## Crate family

```
jmap-types (incl. the data_types registry)
    ├── jmap-server          dispatcher
    └── jmap-blob-types      data types
            └── jmap-blob-server  ← this crate
```

## References

- **[draft-ietf-jmap-blobext-03]** — JMAP Blob Management Extension
- **[RFC 9404]** — JMAP Blob Management Extension
- **[RFC 8620]** — JMAP Core

[draft-ietf-jmap-blobext-03]: https://datatracker.ietf.org/doc/draft-ietf-jmap-blobext/03/
[RFC 9404]: https://www.rfc-editor.org/rfc/rfc9404
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
