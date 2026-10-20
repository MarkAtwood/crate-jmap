# jmap-scene-types

Wire-format types for draft-atwood-jmap-scene-00, the
`urn:ietf:params:jmap:scene` capability that models spatial state (regions,
the objects in them and the users present) over JMAP. Types only: no
method handlers, no I/O, no rendering or simulation.

## What it is

| Item | Spec |
|---|---|
| `JMAP_SCENE_URI` | §Capability |
| `SceneCapability` | §Account-Level Capability Object |
| `Position`, `Orientation`, `Scale`, `IDENTITY_ORIENTATION`, `IDENTITY_SCALE` | §Coordinate System |
| `SceneBounds` | §SceneBounds |
| `SceneAsset` | §SceneAsset |
| `SceneRegion`, `GeoAnchor`, `ViewHint`, `AccessPolicy` | §SceneRegion |
| `SceneObject`, `PhysicsMode` | §SceneObject |
| `SceneAvatar` | §SceneAvatar |
| `query::*` filters and comparators, `WithinRadius`, `WithinBounds` | §…/query, §Spatial Filters |

The session-level capability value is `{}` and has no type.

## What it's for

It gives the planned `jmap-scene-server` and `jmap-scene-client` one
definition of the Scene wire types. It depends on `jmap-types` only; the
Chat, Space and VTCCall ids it carries are plain `Id`s.

## How to use

```rust
use jmap_scene_types::{SceneBounds, IDENTITY_ORIENTATION};

let b: SceneBounds = serde_json::from_str(r#"{
    "min": [-500, -10, -500],
    "max": [500, 200, 500]
}"#)?;
assert_eq!(b.max[1] - b.min[1], 210.0);
assert_eq!(IDENTITY_ORIENTATION, [0.0, 0.0, 0.0, 1.0]);
# Ok::<(), serde_json::Error>(())
```

## How it works

- Coordinates are fixed-length `f64` arrays, so a wrong-length array fails
  to deserialize.
- `T|null` properties are `Option<T>` and serialize `None` as `null`.
  Properties whose absence and `null` mean different things are
  `Option<Option<T>>`: `None` absent, `Some(None)` `null`.
- Properties with a draft default (`spawnPosition`, `spawnOrientation`,
  `accessPolicy`, `scale`, `physicsMode`, `interactable`, `visible`,
  `heading`) take it when absent.
- `#[non_exhaustive]` on every public struct and enum, an `extra` map for
  vendor fields, and `Other(String)` on every wire enum, per workspace
  policy. Filters and comparators reject unknown keys instead.
- No async. `#[forbid(unsafe_code)]` at the crate root.

## Gotchas

- Coordinates are `f64`, so `0` re-serializes as `0.0`: equal as JSON
  numbers, different as text.
- `SceneRegion` binding fields (`chat_id`, `space_id`, `channel_id`,
  `active_call_id`) and `input_uri`, `geo_anchor`, `world_state` are
  `Option<Option<_>>`; the draft's own full region examples omit the last
  three.
- `regionId` on the SceneObject and SceneAvatar filters is an `Option`:
  the server, not the decoder, returns `invalidArguments` when it is
  missing.
- Not enforced on deserialize: unit quaternions, `max >= min` on bounds,
  positive `withinRadius.radius`, `"model/gltf-binary"` in
  `supportedVisualTypes`, and `visualType` tracking `visualRef`.
- `assetUri`, `simulationUri` and `inputUri` are peer-supplied and
  untrusted. Nothing here validates them.

## References

- [draft-atwood-jmap-scene-00] — JMAP Scene
- [RFC 8620] — JMAP Core

[draft-atwood-jmap-scene-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
