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

use jmap_mail_types::{Email, Identity, Mailbox, VacationResponse};
use jmap_server::{
    CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput, HistoryGetParams,
    JmapHandler, NoRestoreHooks, ObjectHistoryBackend, ParentRestoreHooks, history_get,
    history_query, history_query_changes, history_set, parse_history_get_arguments,
    require_history_support, serialize_value, strip_object_history_property,
};
use jmap_types::{Id, JmapError};
use serde_json::Value;

use crate::email::{EmailSource, email_get_impl};
use crate::{
    MailBackend, handle_email_get, handle_email_query, handle_email_query_changes,
    handle_email_set, handle_identity_get, handle_identity_set, handle_mailbox_get,
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

/// `Email/get` with history (§3): [`handle_email_get`] otherwise. Each
/// entry gets the same projection as a plain `Email/get` (default
/// properties, `bodyProperties`, the `fetch*BodyValues` arguments,
/// `header:` properties, metadata), plus `objectHistory`.
///
/// # Errors
///
/// As [`handle_email_get`] and `jmap_server::parse_history_get_arguments`;
/// `invalidArguments` when the account does not list Email.
pub async fn handle_email_get_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let Value::Object(mut map) = args else {
        return handle_email_get(backend, caller, args, using).await;
    };
    let history = parse_history_get_arguments(&mut map, using)?;
    strip_object_history_property(&mut map);
    let Some(history) = history else {
        return handle_email_get(backend, caller, Value::Object(map), using).await;
    };
    let history = &history;
    let fetch = move |account: Id, ids: Option<Vec<Id>>| async move {
        require_history_support::<Email, B>(backend, caller, &account)?;
        let result = backend
            .get_object_history::<Email>(
                caller,
                &account,
                HistoryGetParams::new(history).ids(ids.as_deref()),
            )
            .await
            .map_err(JmapError::from)?;
        let mut rows = Vec::with_capacity(result.entries.len());
        for entry in result.entries {
            let info = serialize_value(&entry.object_history)?;
            rows.push((entry.object, info));
        }
        Ok((rows, result.not_found))
    };
    email_get_impl(
        backend,
        caller,
        Value::Object(map),
        using,
        EmailSource::History(&fetch),
    )
    .await
}

/// `Email/query` with `onlyDestroyed` (§4): [`handle_email_query`]
/// otherwise. For a destroyed query `collapseThreads` is ignored (§4:
/// arguments for current objects only).
///
/// # Errors
///
/// As [`handle_email_query`] and `jmap_server::history_query`.
pub async fn handle_email_query_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query::<Email, B, _, _>(backend, caller, args, using, |a| {
        handle_email_query(backend, caller, a, using)
    })
    .await
}

/// `Email/queryChanges` with `onlyDestroyed` (-04 §4):
/// [`handle_email_query_changes`] otherwise.
///
/// # Errors
///
/// As [`handle_email_query_changes`] and
/// `jmap_server::history_query_changes`.
pub async fn handle_email_query_changes_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query_changes::<Email, B, _, _>(backend, caller, args, using, |a| {
        handle_email_query_changes(backend, caller, a, using)
    })
    .await
}

/// `Email/set` with `restore` (§5.1), then [`handle_email_set`]. A
/// restored Email keeps its `blobId` and `threadId` (§5.2, -04 §5.1).
///
/// # Errors
///
/// As [`handle_email_set`] and `jmap_server::history_set`.
pub async fn handle_email_set_with_history<B: MailBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_set::<Email, B, _, _>(backend, caller, args, using, &NoRestoreHooks, |a| {
        handle_email_set(backend, caller, created_ids, a, using)
    })
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

