//! Object history for the mail types (draft-ietf-jmap-object-history-03
//! with the -04 editor's copy changes; docs/design/object-history.md).
//!
//! The canonical opt-in shape every data-type crate copies: each handler
//! wraps the type's plain handler with `jmap_server::history_get`,
//! `history_query`, `history_query_changes` or `history_set`, so the plain
//! handlers and their bytes are unchanged, and
//! [`register_mail_handlers_with_history`] re-registers them through
//! `Dispatcher::register_history_aware`.

use std::collections::BTreeMap;
use std::sync::Arc;

use jmap_mail_types::{Identity, Mailbox, VacationResponse};
use jmap_server::{
    CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler,
    NoRestoreHooks, ObjectHistoryBackend, ParentRestoreHooks, history_get, history_query,
    history_query_changes, history_set,
};
use jmap_types::{Id, JmapError};
use serde_json::Value;

use crate::{
    MailBackend, handle_identity_get, handle_identity_set, handle_mailbox_get,
    handle_mailbox_query, handle_mailbox_query_changes, handle_vacation_get, handle_vacation_set,
    mailbox, register_mail_handlers,
};

/// `Mailbox/get` with history (§3): [`handle_mailbox_get`] when no history
/// argument is set.
///
/// # Errors
///
/// As [`handle_mailbox_get`] and `jmap_server::history_get`.
pub async fn handle_mailbox_get_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<Mailbox, B, _, _>(backend, caller, args, using, |a| {
        handle_mailbox_get(backend, caller, a, using)
    })
    .await
}

/// `Mailbox/query` with `onlyDestroyed` (§4): [`handle_mailbox_query`]
/// otherwise.
///
/// # Errors
///
/// As [`handle_mailbox_query`] and `jmap_server::history_query`.
pub async fn handle_mailbox_query_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query::<Mailbox, B, _, _>(backend, caller, args, using, |a| {
        handle_mailbox_query(backend, caller, a, using)
    })
    .await
}

/// `Mailbox/queryChanges` with `onlyDestroyed` (-04 §4):
/// [`handle_mailbox_query_changes`] otherwise.
///
/// # Errors
///
/// As [`handle_mailbox_query_changes`] and
/// `jmap_server::history_query_changes`.
pub async fn handle_mailbox_query_changes_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query_changes::<Mailbox, B, _, _>(backend, caller, args, using, |a| {
        handle_mailbox_query_changes(backend, caller, a, using)
    })
    .await
}

/// `Mailbox/set` with `restore` (§5.1): restores run first, a parent before
/// a child whose final `parentId` is another restore key (-04 §5.1), then
/// [`handle_mailbox_set`](crate::handle_mailbox_set).
///
/// # Errors
///
/// As [`handle_mailbox_set`](crate::handle_mailbox_set) and
/// `jmap_server::history_set`.
pub async fn handle_mailbox_set_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    mailbox_set_with_history(backend, caller, created_ids, args, using, false).await
}

async fn mailbox_set_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
    atomic: bool,
) -> Result<HandlerOutput, JmapError> {
    history_set::<Mailbox, B, _, _>(
        backend,
        caller,
        args,
        using,
        &ParentRestoreHooks("parentId"),
        |a| mailbox::mailbox_set(backend, caller, created_ids, a, using, atomic),
    )
    .await
}

/// `Identity/get` with history (§3): [`handle_identity_get`] otherwise.
///
/// # Errors
///
/// As [`handle_identity_get`] and `jmap_server::history_get`.
pub async fn handle_identity_get_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<Identity, B, _, _>(backend, caller, args, using, |a| {
        handle_identity_get(backend, caller, a)
    })
    .await
}

/// `Identity/set` with `restore` (§5.1), then [`handle_identity_set`].
///
/// # Errors
///
/// As [`handle_identity_set`] and `jmap_server::history_set`.
pub async fn handle_identity_set_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_set::<Identity, B, _, _>(backend, caller, args, using, &NoRestoreHooks, |a| {
        handle_identity_set(backend, caller, a)
    })
    .await
}

/// `VacationResponse/get` with history (§3): [`handle_vacation_get`]
/// otherwise. The singleton is never destroyed, so only its replaced
/// versions are history.
///
/// # Errors
///
/// As [`handle_vacation_get`] and `jmap_server::history_get`.
pub async fn handle_vacation_get_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<VacationResponse, B, _, _>(backend, caller, args, using, |a| {
        handle_vacation_get(backend, caller, a)
    })
    .await
}

/// `VacationResponse/set` with `restore` (§5.1), then
/// [`handle_vacation_set`]. Every restore is `notFound`: the singleton is
/// never destroyed.
///
/// # Errors
///
/// As [`handle_vacation_set`] and `jmap_server::history_set`.
pub async fn handle_vacation_set_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_set::<VacationResponse, B, _, _>(backend, caller, args, using, &NoRestoreHooks, |a| {
        handle_vacation_set(backend, caller, a)
    })
    .await
}

/// [`register_mail_handlers`], then the history-aware `Mailbox/get`,
/// `/set`, `/query`, `/queryChanges`, `Identity/get`, `/set` and
/// `VacationResponse/get`, `/set`, registered through
/// `Dispatcher::register_history_aware` (docs/design/DECISIONS.md decision
/// 1). Email (bd JMAP-tna1.3.5) and SieveScript (JMAP-tna1.3.8) follow.
///
/// Call `register_mail_conditional` after it, if used: it attaches to the
/// registered `/set`. Advertise `urn:ietf:params:jmap:object-history` for
/// a type only while `Dispatcher::object_history_gaps` is empty for it.
pub fn register_mail_handlers_with_history<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) where
    B: MailBackend + ObjectHistoryBackend + 'static,
{
    register_mail_handlers(dispatcher, Arc::clone(&backend));
    macro_rules! reg {
        ($method:expr, |$b:ident, $a:ident, $ctx:ident, $using:ident $(, $field:ident)*| $body:expr) => {{
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                Arc::clone(&backend),
                move |$b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        args: $a,
                        caller: $ctx,
                        using: $using,
                        $($field,)*
                        ..
                    } = cx;
                    Box::pin(async move { $body }) as HandlerFuture
                },
            ));
            dispatcher.register_history_aware($method, h);
        }};
    }
    reg!("Mailbox/get", |b, a, ctx, using| {
        handle_mailbox_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Mailbox/query", |b, a, ctx, using| {
        handle_mailbox_query_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Mailbox/queryChanges", |b, a, ctx, using| {
        handle_mailbox_query_changes_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Mailbox/set", |b, a, ctx, using, created_ids, atomic| {
        mailbox_set_with_history(&*b, &ctx, &created_ids, a, &using, atomic).await
    });
    reg!("Identity/get", |b, a, ctx, using| {
        handle_identity_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Identity/set", |b, a, ctx, using| {
        handle_identity_set_with_history(&*b, &ctx, a, &using).await
    });
    reg!("VacationResponse/get", |b, a, ctx, using| {
        handle_vacation_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("VacationResponse/set", |b, a, ctx, using| {
        handle_vacation_set_with_history(&*b, &ctx, a, &using).await
    });
}
