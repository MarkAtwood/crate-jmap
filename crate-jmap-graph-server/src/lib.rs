//! JMAP Graph method handlers (draft-atwood-jmap-graph-00).
//!
//! # Usage
//!
//! Implement [`GraphBackend`] for your storage layer, then call
//! [`register_graph_handlers`] to wire the method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_graph_server::{GraphBackend, register_graph_handlers};
//! # use jmap_server::Dispatcher;
//! # fn example<B: GraphBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_graph_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # Scope
//!
//! The standard RFC 8620 `/get`, `/changes`, `/set`, `/query` and
//! `/queryChanges` of Graph, GraphNode and GraphEdge (draft §Methods), with
//! the draft's additions:
//!
//! - Rights (§Graph/set, §Access Control): `Graph/set` lets any user with
//!   `mayRead` change their own `isSubscribed`, needs `mayWrite` for
//!   `name` and `annotations` and `mayAdmin` for `shareWith` and destroy;
//!   GraphNode and GraphEdge `/set` need `mayWrite` on their Graph. A
//!   missing right is `forbidden`; a Graph the caller may not read, with
//!   its nodes and edges, is `notFound` on `/get` and on `/set` update or
//!   destroy, and `invalidProperties` naming `graphId` on create.
//! - The integrity rule (§GraphEdge/set): `fromId` and `toId` name nodes of
//!   the edge's Graph, on create and on an update that changes them.
//! - Destroy cascades (§Graph/set, §GraphNode/set): a Graph takes its
//!   nodes and edges with it, a node its edges
//!   ([`GraphBackend::destroy_object`]).
//! - `annotations` keys are non-empty and `maxAnnotationSize` is a
//!   `tooLarge` SetError (§Annotations, §Account-Level Capability Object);
//!   a node's `uri` must be a URI (§GraphNode).
//! - GraphNode and GraphEdge `/query` take a single FilterCondition with
//!   `graphId`, anything else `unsupportedFilter` (search properties too,
//!   unless the account has search; see below); each type's `/query`
//!   refuses a sort property the draft does not list with
//!   `unsupportedSort`. [`query`] holds the match and order functions.
//!
//! # Live editing (draft-atwood-jmap-graph-edit-00)
//!
//! For an account whose [`GraphBackend::graph_edit_capability`] is `Some`:
//!
//! - `GraphNode/set` applies the §GraphNode Extensions and §Snapshots
//!   rules to `editFormat` and `snapshotSeq`; a projected Graph's nodes
//!   cannot be given an `editFormat` (`forbidden`).
//! - [`register_graph_edit_handlers`] wires `GraphNodeOp/get`, `/changes`,
//!   create-only `/set` (the backend assigns `seq`), `/query` and
//!   `/queryChanges` (single `nodeId` condition with `afterSeq`, `seq`
//!   ascending, `opsCompacted`), and `GraphNode/cursor`.
//! - [`GraphEditSubscription`] holds the per-connection `GraphEditStreamEnable`
//!   subscription and the [`should_deliver`] check a WebSocket transport
//!   applies to each `GraphNodeOpEvent` and `GraphCursorEvent`.
//!
//! Sequencing, front-only compaction and the destroy cascades to ops are
//! backend duties; see [`GraphBackend`].
//!
//! # Search (draft-atwood-jmap-graph-search-00)
//!
//! For an account whose [`GraphBackend::graph_search_capability`] is
//! `Some`, GraphNode and GraphEdge `/query` accept `where`, `language` and
//! `expression`. The handlers check them against the capability
//! ([`search::check_search_filter`]: well-formedness, `languages`,
//! `supportsText`, `$collation`, `maxDepth` as `queryTooComplex`), report
//! `canCalculateChanges: false`, and answer `/queryChanges` with
//! `cannotCalculateChanges`. The backend evaluates `where` over the
//! filter's Graph as the caller sees it with [`search::GraphView`]. No
//! advertised language is implemented here.
//!
//! Sharing (`shareWith`, Principals, ShareNotifications) is RFC 9670,
//! served by `jmap-sharing-server`; see [`GraphBackend`].
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose `memory::MemoryBackend`, the
//! in-memory reference implementation of [`GraphBackend`] used by this
//! crate's tests. **Not production.** Its API may break in any minor
//! release while the crate is pre-1.0.

#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{CallContext, ClosureHandler, Dispatcher, HandlerFuture, JmapHandler};

mod backend;
mod edge;
mod edit;
mod graph;
mod helpers;
/// In-memory reference implementation of [`GraphBackend`].
///
/// Gated behind `feature = "memory"`. Not production.
#[cfg(feature = "memory")]
pub mod memory;
mod node;
mod op;
pub mod query;
pub mod search;
mod stream;

