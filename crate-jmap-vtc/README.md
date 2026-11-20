# jmap-vtc

JMAP VTC ([draft-atwood-jmap-vtc-00]) in one dependency: re-exports
[`jmap-vtc-types`] always, [`jmap-vtc-client`] with the `client` feature and
[`jmap-vtc-server`] with the `server` feature (`memory` adds the server's
in-memory reference backend).

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_vtc::types` |
| `client` | `jmap_vtc::client` |
| `server` | `jmap_vtc::server` |
| `memory` | `server`, plus `jmap_vtc::server::memory` |

- `types`: the wire types and capability constants.
- `client`: the sealed `JmapVtcExt` extension trait over `jmap-base-client`,
  and VTC WebSocket frames.
- `server`: `VTCCall`, `VTCRecording` and `VTCLivestream` handlers over
  `VtcBackend`, `register_vtc_handlers`.

## What it's for

VTC (video teleconferencing) models calls, recordings and livestreams as
JMAP objects: `VTCCall`, `VTCRecording`, `VTCLivestream`. Applications that
want the whole family depend on this crate and pick `client`, `server` or
both; workspace crates depend on the component crates directly. Bridge and
companion crates are not re-exported: `jmap-chat-vtc-server` and
`jmap-scene-vtc-server` (the bridges); depend on them directly.

## How to use

```toml
[dependencies]
jmap-vtc = { version = "0.6", features = ["client"] }
```

```rust
use jmap_vtc::types::{JMAP_VTC_URI, JMAP_VTC_WEBSOCKET_URI};

assert_eq!(JMAP_VTC_URI, "urn:ietf:params:jmap:vtc");
assert_eq!(JMAP_VTC_WEBSOCKET_URI, "urn:ietf:params:jmap:vtc:websocket");
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_vtc::server::X` is the same item as `jmap_vtc_server::X`. The
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

- [draft-atwood-jmap-vtc-00] — JMAP VTC
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-vtc-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-vtc-types`]: https://crates.io/crates/jmap-vtc-types
[`jmap-vtc-client`]: https://crates.io/crates/jmap-vtc-client
[`jmap-vtc-server`]: https://crates.io/crates/jmap-vtc-server
