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
pub mod backend;
mod helpers;
/// In-memory reference implementation of [`SceneBackend`].
///
/// Gated behind `feature = "memory"`. Not production.
#[cfg(feature = "memory")]
pub mod memory;
pub mod query;
pub mod region;

pub use asset::{
    handle_scene_asset_changes, handle_scene_asset_get, handle_scene_asset_query,
    handle_scene_asset_query_changes, handle_scene_asset_set, GLTF_BINARY,
};
pub use backend::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, BindingTarget,
    ChangesResult, GetObject, JmapBackend, JmapObject, QueryChangesResult, QueryObject,
    QueryResult, SceneBackend, SetError, SetErrorType, SetObject,
};
pub use region::{
    handle_scene_region_changes, handle_scene_region_get, handle_scene_region_query,
    handle_scene_region_query_changes, handle_scene_region_set,
};

/// Capability URI for `urn:ietf:params:jmap:scene`.
pub use jmap_scene_types::JMAP_SCENE_URI;

/// Generic closure-to-[`JmapHandler`] adapter from [`jmap_server`],
/// re-exported for the registration macro.
pub use jmap_server::ClosureHandler;

/// Register the `SceneAsset/*` and `SceneRegion/*` method handlers with
/// `dispatcher`.
///
/// After this call the dispatcher handles `/get`, `/changes`, `/set`,
/// `/query` and `/queryChanges` of `SceneAsset` and `SceneRegion`. [`Dispatcher::register`] silently
/// replaces an existing handler of the same name, so register overrides
/// afterwards.
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
}
