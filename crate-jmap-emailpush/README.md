# jmap-emailpush

JMAP Email Delivery Push ([draft-ietf-jmap-emailpush-05]) in one
dependency: enables the `emailpush` feature of [`jmap-mail-types`],
[`jmap-mail-client`] and [`jmap-mail-server`] and re-exports exactly the
items that feature adds.

## What it is

A facade with no code of its own. EmailPush has no crate family: it is the
`emailpush` feature of the three mail crates (docs/design/DECISIONS.md
decision 3). This crate turns that feature on and re-exports its items
under the extension's name.

| Feature | Re-export |
|---|---|
| (none) | `jmap_emailpush::types` (from `jmap-mail-types`: `JMAP_EMAILPUSH_URI`, `EmailPushCapability`, `EmailPushAccountCapability`, `EmailPush`, `EmailPushConfig`, `EMAIL_PUSH_PROPERTY`, `email_push`, `set_email_push`), `jmap_emailpush::CAPABILITY` (kept from 0.0.1) |
| `client` | `jmap_emailpush::client` (from `jmap-mail-client`: `EmailPushSubscriptionInput`, `PushPayload`, `parse_push_payload`); enables `jmap-mail-client/emailpush` |
| `server` | `jmap_emailpush::server` (= `jmap_mail_server::emailpush`: `EmailPushBackend`, `EmailPushAccount`, validation and delivery helpers); enables `jmap-mail-server/emailpush` |

## What it's for

EmailPush lets a client ask, on its RFC 8620 PushSubscription, for the new
messages themselves (selected properties of each delivered Email) instead
of a bare StateChange. A server implements `EmailPushBackend` on its
`PushSubscriptionBackend`; a client creates the subscription and decodes
the pushes. Applications that want only those items, under the
extension's name, depend on this crate; workspace crates enable the
feature on the mail crates directly.

## How to use

```toml
[dependencies]
jmap-emailpush = { version = "0.5", features = ["client"] }
```

```rust
use jmap_emailpush::types::{EmailPushConfig, JMAP_EMAILPUSH_URI};

assert_eq!(JMAP_EMAILPUSH_URI, "urn:ietf:params:jmap:emailpush");
assert_eq!(jmap_emailpush::CAPABILITY, JMAP_EMAILPUSH_URI);
let config: EmailPushConfig = serde_json::from_str(
    r#"{"properties": ["id", "subject"], "urgency": "high"}"#,
)
.unwrap();
assert!(config.filter.is_none());
```

## How it works

Each item is a `pub use` of the mail crate's item, and `server` is a
`pub use` of the `jmap_mail_server::emailpush` module, so
`jmap_emailpush::server::X` is the same item as
`jmap_mail_server::emailpush::X`. Depending on this crate turns on the
`emailpush` feature of the mail crates for the whole build, as cargo
unifies features. The mail crates version in lockstep with this one on the
shared `jmap-*` line.

## Gotchas

- EmailPush adds no methods and no `register_*` function: it rides on
  `PushSubscription/get` and `/set`, registered with
  `jmap_server::register_push_subscription_handlers`.
- The client calls `email_push_subscription_create` and `set_email_push`
  are inherent methods of `jmap_mail_client::SessionClient`; with the
  `client` feature on they are there, but this crate does not re-export
  `SessionClient` itself.
- There is no `memory` feature. The mail memory backend's EmailPush
  support is `jmap-mail-server`'s `memory` plus `emailpush` features; use
  `jmap_mail_server::memory` directly.
- Depending on this crate and on a mail crate at a different version gives
  two copies of each type, which do not unify.
- Version 0.5.0 replaces the 0.0.1 name-claim placeholder.

## References

- [draft-ietf-jmap-emailpush-05] — JMAP Email Delivery Push
- [RFC 8620] — JMAP Core (§7.2 PushSubscription)
- [RFC 8621] — JMAP for Mail

[draft-ietf-jmap-emailpush-05]: https://datatracker.ietf.org/doc/html/draft-ietf-jmap-emailpush-05
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[`jmap-mail-types`]: https://crates.io/crates/jmap-mail-types
[`jmap-mail-client`]: https://crates.io/crates/jmap-mail-client
[`jmap-mail-server`]: https://crates.io/crates/jmap-mail-server