/// [`register_mail_handlers`], then the history-aware `/get`, `/set`,
/// `/query` and `/queryChanges` of Mailbox and Email, and `/get` and `/set`
/// of Identity and [`VacationResponse`], registered through
/// `Dispatcher::register_history_aware` (docs/design/DECISIONS.md decision
/// 1). `SieveScript` has `register_sieve_handlers_with_history`.
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
    reg!("Email/get", |b, a, ctx, using| {
        handle_email_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Email/query", |b, a, ctx, using| {
        handle_email_query_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Email/queryChanges", |b, a, ctx, using| {
        handle_email_query_changes_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Email/set", |b, a, ctx, using, created_ids| {
        handle_email_set_with_history(&*b, &ctx, &created_ids, a, &using).await
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

/// `SieveScript/get` with history (§3): `handle_sieve_get` otherwise.
///
/// # Errors
///
/// As `handle_sieve_get` and `jmap_server::history_get`.
#[cfg(feature = "sieve")]
pub async fn handle_sieve_get_with_history<B>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError>
where
    B: MailBackend + crate::SieveBackend + ObjectHistoryBackend,
{
    history_get::<jmap_mail_types::SieveScript, B, _, _>(backend, caller, args, using, |a| {
        crate::handle_sieve_get(backend, caller, a)
    })
    .await
}

/// `SieveScript/query` with `onlyDestroyed` (§4): `handle_sieve_query`
/// otherwise. `SieveScript` filters in the handler, so the destroyed query
/// reads every destroyed script's final version through
/// `get_object_history` and runs the same filter, sort and paging over
/// them, most recently destroyed first by default. There is no
/// `SieveScript/queryChanges`.
///
/// # Errors
///
/// As `handle_sieve_query` and
/// `jmap_server::parse_destroyed_query_arguments`; `invalidArguments` when
/// the account does not list `SieveScript`.
#[cfg(feature = "sieve")]
pub async fn handle_sieve_query_with_history<B>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError>
where
    B: MailBackend + crate::SieveBackend + ObjectHistoryBackend,
{
    use jmap_mail_types::SieveScript;
    let Value::Object(mut map) = args else {
        return crate::handle_sieve_query(backend, caller, args).await;
    };
    let Some(window) = jmap_server::parse_destroyed_query_arguments(&mut map, using)? else {
        return crate::handle_sieve_query(backend, caller, Value::Object(map)).await;
    };
    let account = match map.get("accountId").and_then(Value::as_str) {
        Some(s) => Id::new_validated(s)
            .map_err(|e| JmapError::invalid_arguments(format!("accountId: {e}")))?,
        None => return Err(JmapError::invalid_arguments("accountId is required")),
    };
    if !backend
        .account_exists(caller, &account)
        .await
        .map_err(|e| jmap_server::server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    require_history_support::<SieveScript, B>(backend, caller, &account)?;
    let mut all = jmap_object_history_types::HistoryGetArguments::new();
    all.include_destroyed = true;
    let result = backend
        .get_object_history::<SieveScript>(caller, &account, HistoryGetParams::new(&all))
        .await
        .map_err(JmapError::from)?;
    let strictly = |a: &jmap_types::UTCDate, b: &jmap_types::UTCDate, want: std::cmp::Ordering| {
        a.cmp_instant(b).is_ok_and(|o| o == want)
    };
    let mut rows = Vec::new();
    for entry in result.entries {
        // Live scripts have no `replaced`; destroyed ones' final versions do.
        let Some(at) = entry.object_history.replaced.clone() else {
            continue;
        };
        if window
            .destroyed_after
            .as_ref()
            .is_some_and(|after| !strictly(&at, after, std::cmp::Ordering::Greater))
            || window
                .destroyed_before
                .as_ref()
                .is_some_and(|before| !strictly(&at, before, std::cmp::Ordering::Less))
        {
            continue;
        }
        rows.push((entry.object, at));
    }
    crate::sieve::sieve_query_impl(backend, caller, Value::Object(map), Some(rows)).await
}

/// `SieveScript/set` with `restore` (§5.1): restores run first, so a
/// restored script, which comes back inactive (§5.2), is a valid
/// `onSuccessActivateScript` target in the same call.
///
/// # Errors
///
/// As `handle_sieve_set` and `jmap_server::history_set`.
#[cfg(feature = "sieve")]
pub async fn handle_sieve_set_with_history<B>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError>
where
    B: MailBackend + crate::SieveBackend + ObjectHistoryBackend,
{
    sieve_set_with_history(backend, caller, created_ids, args, using, false).await
}

#[cfg(feature = "sieve")]
async fn sieve_set_with_history<B>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
    atomic: bool,
) -> Result<HandlerOutput, JmapError>
where
    B: MailBackend + crate::SieveBackend + ObjectHistoryBackend,
{
    history_set::<jmap_mail_types::SieveScript, B, _, _>(
        backend,
        caller,
        args,
        using,
        &NoRestoreHooks,
        |a| crate::sieve::sieve_set(backend, caller, created_ids, a, atomic),
    )
    .await
}

/// `register_sieve_handlers`, then the history-aware `SieveScript/get`,
/// `/set` and `/query`, bound to the request's `using` (the plain Sieve
/// handlers take none).
#[cfg(feature = "sieve")]
pub fn register_sieve_handlers_with_history<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) where
    B: MailBackend + crate::SieveBackend + ObjectHistoryBackend + 'static,
{
    crate::register_sieve_handlers(dispatcher, Arc::clone(&backend));
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
    reg!("SieveScript/get", |b, a, ctx, using| {
        handle_sieve_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("SieveScript/query", |b, a, ctx, using| {
        handle_sieve_query_with_history(&*b, &ctx, a, &using).await
    });
    reg!("SieveScript/set", |b,
                             a,
                             ctx,
                             using,
                             created_ids,
                             atomic| {
        sieve_set_with_history(&*b, &ctx, &created_ids, a, &using, atomic).await
    });
}
