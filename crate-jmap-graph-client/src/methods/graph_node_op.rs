//! JMAP Graph Live Editing — `GraphNodeOp/*` and `GraphNode/cursor` method
//! implementations on SessionClient (draft-atwood-jmap-graph-edit-00
//! §Methods, §GraphNode/cursor; line numbers cite jmap-chat-spec 6bcd9d8).
//!
//! Every request carries `urn:ietf:params:jmap:graph:edit` in `using`, as
//! in the draft's "Loading a node" example (L414-L418). Ops "are kept in
//! the account of their Graph" (L196), so these methods target the same
//! account as the Graph methods.

use jmap_graph_types::edit::{
    GraphNodeCursorRequest, GraphNodeCursorResponse, GraphNodeOpComparator,
    GraphNodeOpFilterCondition,
};
use jmap_graph_types::GraphNodeOp;
use jmap_types::{Id, State};

use super::{
    ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse,
    USING_GRAPH_EDIT,
};

impl super::SessionClient {
    /// `GraphNodeOp/get` (draft-atwood-jmap-graph-edit-00 L200-L202;
    /// RFC 8620 §5.1).
    ///
    /// `properties: None` returns all properties; a `properties` subset
    /// that omits a required [`GraphNodeOp`] field fails to decode, so
    /// request full objects.
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
    pub async fn graph_node_op_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<GraphNodeOp>, jmap_base_client::ClientError> {
        self.standard_get("GraphNodeOp/get", USING_GRAPH_EDIT, ids, properties)
            .await
    }

    /// `GraphNodeOp/changes` (draft-atwood-jmap-graph-edit-00 L200-L202;
    /// RFC 8620 §5.2). Compacted ops are reported as destroyed (L251).
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
    pub async fn graph_node_op_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        self.standard_changes(
            "GraphNodeOp/changes",
            USING_GRAPH_EDIT,
            since_state,
            max_changes,
        )
        .await
    }

    /// `GraphNodeOp/set` (draft-atwood-jmap-graph-edit-00 L204-L216;
    /// RFC 8620 §5.3), "restricted to create" (L206): there is no `update`
    /// or `destroy` argument.
    ///
    /// `create` maps creation ids to `{ nodeId, baseSeq, data }`, which "are
    /// required" (L208). Each `created` entry carries the op's server-set
    /// `id`, `graphId`, `seq`, `created` and `createdBy`; "That response is
    /// the author's acknowledgment" (L212). Creates in one call are
    /// sequenced in an order the server chooses (L214), so ops that must
    /// keep their order go in separate calls. A retry of a create whose
    /// outcome is unknown "MUST resend the same `baseSeq` and `data`
    /// unchanged" (L216).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:graph`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call), including
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError) for
    ///   RFC 8620 §3.6.2 method-level errors.
    pub async fn graph_node_op_set(
        &self,
        create: serde_json::Value,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        self.standard_set(
            "GraphNodeOp/set",
            USING_GRAPH_EDIT,
            Some(create),
            None,
            None,
        )
        .await
    }

    /// `GraphNodeOp/query` (draft-atwood-jmap-graph-edit-00 L218-L234;
    /// RFC 8620 §5.5).
    ///
    /// "The `filter` argument MUST be a single FilterCondition, not a
    /// FilterOperator, and MUST include `nodeId`" (L222), so `filter` is
    /// required and is the bare condition. "The only supported sort is
    /// `seq` ascending, which is also the order when `sort` is absent"
    /// (L230). Page by sending the next query with `after_seq` set to the
    /// last `seq` received, "not by using `position`" (L232).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:graph`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if serializing an argument fails.
    /// - [`MethodError`](jmap_base_client::ClientError::MethodError) whose
    ///   type is [`OPS_COMPACTED`](jmap_graph_types::edit::OPS_COMPACTED)
    ///   when the server has destroyed an op after `afterSeq` (L234); the
    ///   client then reloads the node from its snapshot.
    /// - Any other transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call).
    pub async fn graph_node_op_query(
        &self,
        filter: &GraphNodeOpFilterCondition,
        sort: Option<&[GraphNodeOpComparator]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        self.standard_query(
            "GraphNodeOp/query",
            USING_GRAPH_EDIT,
            Some(filter),
            sort,
            params,
        )
        .await
    }

    /// `GraphNodeOp/queryChanges` (draft-atwood-jmap-graph-edit-00 L218-L220;
    /// RFC 8620 §5.6).
    ///
    /// `filter` and `sort` must match the `GraphNodeOp/query` call that
    /// returned `since_query_state`.
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
    pub async fn graph_node_op_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: &GraphNodeOpFilterCondition,
        sort: Option<&[GraphNodeOpComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        self.standard_query_changes(
            "GraphNodeOp/queryChanges",
            USING_GRAPH_EDIT,
            since_query_state,
            max_changes,
            Some(filter),
            sort,
            up_to_id,
            calculate_total,
        )
        .await
    }

    /// `GraphNode/cursor` (draft-atwood-jmap-graph-edit-00 L334-L347):
    /// report the calling user's cursor or selection in `node_id` to other
    /// users subscribed to it.
    ///
    /// `data` is "encoded with base64url without padding", with a meaning
    /// defined by the node's `editFormat` (L345). Cursors "are not stored,
    /// are best-effort" (L332): the server "MAY discard calls above the
    /// limit while still returning success" (L347).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:graph`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if serializing the arguments fails.
    /// - [`MethodError`](jmap_base_client::ClientError::MethodError)
    ///   `invalidArguments` when the node is missing, unreadable or has no
    ///   `editFormat`, or `data` is invalid or larger than `maxCursorSize`
    ///   (L347).
    /// - Any other transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call).
    pub async fn graph_node_cursor(
        &self,
        node_id: &Id,
        data: &str,
    ) -> Result<GraphNodeCursorResponse, jmap_base_client::ClientError> {
        const METHOD: &str = "GraphNode/cursor";
        let (api_url, account_id) = self.session_parts()?;
        let req =
            GraphNodeCursorRequest::new(Id::from(account_id), node_id.clone(), data.to_owned());
        let args = super::to_arg(METHOD, "arguments", &req)?;
        self.invoke(api_url, METHOD, args, USING_GRAPH_EDIT).await
    }
}
