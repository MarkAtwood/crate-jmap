//! JMAP Graph — `GraphNode/*` method implementations on SessionClient
//! (draft-atwood-jmap-graph-00 §GraphNode Methods).

use std::collections::BTreeMap;

use jmap_graph_types::GraphNode;
use jmap_graph_types::{GraphNodeComparator, GraphNodeFilterCondition};
use jmap_types::{Id, PatchObject, State};

use super::{
    ChangesResponse, GetResponse, PartialObject, QueryChangesResponse, QueryResponse, SetResponse,
};

impl super::SessionClient {
    /// `GraphNode/get` (draft-atwood-jmap-graph-00 L269-L271; RFC 8620 §5.1).
    ///
    /// `ids: None` asks for every GraphNode in the account, subject to the
    /// server's `maxObjectsInGet` cap. `properties: None` returns all
    /// properties; a `properties` subset that omits a required [`GraphNode`]
    /// field fails to decode, so request full objects.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:graph`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if serializing an argument fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn graph_node_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<GraphNode>, jmap_base_client::ClientError> {
        self.standard_get("GraphNode/get", super::USING_GRAPH, ids, properties)
            .await
    }

    /// `GraphNode/changes` (draft-atwood-jmap-graph-00 L269-L271; RFC 8620 §5.2).
    ///
    /// If `has_more_changes` is true in the response, call again with
    /// `new_state` as `since_state` until it is false.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:graph`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is empty.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn graph_node_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        self.standard_changes(
            "GraphNode/changes",
            super::USING_GRAPH,
            since_state,
            max_changes,
        )
        .await
    }

    /// `GraphNode/set` (draft-atwood-jmap-graph-00 L273-L279; RFC 8620 §5.3).
    ///
    /// "On create, `graphId` is required" and may be a creation id (L277).
    /// Destroying a node destroys every GraphEdge that starts or ends at it
    /// (L279).
    ///
    /// The result is the untyped [`SetResponse`]: per RFC 8620 §5.3 a
    /// `created` entry carries only the server-set properties, which is not
    /// a whole [`GraphNode`].
    ///
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard
    /// (RFC 8620 §5.3 `ifInState`): the server rejects the whole call with
    /// `stateMismatch` unless its current state matches.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:graph`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `create`, `update` and `destroy` are all `None`, or if
    ///   serializing `update` fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn graph_node_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        self.standard_set(
            "GraphNode/set",
            super::USING_GRAPH,
            create,
            update,
            destroy,
            if_in_state,
        )
        .await
    }

    /// `GraphNode/query` (draft-atwood-jmap-graph-00 L281-L292; RFC 8620 §5.5).
    ///
    /// "The `filter` argument MUST be a single FilterCondition, not a
    /// FilterOperator, and MUST include `graphId`" (L285), so `filter` is
    /// required and is the bare condition. Sort properties are `created`
    /// and `updated` (L292).
    ///
    /// When `filter.object_uri` is set (draft-atwood-jmap-graph-projection-00
    /// §GraphNode/query Filter), the request's `using` also carries
    /// `urn:ietf:params:jmap:graph:projection`, as in that draft's Example.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:graph`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if serializing an argument fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn graph_node_query(
        &self,
        filter: &GraphNodeFilterCondition,
        sort: Option<&[GraphNodeComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        self.standard_query(
            "GraphNode/query",
            using_for(filter),
            Some(filter),
            sort,
            params,
        )
        .await
    }

    /// `GraphNode/queryChanges` (draft-atwood-jmap-graph-00 L281-L292; RFC 8620 §5.6).
    ///
    /// `filter` and `sort` must match the `GraphNode/query` call that returned
    /// `since_query_state`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:graph`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is empty or serializing an argument
    ///   fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn graph_node_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: &GraphNodeFilterCondition,
        sort: Option<&[GraphNodeComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        self.standard_query_changes(
            "GraphNode/queryChanges",
            using_for(filter),
            since_query_state,
            max_changes,
            Some(filter),
            sort,
            up_to_id,
            calculate_total,
        )
        .await
    }
}

/// The `using` array for a GraphNode `/query` or `/queryChanges` with
/// `filter`: with the projection capability exactly when `objectUri` is set.
fn using_for(filter: &GraphNodeFilterCondition) -> &'static [&'static str] {
    if filter.object_uri.is_some() {
        super::USING_GRAPH_PROJECTION
    } else {
        super::USING_GRAPH
    }
}
