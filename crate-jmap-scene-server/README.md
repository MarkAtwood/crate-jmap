# jmap-scene-server

Method handlers for the `SceneAsset` data type of the JMAP Scene
extension (draft-atwood-jmap-scene-00), backend-agnostic, for a
`jmap-server` `Dispatcher`. You supply storage through the `SceneBackend`
trait.

## What it is

| Item | Spec |
|---|---|
| `handle_scene_asset_get`, `handle_scene_asset_changes` | §SceneAsset/get, §SceneAsset/changes (RFC 8620 §5.1, §5.2) |
| `handle_scene_asset_set` | §SceneAsset/set and its §SetError Conditions |
| `handle_scene_asset_query`, `handle_scene_asset_query_changes` | §SceneAsset/query, §SceneAsset/queryChanges |
| `GLTF_BINARY` | §Preferred Asset Format: `model/gltf-binary` is mandatory to implement |
| `query::asset_matches`, `query::compare_assets` | §SceneAsset/query filter and sort |
| `SceneBackend` | storage, capability object, blob sizes, owner-or-admin hook |
| `memory::MemoryBackend` (`memory` feature) | in-memory reference backend |
| `register_scene_handlers` | wires the five `SceneAsset/*` methods |

## What it's for

A JMAP server that offers spatial scenes wires these handlers next to its
other extensions. The crate depends on `jmap-types`, `jmap-server` and
`jmap-scene-types` only. SceneRegion, SceneObject and SceneAvatar methods
are not here yet.

## How to use

```rust,ignore
use std::sync::Arc;
use jmap_server::Dispatcher;
use jmap_scene_server::{memory::MemoryBackend, register_scene_handlers};

let backend = Arc::new(MemoryBackend::new());
let mut dispatcher: Dispatcher<String> = Dispatcher::new();
register_scene_handlers(&mut dispatcher, backend);
```

Advertise `urn:ietf:params:jmap:scene` in your Session with the same
account-level object that `SceneBackend::scene_capability` returns, and
list `model/gltf-binary` in its `supportedVisualTypes`.

## How it works

A create must carry `blobId` and `mediaType` and must not carry the
server-set `id`, `accountId`, `size` or `createdAt`. `name`, `assetUri`
and `sha256` must be strings or `null`, and `sha256` must be lowercase
hex. `mediaType` must be `model/gltf-binary`, a type listed in
`supportedVisualTypes`, or any `audio/*` or `image/*` type. Any failure
of these rules is one `invalidProperties` naming every offending
property. The handler then asks `SceneBackend::blob_size`: a missing blob
is `notFound` and a blob larger than `maxAssetSizeBytes` is `overQuota`.
`SceneBackend::create_object` assigns the ULID and applies the
deployment's account storage quota and admin-only rule atomically. The
`created` entry holds the four server-set properties. An update may not
patch the server-set or immutable properties. Update and destroy return
`notFound` for an unknown id, and `forbidden` when
`SceneBackend::may_modify_asset` is `false`. `/query` defaults to
`createdAt` descending. The backend trait is not object-safe (generic
methods); every public type is `#[non_exhaustive]`.

## Gotchas

- `SceneBackend::may_modify_asset` defaults to `Ok(true)` (fail-open,
  bd:JMAP-bghk). Override it, or every caller can rename and delete
  every asset.
- `blob_size` has no default. Return `Err` for a storage failure, not
  `Ok(None)`, or the client sees `notFound` and does not retry.
- `model/gltf-binary` is accepted even if your capability object omits
  it. The draft requires you to list it.
- "Recognized audio/image type" means any `audio/*` or `image/*` media
  type; there is no registry lookup.
- The `name` filter is a case-insensitive substring match, as in the
  draft's example. The `name` sort ignores `collation` and puts assets
  without a name last.
- The `MemoryBackend` caller context is the caller's userId `String`; the
  creator owns an asset. Its `/queryChanges` always returns
  `cannotCalculateChanges`.

## References

- [draft-atwood-jmap-scene-00] — JMAP Scene
- [RFC 8620] — JMAP Core
- [RFC 6838] — Media Type Specifications and Registration Procedures

[draft-atwood-jmap-scene-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 6838]: https://www.rfc-editor.org/rfc/rfc6838
