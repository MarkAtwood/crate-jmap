//! JMAP Chat MIMI provider server rules (draft-atwood-jmap-chat-mimi-00).
//!
//! # Usage
//!
//! Implement [`MimiBackend`] for your storage layer, then call
//! [`register_mimi_handlers`] to wire the method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_chat_mimi_server::{MimiBackend, register_mimi_handlers};
//! # use jmap_server::Dispatcher;
//! # fn example<B: MimiBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_mimi_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # Scope
//!
//! `KeyPackage/get` and `KeyPackage/set` (draft §KeyPackages): the
//! per-KeyPackage create checks in [`check_key_package_create`], parsed and
//! verified with openmls, and the account-wide rules the backend applies
//! atomically in [`MimiBackend::create_key_package`].
//!
//! KeyPackage hand-out (draft §Handing Out KeyPackages):
//! [`MimiBackend::claim_key_packages`] claims KeyPackages atomically,
//! judged by [`KeyMaterialRequirements::fit`]. `KeyMaterial/fetch`,
//! `Room/*` and `GroupInfo/fetch` are not here yet.
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose `memory::MemoryBackend`, the
//! in-memory reference implementation of [`MimiBackend`] used by this
//! crate's tests. **Not production.** Its API may break in any minor
//! release while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
mod handout;
mod keypackage;
/// In-memory reference implementation of [`MimiBackend`].
///
/// Gated behind `feature = "memory"`. Not production.
#[cfg(feature = "memory")]
pub mod memory;

pub use backend::{
    key_package_over_quota, key_package_ref_in_use, signature_key_bound_elsewhere, MimiBackend,
    NewKeyPackage,
};
pub use handout::{KeyMaterialRequirements, KeyPackageFit};
pub use keypackage::{check_key_package_create, handle_key_package_get, handle_key_package_set};

/// Capability URI for `urn:ietf:params:jmap:chat:mimi`.
pub use jmap_chat_mimi_types::JMAP_CHAT_MIMI_URI;

/// Generic closure-to-[`JmapHandler`] adapter from [`jmap_server`],
/// re-exported for the registration macro.
pub use jmap_server::ClosureHandler;

/// Register the `KeyPackage/get` and `KeyPackage/set` handlers on
/// `dispatcher`.
///
/// Advertise [`JMAP_CHAT_MIMI_URI`] in your Session capabilities, with the
/// same account-level object [`MimiBackend::mimi_capability`] returns.
pub fn register_mimi_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: MimiBackend + 'static,
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

    reg!("KeyPackage/get", handle_key_package_get);
    reg!("KeyPackage/set", handle_key_package_set);
}
