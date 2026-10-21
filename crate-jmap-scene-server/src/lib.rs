//! JMAP Scene method handlers (draft-atwood-jmap-scene-00).
//!
//! # Usage
//!
//! Implement [`SceneBackend`] for your storage layer, then call
//! [`register_scene_handlers`] to wire the method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_scene_server::{SceneBackend, register_scene_handlers};
//! # use jmap_server::Dispatcher;
//! # fn example<B: SceneBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_scene_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # Scope
//!
//! The `SceneAsset/*` methods (draft §SceneAsset Methods): standard
//! `/get`, `/changes` and `/queryChanges`; `/set` with the create, update
//! and destroy SetError conditions, glTF (`model/gltf-binary`) as the
//! mandatory-to-implement format and the `maxAssetSizeBytes` quota; and
//! `/query` with the draft's default sort.
//!
//! The `SceneRegion/*` methods (draft §SceneRegion Methods): `/set` with
//! the create, update and destroy SetError conditions, `mayCreateRegion`,
//! `maxRegionsPerAccount`, the binding checks through
//! [`SceneBackend::region_binding`] and the destroy cascade; and `/query`
//! with the draft's default sort and `accessPolicy` filter check.
//!
//! The `SceneObject/*` methods (draft §SceneObject Methods): `/set` with
//! the create, update and destroy SetError conditions, `mayCreateObject`,
//! `maxObjectsPerRegion`, a region-scoped acyclic scene graph at most 16
//! levels deep, [`SceneBackend::may_modify_object`] and the recursive
//! destroy; and `/query` with the draft's default sort, the §Filter
//! Validation checks and the `withinRadius` and `withinBounds` spatial
//! filters over region-local positions.
//!
//! `SceneObject/interact` (draft §SceneObject/interact): each interaction
//! is checked for its properties (`invalidProperties`), region access,
//! existence and the visibility set (`notFound`), an active avatar and
//! `interactable` (`forbidden`), then passed to
//! [`SceneBackend::accept_interaction`], which may refuse it with
//! `rateLimit` and otherwise delivers the `SceneInteractionEvent`.
//!
//! The `SceneAvatar/*` methods (draft §SceneAvatar Methods): `/set` create
//! enters a region (one active avatar per user and region, `alreadyExists`
//! with `existingId` for a duplicate, reconnect to a departed record,
//! auto-eject from another region, `maxAvatarsPerRegion` and
//! [`SceneBackend::may_enter_region`]), an update that sets `leftAt` leaves
//! it or, through [`SceneBackend::may_eject_avatar`], ejects another user,
//! destroy is `forbidden`, a `displayName` from
//! [`SceneBackend::reserved_display_names`] is `forbidden`, and the
//! backend keeps `activeAvatarCount`; and
//! `/query` with the draft's default sort and §Filter Validation.
//!
//! Visibility (draft §Visibility Contract, §Region Access, §Blocked-Contact
//! Spatial Presence): `/get` of SceneRegion, SceneObject and SceneAvatar
//! reports a record in a region the caller has no access to
//! ([`SceneBackend::may_access_region`]), an object outside the caller's
//! visibility set ([`SceneBackend::is_object_visible`]) and an avatar of a
//! user the caller blocked ([`SceneBackend::is_user_blocked`]) in
//! `notFound`, like a missing id; `SceneObject/set` refuses an update or
//! destroy of a hidden object with `notFound`. The backend omits the same
//! records from `/query` and `/queryChanges`. `activeAvatarCount` is the
//! true count, blocked users included, as the draft says deployments
//! "SHOULD prefer".
//!
//! Push (draft §Push Notifications): [`SCENE_DATA_TYPES`], the four types
//! that participate in RFC 8620 state changes, and [`push_urgency`], the
//! §Push Urgency recommendation for a `StateChange`, including the
//! §Interaction with Chat Push rule that an avatar enter or leave already
//! announced by a bound Chat's push is not sent at `normal` or higher.
//!
//! Ephemeral subscriptions (draft-atwood-jmap-scene-wss-00
//! §SceneStreamEnable): the [`ephemeral`] module holds the per-connection
//! subscription a `SceneStreamEnable` frame replaces in its entirety, with
//! a dynamic ([`RegionScope::All`]) or static ([`RegionScope::Only`])
//! region scope and the `invalidArguments` check on `eventTypes`;
//! [`should_deliver`], the per-event delivery check (active avatar,
//! visibility, blocked-sender suppression); and [`validate_interaction`],
//! the check on a client-sent `SceneInteractionEvent` before relay.
//!
//! The crate depends on RFC 8620 only; Chat and VTC bindings live in
//! separate binding crates, which implement
//! [`SceneBackend::has_capability`] and [`SceneBackend::region_binding`].
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose `memory::MemoryBackend`, the
//! in-memory reference implementation of [`SceneBackend`] used by this
//! crate's tests. **Not production.** Its API may break in any minor
//! release while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod asset;
pub mod avatar;
pub mod backend;
pub mod ephemeral;
mod helpers;
pub mod interact;
/// In-memory reference implementation of [`SceneBackend`].
///
/// Gated behind `feature = "memory"`. Not production.
#[cfg(feature = "memory")]
pub mod memory;
pub mod object;
pub mod push;
pub mod query;
pub mod region;

