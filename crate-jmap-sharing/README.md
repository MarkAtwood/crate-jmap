# jmap-sharing

JMAP Sharing ([RFC 9670]) in one dependency: re-exports
[`jmap-sharing-types`] always, [`jmap-sharing-client`] with the `client`
feature and [`jmap-sharing-server`] with the `server` feature (`memory` adds
the server's in-memory reference backend).

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_sharing::types` |
| `client` | `jmap_sharing::client` |
| `server` | `jmap_sharing::server` |
| `memory` | `server`, plus `jmap_sharing::server::memory` |

- `types`: the wire types and capability constants.
- `client`: the sealed `JmapSharingExt` extension trait over
  `jmap-base-client`.
- `server`: `Principal/*` and `ShareNotification/*` handlers over
  `SharingBackend`, `register_sharing_handlers`.

## What it's for

Sharing defines Principals (users, groups, resources) and
ShareNotifications, which other JMAP types use to share data between
accounts. Applications that want the whole family depend on this crate and
pick `client`, `server` or both; workspace crates depend on the component
crates directly. Nothing else in the family is left out.

## How to use

```toml
[dependencies]
jmap-sharing = { version = "0.6", features = ["client"] }
```

```rust
use jmap_sharing::types::{JMAP_PRINCIPALS_URI, PrincipalsCapability};

assert_eq!(JMAP_PRINCIPALS_URI, "urn:ietf:params:jmap:principals");
let cap: PrincipalsCapability =
    serde_json::from_str(r#"{"currentUserPrincipalId": "P1"}"#).unwrap();
assert!(cap.current_user_principal_id.is_some());
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_sharing::server::X` is the same item as `jmap_sharing_server::X`. The
component crates version in lockstep with this one on the shared `jmap-*`
line.

## Gotchas

- `server` alone does not build the reference backend; enable `memory` for
  `server::memory::MemoryBackend` (demonstration quality, not production).
- Depending on this crate and on a component crate at a different version
  gives two copies of each type, which do not unify. Keep them on one
  version line.
- Version 0.6.0 replaces the 0.0.1 name-claim placeholder.

## References

- [RFC 9670] — JMAP Sharing
- [RFC 8620] — JMAP Core

[RFC 9670]: https://www.rfc-editor.org/rfc/rfc9670
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-sharing-types`]: https://crates.io/crates/jmap-sharing-types
[`jmap-sharing-client`]: https://crates.io/crates/jmap-sharing-client
[`jmap-sharing-server`]: https://crates.io/crates/jmap-sharing-server
