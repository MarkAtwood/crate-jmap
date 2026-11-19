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
//! Backends must also implement `MdnBackend`, which has [`MailBackend`] as a
//! supertrait.
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
// bd:JMAP-uqhn.1 — narrowing and sign-changing `as` casts are banned;
// use `try_from`. Widening `usize as u64` stays legal (these lints do
// not fire on it): it is lossless on every target Rust supports.
#![warn(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use std::sync::Arc;

use jmap_mail_types::{Email, EmailSubmission, Identity, Mailbox, VacationResponse};
use jmap_server::{
    CallContext, ClosureHandler, ConditionalBackend, Dispatcher, HandlerFuture, JmapHandler,
    RegistrationError, SetSupport,
};

mod backend;
mod email;
#[cfg(feature = "emailpush")]
pub mod emailpush;
#[cfg(feature = "mdn")]
pub mod mdn;
/// In-memory reference implementation of [`MailBackend`].
///
/// Gated behind `feature = "memory"`. Not production. See [`memory`] for
/// the full module documentation.
#[cfg(feature = "memory")]
pub mod memory;
#[cfg(feature = "sieve")]
mod sieve;
#[cfg(feature = "emailpush")]
pub use jmap_mail_types::JMAP_EMAILPUSH_URI;
#[cfg(feature = "mdn")]
pub use jmap_mail_types::JMAP_MDN_URI;
#[cfg(feature = "sieve")]
pub use jmap_mail_types::JMAP_SIEVE_SCRIPTS_URI;
#[cfg(feature = "sieve")]
pub use sieve::SieveBackend;
#[cfg(feature = "sieve")]
pub use sieve::{handle_sieve_get, handle_sieve_query, handle_sieve_set, handle_sieve_validate};
#[cfg(any(feature = "memory", feature = "emailpush"))]
mod filter;
mod helpers;
mod history;
mod identity;
mod mailbox;
mod parse;
mod project;
mod snippet;
mod submission;
mod thread;
mod vacation;

