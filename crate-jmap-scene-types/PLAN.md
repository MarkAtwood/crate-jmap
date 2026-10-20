# jmap-scene-types — Plan

Typed surface of draft-atwood-jmap-scene-00 (epic bd:JMAP-38uq). Types
only: the handlers live in the planned `jmap-scene-server`, the method
bindings in `jmap-scene-client`.

## Scope

| Bead | Content | State |
|---|---|---|
| JMAP-38uq.1 | Capability, coordinate formats, SceneBounds, GeoAnchor, the four data types, the `/query` filters incl. `withinRadius`/`withinBounds`, comparators | landed |
| JMAP-38uq.3 | `JmapObject` impls for SceneRegion, SceneObject and SceneAvatar | landed |

## Decisions

- The session-level capability value `{}` is not modelled, as in
  `jmap-vtc-types`.
- Position, orientation and scale are type aliases for `[f64; 3]` and
  `[f64; 4]`: serde already enforces the length, and a newtype would add
  no check. Unit length and `max >= min` are server checks.
- `T|null` properties are `Option<T>` and serialize `None` as `null`.
- Properties that may be absent *or* `null` are `Option<Option<T>>`
  (absent `None`, `null` `Some(None)`): the SceneRegion binding fields
  (present only with the companion capability), `inputUri`, `geoAnchor`
  and `worldState` on SceneRegion (the draft's full region examples omit
  them while L447 sends `worldState: null`), and `parentId` in the
  SceneObject filter (`null` selects root objects).
- Properties with a draft default take it when absent.
- `withinRadius` and `withinBounds` are their own `deny_unknown_fields`
  structs; `withinBounds` does not reuse `SceneBounds`, whose `extra` would
  silently accept unknown filter keys.
- `regionId` in the SceneObject and SceneAvatar filters is an `Option` so
  the server returns `invalidArguments`, not a decode failure.
- userId-valued properties (`userId`, `ownerId`) are `String`, as in
  `jmap-vtc-types`. Blob references (`blobId`, `visualRef`) are `Id`.
- `sha256` is a plain `String`; no dependency on `jmap-cid-types`.
- `JmapObject` impls are added by the server beads (JMAP-38uq.2-.5).
