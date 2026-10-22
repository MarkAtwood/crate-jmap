# jmap-metadata-client

## What it is

Pure, stateless client-side helpers for the JMAP Object Metadata
extension ([draft-ietf-jmap-metadata-04]). No `SessionClient`, no
extension trait, no network I/O. Depends only on
[`jmap-metadata-types`] and `serde_json`.

## What it's for

draft-04 has no standalone `Metadata` object and no `Metadata/*`
methods. Metadata is read and written through `metadata` /
`privateMetadata` properties on each listed data type, using that
type's own methods (§3). Use these helpers alongside the typed
extension clients (`jmap-mail-client`, `jmap-calendars-client`, …):

| Helper | Use | Draft § |
|---|---|---|
| `METADATA_PROPERTY`, `PRIVATE_METADATA_PROPERTY` | names for a `Foo/get` `properties` list | §2, §3.1 |
| `IGNORE_METADATA_ONLY_CHANGES` | `Foo/changes` request argument name | §3.3 |
| `is_metadata_property`, `is_metadata_only_change` | classify a `Foo/changes` `updatedProperties` value | §3.3 |
| `metadata_exists_condition`, `private_metadata_exists_condition` | `metadataExists` / `privateMetadataExists` filter conditions | §3.6 |
| `metadata_text_contains_condition`, `private_metadata_text_contains_condition` | `metadataTextContains` / `privateMetadataTextContains` | §3.6 |
| `metadata_text_equals_condition`, `private_metadata_text_equals_condition` | `metadataTextEquals` / `privateMetadataTextEquals` | §3.6 |

`JMAP_METADATA_URI`, `DataTypeMetadataInfo`, `MetadataTextMatch`, and
the namespace validators are re-exported from `jmap-metadata-types`.

## How to use

```rust
use jmap_metadata_client::{metadata_text_contains_condition, METADATA_PROPERTY};
use serde_json::json;

// Fetch an object's shared metadata alongside other properties (§3.1).
let properties = vec!["id", "name", METADATA_PROPERTY];
assert!(properties.contains(&"metadata"));

// Compose a metadata condition with the type's own conditions (§3.6).
// An extension client's `*_query` method takes it as
// `jmap_types::Filter::Condition(filter)`; for a typed tree, use
// `jmap_metadata_types::WithMetadata<C>` leaves instead.
let filter = json!({
    "operator": "AND",
    "conditions": [
        { "inMailbox": "MB-inbox" },
        metadata_text_contains_condition("acme.example.com/memo", "follow up"),
    ]
});
assert_eq!(
    filter["conditions"][1],
    json!({ "metadataTextContains": { "path": "acme.example.com/memo", "value": "follow up" } })
);
```

## Gotchas

- **Pinned to draft-04.** draft-02 was a complete rewrite of -01;
  later revisions (or a final RFC) may change the wire shape again.
- **Capability-dependent conditions.** Any `privateMetadata*` condition
  is rejected with `unsupportedFilter` when the data type's
  `supportsPrivate` is false; servers MAY reject the four text-match
  conditions with `unsupportedFilter` and clients SHOULD be prepared
  to fall back to a client-side scan (§3.6). Check
  `DataTypeMetadataInfo` in the account capability first.
- **Typed filters.** The extension-types `FilterCondition` structs
  carry no metadata fields (bd JMAP-ua37.4). The extension clients'
  `*_query` / `*_query_changes` methods take a generic
  `jmap_types::Filter<C>`, so a typed tree uses
  `jmap_metadata_types::WithMetadata<C>` leaves and an untyped one is
  passed as `Filter::Condition(value)`.

## Migration from 0.1 (draft-01)

The 0.1 crate exposed `JmapMetadataExt`, `SessionClient`,
`metadata_get`, `metadata_changes`, `metadata_set`, `metadata_query`,
`metadata_query_changes`, and their parameter structs. All are removed
in 0.2.

## Crate family

```
jmap-metadata-types
    └── jmap-metadata-client  ← this crate
```

## References

- **[draft-ietf-jmap-metadata-04]** — JMAP Object Metadata (normative
  for property names, the `/changes` argument, and filter conditions).
- **[RFC 8620]** — JMAP Core (`/get`, `/changes`, `/query`, filter
  operators).

[draft-ietf-jmap-metadata-04]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-04
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-metadata-types`]: ../crate-jmap-metadata-types
