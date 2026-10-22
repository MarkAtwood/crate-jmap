//! JMAP Graph — `GraphEdge/*` method implementations on SessionClient
//! (draft-atwood-jmap-graph-00 §GraphEdge Methods).

use std::collections::BTreeMap;

use jmap_graph_types::query::{GraphEdgeComparator, GraphEdgeFilterCondition};
use jmap_graph_types::GraphEdge;
use jmap_types::{Id, PatchObject, State};

use super::{
    ChangesResponse, GetResponse, PartialObject, QueryChangesResponse, QueryResponse, SetResponse,
};

impl super::SessionClient {
    /// `GraphEdge/get` (draft-atwood-jmap-graph-00 L296-L298; RFC 8620 §5.1).
    ///
    /// `ids: None` asks for every GraphEdge in the account, subject to the
    /// server's `maxObjectsInGet` cap. `properties: None` returns all
    /// properties; a `properties` subset that omits a required [`GraphEdge`]
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
    pub async fn graph_edge_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<GraphEdge>, jmap_base_client::ClientError> {
        self.standard_get("GraphEdge/get", super::USING_GRAPH, ids, properties)
            .await
    }

    /// `GraphEdge/changes` (draft-atwood-jmap-graph-00 L296-L298; RFC 8620 §5.2).
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
    pub async fn graph_edge_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        self.standard_changes(
            "GraphEdge/changes",
            super::USING_GRAPH,
            since_state,
            max_changes,
        )
        .await
    }

    /// `GraphEdge/set` (draft-atwood-jmap-graph-00 L300-L308; RFC 8620 §5.3).
    ///
    /// "On create, `graphId`, `fromId`, and `toId` are required"; the
    /// server rejects an endpoint outside the edge's Graph with an
    /// `invalidProperties` SetError (L304). `fromId` and `toId` may be
    /// creation ids (L306).
    ///
    /// The result is the untyped [`SetResponse`]: per RFC 8620 §5.3 a
    /// `created` entry carries only the server-set properties, which is not
    /// a whole [`GraphEdge`].
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
    pub async fn graph_edge_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        self.standard_set(
            "GraphEdge/set",
            super::USING_GRAPH,
            create,
            update,
            destroy,
            if_in_state,
        )
        .await
    }

    /// `GraphEdge/query` (draft-atwood-jmap-graph-00 L310-L330; RFC 8620 §5.5).
    ///
    /// `filter` is required: "a single FilterCondition that includes
    /// `graphId`" (L314). `from_id`/`to_id` accept a whole frontier, so one
    /// call is one traversal hop (§Traversal, L334). Sort properties are
    /// `created` and `updated` (L330).
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
    pub async fn graph_edge_query(
        &self,
        filter: &GraphEdgeFilterCondition,
        sort: Option<&[GraphEdgeComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        self.standard_query(
            "GraphEdge/query",
            super::USING_GRAPH,
            Some(filter),
            sort,
            params,
        )
        .await
    }

    /// `GraphEdge/queryChanges` (draft-atwood-jmap-graph-00 L310-L330; RFC 8620 §5.6).
    ///
    /// `filter` and `sort` must match the `GraphEdge/query` call that returned
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
    pub async fn graph_edge_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: &GraphEdgeFilterCondition,
        sort: Option<&[GraphEdgeComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        self.standard_query_changes(
            "GraphEdge/queryChanges",
            super::USING_GRAPH,
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
