# jmap-scene-server

> **Draft, not published.** Tracks a draft spec that is still changing
> (jmap-chat-spec 63e9d08 revised it); `publish = false` until the crate
> matches it (JMAP-0x0q, JMAP-38uq.14).

Method handlers for the `SceneAsset`, `SceneRegion`, `SceneObject` and
`SceneAvatar` data types of the
JMAP Scene extension (draft-atwood-jmap-scene-00), backend-agnostic, for a
`jmap-server` `Dispatcher`. You supply storage through the `SceneBackend`
trait.

## What it is

| Item | Spec |
|---|---|
| `handle_scene_asset_get`, `handle_scene_asset_changes` | §SceneAsset/get, §SceneAsset/changes (RFC 8620 §5.1, §5.2) |
| `handle_scene_asset_set` | §SceneAsset/set and its §SetError Conditions |
| `handle_scene_asset_query`, `handle_scene_asset_query_changes` | §SceneAsset/query, §SceneAsset/queryChanges |
| `GLTF_BINARY` | §Preferred Asset Format: `model/gltf-binary` is mandatory to implement |
| `handle_scene_region_get`, `handle_scene_region_changes` | §SceneRegion/get, §SceneRegion/changes (RFC 8620 §5.1, §5.2) |
| `handle_scene_region_set` | §SceneRegion/set: create, update, destroy and §SetError Conditions |
| `handle_scene_region_query`, `handle_scene_region_query_changes` | §SceneRegion/query, §SceneRegion/queryChanges |
| `handle_scene_object_get`, `handle_scene_object_changes` | §SceneObject/get, §SceneObject/changes (RFC 8620 §5.1, §5.2) |
| `handle_scene_object_set` | §SceneObject/set: create, update, recursive destroy and §SetError Conditions |
| `handle_scene_object_query`, `handle_scene_object_query_changes` | §SceneObject/query, §Spatial Filters, §SceneObject/queryChanges |
| `handle_scene_object_interact` | §SceneObject/interact: `notFound`, `forbidden`, `rateLimit` and `invalidProperties`; accepted interactions go to `SceneBackend::accept_interaction` |
| `handle_scene_avatar_get`, `handle_scene_avatar_changes` | §SceneAvatar/get, §SceneAvatar/changes (RFC 8620 §5.1, §5.2) |
| `handle_scene_avatar_set` | §SceneAvatar/set: enter, update, leave, re-enter, eject and §SetError Conditions |
| `handle_scene_avatar_query`, `handle_scene_avatar_query_changes` | §SceneAvatar/query, §SceneAvatar/queryChanges |
| `query::asset_matches`, `query::compare_assets` | §SceneAsset/query filter and sort |
| `query::region_matches`, `query::compare_regions` | §SceneRegion/query filter and sort |
| `query::object_matches`, `query::compare_objects`, `query::region_local_position` | §SceneObject/query filter, spatial filters and sort |
| `query::avatar_matches`, `query::compare_avatars` | §SceneAvatar/query filter, spatial filters and sort |
| `SceneBackend` | storage, capability objects, blob sizes, permission and binding hooks |
| `BindingTarget` | what a `chatId`/`spaceId`/`channelId`/`activeCallId` references |
| `memory::MemoryBackend` (`memory` feature) | in-memory reference backend |
| `register_scene_handlers` | wires the `SceneAsset/*`, `SceneRegion/*`, `SceneObject/*` (including `SceneObject/interact`) and `SceneAvatar/*` methods |

## What it's for

A JMAP server that offers spatial scenes wires these handlers next to its
other extensions. The crate depends on `jmap-types`, `jmap-server` and
`jmap-scene-types` only. Chat and VTC lookups for the region bindings
belong to binding crates, which implement `SceneBackend::has_capability`
and `SceneBackend::region_binding`.

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
`created` entry holds the five server-set properties, with `ownerId` from
`SceneBackend::caller_user_id`. An update may patch only `name`,
`assetUri`, `sha256` and extension properties. Update and destroy return
`notFound` for an unknown id, and `forbidden` when
`SceneBackend::may_modify_asset` is `false`. `/query` defaults to
`createdAt` descending. The backend trait is not object-safe (generic
methods); every public type is `#[non_exhaustive]`.

A SceneRegion create needs `mayCreateRegion`, a non-empty `name` and
`bounds` with `min <= max` on every axis; `accessPolicy` must be
`public`, `invite` or `space`, and `spawnOrientation` a unit quaternion
(within 1e-6). `space` needs `urn:ietf:params:jmap:chat` and a `spaceId`.
A non-null `chatId`, `spaceId` or `channelId` needs the Chat capability
and a `BindingTarget::Usable` answer from `region_binding`; otherwise it
is `invalidProperties` naming the property. `activeCallId` is not a
create property. The backend applies `maxRegionsPerAccount` atomically
with the insert. An update is validated as the region the patch leaves.
A non-null `activeCallId` needs `urn:ietf:params:jmap:vtc`: a missing or
ended call is `invalidProperties`, an inaccessible one `forbidden`. The
`updated` entry echoes the new `updatedAt`. Update needs
`may_update_region` and destroy `may_destroy_region` (owner or admin in
`MemoryBackend`). On destroy the backend removes the region's SceneObjects
and SceneAvatars. `/query` defaults to `createdAt` descending, and an
`accessPolicy` filter value outside the three is `invalidArguments`.

