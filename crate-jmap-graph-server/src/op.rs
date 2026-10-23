//! GraphNodeOp/* and GraphNode/cursor method handlers
//! (draft-atwood-jmap-graph-edit-00 §Methods, §GraphNode/cursor).
//!
//! Every handler here answers `accountNotSupportedByMethod` for an account
//! without the `urn:ietf:params:jmap:graph:edit` capability
//! ([`GraphBackend::graph_edit_capability`]).
//!
//! `/get` and `/changes` are standard and need `mayRead` on the op's
//! Graph; `/set` is create-only and leaves sequencing to the backend;
//! `/query` and `/queryChanges` take a single FilterCondition with
//! `nodeId` and sort by `seq` ascending only; `/query` answers
//! `opsCompacted` when the result would not be contiguous with `afterSeq`.

use jmap_graph_types::{GraphCapability, GraphEditCapability, GraphNode, GraphNodeOp};
use jmap_graph_types::{
    GraphCursorEvent, GraphNodeCursorRequest, GraphNodeCursorResponse, GraphNodeOpComparator,
    GraphNodeOpFilterCondition, GraphNodeOpSortProperty, OPS_COMPACTED,
};
use jmap_server::{HandlerOutput, now_utc_string, reject_server_set, server_fail_from_backend};
use jmap_types::{Id, JmapError, SetObject};
use serde::Deserialize as _;
use serde_json::{Map, Value, json};

use crate::edit::base64url_len;
use crate::helpers::{
    SetOps, backend_set_error_value, fetch_one, forbidden, handle_get_visible, handle_set, invalid,
    readable_rights, set_error_value, to_map,
};
use crate::{GraphBackend, JmapBackend, SetError, SetErrorType};

/// The edit capability of the call's account: `accountNotFound` for an
/// unknown account, `accountNotSupportedByMethod` without the capability.
async fn require_edit<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: &Value,
) -> Result<GraphEditCapability, JmapError> {
    let Some(account_id) = args.get("accountId").and_then(Value::as_str).map(Id::from) else {
        return Err(JmapError::invalid_arguments("accountId is required"));
    };
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    backend
        .graph_edit_capability(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
        .ok_or_else(JmapError::account_not_supported_by_method)
}

/// The node `node_id` if the caller may read its Graph and its
/// `editFormat` is not `null`: the node an op or a cursor may target.
async fn editable_node<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    node_id: &Id,
) -> Result<Option<(GraphNode, bool)>, Value> {
    let Some(node) = fetch_one::<GraphNode, B>(backend, caller, account_id, node_id).await? else {
        return Ok(None);
    };
    let Some(rights) = readable_rights(backend, caller, account_id, &node.graph_id).await? else {
        return Ok(None);
    };
    if !node.edit_format().is_ok_and(|f| f.is_some()) {
        return Ok(None);
    }
    Ok(Some((node, rights.may_write)))
}

/// Handle `GraphNodeOp/get`: standard RFC 8620 §5.1 `/get`; "Both require
/// `mayRead` on the op's Graph", so an op of a Graph the caller may not
/// read is in `notFound`.
pub async fn handle_graph_node_op_get<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    require_edit(backend, caller, &args).await?;
    handle_get_visible::<GraphNodeOp, B>(backend, caller, args).await
}

/// Handle `GraphNodeOp/changes`: standard RFC 8620 §5.2 `/changes`.
/// Compacted ops and the ops of a destroyed node or Graph are reported
/// as destroyed.
pub async fn handle_graph_node_op_changes<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    require_edit(backend, caller, &args).await?;
    jmap_server::handle_changes::<GraphNodeOp, B>(backend, caller, args).await
}

/// The `/query` and `/queryChanges` filter and sort checks: "The `filter`
/// argument MUST be a single FilterCondition, not a FilterOperator, and
/// MUST include `nodeId`; a server receiving any other filter, or no
/// filter, rejects the call with an `unsupportedFilter` error", and "The
/// only supported sort is `seq` ascending" (`unsupportedSort` otherwise).
fn op_filter(args: &Value) -> Result<GraphNodeOpFilterCondition, JmapError> {
    let filter = args
        .get("filter")
        .filter(|f| !f.is_null())
        .ok_or_else(JmapError::unsupported_filter)?;
    let filter = GraphNodeOpFilterCondition::deserialize(filter)
        .map_err(|_| JmapError::unsupported_filter())?;
    if let Some(Value::Array(sort)) = args.get("sort") {
        for c in sort {
            if !GraphNodeOpComparator::deserialize(c)
                .is_ok_and(|c| c.is_ascending && c.property == GraphNodeOpSortProperty::Seq)
            {
                return Err(JmapError::unsupported_sort());
            }
        }
    }
    Ok(filter)
}

