//! RFC 8621 JMAP for Mail method handlers.
//!
//! # Usage
//!
//! Implement [`MailBackend`] for your storage layer, then call
//! [`register_mail_handlers`] to wire all 26 RFC 8621 method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_mail_server::{MailBackend, register_mail_handlers};
//! # use jmap_server::Dispatcher;
//! # fn example<B: MailBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_mail_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! The example fixes `CallerCtx = ()` so a `Dispatcher<()>` can register the
//! returned handlers. Backends that thread a real auth identity through
//! `CallerCtx` (e.g. `type CallerCtx = Identity`) use the matching
//! `Dispatcher<Identity>` and omit the `<CallerCtx = ()>` bound.
//!
//! # `mdn` feature
//!
//! Enable the `mdn` feature to add support for the JMAP MDN extension
//! (RFC 9007). This exposes `register_mdn_handlers`, which wires
//! two additional method names into the dispatcher: `MDN/send` and `MDN/parse`.
//! Backends must also implement `MdnBackend` in addition to [`MailBackend`].
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose the `memory::MemoryBackend`
//! reference implementation of [`MailBackend`] (and, when `mdn`/`sieve`
//! are also enabled, of `MdnBackend`/`SieveBackend`). This is the same
//! backend used by this crate's own integration tests, intended for
//! downstream contributors to study and for smoke tests / examples
//! that do not want to stand up a real database. **Not production.**
//! API stability is opt-in via this feature and may break across minor
//! versions while the crate is pre-1.0.

#![forbid(unsafe_code)]

/// Capability URI for JMAP Mail (RFC 8621 §1.3).
pub const JMAP_MAIL_URI: &str = "urn:ietf:params:jmap:mail";

/// Capability URI for JMAP Mail Submission (RFC 8621 §1.3).
pub const JMAP_SUBMISSION_URI: &str = "urn:ietf:params:jmap:submission";

/// Capability URI for JMAP Vacation Response (RFC 8621 §1.3).
pub const JMAP_VACATION_RESPONSE_URI: &str = "urn:ietf:params:jmap:vacationresponse";

use std::sync::Arc;

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
pub mod email;
#[cfg(feature = "mdn")]
pub mod mdn;
/// In-memory reference implementation of [`MailBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;
#[cfg(feature = "sieve")]
pub mod sieve;
#[cfg(feature = "mdn")]
pub use jmap_mail_types::mdn::JMAP_MDN_URI;
#[cfg(feature = "sieve")]
pub use jmap_mail_types::sieve::JMAP_SIEVE_SCRIPTS_URI;
#[cfg(feature = "mdn")]
pub use mdn::MdnBackend;
#[cfg(feature = "mdn")]
pub use mdn::{handle_mdn_parse, handle_mdn_send};
#[cfg(feature = "sieve")]
pub use sieve::SieveBackend;
#[cfg(feature = "sieve")]
pub use sieve::{handle_sieve_get, handle_sieve_query, handle_sieve_set, handle_sieve_validate};
mod helpers;
pub mod identity;
pub mod mailbox;
pub mod snippet;
pub mod submission;
pub mod thread;
pub mod vacation;

pub use backend::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, EmailProperty,
    EmailSubmissionProperty, GetObject, IdentityProperty, JmapBackend, JmapObject, MailBackend,
    MailboxProperty, QueryChangesResult, QueryObject, QueryResult, SearchSnippetProperty, SetError,
    SetErrorType, SetObject, ThreadProperty, VacationResponseProperty,
};
pub use email::{
    handle_email_changes, handle_email_copy, handle_email_get, handle_email_import,
    handle_email_parse, handle_email_query, handle_email_query_changes, handle_email_set,
};
pub use identity::{handle_identity_changes, handle_identity_get, handle_identity_set};
pub use mailbox::{
    handle_mailbox_changes, handle_mailbox_get, handle_mailbox_query, handle_mailbox_query_changes,
    handle_mailbox_set,
};
pub use snippet::handle_search_snippet_get;
pub use submission::{
    handle_submission_changes, handle_submission_get, handle_submission_query,
    handle_submission_query_changes, handle_submission_set,
};
pub use thread::{handle_thread_changes, handle_thread_get};
pub use vacation::{handle_vacation_get, handle_vacation_set};

