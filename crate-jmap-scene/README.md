# jmap-scene

JMAP Scene ([draft-atwood-jmap-scene-00]) in one dependency: re-exports
[`jmap-scene-types`] always, [`jmap-scene-client`] with the `client` feature
and [`jmap-scene-server`] with the `server` feature (`memory` adds the
server's in-memory reference backend).

## What it is

A facade with no code of its own:

| Feature | Re-export |
|---|---|
| (none) | `jmap_scene::types` |
| `client` | `jmap_scene::client` |
| `server` | `jmap_scene::server` |
| `memory` | `server`, plus `jmap_scene::server::memory` |

- `types`: the wire types and capability constants.
- `client`: the sealed `JmapSceneExt` extension trait over
  `jmap-base-client`, `AssetFetcher`, and Scene WebSocket frames.
- `server`: Scene method handlers over `SceneBackend`,
  `register_scene_handlers`.

## What it's for

Scene models shared spatial spaces as JMAP objects: regions, objects, assets
and avatars. Applications that want the whole family depend on this crate
and pick `client`, `server` or both; workspace crates depend on the
component crates directly. Bridge and companion crates are not re-exported:
`jmap-chat-scene-server` and `jmap-scene-vtc-server` (the bridges); depend
on them directly.

## How to use

```toml
[dependencies]
jmap-scene = { version = "0.6", features = ["client"] }
```

```rust
use jmap_scene::types::{JMAP_SCENE_URI, JMAP_SCENE_WEBSOCKET_URI};

assert_eq!(JMAP_SCENE_URI, "urn:ietf:params:jmap:scene");
assert_eq!(JMAP_SCENE_WEBSOCKET_URI, "urn:ietf:params:jmap:scene:websocket");
```

## How it works

Each module is a `pub use` of a component crate, gated by its feature, so
`jmap_scene::server::X` is the same item as `jmap_scene_server::X`. The
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

- [draft-atwood-jmap-scene-00] — JMAP Scene
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-scene-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-scene-types`]: https://crates.io/crates/jmap-scene-types
[`jmap-scene-client`]: https://crates.io/crates/jmap-scene-client
[`jmap-scene-server`]: https://crates.io/crates/jmap-scene-server
