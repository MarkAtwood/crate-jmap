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
//! # fn example<B: MailBackend + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_mail_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # `mdn` feature
//!
//! Enable the `mdn` feature to add support for the JMAP MDN extension
//! (draft-ietf-jmap-mdn). This exposes [`register_mdn_handlers`], which wires
//! two additional method names into the dispatcher: `MDN/send` and `MDN/parse`.
//! Backends must also implement [`MdnBackend`] in addition to [`MailBackend`].

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
#[cfg(feature = "mdn")]
pub use jmap_mail_types::mdn::JMAP_MDN_URI;
#[cfg(feature = "mdn")]
pub use mdn::MdnBackend;
#[cfg(feature = "mdn")]
pub use mdn::{handle_mdn_parse, handle_mdn_send};
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
/// **Caller context `C` is not forwarded to handlers.** Each handler closure
/// receives only `(Arc<B>, call_id, args)`; the `caller: C` value from the
/// dispatcher is discarded. To act on per-request context (e.g. for
/// per-tenant auth or rate limiting), implement [`JmapHandler`] directly
/// rather than using this function. The closure shape used here is stable for
/// v0.1; adding `CallerCtx` forwarding would be a breaking change and is
/// deferred to a future version.
pub fn register_mail_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: MailBackend + 'static,
    C: Clone + Send + 'static,
{
    // Helper: register one method with a closure that takes (Arc<B>, call_id, args).
    //
    // `$ci` is the call_id string (echoed back to the client). Most handlers
    // ignore it and use `_ci` as the identifier. Only handlers that generate
    // onSuccess* side-effect invocations (Email/copy, EmailSubmission/set) need
    // `ci` — they pass it to the extra-invocations builder so the side-effect
    // method call carries the same client-assigned call_id as the original.
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<C>> = Arc::new(ClosureHandler {
                backend: backend_arc,
                call_fn: Box::new(move |$b: Arc<B>, $ci: String, $a: serde_json::Value| {
                    Box::pin(async move { $body }) as HandlerFuture
                }),
            });
            dispatcher.register($method, h);
        }};
    }

    // Mailbox
    reg!("Mailbox/get", backend, |b, _ci, a| handle_mailbox_get(
        &*b, a
    )
    .await);
    reg!("Mailbox/changes", backend, |b, _ci, a| {
        handle_mailbox_changes(&*b, a).await
    });
    reg!("Mailbox/query", backend, |b, _ci, a| handle_mailbox_query(
        &*b, a
    )
    .await);
    reg!("Mailbox/queryChanges", backend, |b, _ci, a| {
        handle_mailbox_query_changes(&*b, a).await
    });
    reg!("Mailbox/set", backend, |b, _ci, a| handle_mailbox_set(
        &*b, a
    )
    .await);

    // Thread
    reg!("Thread/get", backend, |b, _ci, a| handle_thread_get(&*b, a)
        .await);
    reg!("Thread/changes", backend, |b, _ci, a| {
        handle_thread_changes(&*b, a).await
    });

    // Email
    reg!("Email/get", backend, |b, _ci, a| handle_email_get(&*b, a)
        .await);
    reg!("Email/changes", backend, |b, _ci, a| handle_email_changes(
        &*b, a
    )
    .await);
    reg!("Email/query", backend, |b, _ci, a| handle_email_query(
        &*b, a
    )
    .await);
    reg!("Email/queryChanges", backend, |b, _ci, a| {
        handle_email_query_changes(&*b, a).await
    });
    reg!("Email/set", backend, |b, _ci, a| handle_email_set(&*b, a)
        .await);
    reg!("Email/copy", backend, |b, ci, a| handle_email_copy(
        &*b, a, &ci
    )
    .await);
    reg!("Email/import", backend, |b, _ci, a| handle_email_import(
        &*b, a
    )
    .await);
    reg!("Email/parse", backend, |b, _ci, a| handle_email_parse(
        &*b, a
    )
    .await);

    // SearchSnippet
    reg!("SearchSnippet/get", backend, |b, _ci, a| {
        handle_search_snippet_get(&*b, a).await
    });

    // Identity
    reg!("Identity/get", backend, |b, _ci, a| handle_identity_get(
        &*b, a
    )
    .await);
    reg!("Identity/changes", backend, |b, _ci, a| {
        handle_identity_changes(&*b, a).await
    });
    reg!("Identity/set", backend, |b, _ci, a| handle_identity_set(
        &*b, a
    )
    .await);

    // EmailSubmission
    reg!("EmailSubmission/get", backend, |b, _ci, a| {
        handle_submission_get(&*b, a).await
    });
    reg!("EmailSubmission/changes", backend, |b, _ci, a| {
        handle_submission_changes(&*b, a).await
    });
    reg!("EmailSubmission/query", backend, |b, _ci, a| {
        handle_submission_query(&*b, a).await
    });
    reg!("EmailSubmission/queryChanges", backend, |b, _ci, a| {
        handle_submission_query_changes(&*b, a).await
    });
    reg!("EmailSubmission/set", backend, |b, ci, a| {
        handle_submission_set(&*b, a, &ci).await
    });

    // VacationResponse
    reg!("VacationResponse/get", backend, |b, _ci, a| {
        handle_vacation_get(&*b, a).await
    });
    reg!("VacationResponse/set", backend, |b, _ci, a| {
        handle_vacation_set(&*b, a).await
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
/// (draft-ietf-jmap-mdn-17 §2.1). Callers MUST ensure
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
pub fn register_mdn_handlers<B, C>(
    dispatcher: &mut Dispatcher<C>,
    backend: Arc<B>,
    max_blob_ids: usize,
) where
    B: MailBackend + mdn::MdnBackend + 'static,
    C: Clone + Send + 'static,
{
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<C>> = Arc::new(ClosureHandler {
                backend: backend_arc,
                call_fn: Box::new(move |$b: Arc<B>, $ci: String, $a: serde_json::Value| {
                    Box::pin(async move { $body }) as HandlerFuture
                }),
            });
            dispatcher.register($method, h);
        }};
    }

    reg!("MDN/send", backend, |b, ci, a| mdn::handle_mdn_send(
        &*b, a, &ci
    )
    .await);
    reg!("MDN/parse", backend, |b, _ci, a| mdn::handle_mdn_parse(
        &*b,
        a,
        max_blob_ids
    )
    .await);
}