A SceneObject create needs `mayCreateObject`, a `regionId` naming an
existing region the caller can access (else `invalidProperties` naming
`regionId`, the same for both cases) and a finite `position`. `scale` must
be finite, `orientation` within 1e-3 of unit length (stored normalized;
an absent one is the identity), `physicsMode` one of `static`, `dynamic`,
`kinematic` or `none`, and `visualRef` and `visualType` come together.
`visualType` must be listed in `supportedVisualTypes` (or be
`model/gltf-binary`) and `visualRef` must name an uploaded blob (else
`invalidProperties` naming `visualRef`, on create and update). A `parentId` must name an existing object (else
`invalidProperties` naming `parentId`, on create and update) in the same
region, must not make the object its own ancestor, and must
keep every object within 16 levels; an update that re-parents counts the
moved subtree. The backend sets `ownerId` to the caller and applies
`maxObjectsPerRegion` atomically. `regionId` is immutable. Update and
destroy need `may_modify_object` (object owner, region owner or admin);
destroy removes the object's descendants too, and only the requested id
appears in `destroyed`. `/query` defaults to `createdAt` ascending and
returns `invalidArguments` without `regionId` in every condition, for an
unknown `physicsMode`, a `withinRadius.radius` <= 0 or a `withinBounds`
`max` below `min`. The spatial filters compare region-local positions:
a child's parent-relative `position` is mapped through its ancestors'
transforms, and the boundary is inclusive.

A SceneAvatar create enters a region. It needs a `regionId` naming an
existing region the caller has access to under `may_access_region`
(else `invalidProperties` naming `regionId`, the same error either way)
and `may_enter_region`, which bars banned users (else `forbidden`), and must not carry `id`, `joinedAt`, `leftAt` or another
user's `userId` (`forbidden`). The handler sets `userId` from
`caller_user_id`, `displayName` from `avatar_display_name` when absent,
and `position` and `orientation` from the region's spawn values when
absent. A supplied `position` must be finite, an `orientation` within
1e-3 of unit length (stored normalized), and `visualRef` and
`visualType` come together. In one atomic step `create_object` refuses
the create with `alreadyExists` and `existingId` when the user already
has an avatar in the region, present or departed, applies
`maxAvatarsPerRegion` to active avatars, sets `leftAt` on the user's
active avatar in any other region, and keeps every affected region's
`activeAvatarCount`. The `created` entry is the stored avatar. An update
of the caller's own avatar may change anything but `id`, `regionId`,
`userId` and `joinedAt`; setting `leftAt` leaves the region and a second
leave is `invalidProperties` naming `leftAt`. Setting `leftAt` to `null`
on a departed avatar re-enters the region (§Re-entering a Region): lost
region access is `notFound`, `may_enter_region` `false` is `forbidden`,
and a `position` or `orientation` in the same patch is
`invalidProperties`. The handler sets `joinedAt` to now and `position`
and `orientation` to the region's spawn point, and `update_object`
applies `maxAvatarsPerRegion` (`overQuota`), the auto-eject from other
regions and the `activeAvatarCount` update in the same atomic step. On
another user's avatar only an eject (`leftAt`, optionally `worldState`)
is allowed, and only when `may_eject_avatar` agrees; anything else is
`forbidden`. Destroy is always `forbidden`. `/query` defaults to
`joinedAt` ascending and returns `invalidArguments` without `regionId` in
every condition, for a `withinRadius.radius` <= 0 or a `withinBounds`
`max` below `min`.

## Gotchas

- `SceneBackend::may_modify_asset` defaults to `Ok(true)` (fail-open,
  bd:JMAP-bghk). Override it, or every caller can rename and delete
  every asset.
- `may_update_region`, `may_destroy_region` and `region_binding` also
  fail open: any caller may change any region, and every binding id is
  accepted, until you override them.
- `may_modify_object` fails open too: any caller may move or delete any
  SceneObject until you override it.
- `may_enter_region` and `may_eject_avatar` fail open too: no one is
  banned from a region and anyone may eject anyone, until you override
  them. Entry to an `invite` or `space` region is decided by
  `may_access_region`.
- `caller_user_id` has no default. Avatar ownership is a spec rule, not
  a policy hook.
- Avatar ids are backend-assigned, not the userId the draft suggests
  ("SHOULD use the userId as the id within a given region"): one user
  can have avatar records in several regions of an account, and JMAP ids
  are unique per type per account.
- Re-parenting an object reads every SceneObject of the account to find
  the moved subtree's height. A backend with a `parentId` index can
  register a faster `SceneObject/set`.
- `has_capability` has no default. Return `true` only for the companion
  capabilities you advertise on that account.
- Destroying a region does not end its `activeCallId` VTCCall, and this
  crate sends no VTCCall StateChange. That belongs to the Scene/VTC
  binding crate.
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
  creator owns an asset. `grant_region_access` admits a user to an
  `invite` region. Its `/queryChanges` always returns
  `cannotCalculateChanges`.

## References

- [draft-atwood-jmap-scene-00] — JMAP Scene
- [RFC 8620] — JMAP Core
- [RFC 6838] — Media Type Specifications and Registration Procedures

[draft-atwood-jmap-scene-00]: https://github.com/MarkAtwood/jmap-chat-spec
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 6838]: https://www.rfc-editor.org/rfc/rfc6838
