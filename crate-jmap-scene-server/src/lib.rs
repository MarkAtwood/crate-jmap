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
//! `SceneRegion/broadcast` (draft §SceneRegion/broadcast, jmap-chat-spec
//! 93f2258), on accounts whose capability has `supportsRegionBroadcast`
//! (otherwise `unknownMethod`): each region event is checked for its
//! properties (`invalidProperties`), region access (`notFound`) and the
//! owner-or-admin rule ([`SceneBackend::may_update_region`], `forbidden`),
//! then passed to [`SceneBackend::accept_region_event`], which may refuse
//! it with `rateLimit` and otherwise delivers the `SceneRegionEvent`.
//!
//! The `SceneAvatar/*` methods (draft §SceneAvatar Methods): `/set` create
//! enters a region (one avatar record per user and region, `alreadyExists`
//! with `existingId` when the user has a present or departed record there,
//! auto-eject from another region, `maxAvatarsPerRegion` and
//! [`SceneBackend::may_enter_region`]), an update that sets `leftAt` leaves
//! it or, through [`SceneBackend::may_eject_avatar`], ejects another user,
//! an update that sets `leftAt` to `null` re-enters it at the spawn point,
//! destroy is `forbidden`, a `displayName` from
//! [`SceneBackend::reserved_display_names`] is `forbidden`, and the
//! backend keeps `activeAvatarCount`; and
//! `/query` with the draft's default sort and §Filter Validation.
//!
//! Visibility (draft §Visibility Contract, §Region Access, §Blocked-Contact
//! Spatial Presence): `/get` of SceneRegion, SceneObject and SceneAvatar
//! reports a record in a region the caller has no access to
//! ([`SceneBackend::may_access_region`]), an object outside the caller's
//! visibility set ([`SceneBackend::is_object_visible`]), an avatar of a
//! user the caller blocked ([`SceneBackend::is_user_blocked`]) and an
//! avatar in a region bound to an E2EE call the caller is not in
//! ([`SceneBackend::may_see_region_avatars`]) in `notFound`, like a
//! missing id; `SceneObject/set` refuses an update or
//! destroy of a hidden object with `notFound`. The backend omits the same
//! records from `/query` and `/queryChanges`. `activeAvatarCount` is the
//! true count, blocked users included, as the draft says deployments
//! "SHOULD prefer".
//!
//! Avatar Visibility (draft §Avatar Visibility, jmap-chat-spec 93f2258):
//! [`SceneBackend::avatar_view`] hides another user's avatar from the
//! caller ([`AvatarView::Hidden`]: `notFound` from `SceneAvatar/get` and
//! from a `SceneAvatar/set` update, no `SceneAvatarEvent`) or withholds
//! its `position`, `orientation` and `worldState` / `regionState` keys
//! ([`AvatarRedaction`]). The default shows everything. The backend applies
//! the same view in `/query` and, when it hides to protect information, in
//! `SceneAvatar/changes`.
//!
//! Push (draft §Push Notifications): [`SCENE_DATA_TYPES`], the four types
//! that participate in RFC 8620 state changes, and [`push_urgency`], the
//! §Push Urgency suggestion for a `StateChange`, including the
//! §Interaction with Chat Push rule that an avatar enter or leave already
//! announced by a bound Chat's push is not sent at `normal` or higher.
//!
//! Ephemeral subscriptions (draft-atwood-jmap-scene-wss-00
//! §SceneStreamEnable): [`SceneEphemeralSubscription`] holds the per-connection
//! subscription a `SceneStreamEnable` frame replaces in its entirety, with
//! a dynamic ([`RegionScope::All`]) or static ([`RegionScope::Only`])
//! region scope and the `invalidArguments` check on `eventTypes`;
//! [`should_deliver`], the per-event delivery check (active avatar,
//! visibility, E2EE-call avatar suppression, blocked-sender suppression);
//! and [`validate_interaction`],
//! the check on a client-sent `SceneInteractionEvent` before relay.
//!
//! The crate depends on RFC 8620 only; Chat and VTC bindings live in
//! separate binding crates, which implement
//! [`SceneBackend::has_capability`] and [`SceneBackend::region_binding`].
//!
//! # `websocket` feature (ephemeral events over RFC 8887)
//!
//! Enable `websocket` for the `websocket` module: the `jmap-websocket`
//! extension handler for `SceneStreamEnable` / `SceneStreamDisable` and
//! client-sent `SceneInteractionEvent` (draft-atwood-jmap-scene-wss-00) and
//! the server-wide hub the deployment publishes events to. Off by default, so
//! a deployment without the WebSocket transport does not pull in
//! `jmap-websocket`.
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose `memory::MemoryBackend`, the
//! in-memory reference implementation of [`SceneBackend`] used by this
//! crate's tests. **Not production.** Its API may break in any minor
//! release while the crate is pre-1.0.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{
    CallContext, ClosureHandler, ConditionalBackend, Dispatcher, HandlerFuture, JmapHandler,
    RegistrationError, SetSupport,
};

mod asset;
mod avatar;
mod backend;
mod broadcast;
mod ephemeral;
mod helpers;
mod history;
mod interact;
/// In-memory reference implementation of [`SceneBackend`].
///
/// Gated behind `feature = "memory"`. Not production.
#[cfg(feature = "memory")]
pub mod memory;
mod object;
mod push;
pub mod query;
mod region;
#[cfg(feature = "websocket")]
pub mod websocket;

