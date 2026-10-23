//! GraphEdge/* method handlers (draft-atwood-jmap-graph-00 §GraphEdge
//! Methods).
//!
//! `/get` hides the edges of a Graph the caller may not read; `/changes` is
//! the standard RFC 8620 §5.2 method; `/query` and `/queryChanges` require
//! a single FilterCondition with `graphId`. `/set` requires `mayWrite` on
//! the edge's Graph and checks that `fromId` and `toId` name nodes of that
//! Graph.

use jmap_graph_types::CandidateKind;
use jmap_graph_types::GraphEdgeComparator;
use jmap_graph_types::{GraphCapability, GraphEdge, GraphNode};
use jmap_types::{Id, JmapError};
use serde_json::{Map, Value};

use crate::helpers::{
    backend_set_error_value, check_annotations, decode_patch, fetch_one, handle_get_visible,
    handle_graph_item_query, handle_graph_item_query_changes, handle_set, invalid, mistyped,
    not_found, patched, patched_properties, set_error_value, to_map, SetOps, TypeCheck,
};
use crate::node::{create_graph_id, created_entry, fill_server_set, item_graph_writable, touch};
use crate::GraphBackend;
use jmap_server::{reject_read_only_patch, reject_server_set, HandlerOutput};

/// The properties with a draft default, echoed in `created` when the
/// client omits them (RFC 8620 §5.3).
const DEFAULTED: &[&str] = &["type", "annotations"];

/// `fromId` and `toId` are Ids, `type` a String.
const TYPES: &[TypeCheck] = &[
    ("fromId", Value::is_string),
    ("toId", Value::is_string),
    ("type", Value::is_string),
];

fn validate(m: &Map<String, Value>, cap: &GraphCapability) -> Result<(), Value> {
    let bad = mistyped(m, TYPES);
    if !bad.is_empty() {
        return Err(invalid(&bad, "fromId and toId are Ids, type is a String"));
    }
    check_annotations(m, cap)
}

/// "The server MUST verify that each of `fromId` and `toId` identifies an
/// existing GraphNode in the edge's Graph. If either does not, the server
/// MUST reject the operation with an `invalidProperties` SetError naming
/// the failing property. The server SHOULD NOT distinguish a node that does
/// not exist from a node in another Graph in this error." Checks the
/// properties of `which` in edge `m`.
async fn check_endpoints<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    graph_id: &Id,
    m: &Map<String, Value>,
    which: &[&'static str],
) -> Result<(), Value> {
    let mut bad = Vec::new();
    for key in which {
        let Some(id) = m
            .get(*key)
            .and_then(Value::as_str)
            .and_then(|s| Id::new_validated(s).ok())
        else {
            bad.push(*key);
            continue;
        };
        let node: Option<GraphNode> = fetch_one(backend, caller, account_id, &id).await?;
        if node.is_none_or(|n| n.graph_id != *graph_id) {
            bad.push(*key);
        }
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(invalid(
            &bad,
            "fromId and toId must each identify a GraphNode in the edge's Graph",
        ))
    }
}

/// Handle `GraphEdge/get`: standard RFC 8620 §5.1 `/get`, with the edges of
/// a Graph the caller may not read reported in `notFound`.
pub async fn handle_graph_edge_get<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_get_visible::<GraphEdge, B>(backend, caller, args).await
}

/// Handle `GraphEdge/changes`: standard RFC 8620 §5.2 `/changes`.
pub async fn handle_graph_edge_changes<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<GraphEdge, B>(backend, caller, args).await
}

/// Handle `GraphEdge/query` (draft-atwood-jmap-graph-00 §GraphEdge/query):
/// "The `filter` argument is subject to the same restriction as for
/// GraphNode/query"; a sort property other than `created` and `updated` is
/// `unsupportedSort`. The backend evaluates `fromId`, `toId` and `type`
/// ([`crate::query::edge_matches`]) and omits the edges of a Graph the
/// caller may not read. The draft-atwood-jmap-graph-search-00 properties
/// are handled as for [`crate::handle_graph_node_query`].
pub async fn handle_graph_edge_query<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_graph_item_query::<GraphEdge, GraphEdgeComparator, B>(
        backend,
        caller,
        args,
        CandidateKind::Edge,
    )
    .await
}

