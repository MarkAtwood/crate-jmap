//! GraphNode/* method handlers (draft-atwood-jmap-graph-00 §GraphNode
//! Methods).
//!
//! `/get` hides the nodes of a Graph the caller may not read; `/changes` is
//! the standard RFC 8620 §5.2 method; `/query` and `/queryChanges` require
//! a single FilterCondition with `graphId`. `/set` requires `mayWrite` on
//! the node's Graph, validates `graphId` and `uri`, and leaves the edge
//! cascade of a destroy to the backend.

use jmap_graph_types::edit::{GRAPH_NODE_EDIT_FORMAT_PROPERTY, GRAPH_NODE_SNAPSHOT_SEQ_PROPERTY};
use jmap_graph_types::query::GraphNodeComparator;
use jmap_graph_types::{GraphCapability, GraphNode, GraphRights};
use jmap_types::{Id, JmapError};
use serde_json::{json, Map, Value};

use crate::backend::{EditLog, GraphBackend, JmapBackend};
use crate::edit::check_node_edit;
use crate::helpers::{
    backend_set_error_value, check_annotations, decode_patch, fetch_one, forbidden,
    handle_get_visible, handle_set, invalid, is_uri, listed_in, mistyped, not_found, patched,
    patched_properties, readable_rights, require_graph_filter, set_error_value, string_or_null,
    to_map, SetOps, TypeCheck, PLACEHOLDER_ID,
};
use jmap_server::{now_utc_string, HandlerOutput};

/// The server-set properties of a GraphNode and a GraphEdge (`createdBy`,
/// `updatedBy` and the timestamps; `id`).
pub(crate) const SERVER_SET: &[&str] = &["id", "created", "updated", "createdBy", "updatedBy"];

/// [`SERVER_SET`] plus the immutable `graphId`, refused in an update.
pub(crate) const IMMUTABLE: &[&str] = &[
    "id",
    "graphId",
    "created",
    "updated",
    "createdBy",
    "updatedBy",
];

/// The properties with a draft default, echoed in `created` when the
/// client omits them (RFC 8620 §5.3).
const DEFAULTED: &[&str] = &["annotations", "blobId", "mediaType", "uri"];

/// `blobId`, `mediaType` and `uri` are `T|null` strings.
const TYPES: &[TypeCheck] = &[
    ("blobId", string_or_null),
    ("mediaType", string_or_null),
    ("uri", string_or_null),
];

/// The shape checks a create and an update share, on the node as it would
/// be stored. "The server rejects a value that is not a URI with an
/// `invalidProperties` SetError naming `uri`, and does not otherwise
/// interpret it."
fn validate(m: &Map<String, Value>, cap: &GraphCapability) -> Result<(), Value> {
    let bad = mistyped(m, TYPES);
    if !bad.is_empty() {
        return Err(invalid(
            &bad,
            "blobId, mediaType and uri are Strings or null",
        ));
    }
    if m.get("uri")
        .and_then(Value::as_str)
        .is_some_and(|u| !is_uri(u))
    {
        return Err(invalid(&["uri"], "uri is not a URI (RFC 3986)"));
    }
    check_annotations(m, cap)
}

/// Apply the draft-atwood-jmap-graph-edit-00 §GraphNode Extensions rules
/// to `next`, the node as it would be stored, when the account advertises
/// `urn:ietf:params:jmap:graph:edit` ([`GraphBackend::graph_edit_capability`]);
/// otherwise accept it unchanged. `node_id` is `None` on a create, whose
/// edit log is empty. See [`check_node_edit`].
#[allow(clippy::too_many_arguments)]
async fn check_edit<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    graph_id: &Id,
    node_id: Option<&Id>,
    current: Option<&Map<String, Value>>,
    next: &Map<String, Value>,
    sets_snapshot_seq: bool,
) -> Result<bool, Value> {
    let fail = |e: B::Error| jmap_server::server_fail_value_from_backend(&e);
    if backend
        .graph_edit_capability(caller, account_id)
        .await
        .map_err(fail)?
        .is_none()
    {
        return Ok(false);
    }
    let log = match node_id {
        Some(id) => backend
            .edit_log(caller, account_id, id)
            .await
            .map_err(fail)?,
        None => EditLog::default(),
    };
    let projected = backend
        .projected_graph_id(caller, account_id)
        .await
        .map_err(fail)?
        .is_some_and(|g| g == *graph_id);
    check_node_edit(current, next, sets_snapshot_seq, log.head, projected)
        .map_err(|e| set_error_value(&e))?;
    Ok(true)
}

