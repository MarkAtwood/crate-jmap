//! Object history for [`AddressBook`] and [`ContactCard`]
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes;
//! docs/design/object-history.md), in the shape of the canonical
//! `jmap-mail-server` opt-in: each handler wraps the type's plain handler,
//! which runs unchanged when no history argument is set.

use std::collections::BTreeMap;
use std::sync::Arc;

use jmap_contacts_types::{AddressBook, ContactCard};
use jmap_server::{
    CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler,
    NoRestoreHooks, ObjectHistoryBackend, history_get, history_query, history_query_changes,
    history_set,
};
use jmap_types::{Id, JmapError};
use serde_json::Value;

use crate::{
    ContactsBackend, handle_address_book_get, handle_address_book_set, handle_contact_card_get,
    handle_contact_card_query, handle_contact_card_query_changes, handle_contact_card_set,
    register_contacts_handlers,
};

/// `AddressBook/get` with history (§3): [`handle_address_book_get`]
/// otherwise.
///
/// # Errors
///
/// As [`handle_address_book_get`] and `jmap_server::history_get`.
pub async fn handle_address_book_get_with_history<B: ContactsBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<AddressBook, B, _, _>(backend, caller, args, using, |a| {
        handle_address_book_get(backend, caller, a, using)
    })
    .await
}

/// `AddressBook/set` with `restore` (§5.1), then
/// [`handle_address_book_set`]. A restored [`AddressBook`] is not the default
/// while another one is (RFC 9610 §2).
///
/// # Errors
///
/// As [`handle_address_book_set`] and `jmap_server::history_set`.
pub async fn handle_address_book_set_with_history<B: ContactsBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_set::<AddressBook, B, _, _>(backend, caller, args, using, &NoRestoreHooks, |a| {
        handle_address_book_set(backend, caller, a, using)
    })
    .await
}

/// `ContactCard/get` with history (§3): [`handle_contact_card_get`]
/// otherwise.
///
/// # Errors
///
/// As [`handle_contact_card_get`] and `jmap_server::history_get`.
pub async fn handle_contact_card_get_with_history<B: ContactsBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<ContactCard, B, _, _>(backend, caller, args, using, |a| {
        handle_contact_card_get(backend, caller, a, using)
    })
    .await
}

/// `ContactCard/query` with `onlyDestroyed` (§4):
/// [`handle_contact_card_query`] otherwise.
///
/// # Errors
///
/// As [`handle_contact_card_query`] and `jmap_server::history_query`.
pub async fn handle_contact_card_query_with_history<B: ContactsBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query::<ContactCard, B, _, _>(backend, caller, args, using, |a| {
        handle_contact_card_query(backend, caller, a, using)
    })
    .await
}

/// `ContactCard/queryChanges` with `onlyDestroyed` (-04 §4):
/// [`handle_contact_card_query_changes`] otherwise.
///
/// # Errors
///
/// As [`handle_contact_card_query_changes`] and
/// `jmap_server::history_query_changes`.
pub async fn handle_contact_card_query_changes_with_history<
    B: ContactsBackend + ObjectHistoryBackend,
>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query_changes::<ContactCard, B, _, _>(backend, caller, args, using, |a| {
        handle_contact_card_query_changes(backend, caller, a, using)
    })
    .await
}

/// `ContactCard/set` with `restore` (§5.1), then
/// [`handle_contact_card_set`]. A restore whose `addressBookIds` names a
/// destroyed [`AddressBook`] fails as an update would; restore the
/// `AddressBook` in an earlier call (§5.2).
///
/// # Errors
///
/// As [`handle_contact_card_set`] and `jmap_server::history_set`.
pub async fn handle_contact_card_set_with_history<B: ContactsBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_set::<ContactCard, B, _, _>(backend, caller, args, using, &NoRestoreHooks, |a| {
        handle_contact_card_set(backend, caller, created_ids, a, using)
    })
    .await
}

/// [`register_contacts_handlers`], then the history-aware `/get` and `/set`
/// of [`AddressBook`] and [`ContactCard`] and `ContactCard` `/query` and
/// `/queryChanges`, through `Dispatcher::register_history_aware`
/// (docs/design/DECISIONS.md decision 1). Call
/// `register_contacts_conditional` after it, if used.
pub fn register_contacts_handlers_with_history<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) where
    B: ContactsBackend + ObjectHistoryBackend + 'static,
{
    register_contacts_handlers(dispatcher, Arc::clone(&backend));
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
    reg!("AddressBook/get", |b, a, ctx, using| {
        handle_address_book_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("AddressBook/set", |b, a, ctx, using| {
        handle_address_book_set_with_history(&*b, &ctx, a, &using).await
    });
    reg!("ContactCard/get", |b, a, ctx, using| {
        handle_contact_card_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("ContactCard/query", |b, a, ctx, using| {
        handle_contact_card_query_with_history(&*b, &ctx, a, &using).await
    });
    reg!("ContactCard/queryChanges", |b, a, ctx, using| {
        handle_contact_card_query_changes_with_history(&*b, &ctx, a, &using).await
    });
    reg!("ContactCard/set", |b, a, ctx, using, created_ids| {
        handle_contact_card_set_with_history(&*b, &ctx, &created_ids, a, &using).await
    });
}
