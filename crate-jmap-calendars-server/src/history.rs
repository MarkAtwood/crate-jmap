//! Object history for Calendar, CalendarEvent and ParticipantIdentity
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes;
//! docs/design/object-history.md), in the shape of the canonical
//! `jmap-mail-server` opt-in: each handler wraps the type's plain handler,
//! which runs unchanged when no history argument is set.

use std::collections::BTreeMap;
use std::sync::Arc;

use jmap_calendars_types::{Calendar, CalendarEvent, ParticipantIdentity};
use jmap_server::{
    CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler,
    NoRestoreHooks, ObjectHistoryBackend, history_get, history_query, history_query_changes,
    history_set,
};
use jmap_types::{Id, JmapError};
use serde_json::Value;

use crate::{
    CalendarsBackend, handle_calendar_event_get, handle_calendar_event_query,
    handle_calendar_event_query_changes, handle_calendar_event_set, handle_calendar_get,
    handle_calendar_set, handle_participant_identity_get, handle_participant_identity_set,
    register_calendars_handlers,
};

/// `Calendar/get` with history (§3): [`handle_calendar_get`] otherwise.
///
/// # Errors
///
/// As [`handle_calendar_get`] and `jmap_server::history_get`.
pub async fn handle_calendar_get_with_history<B: CalendarsBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<Calendar, B, _, _>(backend, caller, args, using, |a| {
        handle_calendar_get(backend, caller, a, using)
    })
    .await
}

/// `Calendar/set` with `restore` (§5.1), then [`handle_calendar_set`].
///
/// # Errors
///
/// As [`handle_calendar_set`] and `jmap_server::history_set`.
pub async fn handle_calendar_set_with_history<B: CalendarsBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_set::<Calendar, B, _, _>(backend, caller, args, using, &NoRestoreHooks, |a| {
        handle_calendar_set(backend, caller, a, using)
    })
    .await
}

/// `CalendarEvent/get` with history (§3): [`handle_calendar_event_get`]
/// otherwise.
///
/// # Errors
///
/// As [`handle_calendar_event_get`] and `jmap_server::history_get`.
pub async fn handle_calendar_event_get_with_history<B: CalendarsBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<CalendarEvent, B, _, _>(backend, caller, args, using, |a| {
        handle_calendar_event_get(backend, caller, a, using)
    })
    .await
}

/// `CalendarEvent/query` with `onlyDestroyed` (§4):
/// [`handle_calendar_event_query`] otherwise. `expandRecurrences` and the
/// time-range arguments are ignored for a destroyed query (§4: arguments
/// for current objects only).
///
/// # Errors
///
/// As [`handle_calendar_event_query`] and `jmap_server::history_query`.
pub async fn handle_calendar_event_query_with_history<
    B: CalendarsBackend + ObjectHistoryBackend,
>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query::<CalendarEvent, B, _, _>(backend, caller, args, using, |a| {
        handle_calendar_event_query(backend, caller, a, using)
    })
    .await
}

/// `CalendarEvent/queryChanges` with `onlyDestroyed` (-04 §4):
/// [`handle_calendar_event_query_changes`] otherwise.
///
/// # Errors
///
/// As [`handle_calendar_event_query_changes`] and
/// `jmap_server::history_query_changes`.
pub async fn handle_calendar_event_query_changes_with_history<
    B: CalendarsBackend + ObjectHistoryBackend,
>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query_changes::<CalendarEvent, B, _, _>(backend, caller, args, using, |a| {
        handle_calendar_event_query_changes(backend, caller, a, using)
    })
    .await
}

/// `CalendarEvent/set` with `restore` (§5.1), then
/// [`handle_calendar_event_set`]. A restore whose `calendarIds` names a
/// destroyed Calendar fails as an update would; restore the Calendar in
/// an earlier call (§5.2).
///
/// # Errors
///
/// As [`handle_calendar_event_set`] and `jmap_server::history_set`.
pub async fn handle_calendar_event_set_with_history<B: CalendarsBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_set::<CalendarEvent, B, _, _>(backend, caller, args, using, &NoRestoreHooks, |a| {
        handle_calendar_event_set(backend, caller, created_ids, a, using)
    })
    .await
}

/// `ParticipantIdentity/get` with history (§3):
/// [`handle_participant_identity_get`] otherwise.
///
/// # Errors
///
/// As [`handle_participant_identity_get`] and `jmap_server::history_get`.
pub async fn handle_participant_identity_get_with_history<
    B: CalendarsBackend + ObjectHistoryBackend,
>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<ParticipantIdentity, B, _, _>(backend, caller, args, using, |a| {
        handle_participant_identity_get(backend, caller, a)
    })
    .await
}

/// `ParticipantIdentity/set` with `restore` (§5.1), then
/// [`handle_participant_identity_set`].
///
/// # Errors
///
/// As [`handle_participant_identity_set`] and `jmap_server::history_set`.
pub async fn handle_participant_identity_set_with_history<
    B: CalendarsBackend + ObjectHistoryBackend,
>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_set::<ParticipantIdentity, B, _, _>(
        backend,
        caller,
        args,
        using,
        &NoRestoreHooks,
        |a| handle_participant_identity_set(backend, caller, a),
    )
    .await
}

/// [`register_calendars_handlers`], then the history-aware `/get` and
/// `/set` of Calendar, CalendarEvent and ParticipantIdentity and
/// CalendarEvent `/query` and `/queryChanges`, through
/// `Dispatcher::register_history_aware` (docs/design/DECISIONS.md decision
/// 1). ParticipantIdentity's handlers are bound to the request's `using`,
/// which the plain ones do not take. Call `register_calendars_conditional`
/// after it, if used.
pub fn register_calendars_handlers_with_history<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) where
    B: CalendarsBackend + ObjectHistoryBackend + 'static,
{
    register_calendars_handlers(dispatcher, Arc::clone(&backend));
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
    reg!("Calendar/get", |b, a, ctx, using| {
        handle_calendar_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Calendar/set", |b, a, ctx, using| {
        handle_calendar_set_with_history(&*b, &ctx, a, &using).await
    });
    reg!("CalendarEvent/get", |b, a, ctx, using| {
        handle_calendar_event_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("CalendarEvent/query", |b, a, ctx, using| {
        handle_calendar_event_query_with_history(&*b, &ctx, a, &using).await
    });
    reg!("CalendarEvent/queryChanges", |b, a, ctx, using| {
        handle_calendar_event_query_changes_with_history(&*b, &ctx, a, &using).await
    });
    reg!("CalendarEvent/set", |b, a, ctx, using, created_ids| {
        handle_calendar_event_set_with_history(&*b, &ctx, &created_ids, a, &using).await
    });
    reg!("ParticipantIdentity/get", |b, a, ctx, using| {
        handle_participant_identity_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("ParticipantIdentity/set", |b, a, ctx, using| {
        handle_participant_identity_set_with_history(&*b, &ctx, a, &using).await
    });
}