pub use backend::{EditLog, GraphBackend};
pub use edge::{
    handle_graph_edge_changes, handle_graph_edge_get, handle_graph_edge_query,
    handle_graph_edge_query_changes, handle_graph_edge_set,
};
pub use graph::{
    handle_graph_changes, handle_graph_get, handle_graph_query, handle_graph_query_changes,
    handle_graph_set,
};
pub use jmap_server::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams,
    QueryResult, SetError, SetErrorType, SetObject,
};
pub use node::{
    handle_graph_node_changes, handle_graph_node_get, handle_graph_node_query,
    handle_graph_node_query_changes, handle_graph_node_set,
};
pub use op::{
    handle_graph_node_cursor, handle_graph_node_op_changes, handle_graph_node_op_get,
    handle_graph_node_op_query, handle_graph_node_op_query_changes, handle_graph_node_op_set,
};
pub use stream::{should_deliver, GraphEditStreamFilter, GraphEditSubscription};

/// Capability URI for `urn:ietf:params:jmap:graph`.
pub use jmap_graph_types::JMAP_GRAPH_URI;

/// Capability URI for `urn:ietf:params:jmap:graph:edit`.
pub use jmap_graph_types::JMAP_GRAPH_EDIT_URI;

/// Capability URI for `urn:ietf:params:jmap:graph:search`.
pub use jmap_graph_types::JMAP_GRAPH_SEARCH_URI;

/// Register the `Graph/*`, `GraphNode/*` and `GraphEdge/*` method handlers
/// with `dispatcher`.
///
/// After this call the dispatcher handles `/get`, `/changes`, `/set`,
/// `/query` and `/queryChanges` of `Graph`, `GraphNode` and `GraphEdge`.
/// [`Dispatcher::register`] silently replaces an existing handler of the
/// same name, so register overrides afterwards.
///
/// This only wires dispatch. The consumer advertises [`JMAP_GRAPH_URI`] in
/// its Session capabilities, with the same account-level object
/// [`GraphBackend::graph_capability`] returns.
pub fn register_graph_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: GraphBackend + 'static,
{
    macro_rules! reg {
        ($method:expr, $handler:ident) => {{
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                Arc::clone(&backend),
                move |b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        call_id: _ci,
                        args: a,
                        caller: ctx,
                        ..
                    } = cx;
                    Box::pin(async move { $handler(&*b, &ctx, a).await }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }
    reg!("Graph/get", handle_graph_get);
    reg!("Graph/changes", handle_graph_changes);
    reg!("Graph/set", handle_graph_set);
    reg!("Graph/query", handle_graph_query);
    reg!("Graph/queryChanges", handle_graph_query_changes);
    reg!("GraphNode/get", handle_graph_node_get);
    reg!("GraphNode/changes", handle_graph_node_changes);
    reg!("GraphNode/set", handle_graph_node_set);
    reg!("GraphNode/query", handle_graph_node_query);
    reg!("GraphNode/queryChanges", handle_graph_node_query_changes);
    reg!("GraphEdge/get", handle_graph_edge_get);
    reg!("GraphEdge/changes", handle_graph_edge_changes);
    reg!("GraphEdge/set", handle_graph_edge_set);
    reg!("GraphEdge/query", handle_graph_edge_query);
    reg!("GraphEdge/queryChanges", handle_graph_edge_query_changes);
}

/// Register the draft-atwood-jmap-graph-edit-00 method handlers with
/// `dispatcher`: `GraphNodeOp/get`, `/changes`, `/set`, `/query`,
/// `/queryChanges` and `GraphNode/cursor`.
///
/// Call it alongside [`register_graph_handlers`] when the consumer
/// advertises [`JMAP_GRAPH_EDIT_URI`] with the account-level object
/// [`GraphBackend::graph_edit_capability`] returns. The handlers answer
/// `accountNotSupportedByMethod` for an account without it. The WebSocket
/// stream frames are not methods; see [`GraphEditSubscription`].
pub fn register_graph_edit_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: GraphBackend + 'static,
{
    macro_rules! reg {
        ($method:expr, $handler:ident) => {{
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                Arc::clone(&backend),
                move |b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        call_id: _ci,
                        args: a,
                        caller: ctx,
                        ..
                    } = cx;
                    Box::pin(async move { $handler(&*b, &ctx, a).await }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }

    reg!("GraphNodeOp/get", handle_graph_node_op_get);
    reg!("GraphNodeOp/changes", handle_graph_node_op_changes);
    reg!("GraphNodeOp/set", handle_graph_node_op_set);
    reg!("GraphNodeOp/query", handle_graph_node_op_query);
    reg!(
        "GraphNodeOp/queryChanges",
        handle_graph_node_op_query_changes
    );
    reg!("GraphNode/cursor", handle_graph_node_cursor);
}