// ---------------------------------------------------------------------------
// register_mail_handlers — the main entry point for consumers
// ---------------------------------------------------------------------------

/// Register all 26 RFC 8621 JMAP Mail method handlers with `dispatcher`.
///
/// `backend` is wrapped in [`Arc`] so it is cloned cheaply into each handler.
/// Pass the same `Arc<B>` to both this function and any application-level code
/// that needs to access the backend.
///
/// After this call, the dispatcher handles:
/// `Mailbox/*`, `Thread/*`, `Email/*`, `SearchSnippet/get`,
/// `Identity/*`, `EmailSubmission/*`, and `VacationResponse/*`.
///
/// The dispatcher's `CallerCtx` is taken from `B::CallerCtx`; every registered
/// closure forwards it as `&ctx` into the wrapped `handle_*` function. Backends
/// that use `type CallerCtx = ()` therefore see `&()` inside every handler.
pub fn register_mail_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: MailBackend + 'static,
{
    // Helper: register one method with a closure that takes
    // (Arc<B>, call_id, args, ctx).
    //
    // `$ci` is the call_id string (echoed back to the client). Most handlers
    // ignore it and use `_ci` as the identifier. Only handlers that generate
    // onSuccess* side-effect invocations (Email/copy, EmailSubmission/set) need
    // `ci` — they pass it to the extra-invocations builder so the side-effect
    // method call carries the same client-assigned call_id as the original.
    //
    // `$ctx` is the per-request caller context (`B::CallerCtx`) forwarded
    // by the dispatcher. Closures pass `&ctx` to the inner `handle_*` fn.
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

    // Mailbox
    reg!("Mailbox/get", backend, |b, _ci, a, ctx| {
        handle_mailbox_get(&*b, &ctx, a).await
    });
    reg!("Mailbox/changes", backend, |b, _ci, a, ctx| {
        handle_mailbox_changes(&*b, &ctx, a).await
    });
    reg!("Mailbox/query", backend, |b, _ci, a, ctx| {
        handle_mailbox_query(&*b, &ctx, a).await
    });
    reg!("Mailbox/queryChanges", backend, |b, _ci, a, ctx| {
        handle_mailbox_query_changes(&*b, &ctx, a).await
    });
    reg!("Mailbox/set", backend, |b, _ci, a, ctx| {
        handle_mailbox_set(&*b, &ctx, a).await
    });

    // Thread
    reg!("Thread/get", backend, |b, _ci, a, ctx| {
        handle_thread_get(&*b, &ctx, a).await
    });
    reg!("Thread/changes", backend, |b, _ci, a, ctx| {
        handle_thread_changes(&*b, &ctx, a).await
    });

    // Email
    reg!("Email/get", backend, |b, _ci, a, ctx| {
        handle_email_get(&*b, &ctx, a).await
    });
    reg!("Email/changes", backend, |b, _ci, a, ctx| {
        handle_email_changes(&*b, &ctx, a).await
    });
    reg!("Email/query", backend, |b, _ci, a, ctx| {
        handle_email_query(&*b, &ctx, a).await
    });
    reg!("Email/queryChanges", backend, |b, _ci, a, ctx| {
        handle_email_query_changes(&*b, &ctx, a).await
    });
    reg!("Email/set", backend, |b, _ci, a, ctx| {
        handle_email_set(&*b, &ctx, a).await
    });
    reg!("Email/copy", backend, |b, ci, a, ctx| {
        handle_email_copy(&*b, &ctx, a, &ci).await
    });
    reg!("Email/import", backend, |b, _ci, a, ctx| {
        handle_email_import(&*b, &ctx, a).await
    });
    reg!("Email/parse", backend, |b, _ci, a, ctx| {
        handle_email_parse(&*b, &ctx, a).await
    });

    // SearchSnippet
    reg!("SearchSnippet/get", backend, |b, _ci, a, ctx| {
        handle_search_snippet_get(&*b, &ctx, a).await
    });

    // Identity
    reg!("Identity/get", backend, |b, _ci, a, ctx| {
        handle_identity_get(&*b, &ctx, a).await
    });
    reg!("Identity/changes", backend, |b, _ci, a, ctx| {
        handle_identity_changes(&*b, &ctx, a).await
    });
    reg!("Identity/set", backend, |b, _ci, a, ctx| {
        handle_identity_set(&*b, &ctx, a).await
    });

    // EmailSubmission
    reg!("EmailSubmission/get", backend, |b, _ci, a, ctx| {
        handle_submission_get(&*b, &ctx, a).await
    });
    reg!("EmailSubmission/changes", backend, |b, _ci, a, ctx| {
        handle_submission_changes(&*b, &ctx, a).await
    });
    reg!("EmailSubmission/query", backend, |b, _ci, a, ctx| {
        handle_submission_query(&*b, &ctx, a).await
    });
    reg!("EmailSubmission/queryChanges", backend, |b, _ci, a, ctx| {
        handle_submission_query_changes(&*b, &ctx, a).await
    });
    reg!("EmailSubmission/set", backend, |b, ci, a, ctx| {
        handle_submission_set(&*b, &ctx, a, &ci).await
    });

    // VacationResponse
    reg!("VacationResponse/get", backend, |b, _ci, a, ctx| {
        handle_vacation_get(&*b, &ctx, a).await
    });
    reg!("VacationResponse/set", backend, |b, _ci, a, ctx| {
        handle_vacation_set(&*b, &ctx, a).await
    });
}