pub use backend::MailBackend;
pub use email::{
    handle_email_changes, handle_email_copy, handle_email_get, handle_email_import,
    handle_email_parse, handle_email_query, handle_email_query_changes, handle_email_set,
};
pub use history::{
    handle_email_get_with_history, handle_email_query_changes_with_history,
    handle_email_query_with_history, handle_email_set_with_history,
    handle_identity_get_with_history, handle_identity_set_with_history,
    handle_mailbox_get_with_history, handle_mailbox_query_changes_with_history,
    handle_mailbox_query_with_history, handle_mailbox_set_with_history,
    handle_vacation_get_with_history, handle_vacation_set_with_history,
    register_mail_handlers_with_history,
};
#[cfg(feature = "sieve")]
pub use history::{
    handle_sieve_get_with_history, handle_sieve_query_with_history, handle_sieve_set_with_history,
    register_sieve_handlers_with_history,
};
pub use identity::{handle_identity_changes, handle_identity_get, handle_identity_set};
pub use jmap_mail_types::{
    EmailProperty, EmailSubmissionProperty, IdentityProperty, MailboxProperty,
    SearchSnippetProperty, ThreadProperty, VacationResponseProperty,
};
pub use jmap_mail_types::{JMAP_MAIL_URI, JMAP_SUBMISSION_URI, JMAP_VACATIONRESPONSE_URI};
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
/// You may pass any `Arc<B>` — the function clones it internally into each
/// registered handler closure. Sharing the same `Arc<B>` across this call
/// and other application-level uses of the backend is a memory
/// optimization, not a correctness requirement; separate `Arc<B>` instances
/// pointing at the same underlying backend would also work.
///
/// After this call, the dispatcher handles:
/// `Mailbox/*`, `Thread/*`, `Email/*`, `SearchSnippet/get`,
/// `Identity/*`, `EmailSubmission/*`, and `VacationResponse/*`.
///
/// The dispatcher's `CallerCtx` is taken from `B::CallerCtx`; every registered
/// closure forwards it as `&ctx` into the wrapped `handle_*` function. Backends
/// that use `type CallerCtx = ()` therefore see `&()` inside every handler.
///
/// # Re-registration semantics
///
/// This function calls [`Dispatcher::register`] once per RFC 8621 method
/// name. `Dispatcher::register` **silently overwrites** any pre-existing
/// handler under the same method name (the underlying primitive is
/// `HashMap::insert`). Three consequences callers MUST be aware of:
///
/// - **Double-call**: invoking this function twice on the same
///   dispatcher loses the first set's handlers. The second call wins.
/// - **Custom overrides go LAST**: to replace a single handler (e.g.
///   provide a custom `Email/get`), call this function FIRST, then
///   `dispatcher.register("Email/get", my_override)`. The inverse order
///   silently undoes the custom handler.
/// - **No collision diagnostic**: there is no error or log when a
///   handler is overwritten. The contract is "last register wins" and
///   the caller is responsible for ordering.
///
/// [`Dispatcher::register`]: jmap_server::Dispatcher::register
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
    //
    // Each optional trailing ident binds the `CallContext` field of that name:
    // `created_ids`, the request-wide creation id map (RFC 8620 §5.3) that
    // `/set`, `/copy`, `Email/import` and `Email/parse` handlers use to resolve `#creationId` references, or `using`,
    // the request's capability list (draft-ietf-jmap-metadata-05 §3).
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident, $ctx:ident $(, $field:ident)*| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                backend_arc,
                move |$b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        call_id: $ci,
                        args: $a,
                        caller: $ctx,
                        $($field,)*
                        ..
                    } = cx;
                    Box::pin(async move { $body }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }

    // Mailbox
    reg!("Mailbox/get", backend, |b, _ci, a, ctx, using| {
        handle_mailbox_get(&*b, &ctx, a, &using).await
    });
    reg!("Mailbox/changes", backend, |b, _ci, a, ctx, using| {
        handle_mailbox_changes(&*b, &ctx, a, &using).await
    });
    reg!("Mailbox/query", backend, |b, _ci, a, ctx, using| {
        handle_mailbox_query(&*b, &ctx, a, &using).await
    });
    reg!("Mailbox/queryChanges", backend, |b, _ci, a, ctx, using| {
        handle_mailbox_query_changes(&*b, &ctx, a, &using).await
    });
    reg!(
        "Mailbox/set",
        backend,
        |b, _ci, a, ctx, created_ids, using, atomic| {
            mailbox::mailbox_set(&*b, &ctx, &created_ids, a, &using, atomic).await
        }
    );

    // Thread
    reg!("Thread/get", backend, |b, _ci, a, ctx| {
        handle_thread_get(&*b, &ctx, a).await
    });
    reg!("Thread/changes", backend, |b, _ci, a, ctx| {
        handle_thread_changes(&*b, &ctx, a).await
    });

    // Email
    reg!("Email/get", backend, |b, _ci, a, ctx, using| {
        handle_email_get(&*b, &ctx, a, &using).await
    });
    reg!("Email/changes", backend, |b, _ci, a, ctx, using| {
        handle_email_changes(&*b, &ctx, a, &using).await
    });
    reg!("Email/query", backend, |b, _ci, a, ctx, using| {
        handle_email_query(&*b, &ctx, a, &using).await
    });
    reg!("Email/queryChanges", backend, |b, _ci, a, ctx, using| {
        handle_email_query_changes(&*b, &ctx, a, &using).await
    });
    reg!(
        "Email/set",
        backend,
        |b, _ci, a, ctx, created_ids, using| {
            handle_email_set(&*b, &ctx, &created_ids, a, &using).await
        }
    );
    reg!(
        "Email/copy",
        backend,
        |b, ci, a, ctx, created_ids, using| {
            handle_email_copy(&*b, &ctx, &created_ids, a, &ci, &using).await
        }
    );
    reg!("Email/import", backend, |b, _ci, a, ctx, created_ids| {
        handle_email_import(&*b, &ctx, &created_ids, a).await
    });
    reg!("Email/parse", backend, |b, _ci, a, ctx, created_ids| {
        email::email_parse(&*b, &ctx, &created_ids, a).await
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
    reg!(
        "EmailSubmission/set",
        backend,
        |b, ci, a, ctx, created_ids| {
            handle_submission_set(&*b, &ctx, &created_ids, a, &ci).await
        }
    );

    // VacationResponse
    reg!("VacationResponse/get", backend, |b, _ci, a, ctx| {
        handle_vacation_get(&*b, &ctx, a).await
    });
    reg!("VacationResponse/set", backend, |b, _ci, a, ctx| {
        handle_vacation_set(&*b, &ctx, a).await
    });
}

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
/// The per-request `MDN/parse` blob-ID cap is read from
/// [`mdn::MdnBackend::max_mdn_parse_blob_ids`] at request time.
/// Backends override that method to choose a per-account value;
/// the default is [`mdn::MDN_PARSE_MAX_BLOB_IDS`] (16).
///
/// The handlers themselves do not inspect the `using` field — that
/// validation is the dispatcher/framework layer's responsibility.
///
/// # Re-registration semantics
///
/// Calls [`Dispatcher::register`] for each MDN method name. The
/// dispatcher's underlying `HashMap::insert` silently overwrites any
/// existing handler under the same name. See
/// [`register_mail_handlers`] for the full re-registration contract
/// (double-call, custom overrides, no collision diagnostic).
///
/// [`Dispatcher::register`]: jmap_server::Dispatcher::register
#[cfg(feature = "mdn")]
pub fn register_mdn_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: MailBackend + mdn::MdnBackend + 'static,
{
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident, $ctx:ident| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                backend_arc,
                move |$b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        call_id: $ci,
                        args: $a,
                        caller: $ctx,
                        ..
                    } = cx;
                    Box::pin(async move { $body }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }

    reg!("MDN/send", backend, |b, ci, a, ctx| {
        mdn::handle_mdn_send(&*b, &ctx, a, &ci).await
    });
    reg!("MDN/parse", backend, |b, _ci, a, ctx| {
        mdn::handle_mdn_parse(&*b, &ctx, a).await
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
///
/// # Re-registration semantics
///
/// Calls [`Dispatcher::register`] for each `SieveScript/*` method
/// name. The dispatcher's underlying `HashMap::insert` silently
/// overwrites any existing handler under the same name. See
/// [`register_mail_handlers`] for the full re-registration contract
/// (double-call, custom overrides, no collision diagnostic).
///
/// [`Dispatcher::register`]: jmap_server::Dispatcher::register
#[cfg(feature = "sieve")]
pub fn register_sieve_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: MailBackend + sieve::SieveBackend + 'static,
{
    macro_rules! reg {
        ($method:expr, $backend:expr, |$b:ident, $ci:ident, $a:ident, $ctx:ident $(, $field:ident)*| $body:expr) => {{
            let backend_arc: Arc<B> = Arc::clone(&$backend);
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                backend_arc,
                move |$b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        call_id: $ci,
                        args: $a,
                        caller: $ctx,
                        $($field,)*
                        ..
                    } = cx;
                    Box::pin(async move { $body }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }

    reg!("SieveScript/get", backend, |b, _ci, a, ctx| {
        sieve::handle_sieve_get(&*b, &ctx, a).await
    });
    reg!(
        "SieveScript/set",
        backend,
        |b, _ci, a, ctx, created_ids, atomic| {
            sieve::sieve_set(&*b, &ctx, &created_ids, a, atomic).await
        }
    );
    reg!("SieveScript/query", backend, |b, _ci, a, ctx| {
        sieve::handle_sieve_query(&*b, &ctx, a).await
    });
    reg!(
        "SieveScript/validate",
        backend,
        |b, _ci, a, ctx, created_ids| sieve::sieve_validate(&*b, &ctx, &created_ids, a).await
    );
}

// ---------------------------------------------------------------------------
// register_mail_conditional — draft-ietf-jmap-conditional-02 opt-in
// ---------------------------------------------------------------------------

/// Opt this crate's registered Foo/set methods in to
/// draft-ietf-jmap-conditional-02 (`ifUnchangedBy` and `atomic`;
/// docs/design/DECISIONS.md decisions 1 and 17).
///
/// Call after [`register_mail_handlers`] (and `register_sieve_handlers`,
/// if used). Attaches [`SetSupport`] to each of `Mailbox/set`, `Email/set`,
/// `EmailSubmission/set`, `Identity/set`, `VacationResponse/set` and
/// `SieveScript/set` that is registered, and skips the rest, so one call
/// covers every registration combination. No handler is re-registered.
///
/// The dispatcher then reads preconditions through each type's `/get` and
/// runs each call in the scope `backend` opens
/// ([`ConditionalBackend::begin_set`]). Under `atomic`, the `Mailbox/set`
/// and `SieveScript/set` handlers registered here leave their
/// cross-object checks (role and name uniqueness, script count) to that
/// scope's commit, which must check them against the final state; the
/// memory backend does. An `EmailSubmission/set`'s implicit `Email/set`
/// (`onSuccessUpdateEmail`, `onSuccessDestroyEmail`) runs inside the same
/// scope.
///
/// Advertise `urn:ietf:params:jmap:conditional` only once
/// [`Dispatcher::conditional_ready`] is true for the whole dispatcher.
pub fn register_mail_conditional<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: MailBackend + ConditionalBackend + 'static,
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
    attach(
        dispatcher,
        "Mailbox/set",
        SetSupport::new::<Mailbox, B>(Arc::clone(&backend)),
    );
    attach(
        dispatcher,
        "Email/set",
        SetSupport::new::<Email, B>(Arc::clone(&backend)),
    );
    attach(
        dispatcher,
        "EmailSubmission/set",
        SetSupport::new::<EmailSubmission, B>(Arc::clone(&backend)),
    );
    attach(
        dispatcher,
        "Identity/set",
        SetSupport::new::<Identity, B>(Arc::clone(&backend)),
    );
    attach(
        dispatcher,
        "VacationResponse/set",
        SetSupport::new::<VacationResponse, B>(Arc::clone(&backend)),
    );
    #[cfg(feature = "sieve")]
    attach(
        dispatcher,
        "SieveScript/set",
        SetSupport::new::<jmap_mail_types::SieveScript, B>(Arc::clone(&backend)),
    );
}
