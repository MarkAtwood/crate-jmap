//! Object history for TaskList and Task (TaskNotification has no history, decision 22)
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes;
//! docs/design/object-history.md), in the shape of the canonical
//! `jmap-mail-server` opt-in: each handler wraps the type's plain handler,
//! which runs unchanged when no history argument is set.

use std::collections::BTreeMap;
use std::sync::Arc;

use jmap_server::{
    CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler,
    NoRestoreHooks, ObjectHistoryBackend, history_get, history_query, history_query_changes,
    history_set,
};
use jmap_tasks_types::{Task, TaskList};
use jmap_types::{Id, JmapError};
use serde_json::Value;

use crate::{
    TasksBackend, handle_task_get, handle_task_list_get, handle_task_list_set, handle_task_query,
    handle_task_query_changes, handle_task_set, register_tasks_handlers,
};

/// `TaskList/get` with history (§3): [`handle_task_list_get`]
/// otherwise.
///
/// # Errors
///
/// As [`handle_task_list_get`] and `jmap_server::history_get`.
pub async fn handle_task_list_get_with_history<B: TasksBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<TaskList, B, _, _>(backend, caller, args, using, |a| {
        handle_task_list_get(backend, caller, a, using)
    })
    .await
}

/// `TaskList/set` with `restore` (§5.1), then
/// [`handle_task_list_set`].
///
/// # Errors
///
/// As [`handle_task_list_set`] and `jmap_server::history_set`.
pub async fn handle_task_list_set_with_history<B: TasksBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_set::<TaskList, B, _, _>(backend, caller, args, using, &NoRestoreHooks, |a| {
        handle_task_list_set(backend, caller, a, using)
    })
    .await
}

/// `Task/get` with history (§3): [`handle_task_get`]
/// otherwise.
///
/// # Errors
///
/// As [`handle_task_get`] and `jmap_server::history_get`.
pub async fn handle_task_get_with_history<B: TasksBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<Task, B, _, _>(backend, caller, args, using, |a| {
        handle_task_get(backend, caller, a, using)
    })
    .await
}

/// `Task/query` with `onlyDestroyed` (§4):
/// [`handle_task_query`] otherwise.
///
/// # Errors
///
/// As [`handle_task_query`] and `jmap_server::history_query`.
pub async fn handle_task_query_with_history<B: TasksBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query::<Task, B, _, _>(backend, caller, args, using, |a| {
        handle_task_query(backend, caller, a, using)
    })
    .await
}

/// `Task/queryChanges` with `onlyDestroyed` (-04 §4):
/// [`handle_task_query_changes`] otherwise.
///
/// # Errors
///
/// As [`handle_task_query_changes`] and
/// `jmap_server::history_query_changes`.
pub async fn handle_task_query_changes_with_history<B: TasksBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query_changes::<Task, B, _, _>(backend, caller, args, using, |a| {
        handle_task_query_changes(backend, caller, a, using)
    })
    .await
}

/// `Task/set` with `restore` (§5.1), then
/// [`handle_task_set`]. A restore whose `taskListId` names a
/// destroyed TaskList fails as an update would; restore the
/// TaskList in an earlier call (§5.2). Task/copy mints fresh ids, never
/// one of a destroyed Task with history (§5.1).
///
/// # Errors
///
/// As [`handle_task_set`] and `jmap_server::history_set`.
pub async fn handle_task_set_with_history<B: TasksBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_set::<Task, B, _, _>(backend, caller, args, using, &NoRestoreHooks, |a| {
        handle_task_set(backend, caller, created_ids, a, using)
    })
    .await
}

/// [`register_tasks_handlers`], then the history-aware `/get` and `/set`
/// of TaskList and Task (TaskNotification has no history, decision 22) and Task `/query` and
/// `/queryChanges`, through `Dispatcher::register_history_aware`
/// (docs/design/DECISIONS.md decision 1). Call
/// `register_tasks_conditional` after it, if used.
pub fn register_tasks_handlers_with_history<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) where
    B: TasksBackend + ObjectHistoryBackend + 'static,
{
    register_tasks_handlers(dispatcher, Arc::clone(&backend));
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
    reg!("TaskList/get", |b, a, ctx, using| {
        handle_task_list_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("TaskList/set", |b, a, ctx, using| {
        handle_task_list_set_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Task/get", |b, a, ctx, using| {
        handle_task_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Task/query", |b, a, ctx, using| {
        handle_task_query_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Task/queryChanges", |b, a, ctx, using| {
        handle_task_query_changes_with_history(&*b, &ctx, a, &using).await
    });
    reg!("Task/set", |b, a, ctx, using, created_ids| {
        handle_task_set_with_history(&*b, &ctx, &created_ids, a, &using).await
    });
}