use asset::scene_asset_set;
pub use asset::{
    GLTF_BINARY, handle_scene_asset_changes, handle_scene_asset_get, handle_scene_asset_query,
    handle_scene_asset_query_changes, handle_scene_asset_set,
};
pub use avatar::{
    handle_scene_avatar_changes, handle_scene_avatar_get, handle_scene_avatar_query,
    handle_scene_avatar_query_changes, handle_scene_avatar_set,
};
pub use backend::{AvatarRedaction, AvatarView, BindingProperty, BindingTarget, SceneBackend};
pub use broadcast::handle_scene_region_broadcast;
pub use ephemeral::{
    RegionScope, SceneEphemeralSubscription, SceneStreamFilter, should_deliver,
    validate_interaction,
};
pub use history::register_scene_handlers_with_history;
pub use interact::handle_scene_object_interact;
pub use object::{
    handle_scene_object_changes, handle_scene_object_get, handle_scene_object_query,
    handle_scene_object_query_changes, handle_scene_object_set,
};
pub use push::{SCENE_DATA_TYPES, SceneEvent, Urgency, push_urgency};
pub use region::{
    handle_scene_region_changes, handle_scene_region_get, handle_scene_region_query,
    handle_scene_region_query_changes, handle_scene_region_set,
};

/// Capability URI for `urn:ietf:params:jmap:scene`.
pub use jmap_scene_types::JMAP_SCENE_URI;

/// Register the `SceneAsset/*`, `SceneRegion/*`, `SceneObject/*` and
/// `SceneAvatar/*` method handlers with `dispatcher`.
///
/// After this call the dispatcher handles `/get`, `/changes`, `/set`,
/// `/query` and `/queryChanges` of `SceneAsset`, `SceneRegion`,
/// `SceneObject` and `SceneAvatar`, `SceneObject/interact` and
/// `SceneRegion/broadcast` (which answers `unknownMethod` on an account
/// without `supportsRegionBroadcast`).
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
    // `ids` marks a `/set` handler that resolves `#creationId` references
    // against `CallContext::created_ids` (RFC 8620 §5.3).
    macro_rules! reg {
        ($method:expr, $handler:ident) => {{
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                Arc::clone(&backend),
                move |b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        args: a,
                        caller: ctx,
                        ..
                    } = cx;
                    Box::pin(async move { $handler(&*b, &ctx, a).await }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
        ($method:expr, $handler:ident, ids) => {{
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                Arc::clone(&backend),
                move |b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        args: a,
                        caller: ctx,
                        created_ids: ids,
                        ..
                    } = cx;
                    Box::pin(async move { $handler(&*b, &ctx, &ids, a).await }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }

    reg!("SceneAsset/get", handle_scene_asset_get);
    reg!("SceneAsset/changes", handle_scene_asset_changes);
    reg!("SceneAsset/set", scene_asset_set, ids);
    reg!("SceneAsset/query", handle_scene_asset_query);
    reg!("SceneAsset/queryChanges", handle_scene_asset_query_changes);
    reg!("SceneRegion/get", handle_scene_region_get);
    reg!("SceneRegion/changes", handle_scene_region_changes);
    reg!("SceneRegion/set", handle_scene_region_set, ids);
    reg!("SceneRegion/query", handle_scene_region_query);
    reg!(
        "SceneRegion/queryChanges",
        handle_scene_region_query_changes
    );
    reg!("SceneObject/get", handle_scene_object_get);
    reg!("SceneObject/changes", handle_scene_object_changes);
    reg!("SceneObject/set", handle_scene_object_set, ids);
    reg!("SceneObject/query", handle_scene_object_query);
    reg!(
        "SceneObject/queryChanges",
        handle_scene_object_query_changes
    );
    reg!("SceneObject/interact", handle_scene_object_interact);
    reg!("SceneRegion/broadcast", handle_scene_region_broadcast);
    reg!("SceneAvatar/get", handle_scene_avatar_get);
    reg!("SceneAvatar/changes", handle_scene_avatar_changes);
    reg!("SceneAvatar/set", handle_scene_avatar_set, ids);
    reg!("SceneAvatar/query", handle_scene_avatar_query);
    reg!(
        "SceneAvatar/queryChanges",
        handle_scene_avatar_query_changes
    );
}

/// Opt this crate's registered Foo/set methods in to
/// draft-ietf-jmap-conditional-02 (`ifUnchangedBy` and `atomic`;
/// docs/design/DECISIONS.md decisions 1 and 17).
///
/// Call after [`register_scene_handlers`]. Attaches [`SetSupport`] to each
/// of `SceneAsset/set`, `SceneRegion/set`, `SceneObject/set`, `SceneAvatar/set` that is registered, and skips the rest; no handler is
/// re-registered.
///
/// The dispatcher reads preconditions through each type's `/get` and runs
/// each call in the scope `backend` opens
/// ([`ConditionalBackend::begin_set`]).
/// Under `atomic` that scope must check `maxAvatarsPerRegion` on the final
/// state; the memory backend does.
/// Advertise `urn:ietf:params:jmap:conditional` only once
/// [`Dispatcher::conditional_ready`] is true for the whole dispatcher.
pub fn register_scene_conditional<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: SceneBackend + ConditionalBackend + 'static,
{
    fn attach<C: Clone + Send + 'static>(
        dispatcher: &mut Dispatcher<C>,
        method: &str,
        support: SetSupport<C>,
    ) {
        match dispatcher.attach_set_support(method, support) {
            Ok(()) | Err(RegistrationError::NotRegistered(_)) => {}
            Err(e) => unreachable!("{method} pairs with its own data type: {e}"),
        }
    }
    macro_rules! attach {
        ($($t:ident),*) => {$(
            attach(
                dispatcher,
                concat!(stringify!($t), "/set"),
                SetSupport::new::<jmap_scene_types::$t, B>(Arc::clone(&backend)),
            );
        )*};
    }
    attach!(SceneAsset, SceneRegion, SceneObject, SceneAvatar);
}
