# jmap-metadata-client — Implementation Plan

Pure client-side helpers for the JMAP Object Metadata extension
([draft-ietf-jmap-metadata-04]).

## Crate Family Position

```
jmap-metadata-types
    └── jmap-metadata-client  ← this crate
```

## What This Crate Is

Constants and JSON builders that callers combine with the typed
extension clients' own `Foo/get`, `Foo/changes`, and `Foo/query`
calls. draft-02 removed the -01 standalone `Metadata` object and its
`Metadata/*` methods, so the -01 `JmapMetadataExt` trait,
`SessionClient`, and five method bindings were deleted in the -02
rewrite (bd JMAP-ua37.3).

| Item | Draft § |
|---|---|
| `METADATA_PROPERTY`, `PRIVATE_METADATA_PROPERTY` | §2, §3.1 |
| `IGNORE_METADATA_ONLY_CHANGES` | §3.3 |
| `is_metadata_property`, `is_metadata_only_change` | §3.3 |
| `metadata_exists_condition`, `private_metadata_exists_condition` | §3.6 |
| `metadata_text_contains_condition`, `private_metadata_text_contains_condition` | §3.6 |
| `metadata_text_equals_condition`, `private_metadata_text_equals_condition` | §3.6 |
| re-exports from `jmap-metadata-types` | §1.2.1, §2.1, §3.6 |

## What This Crate Is Not

- Not a server-side crate (see `jmap-metadata-server`).
- Not an HTTP client — no transport, no session handling.
- Not a typed extension of each type's `FilterCondition`: by decision
  (bd JMAP-ua37.4) the builders return `serde_json::Value`. Extension
  clients whose `*_query` takes a `Value` filter accept them directly;
  the chat client and two calendars-client query methods take typed
  filters and cannot (bd JMAP-ua37.7).

## Module Layout

```
src/
  lib.rs   constants, classifiers, condition builders, re-exports
```

## Test Strategy

Inline `#[cfg(test)]` unit tests plus rustdoc examples. Oracles are
wire names and hand-written JSON literals taken from the §3.3 and §3.6
field definitions — never derived from the code under test.

## References

- **[draft-ietf-jmap-metadata-04]** — JMAP Object Metadata (normative
  for property names, the `/changes` argument, and filter conditions).
- **[RFC 8620]** — JMAP Core (`/get`, `/changes`, `/query`, filter
  operators).

[draft-ietf-jmap-metadata-04]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-04
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