/// Handle `GraphEdge/queryChanges`: standard RFC 8620 §5.6, with the
/// `/query` filter and sort rules; a search filter is
/// `cannotCalculateChanges` (draft-atwood-jmap-graph-search-00 §Query
/// Changes).
pub async fn handle_graph_edge_query_changes<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_graph_item_query_changes::<GraphEdge, GraphEdgeComparator, B>(
        backend,
        caller,
        args,
        CandidateKind::Edge,
    )
    .await
}

/// Handle `GraphEdge/set` (draft-atwood-jmap-graph-00 §GraphEdge/set).
///
/// "Creating, updating, or destroying an edge requires `mayWrite` on its
/// Graph." On create "`graphId`, `fromId`, and `toId` are required, with
/// `graphId` validated as for GraphNode/set"; the create, and an update
/// that changes `fromId` or `toId`, checks that each names a GraphNode of
/// the edge's Graph. `graphId` is immutable. "Destroying an edge does not
/// affect its nodes."
pub async fn handle_graph_edge_set<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_set::<EdgeSet, B>(backend, caller, args).await
}

struct EdgeSet;

impl<B: GraphBackend> SetOps<B> for EdgeSet {
    type Object = GraphEdge;

    async fn create(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        cap: &GraphCapability,
        create_id: &str,
        obj: Value,
    ) -> Result<Value, Value> {
        let Value::Object(mut m) = obj else {
            return Err(invalid(&[], "a GraphEdge create must be a JSON object"));
        };
        if let Some(e) = reject_server_set::<GraphEdge>(&m) {
            return Err(set_error_value(
                &e.with_description("these properties are set by the server"),
            ));
        }
        let missing: Vec<&str> = ["fromId", "toId"]
            .into_iter()
            .filter(|k| !m.contains_key(*k))
            .collect();
        if !missing.is_empty() {
            return Err(invalid(&missing, "fromId and toId are required"));
        }
        validate(&m, cap)?;
        let graph_id = create_graph_id(backend, caller, account_id, &m).await?;
        check_endpoints(
            backend,
            caller,
            account_id,
            &graph_id,
            &m,
            &["fromId", "toId"],
        )
        .await?;
        let omitted: Vec<&str> = DEFAULTED
            .iter()
            .copied()
            .filter(|k| !m.contains_key(*k))
            .collect();
        fill_server_set::<B>(caller, &mut m);
        let mut edge: GraphEdge = serde_json::from_value(Value::Object(m))
            .map_err(|e| invalid(&[], &format!("invalid GraphEdge: {e}")))?;
        let (id, stored) = backend
            .create_object(caller, account_id, create_id, move |id: &Id| {
                edge.id = id.clone();
                edge
            })
            .await
            .map_err(backend_set_error_value)?;
        Ok(created_entry(&id, &stored, &omitted))
    }

    async fn update(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        cap: &GraphCapability,
        id: &Id,
        patch: Value,
    ) -> Result<Value, Value> {
        let mut patch = decode_patch(patch)?;
        let edge: GraphEdge = fetch_one(backend, caller, account_id, id)
            .await?
            .ok_or_else(not_found)?;
        item_graph_writable(backend, caller, account_id, &edge.graph_id).await?;
        let keys = patched_properties(&patch);
        if let Some(e) = reject_read_only_patch::<GraphEdge>(patch.as_map(), Some(&to_map(&edge))) {
            return Err(set_error_value(
                &e.with_description("these properties are immutable or server-set"),
            ));
        }
        let result = patched(&edge, &patch)?;
        validate(&result, cap)?;
        let moved: Vec<&'static str> = ["fromId", "toId"]
            .into_iter()
            .filter(|k| keys.iter().any(|p| p == k))
            .collect();
        check_endpoints(backend, caller, account_id, &edge.graph_id, &result, &moved).await?;
        let echo = touch::<B>(caller, &mut patch);
        match backend
            .update_object::<GraphEdge>(caller, account_id, id, patch)
            .await
            .map_err(backend_set_error_value)?
        {
            Some(e) => Ok(Value::Object(to_map(&e))),
            None => Ok(echo),
        }
    }

    async fn destroy(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), Value> {
        let edge: GraphEdge = fetch_one(backend, caller, account_id, id)
            .await?
            .ok_or_else(not_found)?;
        item_graph_writable(backend, caller, account_id, &edge.graph_id).await?;
        backend
            .destroy_object::<GraphEdge>(caller, account_id, id)
            .await
            .map_err(backend_set_error_value)
    }
}