pub use jmap_server::ClosureHandler;

// ---------------------------------------------------------------------------
// register_mdn_handlers — MDN extension entry point (feature = "mdn")
// ---------------------------------------------------------------------------

/// Register MDN method handlers with the dispatcher.
///
/// Registers `MDN/send` and `MDN/parse`. Both methods require
/// `urn:ietf:params:jmap:mdn` in the JMAP request `using` array;
/// `MDN/send` additionally requires `urn:ietf:params:jmap:mail`
/// (RFC 9007 §2.1). Callers MUST ensure
/// `check_known_capabilities` is called with both URIs listed as known,
/// so that clients omitting either capability receive an appropriate
/// error before method dispatch.
///
/// `max_blob_ids` caps the number of blob IDs accepted in a single
/// `MDN/parse` request.  Pass [`mdn::MDN_PARSE_MAX_BLOB_IDS`] for the
/// default (16); use a larger value for high-volume deployments.
///
/// The handlers themselves do not inspect the `using` field — that
/// validation is the dispatcher/framework layer's responsibility.
#[cfg(feature = "mdn")]
pub fn register_mdn_handlers<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
    max_blob_ids: usize,
) where
    B: MailBackend + mdn::MdnBackend + 'static,
{
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

    reg!("MDN/send", backend, |b, ci, a, ctx| {
        mdn::handle_mdn_send(&*b, &ctx, a, &ci).await
    });
    reg!("MDN/parse", backend, |b, _ci, a, ctx| {
        mdn::handle_mdn_parse(&*b, &ctx, a, max_blob_ids).await
    });
}

// ---------------------------------------------------------------------------
// register_sieve_handlers — Sieve extension entry point (feature = "sieve")
// ---------------------------------------------------------------------------

/// Register Sieve method handlers with the dispatcher.
///
/// Registers `SieveScript/get`, `SieveScript/set`, `SieveScript/query`, and
/// `SieveScript/validate`. All four methods require
/// `urn:ietf:params:jmap:sieve` in the JMAP request `using` array
/// (RFC 9661). Callers MUST ensure
/// `check_known_capabilities` is called with that URI listed as known,
/// so that clients omitting the capability receive an appropriate error
/// before method dispatch.
///
/// Backends must implement both [`MailBackend`] and [`SieveBackend`].
#[cfg(feature = "sieve")]
pub fn register_sieve_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: MailBackend + sieve::SieveBackend + 'static,
{
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

    reg!("SieveScript/get", backend, |b, _ci, a, ctx| {
        sieve::handle_sieve_get(&*b, &ctx, a).await
    });
    reg!("SieveScript/set", backend, |b, _ci, a, ctx| {
        sieve::handle_sieve_set(&*b, &ctx, a).await
    });
    reg!("SieveScript/query", backend, |b, _ci, a, ctx| {
        sieve::handle_sieve_query(&*b, &ctx, a).await
    });
    reg!("SieveScript/validate", backend, |b, _ci, a, ctx| {
        sieve::handle_sieve_validate(&*b, &ctx, a).await
    });
}
