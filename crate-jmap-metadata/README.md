# jmap-metadata

JMAP Object Metadata ([draft-ietf-jmap-metadata-05]) in one dependency:
re-exports [`jmap-metadata-types`] always, [`jmap-metadata-client`] with the
`client` feature and [`jmap-metadata-server`] with the `server` feature.

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_metadata::types` |
| `client` | `jmap_metadata::client` |
| `server` | `jmap_metadata::server` |

- `types`: the wire types and capability constants.
- `client`: property-name constants, filter-condition builders and
  `/changes` helpers (no extension trait).
- `server`: pure helper functions for depth validation, namespace support,
  filter predicates and `/changes` classification (no handlers, no backend
  trait).

## What it's for

Object Metadata adds `metadata` and `privateMetadata` properties to other
JMAP data types, with per-type namespaces and filter conditions.
Applications that want the whole family depend on this crate and pick
`client`, `server` or both; workspace crates depend on the component crates
directly. Nothing else in the family is left out.

## How to use

```toml
[dependencies]
jmap-metadata = { version = "0.6", features = ["client"] }
```

```rust
use jmap_metadata::types::{DataTypeMetadataInfo, JMAP_METADATA_URI};

assert_eq!(JMAP_METADATA_URI, "urn:ietf:params:jmap:metadata");
let info: DataTypeMetadataInfo = serde_json::from_str(r#"{"namespaces": ["photography"],
    "supportsVendorNamespaces": false, "supportsPrivate": false, "maxDepth": 3}"#)
    .unwrap();
assert_eq!(info.max_depth, Some(3));
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_metadata::server::X` is the same item as `jmap_metadata_server::X`.
The component crates version in lockstep with this one on the shared
`jmap-*` line.

## Gotchas

- There is no `memory` feature: `jmap-metadata-server` has no reference
  backend.
- Depending on this crate and on a component crate at a different version
  gives two copies of each type, which do not unify. Keep them on one
  version line.
- Version 0.6.0 replaces the 0.0.1 name-claim placeholder.

## References

- [draft-ietf-jmap-metadata-05] — JMAP Object Metadata
- [RFC 8620] — JMAP Core

[draft-ietf-jmap-metadata-05]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-metadata-05
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-metadata-types`]: https://crates.io/crates/jmap-metadata-types
[`jmap-metadata-client`]: https://crates.io/crates/jmap-metadata-client
[`jmap-metadata-server`]: https://crates.io/crates/jmap-metadata-server
