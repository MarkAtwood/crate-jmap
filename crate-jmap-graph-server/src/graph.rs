//! Graph/* method handlers (draft-atwood-jmap-graph-00 §Graph Methods).
//!
//! `/get` hides a Graph without `mayRead`; `/changes` is the standard RFC
//! 8620 §5.2 method; `/query` and `/queryChanges` refuse a sort property
//! the draft does not list. `/set` applies the rights of §Graph/set and
//! the `annotations` rules; the backend cascades a destroy to the Graph's
//! nodes and edges.

use std::collections::BTreeMap;

use jmap_graph_types::GraphComparator;
use jmap_graph_types::{Graph, GraphCapability, GraphRights};
use jmap_types::{Id, JmapError};
use serde_json::{json, Map, Value};

use crate::helpers::{
    backend_set_error_value, check_annotations, decode_patch, fetch_one, forbidden,
    handle_get_visible, handle_set, invalid, mistyped, not_found, patched, patched_properties,
    reject_unsupported_sort, set_error_value, to_map, SetOps, TypeCheck,
};
use crate::GraphBackend;
use jmap_server::{now_utc_string, reject_read_only_patch, reject_server_set, HandlerOutput};

/// The properties with a draft default, echoed in `created` when the
/// client omits them (RFC 8620 §5.3).
const DEFAULTED: &[&str] = &["name", "annotations", "isSubscribed", "shareWith"];

fn share_with_ok(v: &Value) -> bool {
    v.is_null() || serde_json::from_value::<BTreeMap<Id, GraphRights>>(v.clone()).is_ok()
}

/// `name` (String), `isSubscribed` (Boolean), `shareWith`
/// (Id[GraphRights]|null).
const TYPES: &[TypeCheck] = &[
    ("name", Value::is_string),
    ("isSubscribed", Value::is_boolean),
    ("shareWith", share_with_ok),
];

/// The shape checks a create and an update share, on the Graph as it would
/// be stored.
fn validate(m: &Map<String, Value>, cap: &GraphCapability) -> Result<(), Value> {
    let bad = mistyped(m, TYPES);
    if !bad.is_empty() {
        return Err(invalid(
            &bad,
            "name is a String, isSubscribed a Boolean, shareWith an Id[GraphRights] or null",
        ));
    }
    check_annotations(m, cap)
}

/// Handle `Graph/get` (draft-atwood-jmap-graph-00 §Graph/get): standard
/// RFC 8620 §5.1 `/get`, with a Graph the caller may not read reported in
/// `notFound` (§Access Control: "`notFound` on `/get`").
pub async fn handle_graph_get<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_get_visible::<Graph, B>(backend, caller, args).await
}

/// Handle `Graph/changes`: standard RFC 8620 §5.2 `/changes`.
pub async fn handle_graph_changes<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<Graph, B>(backend, caller, args).await
}

/// Handle `Graph/query` (draft-atwood-jmap-graph-00 §Graph/query): "The
/// following sort properties are supported: `name`, `created`,
/// `updated`"; any other is `unsupportedSort`. The backend evaluates the
/// `isSubscribed` filter and omits Graphs the caller may not read;
/// [`crate::query::graph_matches`] and [`crate::query::compare_graphs`]
/// implement it.
pub async fn handle_graph_query<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    reject_unsupported_sort::<GraphComparator>(args.get("sort"))?;
    jmap_server::handle_query::<Graph, B>(backend, caller, args).await
}

/// Handle `Graph/queryChanges`: standard RFC 8620 §5.6, with the `/query`
/// sort check.
pub async fn handle_graph_query_changes<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    reject_unsupported_sort::<GraphComparator>(args.get("sort"))?;
    jmap_server::handle_query_changes::<Graph, B>(backend, caller, args).await
}

/// Handle `Graph/set` (draft-atwood-jmap-graph-00 §Graph/set).
///
/// "Any user with `mayRead` may change their own `isSubscribed` value.
/// Changing `name` or `annotations` requires `mayWrite`. Changing
/// `shareWith` ... requires `mayAdmin`. Destroying a Graph requires
/// `mayAdmin`. A user who has `mayRead` but lacks the needed right receives
/// a `forbidden` SetError; a user without `mayRead` receives `notFound`."
/// Annotation keys must be non-empty and `maxAnnotationSize` gives
/// `tooLarge`. A destroy also destroys the Graph's nodes and edges
/// ([`GraphBackend::destroy_object`]).
pub async fn handle_graph_set<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_set::<GraphSet, B>(backend, caller, args).await
}

struct GraphSet;