/// Handle `GraphNodeOp/query` (draft-atwood-jmap-graph-edit-00
/// §GraphNodeOp/query and GraphNodeOp/queryChanges).
///
/// "If the server has destroyed any op of the node whose `seq` is greater
/// than `afterSeq`, it MUST reject the query with an `opsCompacted`
/// error." The compaction bound is read after the query: compaction only
/// removes ops from the front and never undoes it, so a bound at or below
/// `afterSeq` then means no op the query could return had been destroyed.
/// A node the caller may not read gets no `opsCompacted`, so the error
/// does not reveal it; the backend omits its ops.
pub async fn handle_graph_node_op_query<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    require_edit(backend, caller, &args).await?;
    let filter = op_filter(&args)?;
    // Id::from: handle_query below rejects a malformed accountId before
    // this value is used.
    let account_id = Id::from(args["accountId"].as_str().unwrap_or_default());
    let out = jmap_server::handle_query::<GraphNodeOp, B>(backend, caller, args).await?;
    let readable = match fetch_one::<GraphNode, B>(backend, caller, &account_id, &filter.node_id)
        .await
        .map_err(|_| JmapError::server_fail("backend error"))?
    {
        Some(node) => readable_rights(backend, caller, &account_id, &node.graph_id)
            .await
            .map_err(|_| JmapError::server_fail("backend error"))?
            .is_some(),
        None => false,
    };
    if readable {
        let log = backend
            .edit_log(caller, &account_id, &filter.node_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        if log.compacted_through > filter.after_seq.unwrap_or(0) {
            let mut err = JmapError::custom(OPS_COMPACTED);
            err.description = Some(
                "ops after afterSeq have been compacted; reload the node from its snapshot".into(),
            );
            return Err(err);
        }
    }
    Ok(out)
}

/// Handle `GraphNodeOp/queryChanges`: standard RFC 8620 §5.6, with the
/// `/query` filter and sort rules.
pub async fn handle_graph_node_op_query_changes<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    require_edit(backend, caller, &args).await?;
    op_filter(&args)?;
    jmap_server::handle_query_changes::<GraphNodeOp, B>(backend, caller, args).await
}

/// Handle `GraphNodeOp/set` (draft-atwood-jmap-graph-edit-00
/// §GraphNodeOp/set): "a standard `/set` method ..., restricted to
/// create". "An update or destroy is rejected with a `forbidden` SetError".
///
/// A create needs `nodeId`, `baseSeq` and `data`. It is `invalidProperties`
/// naming `nodeId` "if `nodeId` does not identify a node the user may read
/// whose `editFormat` is not `null`", naming `baseSeq` "if `baseSeq` is
/// greater than the head of the node's edit log", naming `data` "if `data`
/// is not valid base64url", and `tooLarge` when the decoded `data` exceeds
/// `maxOpSize`; it needs `mayWrite` (`forbidden`). An old `baseSeq` is
/// accepted: "The server MUST NOT reject an op because its `baseSeq` is
/// old." The backend assigns the `seq` ([`GraphBackend::create_object`]);
/// the `created` entry carries every server-set property.
pub async fn handle_graph_node_op_set<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    require_edit(backend, caller, &args).await?;
    handle_set::<OpSet, B>(backend, caller, args).await
}

struct OpSet;

impl<B: GraphBackend> SetOps<B> for OpSet {
    type Object = GraphNodeOp;

