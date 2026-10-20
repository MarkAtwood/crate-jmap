//! JMAP VTC method handlers (draft-atwood-jmap-vtc-00).
//!
//! # Usage
//!
//! Implement [`VtcBackend`] for your storage layer, then call
//! [`register_vtc_handlers`] to wire the method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_vtc_server::{VtcBackend, register_vtc_handlers};
//! # use jmap_server::Dispatcher;
//! # fn example<B: VtcBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_vtc_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # Scope
//!
//! The `VTCCall/*` methods: create with the capability gates, the state
//! machine, ending a call, the call-level moderator actions and `/query`.
//! The `VTCRecording/*` and `VTCLivestream/*` methods: moderator-only
//! `/set` with each type's state machine, the `VTCRecordingStateEvent`
//! consent signal, and `streamKey` redaction in `VTCLivestream/get`.
//! The crate depends on RFC 8620 only; the Chat binding (membership,
//! `start_call`, `Chat.activeCallId`) lives in a separate binding crate.
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose `memory::MemoryBackend`, the
//! in-memory reference implementation of [`VtcBackend`] used by this
//! crate's tests. **Not production.** Its API may break in any minor
//! release while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
pub mod call;
mod helpers;
mod media;
/// In-memory reference implementation of [`VtcBackend`].
///
/// Gated behind `feature = "memory"`. Not production.
#[cfg(feature = "memory")]
pub mod memory;
pub mod query;

pub use backend::{
    call_already_active_on_chat, AddedItem, BackendChangesError, BackendQueryError,
    BackendSetError, ChangesResult, GetObject, JmapBackend, JmapObject, QueryChangesResult,
    QueryObject, QueryResult, SetError, SetErrorType, SetObject, VtcBackend,
};
pub use call::{
    handle_vtc_call_changes, handle_vtc_call_get, handle_vtc_call_query,
    handle_vtc_call_query_changes, handle_vtc_call_set,
};
pub use media::{
    handle_vtc_livestream_changes, handle_vtc_livestream_get, handle_vtc_livestream_query,
    handle_vtc_livestream_query_changes, handle_vtc_livestream_set, handle_vtc_recording_changes,
    handle_vtc_recording_get, handle_vtc_recording_query, handle_vtc_recording_query_changes,
    handle_vtc_recording_set,
};

/// Capability URI for `urn:ietf:params:jmap:vtc`.
pub use jmap_vtc_types::JMAP_VTC_URI;

/// Generic closure-to-[`JmapHandler`] adapter from [`jmap_server`],
/// re-exported for the registration macro.
pub use jmap_server::ClosureHandler;

/// Register the `VTCCall/*`, `VTCRecording/*` and `VTCLivestream/*` method
/// handlers with `dispatcher`.
///
/// After this call the dispatcher handles `/get`, `/changes`, `/set`,
/// `/query` and `/queryChanges` for each of the three types.
/// [`Dispatcher::register`] silently replaces an existing handler of the
/// same name, so register overrides afterwards.
///
/// This only wires dispatch. The consumer advertises
/// [`JMAP_VTC_URI`] in its Session capabilities, with the same
/// account-level object [`VtcBackend::vtc_capability`] returns.
pub fn register_vtc_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: VtcBackend + 'static,
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

    reg!("VTCCall/get", handle_vtc_call_get);
    reg!("VTCCall/changes", handle_vtc_call_changes);
    reg!("VTCCall/set", handle_vtc_call_set);
    reg!("VTCCall/query", handle_vtc_call_query);
    reg!("VTCCall/queryChanges", handle_vtc_call_query_changes);
    reg!("VTCRecording/get", handle_vtc_recording_get);
    reg!("VTCRecording/changes", handle_vtc_recording_changes);
    reg!("VTCRecording/set", handle_vtc_recording_set);
    reg!("VTCRecording/query", handle_vtc_recording_query);
    reg!(
        "VTCRecording/queryChanges",
        handle_vtc_recording_query_changes
    );
    reg!("VTCLivestream/get", handle_vtc_livestream_get);
    reg!("VTCLivestream/changes", handle_vtc_livestream_changes);
    reg!("VTCLivestream/set", handle_vtc_livestream_set);
    reg!("VTCLivestream/query", handle_vtc_livestream_query);
    reg!(
        "VTCLivestream/queryChanges",
        handle_vtc_livestream_query_changes
    );
}
