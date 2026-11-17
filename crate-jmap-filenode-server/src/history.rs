//! Object history for FileNode (draft-ietf-jmap-object-history-03 with the
//! -04 editor's copy changes; docs/design/object-history.md), in the shape
//! of the canonical `jmap-mail-server` opt-in: each handler wraps the plain
//! handler, which runs unchanged when no history argument is set.

use std::collections::BTreeMap;
use std::sync::Arc;

use jmap_filenode_types::FileNode;
use jmap_server::{
    CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler,
    ObjectHistoryBackend, ParentRestoreHooks, history_get, history_query, history_query_changes,
    history_set,
};
use jmap_types::{Id, JmapError};
use serde_json::Value;

use crate::{
    FileNodeBackend, filenode, handle_filenode_get, handle_filenode_query,
    handle_filenode_query_changes, register_filenode_handlers,
};

/// `FileNode/get` with history (§3): [`handle_filenode_get`] otherwise.
///
/// # Errors
///
/// As [`handle_filenode_get`] and `jmap_server::history_get`.
pub async fn handle_filenode_get_with_history<B: FileNodeBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<FileNode, B, _, _>(backend, caller, args, using, |a| {
        handle_filenode_get(backend, caller, a, using)
    })
    .await
}

/// `FileNode/query` with `onlyDestroyed` (§4): [`handle_filenode_query`]
/// otherwise.
///
/// # Errors
///
/// As [`handle_filenode_query`] and `jmap_server::history_query`.
pub async fn handle_filenode_query_with_history<B: FileNodeBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query::<FileNode, B, _, _>(backend, caller, args, using, |a| {
        handle_filenode_query(backend, caller, a, using)
    })
    .await
}

/// `FileNode/queryChanges` with `onlyDestroyed` (-04 §4):
/// [`handle_filenode_query_changes`] otherwise.
///
/// # Errors
///
/// As [`handle_filenode_query_changes`] and
/// `jmap_server::history_query_changes`.
pub async fn handle_filenode_query_changes_with_history<
    B: FileNodeBackend + ObjectHistoryBackend,
>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_query_changes::<FileNode, B, _, _>(backend, caller, args, using, |a| {
        handle_filenode_query_changes(backend, caller, a, using)
    })
    .await
}

/// `FileNode/set` with `restore` (§5.1), then
/// [`handle_filenode_set`](crate::handle_filenode_set). A parent is
/// restored before a child whose final `parentId` is another restore key
/// (-04 §5.1); a missing parent is `invalidProperties` and a sibling with
/// the same name `alreadyExists` (§5.2).
///
/// # Errors
///
/// As [`handle_filenode_set`](crate::handle_filenode_set) and
/// `jmap_server::history_set`.
pub async fn handle_filenode_set_with_history<B: FileNodeBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    filenode_set_with_history(backend, caller, created_ids, args, using, false).await
}

async fn filenode_set_with_history<B: FileNodeBackend + ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
    atomic: bool,
) -> Result<HandlerOutput, JmapError> {
    history_set::<FileNode, B, _, _>(
        backend,
        caller,
        args,
        using,
        &ParentRestoreHooks("parentId"),
        |a| filenode::filenode_set(backend, caller, created_ids, a, using, atomic),
    )
    .await
}

/// [`register_filenode_handlers`], then the history-aware FileNode `/get`,
/// `/set`, `/query` and `/queryChanges`, through
/// `Dispatcher::register_history_aware` (docs/design/DECISIONS.md decision
/// 1). Call `register_filenode_conditional` after it, if used.
pub fn register_filenode_handlers_with_history<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) where
    B: FileNodeBackend + ObjectHistoryBackend + 'static,
{
    register_filenode_handlers(dispatcher, Arc::clone(&backend));
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
    reg!("FileNode/get", |b, a, ctx, using| {
        handle_filenode_get_with_history(&*b, &ctx, a, &using).await
    });
    reg!("FileNode/query", |b, a, ctx, using| {
        handle_filenode_query_with_history(&*b, &ctx, a, &using).await
    });
    reg!("FileNode/queryChanges", |b, a, ctx, using| {
        handle_filenode_query_changes_with_history(&*b, &ctx, a, &using).await
    });
    reg!("FileNode/set", |b, a, ctx, using, created_ids, atomic| {
        filenode_set_with_history(&*b, &ctx, &created_ids, a, &using, atomic).await
    });
}
