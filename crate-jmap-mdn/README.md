# jmap-mdn

JMAP Message Disposition Notifications ([RFC 9007]) in one dependency:
enables the `mdn` feature of [`jmap-mail-types`] and [`jmap-mail-server`]
and re-exports exactly the items that feature adds.

## What it is

A facade with no code of its own. MDN has no crate family: it is the `mdn`
feature of the mail crates. This crate turns that feature on and
re-exports its items under the extension's name.

| Feature | Re-export |
|---|---|
| (none) | `jmap_mdn::types` (from `jmap-mail-types`: `JMAP_MDN_URI`, `Mdn`, `Disposition`, `DispositionType`, `ActionMode`, `SendingMode`, `MdnSendRequest`, `MdnSendResponse`, `MdnParseRequest`, `MdnParseResponse`), `jmap_mdn::CAPABILITY` (kept from 0.0.1) |
| `server` | `jmap_mdn::server` (from `jmap-mail-server`: `MdnBackend`, `MdnSendResult`, `MdnParseResult`, `handle_mdn_send`, `handle_mdn_parse`, `MDN_PARSE_MAX_BLOB_IDS`, `register_mdn_handlers`); enables `jmap-mail-server/mdn` |

There is no `client` feature: `jmap-mail-client` has no MDN bindings.

## What it's for

RFC 9007 adds `MDN/send`, to send a read receipt for a received message,
and `MDN/parse`, to read the receipts that come back. A server implements
`MdnBackend` on its `MailBackend` and calls `register_mdn_handlers`.
Applications that want only those items, under the extension's name,
depend on this crate; workspace crates enable the feature on the mail
crates directly.

## How to use

```toml
[dependencies]
jmap-mdn = { version = "0.5", features = ["server"] }
```

```rust
use jmap_mdn::types::{Disposition, DispositionType, JMAP_MDN_URI};

assert_eq!(JMAP_MDN_URI, "urn:ietf:params:jmap:mdn");
assert_eq!(jmap_mdn::CAPABILITY, JMAP_MDN_URI);
let d: Disposition = serde_json::from_str(
    r#"{"actionMode": "manual-action", "sendingMode": "mdn-sent-manually",
        "type": "displayed"}"#,
)
.unwrap();
assert_eq!(d.type_, DispositionType::Displayed);
```

## How it works

Each item is a `pub use` of the mail crate's item (`jmap_mail_server::mdn`
for the backend and handlers, the crate root for
`register_mdn_handlers`), so `jmap_mdn::server::X` is the same item as the
mail crate's. Depending on this crate turns on the `mdn` feature of the
mail crates for the whole build, as cargo unifies features. The mail
crates version in lockstep with this one on the shared `jmap-*` line.

## Gotchas

- `MDN/send` also needs `urn:ietf:params:jmap:mail` in `using`
  (RFC 9007 §2.1); list both URIs as known to `check_known_capabilities`.
- There is no `memory` feature. The mail memory backend implements
  `MdnBackend` with `jmap-mail-server`'s `memory` plus `mdn` features; use
  `jmap_mail_server::memory` directly.
- Depending on this crate and on a mail crate at a different version gives
  two copies of each type, which do not unify.
- Version 0.5.0 replaces the 0.0.1 name-claim placeholder.

## References

- [RFC 9007] — Handling Message Disposition Notification with JMAP
- [RFC 8098] — Message Disposition Notification
- [RFC 8621] — JMAP for Mail

[RFC 9007]: https://www.rfc-editor.org/rfc/rfc9007
[RFC 8098]: https://www.rfc-editor.org/rfc/rfc8098
[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[`jmap-mail-types`]: https://crates.io/crates/jmap-mail-types
[`jmap-mail-server`]: https://crates.io/crates/jmap-mail-server
