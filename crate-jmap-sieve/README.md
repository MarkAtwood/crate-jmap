# jmap-sieve

JMAP for Sieve Scripts ([RFC 9661]) in one dependency: enables the `sieve`
feature of [`jmap-mail-types`] and [`jmap-mail-server`] and re-exports
exactly the items that feature adds.

## What it is

A facade with no code of its own. Sieve has no crate family: it is the
`sieve` feature of the mail crates. This crate turns that feature on and
re-exports its items under the extension's name.

| Feature | Re-export |
|---|---|
| (none) | `jmap_sieve::types` (from `jmap-mail-types`: `JMAP_SIEVE_SCRIPTS_URI`, `SieveCapability`, `SieveAccountCapability`, `SieveScript`, `SieveScriptProperty`), `jmap_sieve::CAPABILITY` (kept from 0.0.1) |
| `server` | `jmap_sieve::server` (from `jmap-mail-server`: `SieveBackend`, `handle_sieve_get`, `handle_sieve_set`, `handle_sieve_query`, `handle_sieve_validate`, their `_with_history` variants, `register_sieve_handlers`, `register_sieve_handlers_with_history`); enables `jmap-mail-server/sieve` |

There is no `client` feature: `jmap-mail-client` has no Sieve bindings.

## What it's for

RFC 9661 manages a user's Sieve filtering scripts over JMAP:
`SieveScript/get`, `/set`, `/query` and `/validate`, with the script
content in a blob. A server implements `SieveBackend` on its
`MailBackend` and calls `register_sieve_handlers`. Applications that want
only those items, under the extension's name, depend on this crate;
workspace crates enable the feature on the mail crates directly.

## How to use

```toml
[dependencies]
jmap-sieve = { version = "0.6", features = ["server"] }
```

```rust
use jmap_sieve::types::{JMAP_SIEVE_SCRIPTS_URI, SieveCapability};

assert_eq!(JMAP_SIEVE_SCRIPTS_URI, "urn:ietf:params:jmap:sieve");
assert_eq!(jmap_sieve::CAPABILITY, JMAP_SIEVE_SCRIPTS_URI);
let cap: SieveCapability =
    serde_json::from_str(r#"{"implementation": "ACME Email Filtering"}"#).unwrap();
assert_eq!(cap.implementation, "ACME Email Filtering");
```

## How it works

Each item is a `pub use` of the same item at the mail crate's root, so
`jmap_sieve::server::X` is the same item as `jmap_mail_server::X`.
Depending on this crate turns on the `sieve` feature of the mail crates
for the whole build, as cargo unifies features. The mail crates version in
lockstep with this one on the shared `jmap-*` line.

## Gotchas

- Conditional `SieveScript/set` (draft-ietf-jmap-conditional-02) is
  attached by `jmap_mail_server::register_mail_conditional`, which covers
  every mail `/set` method and is not re-exported here.
- There is no `memory` feature. The mail memory backend implements
  `SieveBackend` with `jmap-mail-server`'s `memory` plus `sieve` features;
  use `jmap_mail_server::memory` directly.
- The capability constant is `JMAP_SIEVE_SCRIPTS_URI`, its name in
  `jmap-mail-types`; its value is `urn:ietf:params:jmap:sieve`.
- Depending on this crate and on a mail crate at a different version gives
  two copies of each type, which do not unify.
- Version 0.6.0 replaces the 0.0.1 name-claim placeholder.

## References

- [RFC 9661] — JMAP for Sieve Scripts
- [RFC 5228] — Sieve: An Email Filtering Language
- [RFC 8621] — JMAP for Mail

[RFC 9661]: https://www.rfc-editor.org/rfc/rfc9661
[RFC 5228]: https://www.rfc-editor.org/rfc/rfc5228
[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[`jmap-mail-types`]: https://crates.io/crates/jmap-mail-types
[`jmap-mail-server`]: https://crates.io/crates/jmap-mail-server
