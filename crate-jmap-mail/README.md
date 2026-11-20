# jmap-mail

JMAP Mail ([RFC 8621]) in one dependency: re-exports
[`jmap-mail-types`] always, [`jmap-mail-client`] with the `client`
feature and [`jmap-mail-server`] with the `server` feature (`memory`
adds the server's in-memory reference backend).

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_mail::types` |
| `client` | `jmap_mail::client` |
| `server` | `jmap_mail::server` |
| `memory` | `server`, plus `jmap_mail::server::memory` |
| `emailpush`, `mdn`, `sieve` | the same features of the component crates that are enabled |

- `types`: the wire types and capability constants.
- `client`: the sealed `JmapMailExt` extension trait over
  `jmap-base-client`.
- `server`: method handlers for `Mailbox/*`, `Email/*`, `Thread/*`, `EmailSubmission/*` and the other Mail methods over `MailBackend`.

## What it's for

Mail is email: mailboxes, threads, messages and submission over JMAP. Applications that want the whole family depend on this crate
and pick `client`, `server` or both; workspace crates depend on the
component crates directly. Bridges to other families (for example the
`jmap-chat-*-server` crates) are not re-exported; depend on them directly.

## How to use

```toml
[dependencies]
jmap-mail = { version = "0.5", features = ["client"] }
```

```rust
use jmap_mail::types::JMAP_MAIL_URI;

assert_eq!(JMAP_MAIL_URI, "urn:ietf:params:jmap:mail");
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_mail::server::X` is the same item as `jmap_mail_server::X`.
The component crates version in lockstep with this one on the shared
`jmap-*` line.

## Gotchas

- `server` alone does not build the reference backend; enable `memory` for
  `server::memory::MemoryBackend` (demonstration quality, not production).
- Depending on this crate and on a component crate at a different version
  gives two copies of each type, which do not unify. Keep them on one
  version line.
- Version 0.5.0 replaces the 0.0.1 name-claim placeholder.

## References

- [RFC 8621] — JMAP Mail
- [RFC 8620] — JMAP Core

[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-mail-types`]: https://crates.io/crates/jmap-mail-types
[`jmap-mail-client`]: https://crates.io/crates/jmap-mail-client
[`jmap-mail-server`]: https://crates.io/crates/jmap-mail-server