impl<B: GraphBackend> SetOps<B> for GraphSet {
    type Object = Graph;

    /// One create. `Ok` is the `created` entry: the server-set `id`,
    /// `created`, `updated` and `myRights`, and each defaulted property
    /// the client omitted.
    async fn create(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        cap: &GraphCapability,
        create_id: &str,
        obj: Value,
    ) -> Result<Value, Value> {
        let Value::Object(mut m) = obj else {
            return Err(invalid(&[], "a Graph create must be a JSON object"));
        };
        if let Some(e) = reject_server_set::<Graph>(&m) {
            return Err(set_error_value(
                &e.with_description("these properties are set by the server"),
            ));
        }
        validate(&m, cap)?;
        let omitted: Vec<&str> = DEFAULTED
            .iter()
            .copied()
            .filter(|k| !m.contains_key(*k))
            .collect();
        let now = now_utc_string();
        // `id` is required by the type, so the parse uses "", which is not an
        // Id (RFC 8620 §1.2: "at least 1 and a maximum of 255 octets"); the
        // `create_object` builder overwrites it with the minted id.
        m.insert("id".to_owned(), json!(""));
        m.insert("created".to_owned(), json!(now));
        m.insert("updated".to_owned(), json!(now));
        m.insert(
            "myRights".to_owned(),
            json!({ "mayRead": false, "mayWrite": false, "mayAdmin": false }),
        );
        let mut graph: Graph = serde_json::from_value(Value::Object(m))
            .map_err(|e| invalid(&[], &format!("invalid Graph: {e}")))?;
        let (id, stored) = backend
            .create_object(caller, account_id, create_id, move |id: &Id| {
                graph.id = id.clone();
                graph
            })
            .await
            .map_err(backend_set_error_value)?;
        let stored = to_map(&stored);
        let mut created = json!({
            "id": id,
            "created": stored["created"],
            "updated": stored["updated"],
            "myRights": stored["myRights"],
        });
        for k in omitted {
            created[k] = stored.get(k).cloned().unwrap_or(Value::Null);
        }
        Ok(created)
    }

    /// One update. `Ok` is the `updated` echo: the new `updated` when a
    /// property other than the caller's own `isSubscribed` changed ("When a
    /// property of the Graph object itself was last changed").
    async fn update(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        cap: &GraphCapability,
        id: &Id,
        patch: Value,
    ) -> Result<Value, Value> {
        let mut patch = decode_patch(patch)?;
        let graph = readable(backend, caller, account_id, id).await?;
        let keys = patched_properties(&patch);
        if let Some(e) = reject_read_only_patch::<Graph>(patch.as_map(), Some(&to_map(&graph))) {
            return Err(set_error_value(
                &e.with_description("these properties are set by the server"),
            ));
        }
        let r = &graph.my_rights;
        if keys.iter().any(|k| k == "shareWith") && !r.may_admin {
            return Err(forbidden("changing shareWith requires mayAdmin"));
        }
        if keys.iter().any(|k| k != "isSubscribed" && k != "shareWith") && !r.may_write {
            return Err(forbidden("changing name or annotations requires mayWrite"));
        }
        validate(&patched(&graph, &patch)?, cap)?;

        let mut echo = Value::Null;
        if keys.iter().any(|k| k != "isSubscribed") {
            let now = now_utc_string();
            patch.as_map_mut().insert("updated".to_owned(), json!(now));
            echo = json!({ "updated": now });
        }
        match backend
            .update_object::<Graph>(caller, account_id, id, patch)
            .await
            .map_err(backend_set_error_value)?
        {
            Some(g) => Ok(Value::Object(to_map(&g))),
            None => Ok(echo),
        }
    }

    /// One destroy: "Destroying a Graph requires `mayAdmin`."
    async fn destroy(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        id: &Id,
    ) -> Result<(), Value> {
        let graph = readable(backend, caller, account_id, id).await?;
        if !graph.my_rights.may_admin {
            return Err(forbidden("destroying a Graph requires mayAdmin"));
        }
        backend
            .destroy_object::<Graph>(caller, account_id, id)
            .await
            .map_err(backend_set_error_value)
    }
}

/// The Graph `id` an update or destroy acts on: `notFound` when it does
/// not exist or the caller lacks `mayRead` ("a user without `mayRead`
/// receives `notFound`, as if the Graph did not exist").
async fn readable<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<Graph, Value> {
    fetch_one::<Graph, B>(backend, caller, account_id, id)
        .await?
        .filter(|g| g.my_rights.may_read)
        .ok_or_else(not_found)
}
