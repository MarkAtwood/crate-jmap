//! JMAP Graph — `Graph/*` method implementations on SessionClient
//! (draft-atwood-jmap-graph-00 §Graph Methods).

use std::collections::BTreeMap;

use jmap_graph_types::query::{GraphComparator, GraphFilter};
use jmap_graph_types::Graph;
use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// `Graph/get` (draft-atwood-jmap-graph-00 L242-L244; RFC 8620 §5.1).
    ///
    /// `ids: None` asks for every Graph in the account, subject to the
    /// server's `maxObjectsInGet` cap. `properties: None` returns all
    /// properties; a `properties` subset that omits a required [`Graph`]
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
    pub async fn graph_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<Graph>, jmap_base_client::ClientError> {
        self.standard_get("Graph/get", super::USING_GRAPH, ids, properties)
            .await
    }

    /// `Graph/changes` (draft-atwood-jmap-graph-00 L242-L244; RFC 8620 §5.2).
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
    pub async fn graph_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        self.standard_changes(
            "Graph/changes",
            super::USING_GRAPH,
            since_state,
            max_changes,
        )
        .await
    }

    /// `Graph/set` (draft-atwood-jmap-graph-00 L246-L254; RFC 8620 §5.3).
    ///
    /// Changing `isSubscribed` needs `mayRead`, `name` and `annotations`
    /// `mayWrite`, `shareWith` and destroy `mayAdmin` (L250). "When a Graph
    /// is destroyed, the server MUST also destroy every GraphNode and
    /// GraphEdge in it" (L254). Patch single annotation keys with
    /// `annotations/<key>` paths (§Annotations).
    ///
    /// The result is the untyped [`SetResponse`]: per RFC 8620 §5.3 a
    /// `created` entry carries only the server-set properties, which is not
    /// a whole [`Graph`].
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
    pub async fn graph_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        self.standard_set("Graph/set", super::USING_GRAPH, create, update, destroy)
            .await
    }

    /// `Graph/query` (draft-atwood-jmap-graph-00 L256-L265; RFC 8620 §5.5).
    ///
    /// Pass `None` to omit `filter` or `sort`. The filter condition is
    /// `isSubscribed` (L262); sort properties are `name`, `created` and
    /// `updated` (L265).
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
    pub async fn graph_query(
        &self,
        filter: Option<&GraphFilter>,
        sort: Option<&[GraphComparator]>,
        position: Option<u64>,
        limit: Option<u64>,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        self.standard_query(
            "Graph/query",
            super::USING_GRAPH,
            filter,
            sort,
            position,
            limit,
        )
        .await
    }

    /// `Graph/queryChanges` (draft-atwood-jmap-graph-00 L256-L265; RFC 8620 §5.6).
    ///
    /// `filter` and `sort` must match the `Graph/query` call that returned
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
    pub async fn graph_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<&GraphFilter>,
        sort: Option<&[GraphComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        self.standard_query_changes(
            "Graph/queryChanges",
            super::USING_GRAPH,
            since_query_state,
            max_changes,
            filter,
            sort,
            up_to_id,
            calculate_total,
        )
        .await
    }
}