pub use asset::{
    handle_scene_asset_changes, handle_scene_asset_get, handle_scene_asset_query,
    handle_scene_asset_query_changes, handle_scene_asset_set, GLTF_BINARY,
};
pub use avatar::{
    handle_scene_avatar_changes, handle_scene_avatar_get, handle_scene_avatar_query,
    handle_scene_avatar_query_changes, handle_scene_avatar_set,
};
pub use backend::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, BindingTarget,
    ChangesResult, GetObject, JmapBackend, JmapObject, QueryChangesResult, QueryObject,
    QueryResult, SceneBackend, SetError, SetErrorType, SetObject,
};
pub use ephemeral::{
    should_deliver, validate_interaction, RegionScope, SceneEphemeralSubscription,
    SceneStreamFilter,
};
pub use interact::handle_scene_object_interact;
pub use object::{
    handle_scene_object_changes, handle_scene_object_get, handle_scene_object_query,
    handle_scene_object_query_changes, handle_scene_object_set,
};
pub use push::{push_urgency, SceneEvent, Urgency, SCENE_DATA_TYPES};
pub use region::{
    handle_scene_region_changes, handle_scene_region_get, handle_scene_region_query,
    handle_scene_region_query_changes, handle_scene_region_set,
};

/// Capability URI for `urn:ietf:params:jmap:scene`.
pub use jmap_scene_types::JMAP_SCENE_URI;

/// Generic closure-to-[`JmapHandler`] adapter from [`jmap_server`],
/// re-exported for the registration macro.
pub use jmap_server::ClosureHandler;

/// Register the `SceneAsset/*`, `SceneRegion/*`, `SceneObject/*` and
/// `SceneAvatar/*` method handlers with `dispatcher`.
///
/// After this call the dispatcher handles `/get`, `/changes`, `/set`,
/// `/query` and `/queryChanges` of `SceneAsset`, `SceneRegion`,
/// `SceneObject` and `SceneAvatar`, and `SceneObject/interact`.
/// [`Dispatcher::register`] silently replaces an existing handler of the
/// same name, so register overrides afterwards.
///
/// This only wires dispatch. The consumer advertises [`JMAP_SCENE_URI`] in
/// its Session capabilities, with the same account-level object
/// [`SceneBackend::scene_capability`] returns.
pub fn register_scene_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: SceneBackend + 'static,
{
    macro_rules! reg {
        ($method:expr, $handler:ident) => {{
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                Arc::clone(&backend),
                move |b: Arc<B>, _ci: String, a: serde_json::Value, ctx: B::CallerCtx| {
                    Box::pin(async move { $handler(&*b, &ctx, a).await }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }

    reg!("SceneAsset/get", handle_scene_asset_get);
    reg!("SceneAsset/changes", handle_scene_asset_changes);
    reg!("SceneAsset/set", handle_scene_asset_set);
    reg!("SceneAsset/query", handle_scene_asset_query);
    reg!("SceneAsset/queryChanges", handle_scene_asset_query_changes);
    reg!("SceneRegion/get", handle_scene_region_get);
    reg!("SceneRegion/changes", handle_scene_region_changes);
    reg!("SceneRegion/set", handle_scene_region_set);
    reg!("SceneRegion/query", handle_scene_region_query);
    reg!(
        "SceneRegion/queryChanges",
        handle_scene_region_query_changes
    );
    reg!("SceneObject/get", handle_scene_object_get);
    reg!("SceneObject/changes", handle_scene_object_changes);
    reg!("SceneObject/set", handle_scene_object_set);
    reg!("SceneObject/query", handle_scene_object_query);
    reg!(
        "SceneObject/queryChanges",
        handle_scene_object_query_changes
    );
    reg!("SceneObject/interact", handle_scene_object_interact);
    reg!("SceneAvatar/get", handle_scene_avatar_get);
    reg!("SceneAvatar/changes", handle_scene_avatar_changes);
    reg!("SceneAvatar/set", handle_scene_avatar_set);
    reg!("SceneAvatar/query", handle_scene_avatar_query);
    reg!(
        "SceneAvatar/queryChanges",
        handle_scene_avatar_query_changes
    );
}