/// The `graphId` of create `m`, required, and the caller's rights on it:
/// "If it does not identify a Graph the user may read, the server rejects
/// the create with an `invalidProperties` SetError naming `graphId`";
/// "Creating ... requires `mayWrite` on its Graph" (`forbidden`).
pub(crate) async fn create_graph_id<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    m: &Map<String, Value>,
) -> Result<Id, Value> {
    let Some(graph_id) = m.get("graphId").and_then(Value::as_str).map(Id::from) else {
        return Err(invalid(&["graphId"], "graphId is required"));
    };
    let rights = readable_rights(backend, caller, account_id, &graph_id)
        .await?
        .ok_or_else(|| {
            invalid(
                &["graphId"],
                "graphId does not identify a Graph the user may read",
            )
        })?;
    require_write(&rights)?;
    Ok(graph_id)
}

/// The caller's rights on `graph_id` for an update or destroy of an item
/// in it: `notFound` without `mayRead` (draft §Access Control: "`notFound`
/// on ... `/set` update or destroy"), `forbidden` without `mayWrite`.
pub(crate) async fn item_graph_writable<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    graph_id: &Id,
) -> Result<(), Value> {
    let rights = readable_rights(backend, caller, account_id, graph_id)
        .await?
        .ok_or_else(not_found)?;
    require_write(&rights)
}

fn require_write(rights: &GraphRights) -> Result<(), Value> {
    if rights.may_write {
        Ok(())
    } else {
        Err(forbidden("this requires mayWrite on the Graph"))
    }
}

/// Fill the server-set properties of create `m`: `id` (a placeholder),
/// `created`, `updated`, and `createdBy`/`updatedBy` from
/// [`JmapBackend::principal_id`] ("or `null` if the server does not
/// support Principals or cannot determine it").
pub(crate) fn fill_server_set<B: GraphBackend>(caller: &B::CallerCtx, m: &mut Map<String, Value>) {
    let now = now_utc_string();
    let principal = json!(<B as JmapBackend>::principal_id(caller));
    m.insert("id".to_owned(), json!(PLACEHOLDER_ID));
    m.insert("created".to_owned(), json!(now));
    m.insert("updated".to_owned(), json!(now));
    m.insert("createdBy".to_owned(), principal.clone());
    m.insert("updatedBy".to_owned(), principal);
}

/// The `updated` and `updatedBy` an update sets, added to `patch` and
/// returned as the `updated` echo.
pub(crate) fn touch<B: GraphBackend>(
    caller: &B::CallerCtx,
    patch: &mut jmap_types::PatchObject,
) -> Value {
    let echo = json!({
        "updated": now_utc_string(),
        "updatedBy": <B as JmapBackend>::principal_id(caller),
    });
    let map = patch.as_map_mut();
    map.insert("updated".to_owned(), echo["updated"].clone());
    map.insert("updatedBy".to_owned(), echo["updatedBy"].clone());
    echo
}

/// The `created` entry for `stored`: the server-set properties and each
/// property in `omitted` (RFC 8620 §5.3: "any properties that were omitted
/// or not set to the requested value by the server").
pub(crate) fn created_entry(id: &Id, stored: &impl serde::Serialize, omitted: &[&str]) -> Value {
    let stored = to_map(stored);
    let mut created = json!({ "id": id });
    for k in SERVER_SET[1..].iter().chain(omitted) {
        created[*k] = stored.get(*k).cloned().unwrap_or(Value::Null);
    }
    created
}

/// Handle `GraphNode/get`: standard RFC 8620 §5.1 `/get`, with the nodes of
/// a Graph the caller may not read reported in `notFound`.
pub async fn handle_graph_node_get<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_get_visible::<GraphNode, B>(backend, caller, args).await
}

/// Handle `GraphNode/changes`: standard RFC 8620 §5.2 `/changes`.
pub async fn handle_graph_node_changes<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_changes::<GraphNode, B>(backend, caller, args).await
}

/// Handle `GraphNode/query` (draft-atwood-jmap-graph-00 §GraphNode/query):
/// no filter, a FilterOperator or a condition without `graphId` is
/// `unsupportedFilter`; a sort property other than `created` and `updated`
/// is `unsupportedSort`. The backend omits the nodes of a Graph the caller
/// may not read.
pub async fn handle_graph_node_query<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args = require_graph_filter::<GraphNodeComparator>(args)?;
    jmap_server::handlers::handle_query::<GraphNode, B>(backend, caller, args).await
}

