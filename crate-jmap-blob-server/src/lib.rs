//! JMAP Blob Management method handlers for the `jmap-server` dispatcher:
//! draft-ietf-jmap-blobext-03 (`urn:ietf:params:jmap:blob2`) and RFC 9404
//! (`urn:ietf:params:jmap:blob`).
//!
//! [`register_blob_handlers`] mounts `Blob/get`, `Blob/set`, `Blob/lookup`
//! and `Blob/convert` over any [`BlobBackend`]. The handlers own argument
//! validation, `#creationId` resolution, range selection, digests and error
//! mapping; the backend owns the octets, the reverse reference index and
//! conversions. A request's `using` picks blob2 or RFC 9404 behaviour
//! ([`BlobMode`]).
//!
//! # Status
//!
//! The public surface is fixed (bd JMAP-tna1.2.4) and `Blob/get` and
//! `Blob/set` are implemented (JMAP-tna1.2.7, .8); the other handlers
//! answer `serverFail` until bd JMAP-tna1.2.9 (lookup) and .10 (convert)
//! land. The `memory` reference backend arrives with
//! JMAP-tna1.2.5, RFC 9404 `Blob/upload` with JMAP-tna1.2.11.

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
mod convert;
mod get;
mod lookup;
/// In-memory reference implementation of [`BlobBackend`].
///
/// Gated behind `feature = "memory"`. Not production.
#[cfg(feature = "memory")]
pub mod memory;
mod mode;
mod order;
mod range;
mod set;
mod start;

pub use backend::{
    BlobBackend, BlobCreate, BlobMeta, BlobSource, ChunkRef, ConvertOutcome, ConvertParams,
    CreatedBlob, DigestQuery, TouchOutcome,
};
pub use convert::handle_blob_convert;
pub use get::handle_blob_get;
pub use lookup::handle_blob_lookup;
pub use mode::BlobMode;
pub use set::handle_blob_set;

/// Register `Blob/get`, `Blob/set`, `Blob/lookup` and `Blob/convert` on
/// `dispatcher`.
///
/// Each handler receives the request's creation ids and `using`. Mount the
/// capabilities in the Session yourself (blobext-03 §2.1): `{}` under
/// `urn:ietf:params:jmap:blob2` in `capabilities`, and
/// [`BlobBackend::blob_capability`] under it in each account's
/// `accountCapabilities`. To serve RFC 9404 clients too, add
/// `urn:ietf:params:jmap:blob` with `{}` and the capability's
/// [`rfc9404`](jmap_blob_types::BlobAccountCapability::rfc9404) view.
pub fn register_blob_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: BlobBackend + 'static,
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
                        created_ids,
                        using,
                        ..
                    } = cx;
                    Box::pin(
                        async move { $handler(&*b, &caller, args, &created_ids, &using).await },
                    ) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }
    reg!("Blob/get", handle_blob_get);
    reg!("Blob/set", handle_blob_set);
    reg!("Blob/lookup", handle_blob_lookup);
    reg!("Blob/convert", handle_blob_convert);
}
