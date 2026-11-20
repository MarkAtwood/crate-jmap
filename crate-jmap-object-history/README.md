# jmap-object-history

JMAP Object History ([draft-ietf-jmap-object-history-03]) in one dependency:
re-exports [`jmap-object-history-types`] always,
[`jmap-object-history-client`] with the `client` feature and
[`jmap-object-history-server`] with the `server` feature.

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_object_history::types`, `jmap_object_history::CAPABILITY` (kept from 0.0.1) |
| `client` | `jmap_object_history::client` |
| `server` | `jmap_object_history::server` |

- `types`: the wire types and capability constants.
- `client`: the sealed `JmapObjectHistoryExt` extension trait over
  `jmap-base-client`.
- `server`: backend helpers: version selection, retention and change folding
  (no method handlers; those are in `jmap-server`).

## What it's for

Object History lets a client fetch earlier versions of objects and restore
destroyed ones. Applications that want the whole family depend on this crate
and pick `client`, `server` or both; workspace crates depend on the
component crates directly. Nothing else in the family is left out; the
history method handlers live in `jmap-server`'s `history` module.

## How to use

```toml
[dependencies]
jmap-object-history = { version = "0.6", features = ["client"] }
```

```rust
use jmap_object_history::types::{JMAP_OBJECT_HISTORY_URI, ObjectHistoryAccountCapability};

assert_eq!(JMAP_OBJECT_HISTORY_URI, "urn:ietf:params:jmap:object-history");
assert_eq!(jmap_object_history::CAPABILITY, JMAP_OBJECT_HISTORY_URI);
let cap = ObjectHistoryAccountCapability::new(
    Some(2_592_000),
    vec!["ContactCard".to_owned(), "Email".to_owned()],
);
assert!(cap.supports_type("Email"));
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_object_history::server::X` is the same item as
`jmap_object_history_server::X`. The component crates version in lockstep
with this one on the shared `jmap-*` line.

## Gotchas

- There is no `memory` feature: `jmap-object-history-server` has no
  reference backend.
- Depending on this crate and on a component crate at a different version
  gives two copies of each type, which do not unify. Keep them on one
  version line.
- Version 0.6.0 replaces the 0.0.1 name-claim placeholder.

## References

- [draft-ietf-jmap-object-history-03] — JMAP Object History
- [RFC 8620] — JMAP Core

[draft-ietf-jmap-object-history-03]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-object-history-03
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-object-history-types`]: https://crates.io/crates/jmap-object-history-types
[`jmap-object-history-client`]: https://crates.io/crates/jmap-object-history-client
[`jmap-object-history-server`]: https://crates.io/crates/jmap-object-history-server
