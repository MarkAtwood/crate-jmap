//! Object history for Graph, [`GraphNode`] and [`GraphEdge`]
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes;
//! docs/design/object-history.md; docs/design/DECISIONS.md decision 22).
//! `GraphNodeOp` keeps none.

use std::sync::Arc;

use jmap_graph_types::{Graph, GraphEdge, GraphNode};
use jmap_server::{
    CallContext, ClosureHandler, Dispatcher, HandlerFuture, JmapHandler, NoRestoreHooks,
    ObjectHistoryBackend, history_get, history_query, history_query_changes, history_set,
};

use crate::{
    GraphBackend, handle_graph_edge_get, handle_graph_edge_query, handle_graph_edge_query_changes,
    handle_graph_edge_set, handle_graph_get, handle_graph_node_get, handle_graph_node_query,
    handle_graph_node_query_changes, handle_graph_node_set, handle_graph_query,
    handle_graph_query_changes, handle_graph_set, register_graph_handlers,
};

/// [`register_graph_handlers`], then history-aware `/get`, `/set`,
/// `/query` and `/queryChanges` of Graph, [`GraphNode`] and [`GraphEdge`], each
/// wrapping the unchanged plain handler, through
/// `Dispatcher::register_history_aware` (docs/design/DECISIONS.md decision
/// 1). `register_graph_edit_handlers` is unchanged: `GraphNodeOp` keeps no
/// history.
///
/// A restored `GraphNode` comes back at its final snapshot (`blobId`,
/// `snapshotSeq`) with an empty op log whose head is `snapshotSeq`: its
/// ops died with it (graph-edit §Compaction), so edits after the final
/// snapshot are not recoverable (decision 22). A `GraphEdge` restore whose
/// `fromId` or `toId` node is destroyed is `invalidProperties`: restore the
/// nodes in an earlier `GraphNode/set`. Nodes of a projected Graph are
/// derived views; a backend may keep no history for them (§2.1).
pub fn register_graph_handlers_with_history<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) where
    B: GraphBackend + ObjectHistoryBackend + 'static,
{
    register_graph_handlers(dispatcher, Arc::clone(&backend));
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
    macro_rules! type_handlers {
        ($t:ty, $name:literal, $get:ident, $query:ident, $qc:ident) => {
            reg!(concat!($name, "/get"), |b, a, ctx, using| {
                history_get::<$t, B, _, _>(&*b, &ctx, a, &using, |a| $get(&*b, &ctx, a)).await
            });
            reg!(concat!($name, "/query"), |b, a, ctx, using| {
                history_query::<$t, B, _, _>(&*b, &ctx, a, &using, |a| $query(&*b, &ctx, a)).await
            });
            reg!(concat!($name, "/queryChanges"), |b, a, ctx, using| {
                history_query_changes::<$t, B, _, _>(&*b, &ctx, a, &using, |a| $qc(&*b, &ctx, a))
                    .await
            });
        };
    }
    type_handlers!(
        Graph,
        "Graph",
        handle_graph_get,
        handle_graph_query,
        handle_graph_query_changes
    );
    type_handlers!(
        GraphNode,
        "GraphNode",
        handle_graph_node_get,
        handle_graph_node_query,
        handle_graph_node_query_changes
    );
    type_handlers!(
        GraphEdge,
        "GraphEdge",
        handle_graph_edge_get,
        handle_graph_edge_query,
        handle_graph_edge_query_changes
    );
    reg!("Graph/set", |b, a, ctx, using| {
        history_set::<Graph, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_graph_set(&*b, &ctx, a)
        })
        .await
    });
    reg!("GraphNode/set", |b, a, ctx, using, created_ids| {
        history_set::<GraphNode, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_graph_node_set(&*b, &ctx, &created_ids, a)
        })
        .await
    });
    reg!("GraphEdge/set", |b, a, ctx, using, created_ids| {
        history_set::<GraphEdge, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_graph_edge_set(&*b, &ctx, &created_ids, a)
        })
        .await
    });
}