    async fn create(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        _cap: &GraphCapability,
        create_id: &str,
        obj: Value,
    ) -> Result<Value, Value> {
        let Value::Object(mut m) = obj else {
            return Err(invalid(&[], "a GraphNodeOp create must be a JSON object"));
        };
        if let Some(e) = reject_server_set::<GraphNodeOp>(&m) {
            return Err(set_error_value(
                &e.with_description("these properties are set by the server"),
            ));
        }
        let bad: Vec<&str> = [
            ("nodeId", m.get("nodeId").is_some_and(Value::is_string)),
            ("baseSeq", m.get("baseSeq").is_some_and(Value::is_u64)),
            ("data", m.get("data").is_some_and(Value::is_string)),
        ]
        .into_iter()
        .filter(|(_, ok)| !ok)
        .map(|(k, _)| k)
        .collect();
        if !bad.is_empty() {
            return Err(invalid(
                &bad,
                "nodeId (Id), baseSeq (UnsignedInt) and data (String) are required",
            ));
        }
        let node_id = Id::new_validated(m["nodeId"].as_str().unwrap_or_default())
            .map_err(|e| invalid(&["nodeId"], &e.to_string()))?;
        let base_seq = m["baseSeq"].as_u64().unwrap_or_default();
        let fail = |e: B::Error| jmap_server::server_fail_value_from_backend(&e);
        // ponytail: re-reads the edit capability per create; one call per
        // op, bounded by maxObjectsInSet.
        let cap = backend
            .graph_edit_capability(caller, account_id)
            .await
            .map_err(fail)?
            .ok_or_else(|| forbidden("the account does not support live editing"))?;

        let Some((node, may_write)) = editable_node(backend, caller, account_id, &node_id).await?
        else {
            return Err(invalid(
                &["nodeId"],
                "nodeId does not identify a node the user may read whose editFormat is not null",
            ));
        };
        if !may_write {
            return Err(forbidden("this requires mayWrite on the Graph"));
        }
        let head = backend
            .edit_log(caller, account_id, &node_id)
            .await
            .map_err(fail)?
            .head;
        if base_seq > head {
            return Err(invalid(
                &["baseSeq"],
                "baseSeq is greater than the head of the edit log",
            ));
        }
        let Some(len) = base64url_len(m["data"].as_str().unwrap_or_default()) else {
            return Err(invalid(&["data"], "data is not base64url without padding"));
        };
        if len as u64 > cap.max_op_size {
            return Err(set_error_value(
                &SetError::new(SetErrorType::TooLarge).with_description("data exceeds maxOpSize"),
            ));
        }

        // `id` is required by the type, so the parse uses "", which is not an
        // Id (RFC 8620 §1.2: "at least 1 and a maximum of 255 octets"); the
        // `create_object` builder overwrites it with the minted id.
        m.insert("id".to_owned(), json!(""));
        m.insert("graphId".to_owned(), json!(node.graph_id));
        m.insert("seq".to_owned(), json!(0));
        m.insert("created".to_owned(), json!(now_utc_string()));
        m.insert(
            "createdBy".to_owned(),
            json!(<B as JmapBackend>::principal_id(caller)),
        );
        let mut op: GraphNodeOp = serde_json::from_value(Value::Object(m))
            .map_err(|e| invalid(&[], &format!("invalid GraphNodeOp: {e}")))?;
        let (id, stored) = backend
            .create_object(caller, account_id, create_id, move |id: &Id| {
                op.id = id.clone();
                op
            })
            .await
            .map_err(backend_set_error_value)?;
        let stored = to_map(&stored);
        let mut created = Map::new();
        created.insert("id".to_owned(), json!(id));
        for k in GraphNodeOp::SERVER_SET.iter().filter(|k| **k != "id") {
            created.insert(
                (*k).to_owned(),
                stored.get(*k).cloned().unwrap_or(Value::Null),
            );
        }
        Ok(Value::Object(created))
    }

    async fn update(
        _backend: &B,
        _caller: &B::CallerCtx,
        _account_id: &Id,
        _cap: &GraphCapability,
        _id: &Id,
        _patch: Value,
    ) -> Result<Value, Value> {
        Err(immutable())
    }

    async fn destroy(
        _backend: &B,
        _caller: &B::CallerCtx,
        _account_id: &Id,
        _id: &Id,
    ) -> Result<(), Value> {
        Err(immutable())
    }
}

/// "ops change only by being sequenced and leave only through compaction
/// or when their node is destroyed."
fn immutable() -> Value {
    forbidden("ops change only by being sequenced and leave only through compaction")
}

/// Handle `GraphNode/cursor` (draft-atwood-jmap-graph-edit-00
/// §GraphNode/cursor), which "reports the calling user's cursor or
/// selection in a node to other users subscribed to it".
///
/// "It fails with an `invalidArguments` error ... if `nodeId` does not
/// identify a node the user may read whose `editFormat` is not `null`, if
/// `data` is not valid base64url, or if its decoded size exceeds
/// `maxCursorSize`; one error covers missing and unreadable nodes". The
/// accepted call becomes a [`GraphCursorEvent`] whose `senderId` is
/// [`JmapBackend::principal_id`], handed to
/// [`GraphBackend::publish_cursor`]. "The response has the arguments
/// `accountId` and `nodeId` only."
pub async fn handle_graph_node_cursor<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let cap = require_edit(backend, caller, &args).await?;
    let req = GraphNodeCursorRequest::deserialize(&args).map_err(|_| {
        JmapError::invalid_arguments("accountId, nodeId and data (String) are required")
    })?;
    if editable_node(backend, caller, &req.account_id, &req.node_id)
        .await
        .map_err(|_| JmapError::server_fail("backend error"))?
        .is_none()
    {
        return Err(JmapError::invalid_arguments(
            "nodeId does not identify a node the user may read whose editFormat is not null",
        ));
    }
    match base64url_len(&req.data) {
        None => {
            return Err(JmapError::invalid_arguments(
                "data is not base64url without padding",
            ));
        }
        Some(len) if len as u64 > cap.max_cursor_size => {
            return Err(JmapError::invalid_arguments("data exceeds maxCursorSize"));
        }
        Some(_) => {}
    }
    let event = GraphCursorEvent::new(
        req.account_id.clone(),
        req.node_id.clone(),
        <B as JmapBackend>::principal_id(caller).cloned(),
        req.data,
    );
    backend
        .publish_cursor(caller, &req.account_id, &event)
        .await;
    Ok(HandlerOutput::new(
        serde_json::to_value(GraphNodeCursorResponse::new(req.account_id, req.node_id))
            .map_err(|e| JmapError::server_fail(e.to_string()))?,
    ))
}
