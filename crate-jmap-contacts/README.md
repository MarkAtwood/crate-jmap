# jmap-contacts

JMAP Contacts ([RFC 9610]) in one dependency: re-exports
[`jmap-contacts-types`] always, [`jmap-contacts-client`] with the `client`
feature and [`jmap-contacts-server`] with the `server` feature (`memory`
adds the server's in-memory reference backend).

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_contacts::types` |
| `client` | `jmap_contacts::client` |
| `server` | `jmap_contacts::server` |
| `memory` | `server`, plus `jmap_contacts::server::memory` |

- `types`: the wire types and capability constants.
- `client`: the sealed `JmapContactsExt` extension trait over
  `jmap-base-client`.
- `server`: method handlers for `AddressBook/*` and `ContactCard/*` over `ContactsBackend`.

## What it's for

Contacts are JSContact cards in address books, synchronised over JMAP. Applications that want the whole family depend on this crate
and pick `client`, `server` or both; workspace crates depend on the
component crates directly. Bridges to other families (for example the
`jmap-chat-*-server` crates) are not re-exported; depend on them directly.

## How to use

```toml
[dependencies]
jmap-contacts = { version = "0.6", features = ["client"] }
```

```rust
use jmap_contacts::types::JMAP_CONTACTS_URI;

assert_eq!(JMAP_CONTACTS_URI, "urn:ietf:params:jmap:contacts");
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_contacts::server::X` is the same item as `jmap_contacts_server::X`.
The component crates version in lockstep with this one on the shared
`jmap-*` line.

## Gotchas

- `server` alone does not build the reference backend; enable `memory` for
  `server::memory::MemoryBackend` (demonstration quality, not production).
- Depending on this crate and on a component crate at a different version
  gives two copies of each type, which do not unify. Keep them on one
  version line.
- Version 0.6.0 replaces the 0.0.1 name-claim placeholder.

## References

- [RFC 9610] — JMAP Contacts
- [RFC 8620] — JMAP Core

[RFC 9610]: https://www.rfc-editor.org/rfc/rfc9610
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-contacts-types`]: https://crates.io/crates/jmap-contacts-types
[`jmap-contacts-client`]: https://crates.io/crates/jmap-contacts-client
[`jmap-contacts-server`]: https://crates.io/crates/jmap-contacts-server
