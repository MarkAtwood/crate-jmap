# jmap-mail-sharing

JMAP for Mail Sharing ([draft-ietf-jmap-mail-sharing-02]) in one
dependency: re-exports the RFC 9670 sharing crates it builds on,
[`jmap-sharing-types`] always, [`jmap-sharing-client`] with the `client`
feature and [`jmap-sharing-server`] with the `server` feature (`memory`
adds the server's in-memory reference backend).

## What it is

A facade with no code of its own. Mail sharing is not a separate crate
family: the Principals and ShareNotifications it relies on are the
`jmap-sharing-*` crates, which this crate re-exports whole, the same set
the `jmap-sharing` umbrella re-exports.

| Feature | Re-export |
|---|---|
| (none) | `jmap_mail_sharing::types`, `jmap_mail_sharing::CAPABILITY` (kept from 0.0.1: `urn:ietf:params:jmap:mail:share`) |
| `client` | `jmap_mail_sharing::client` |
| `server` | `jmap_mail_sharing::server` |
| `memory` | `server`, plus `jmap_mail_sharing::server::memory` |

## What it's for

The draft extends the RFC 8621 Mailbox with the RFC 9670 `shareWith` map
(Principal id to MailboxRights) and adds a `mayShare` right, so a user can
share a mailbox with other principals. Applications that want the sharing
crates under the mail-sharing name depend on this crate; workspace crates
depend on the component crates directly.

## How to use

```toml
[dependencies]
jmap-mail-sharing = { version = "0.5", features = ["client"] }
```

```rust
use jmap_mail_sharing::types::{JMAP_PRINCIPALS_OWNER_URI, PrincipalsOwnerCapability};

assert_eq!(jmap_mail_sharing::CAPABILITY, "urn:ietf:params:jmap:mail:share");
assert_eq!(JMAP_PRINCIPALS_OWNER_URI, "urn:ietf:params:jmap:principals:owner");
let owner: PrincipalsOwnerCapability = serde_json::from_str(
    r#"{"accountIdForPrincipal": "A1", "principalId": "P1"}"#,
)
.unwrap();
assert_eq!(serde_json::to_value(&owner.principal_id).unwrap(), "P1");
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_mail_sharing::server::X` is the same item as
`jmap_sharing_server::X`. `CAPABILITY` is this crate's one own item, a
string constant. The component crates version in lockstep with this one on
the shared `jmap-*` line.

## Gotchas

- The Mailbox side of the draft is not typed yet: `jmap-mail-types` has no
  `shareWith` field on `Mailbox` and no `mayShare` field on
  `MailboxRights`. Both ride in the types' `extra` maps, and
  `jmap-mail-server` does not check `mayShare` before a Mailbox
  `shareWith` change. `CAPABILITY` is
  the only place the workspace names `urn:ietf:params:jmap:mail:share`.
- `server` alone does not build the reference backend; enable `memory` for
  `server::memory::MemoryBackend` (demonstration quality, not production).
- Depending on this crate and on a component crate at a different version
  gives two copies of each type, which do not unify. Keep them on one
  version line.
- Version 0.5.0 replaces the 0.0.1 name-claim placeholder.

## References

- [draft-ietf-jmap-mail-sharing-02] — JMAP for Mail Sharing
- [RFC 9670] — JMAP Sharing
- [RFC 8621] — JMAP for Mail

[draft-ietf-jmap-mail-sharing-02]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-mail-sharing-02
[RFC 9670]: https://www.rfc-editor.org/rfc/rfc9670
[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[`jmap-sharing-types`]: https://crates.io/crates/jmap-sharing-types
[`jmap-sharing-client`]: https://crates.io/crates/jmap-sharing-client
[`jmap-sharing-server`]: https://crates.io/crates/jmap-sharing-server
