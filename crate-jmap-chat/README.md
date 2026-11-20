# jmap-chat

JMAP Chat ([draft-atwood-jmap-chat-00]) in one dependency: re-exports
[`jmap-chat-types`] always, [`jmap-chat-client`] with the `client`
feature and [`jmap-chat-server`] with the `server` feature (`memory`
adds the server's in-memory reference backend).

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_chat::types` |
| `client` | `jmap_chat::client` |
| `server` | `jmap_chat::server` |
| `memory` | `server`, plus `jmap_chat::server::memory` |

- `types`: the wire types and capability constants.
- `client`: the sealed `JmapChatExt` extension trait over
  `jmap-base-client`.
- `server`: method handlers for `Chat/*`, `Message/*`, `Space/*` and the other Chat methods over `ChatBackend`.

## What it's for

Chat is direct messages, group chats and Spaces over JMAP. Applications that want the whole family depend on this crate
and pick `client`, `server` or both; workspace crates depend on the
component crates directly. Bridges to other families (for example the
`jmap-chat-*-server` crates) are not re-exported; depend on them directly.

## How to use

```toml
[dependencies]
jmap-chat = { version = "0.6", features = ["client"] }
```

```rust
use jmap_chat::types::JMAP_CHAT_URI;

assert_eq!(JMAP_CHAT_URI, "urn:ietf:params:jmap:chat");
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_chat::server::X` is the same item as `jmap_chat_server::X`.
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

- [draft-atwood-jmap-chat-00] — JMAP Chat
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-chat-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-chat-types`]: https://crates.io/crates/jmap-chat-types
[`jmap-chat-client`]: https://crates.io/crates/jmap-chat-client
[`jmap-chat-server`]: https://crates.io/crates/jmap-chat-server