/// Handle `GraphNode/queryChanges`: standard RFC 8620 §5.6, with the
/// `/query` filter and sort rules.
pub async fn handle_graph_node_query_changes<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args = require_graph_filter::<GraphNodeComparator>(args)?;
    jmap_server::handlers::handle_query_changes::<GraphNode, B>(backend, caller, args).await
}

/// Handle `GraphNode/set` (draft-atwood-jmap-graph-00 §GraphNode/set).
///
/// "Creating, updating, or destroying a node requires `mayWrite` on its
/// Graph." On create "`graphId` is required"; `graphId` is immutable. A
/// destroy also destroys the node's edges
/// ([`GraphBackend::destroy_object`]).
///
/// When the account has the `urn:ietf:params:jmap:graph:edit` capability,
/// the `editFormat` and `snapshotSeq` extensions default to `null` and
/// follow draft-atwood-jmap-graph-edit-00 §GraphNode Extensions and
/// §Snapshots against the head of the node's edit log
/// ([`GraphBackend::edit_log`]).
pub async fn handle_graph_node_set<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_set::<NodeSet, B>(backend, caller, args).await
}

struct NodeSet;

impl<B: GraphBackend> SetOps<B> for NodeSet {
    type Object = GraphNode;

    async fn create(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        cap: &GraphCapability,
        create_id: &str,
        obj: Value,
    ) -> Result<Value, Value> {
        let Value::Object(mut m) = obj else {
            return Err(invalid(&[], "a GraphNode create must be a JSON object"));
        };
        let fixed = listed_in(m.keys().map(String::as_str), SERVER_SET);
        if !fixed.is_empty() {
            return Err(invalid(&fixed, "these properties are set by the server"));
        }
        validate(&m, cap)?;
        let graph_id = create_graph_id(backend, caller, account_id, &m).await?;
        let sets_snapshot_seq = m.contains_key(GRAPH_NODE_SNAPSHOT_SEQ_PROPERTY);
        let edit = check_edit(
            backend,
            caller,
            account_id,
            &graph_id,
            None,
            None,
            &m,
            sets_snapshot_seq,
        )
        .await?;
        // With the edit capability, both extensions default to `null`
        // (draft-atwood-jmap-graph-edit-00 §GraphNode Extensions) and are
        // stored and echoed like the graph-00 defaults.
        let edit_defaults: &[&str] = if edit {
            &[
                GRAPH_NODE_EDIT_FORMAT_PROPERTY,
                GRAPH_NODE_SNAPSHOT_SEQ_PROPERTY,
            ]
        } else {
            &[]
        };
        let omitted: Vec<&str> = DEFAULTED
            .iter()
            .chain(edit_defaults)
            .copied()
            .filter(|k| !m.contains_key(*k))
            .collect();
        for k in edit_defaults {
            m.entry(*k).or_insert(Value::Null);
        }
        fill_server_set::<B>(caller, &mut m);
        let node: GraphNode = serde_json::from_value(Value::Object(m))
            .map_err(|e| invalid(&[], &format!("invalid GraphNode: {e}")))?;
        let (id, stored) = backend
            .create_object(caller, account_id, create_id, node)
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
        let node: GraphNode = fetch_one(backend, caller, account_id, id)
            .await?
            .ok_or_else(not_found)?;
        item_graph_writable(backend, caller, account_id, &node.graph_id).await?;
        let keys = patched_properties(&patch);
        let fixed = listed_in(keys.iter().map(String::as_str), IMMUTABLE);
        if !fixed.is_empty() {
            return Err(invalid(
                &fixed,
                "these properties are immutable or server-set",
            ));
        }
        let next = patched(&node, &patch)?;
        validate(&next, cap)?;
        check_edit(
            backend,
            caller,
            account_id,
            &node.graph_id,
            Some(id),
            Some(&to_map(&node)),
            &next,
            keys.iter().any(|k| k == GRAPH_NODE_SNAPSHOT_SEQ_PROPERTY),
        )
        .await?;
        let echo = touch::<B>(caller, &mut patch);
        match backend
            .update_object::<GraphNode>(caller, account_id, id, patch)
            .await
            .map_err(backend_set_error_value)?
        {
            Some(n) => Ok(Value::Object(to_map(&n))),
            None => Ok(echo),
        }
    }

    async fn destroy(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), Value> {
        let node: GraphNode = fetch_one(backend, caller, account_id, id)
            .await?
            .ok_or_else(not_found)?;
        item_graph_writable(backend, caller, account_id, &node.graph_id).await?;
        backend
            .destroy_object::<GraphNode>(caller, account_id, id)
            .await
            .map_err(backend_set_error_value)
    }
}
