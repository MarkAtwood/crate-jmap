//! RFC 9425 JMAP Quotas method handlers for the `jmap-server` dispatcher.
//!
//! Provides `Quota/get`, `Quota/changes`, `Quota/query` and
//! `Quota/queryChanges` (RFC 9425 §4.2 to §4.5) over any [`QuotasBackend`].
//! There is no `Quota/set`: RFC 9425 defines none.
//!
//! On top of the generic RFC 8620 handlers these apply RFC 9425 §4.1: "The
//! server MUST filter out any types for which the client did not request
//! the associated capability in the "using" section of the request.
//! Further, the server MUST NOT return Quota objects for which there are no
//! types recognized by the client." [`QuotasBackend::recognized_types`]
//! says which type names a request's `using` makes recognized.
//!
//! # Example
//!
//! ```
//! use std::sync::Arc;
//! use jmap_quotas_server::{memory::MemoryBackend, register_quotas_handlers};
//! use jmap_server::Dispatcher;
//!
//! let backend = Arc::new(MemoryBackend::new());
//! let mut dispatcher = Dispatcher::new();
//! register_quotas_handlers(&mut dispatcher, backend);
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
// bd:JMAP-uqhn.6 — narrowing and sign-changing `as` casts are banned;
// use `try_from`.
#![warn(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use std::sync::Arc;

use jmap_server::{CallContext, ClosureHandler, Dispatcher, HandlerFuture, JmapHandler};

mod backend;
/// In-memory reference implementation of [`QuotasBackend`].
///
/// Gated behind `feature = "memory"`. Not production.
#[cfg(feature = "memory")]
pub mod memory;
mod quota;

pub use backend::{QuotasBackend, registry_types};
pub use jmap_quotas_types::{JMAP_QUOTA_URI, QuotaProperty};
pub use quota::{
    handle_quota_changes, handle_quota_get, handle_quota_query, handle_quota_query_changes,
};

/// Register the four `Quota/*` methods on `dispatcher`.
///
/// Each handler receives the request's `using` so it can apply RFC 9425
/// §4.1. Mount `urn:ietf:params:jmap:quota` in the Session yourself: its
/// value is `{}` (§2.1), in `capabilities` and in each account's
/// `accountCapabilities`.
pub fn register_quotas_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: QuotasBackend + 'static,
{
    macro_rules! reg {
        ($method:expr, $handler:ident) => {{
            let backend_arc: Arc<B> = Arc::clone(&backend);
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                backend_arc,
                move |b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        args,
                        caller,
                        using,
                        ..
                    } = cx;
                    Box::pin(async move { $handler(&*b, &caller, args, &using).await })
                        as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }
    reg!("Quota/get", handle_quota_get);
    reg!("Quota/changes", handle_quota_changes);
    reg!("Quota/query", handle_quota_query);
    reg!("Quota/queryChanges", handle_quota_query_changes);
}
