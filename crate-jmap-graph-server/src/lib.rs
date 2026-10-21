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
//!   `graphId`, anything else `unsupportedFilter`; each type's `/query`
//!   refuses a sort property the draft does not list with
//!   `unsupportedSort`. [`query`] holds the match and order functions.
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

use jmap_server::{Dispatcher, HandlerFuture, JmapHandler};

pub mod backend;
pub mod edge;
pub mod graph;
mod helpers;
/// In-memory reference implementation of [`GraphBackend`].
///
/// Gated behind `feature = "memory"`. Not production.
#[cfg(feature = "memory")]
pub mod memory;
pub mod node;
pub mod query;

pub use backend::{
    AddedItem, BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
    GraphBackend, JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError,
    SetErrorType, SetObject,
};
pub use edge::{
    handle_graph_edge_changes, handle_graph_edge_get, handle_graph_edge_query,
    handle_graph_edge_query_changes, handle_graph_edge_set,
};
pub use graph::{
    handle_graph_changes, handle_graph_get, handle_graph_query, handle_graph_query_changes,
    handle_graph_set,
};
pub use node::{
    handle_graph_node_changes, handle_graph_node_get, handle_graph_node_query,
    handle_graph_node_query_changes, handle_graph_node_set,
};

/// Capability URI for `urn:ietf:params:jmap:graph`.
pub use jmap_graph_types::JMAP_GRAPH_URI;

/// Generic closure-to-[`JmapHandler`] adapter from [`jmap_server`],
/// re-exported for the registration macro.
pub use jmap_server::ClosureHandler;

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
                move |b: Arc<B>, _ci: String, a: serde_json::Value, ctx: B::CallerCtx| {
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
