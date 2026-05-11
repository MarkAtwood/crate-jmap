//! JMAP Object Metadata extension method handlers
//! ([draft-ietf-jmap-metadata-01](https://www.ietf.org/archive/id/draft-ietf-jmap-metadata-01.txt)).
//!
//! # Usage
//!
//! Implement [`MetadataBackend`] for your storage layer, then call
//! [`register_metadata_handlers`] to wire all method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_metadata_server::{MetadataBackend, register_metadata_handlers};
//! # use jmap_server::{Dispatcher, JmapBackend};
//! # fn example<B: MetadataBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_metadata_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose the `memory::MemoryBackend`
//! reference implementation of [`MetadataBackend`]. This is the same
//! backend used by this crate's own integration tests, intended for
//! downstream contributors to study and for smoke tests / examples
//! that do not want to stand up a real database. **Not production.**
//! API stability is opt-in via this feature and may break across minor
//! versions while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
mod helpers;
/// In-memory reference implementation of [`MetadataBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;
pub mod metadata;

pub use backend::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, MetadataBackend, QueryChangesResult, QueryObject, QueryResult, SetError,
    SetErrorType, SetObject,
};
pub use metadata::{
    handle_metadata_changes, handle_metadata_get, handle_metadata_query,
    handle_metadata_query_changes, handle_metadata_set,
};

/// Capability URI for `urn:ietf:params:jmap:metadata` (draft-ietf-jmap-metadata-01 §1.2.1).
pub use jmap_metadata_types::JMAP_METADATA_URI;

// ---------------------------------------------------------------------------
// register_metadata_handlers — main entry point for consumers
// ---------------------------------------------------------------------------

/// Register all JMAP Metadata method handlers with `dispatcher`.
///
/// `backend` is wrapped in [`Arc`] so it is cloned cheaply into each handler
/// closure. Pass the same `Arc<B>` to both this function and any
/// application-level code that needs the backend directly.
///
/// After this call the dispatcher handles:
/// `Metadata/get`, `Metadata/changes`, `Metadata/set`,
/// `Metadata/query`, `Metadata/queryChanges`.
///
/// The dispatcher's `CallerCtx` is taken from `B::CallerCtx`; every registered
/// closure forwards it as `&ctx` into the wrapped `handle_*` function. Backends
/// that use `type CallerCtx = ()` therefore see `&()` inside every handler.
pub fn register_metadata_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: MetadataBackend + 'static,
{
    // Helper: register one method with a closure that takes
    // (Arc<B>, call_id, args, ctx). `$ctx` is the per-request caller context
    // (`B::CallerCtx`) forwarded by the dispatcher; closures pass `&ctx` to the
    // inner `handle_*` fn.
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident, $ctx:ident| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler {
                backend: backend_arc,
                call_fn: Box::new(
                    move |$b: Arc<B>, $ci: String, $a: serde_json::Value, $ctx: B::CallerCtx| {
                        Box::pin(async move { $body }) as HandlerFuture
                    },
                ),
            });
            dispatcher.register($method, h);
        }};
    }

    reg!("Metadata/get", backend, |b, _ci, a, ctx| {
        handle_metadata_get(&*b, &ctx, a).await
    });
    reg!("Metadata/changes", backend, |b, _ci, a, ctx| {
        handle_metadata_changes(&*b, &ctx, a).await
    });
    reg!("Metadata/set", backend, |b, _ci, a, ctx| {
        handle_metadata_set(&*b, &ctx, a).await
    });
    reg!("Metadata/query", backend, |b, _ci, a, ctx| {
        handle_metadata_query(&*b, &ctx, a).await
    });
    reg!("Metadata/queryChanges", backend, |b, _ci, a, ctx| {
        handle_metadata_query_changes(&*b, &ctx, a).await
    });
}

pub use jmap_server::ClosureHandler;
