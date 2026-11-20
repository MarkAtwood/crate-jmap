//! JMAP Blob Management method handlers for the `jmap-server` dispatcher:
//! draft-ietf-jmap-blobext-03 (`urn:ietf:params:jmap:blob2`) and RFC 9404
//! (`urn:ietf:params:jmap:blob`).
//!
//! [`register_blob_handlers`] mounts `Blob/get`, `Blob/set`, `Blob/lookup`,
//! `Blob/convert`, RFC 9404 `Blob/upload` and RFC 8620 §6.3 `Blob/copy`
//! over any [`BlobBackend`]. The handlers own argument
//! validation, `#creationId` resolution, range selection, digests and error
//! mapping; the backend owns the octets, the reverse reference index and
//! conversions. A request's `using` picks blob2 or RFC 9404 behaviour
//! ([`BlobMode`]).
//!
//! # Status
//!
//! Every handler is implemented: the four blobext ones (bd JMAP-tna1.2.7
//! to .10), RFC 9404 `Blob/upload` (JMAP-tna1.2.11), conditional
//! `Blob/set` (JMAP-tna1.2.12) and `Blob/copy` (JMAP-tna1.2.14).

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

use jmap_blob_types::BlobObject;
use jmap_server::{
    CallContext, ClosureHandler, ConditionalBackend, Dispatcher, HandlerFuture, JmapHandler,
    RegistrationError, SetSupport,
};

mod backend;
mod convert;
mod copy;
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
mod upload;

pub use backend::{
    BlobBackend, BlobCreate, BlobMeta, BlobSource, ChunkRef, ConvertOutcome, ConvertParams,
    CreatedBlob, DigestQuery, TouchOutcome,
};
pub use convert::handle_blob_convert;
pub use copy::handle_blob_copy;
pub use get::handle_blob_get;
pub use lookup::handle_blob_lookup;
pub use mode::BlobMode;
pub use set::handle_blob_set;
pub use upload::handle_blob_upload;

/// Register `Blob/get`, `Blob/set`, `Blob/lookup`, `Blob/convert`,
/// `Blob/upload` and `Blob/copy` on `dispatcher`.
///
/// Each handler receives the request's creation ids, and all but the core
/// `Blob/copy` the request's `using`. Mount the
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
    reg!("Blob/upload", handle_blob_upload);
    let backend_arc: Arc<B> = Arc::clone(&backend);
    let copy: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
        backend_arc,
        move |b: Arc<B>, cx: CallContext<B::CallerCtx>| {
            let CallContext {
                args,
                caller,
                created_ids,
                ..
            } = cx;
            Box::pin(async move { handle_blob_copy(&*b, &caller, args, &created_ids).await })
                as HandlerFuture
        },
    ));
    dispatcher.register("Blob/copy", copy);
}

/// Opt `Blob/set` in to draft-ietf-jmap-conditional-02 (`ifUnchangedBy`
/// and `atomic`), after [`register_blob_handlers`] (docs/design/DECISIONS.md
/// decisions 1 and 17).
///
/// Attaches [`SetSupport`] for [`BlobObject`] to the registered `Blob/set`;
/// it does not re-register the handler. The dispatcher then reads
/// preconditions through the registered `Blob/get` and runs the call in
/// the scope `backend` opens ([`ConditionalBackend::begin_set`]), which
/// must discard created blobs on an atomic rollback. Advertise
/// `urn:ietf:params:jmap:conditional` only once
/// [`Dispatcher::conditional_ready`] is true for the whole dispatcher.
///
/// Preconditions on `size`, `digest:<alg>`, `{}` and `null` work; Blob/get
/// never returns `type` or `expires`, so a pointer to either is
/// `invalidArguments` (decision 19).
///
/// # Errors
///
/// [`RegistrationError::NotRegistered`] when `Blob/set` is not registered
/// yet.
pub fn register_blob_conditional<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) -> Result<(), RegistrationError>
where
    B: BlobBackend + ConditionalBackend + 'static,
{
    dispatcher.attach_set_support("Blob/set", SetSupport::new::<BlobObject, B>(backend))
}
