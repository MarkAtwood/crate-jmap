//! Private helper utilities for the handler modules.

use std::collections::HashMap;

use jmap_graph_types::{Graph, GraphCapability, GraphEdge, GraphNode, GraphNodeOp, GraphRights};
use jmap_types::{GetObject, Id, JmapError, JmapObject, PatchObject, State};
use serde_json::{json, Map, Value};

use jmap_graph_types::CandidateKind;

use crate::search::{check_search_filter, uses_search};
use crate::{BackendSetError, GraphBackend, QueryObject, SetError, SetErrorType};
use jmap_server::{
    enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id, not_found_json,
    optional_arg, serialize_value, server_fail_from_backend, server_fail_value_from_backend,
    HandlerOutput,
};

/// Serialize a [`SetError`] for a `notCreated` / `notUpdated` /
/// `notDestroyed` map.
///
/// `SetError` is plain data with a derived `Serialize`, so the `.expect`
/// is unreachable.
pub(crate) fn set_error_value(e: &SetError) -> Value {
    serde_json::to_value(e).expect("derive(Serialize) on plain data is infallible")
}

/// An `invalidProperties` SetError naming `properties` (none when empty).
pub(crate) fn invalid(properties: &[&str], description: &str) -> Value {
    let err = SetError::new(SetErrorType::InvalidProperties).with_description(description);
    set_error_value(&if properties.is_empty() {
        err
    } else {
        err.with_properties(properties.iter().copied())
    })
}

/// Check the arguments of a GraphNode (`kind` [`CandidateKind::Node`]) or
/// GraphEdge `/query` or `/queryChanges` and return whether the filter uses
/// draft-atwood-jmap-graph-search-00.
///
/// "The `filter` argument MUST be a single FilterCondition, not a
/// FilterOperator, and MUST include `graphId`. A server receiving any other
/// filter, or no filter, rejects the call with an `unsupportedFilter`
/// error" (draft-atwood-jmap-graph-00 §GraphNode/query). A FilterOperator
/// or a condition without `graphId` fails the typed decode, which the
/// generic handler reports as `unsupportedFilter`. The sort is checked with
/// [`reject_unsupported_sort`].
///
/// The `where`, `language` and `expression` properties are
/// `unsupportedFilter`, not silently ignored, unless the account has
/// [`GraphBackend::graph_search_capability`]; then they are checked with
/// [`check_search_filter`].
async fn graph_query_args<C: serde::de::DeserializeOwned + KnownSort, B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: &Value,
    kind: CandidateKind,
) -> Result<bool, JmapError> {
    let Some(filter) = args.get("filter").filter(|f| !f.is_null()) else {
        return Err(JmapError::unsupported_filter());
    };
    reject_unsupported_sort::<C>(args.get("sort"))?;
    if !uses_search(filter) {
        return Ok(false);
    }
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
    let cap = backend
        .graph_search_capability(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
        .ok_or_else(JmapError::unsupported_filter)?;
    check_search_filter(filter, kind, &cap)?;
    Ok(true)
}

/// GraphNode or GraphEdge `/query` (`O`, sorted by `C`, candidates of
/// `kind`) after [`graph_query_args`]. A search result says
/// `canCalculateChanges: false`, since its `/queryChanges` is
/// `cannotCalculateChanges`.
pub(crate) async fn handle_graph_item_query<O, C, B>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    kind: CandidateKind,
) -> Result<HandlerOutput, JmapError>
where
    O: QueryObject + Send + Sync,
    C: serde::de::DeserializeOwned + KnownSort,
    B: GraphBackend,
{
    let searched = graph_query_args::<C, B>(backend, caller, &args, kind).await?;
    let mut out = jmap_server::handlers::handle_query::<O, B>(backend, caller, args).await?;
    if searched {
        out.primary["canCalculateChanges"] = Value::Bool(false);
    }
    Ok(out)
}

/// GraphNode or GraphEdge `/queryChanges` after [`graph_query_args`]: a
/// filter using draft-atwood-jmap-graph-search-00 is
/// `cannotCalculateChanges` ("Servers MAY return `cannotCalculateChanges`
/// for any `/queryChanges` call whose filter uses a property defined in
/// this document", §Query Changes).
pub(crate) async fn handle_graph_item_query_changes<O, C, B>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    kind: CandidateKind,
) -> Result<HandlerOutput, JmapError>
where
    O: QueryObject + Send + Sync,
    C: serde::de::DeserializeOwned + KnownSort,
    B: GraphBackend,
{
    if graph_query_args::<C, B>(backend, caller, &args, kind).await? {
        return Err(JmapError::cannot_calculate_changes());
    }
    jmap_server::handlers::handle_query_changes::<O, B>(backend, caller, args).await
}

/// A `forbidden` SetError.
pub(crate) fn forbidden(description: &str) -> Value {
    set_error_value(&SetError::new(SetErrorType::Forbidden).with_description(description))
}

/// A `notFound` SetError.
pub(crate) fn not_found() -> Value {
    set_error_value(&SetError::new(SetErrorType::NotFound))
}

/// An `invalidPatch` SetError.
pub(crate) fn invalid_patch(description: &str) -> Value {
    set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(description))
}

/// The SetError JSON for a backend `/set` failure; storage errors become
/// `serverFail` without their `Display` text.
pub(crate) fn backend_set_error_value<E: std::fmt::Display>(e: BackendSetError<E>) -> Value {
    match e {
        BackendSetError::SetError(se) => set_error_value(&se),
        BackendSetError::Other(e) => server_fail_value_from_backend(&e),
        _ => json!({
            "type": "serverFail",
            "description": "unhandled backend error variant",
        }),
    }
}

/// `o` as a JSON object.
pub(crate) fn to_map(o: &impl serde::Serialize) -> Map<String, Value> {
    match serde_json::to_value(o).expect("derive(Serialize) on plain data is infallible") {
        Value::Object(m) => m,
        _ => unreachable!("a Graph data type serializes as a JSON object"),
    }
}

/// Fetch one object of type `O`, or `None` if the backend does not have it.
pub(crate) async fn fetch_one<O: GetObject + Send + Sync, B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<Option<O>, Value> {
    backend
        .get_objects::<O>(caller, account_id, Some(std::slice::from_ref(id)), None)
        .await
        .map(|(found, _)| found.into_iter().next())
        .map_err(|e| server_fail_value_from_backend(&e))
}

/// The caller's rights on Graph `graph_id`, or `None` when the Graph does
/// not exist or the caller lacks `mayRead`: "A Graph the user may not read
/// SHOULD be indistinguishable from one that does not exist".
pub(crate) async fn readable_rights<B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    graph_id: &Id,
) -> Result<Option<GraphRights>, Value> {
    Ok(fetch_one::<Graph, B>(backend, caller, account_id, graph_id)
        .await?
        .map(|g| g.my_rights)
        .filter(|r| r.may_read))
}

/// A Graph data type: an object reached through the rights of one Graph.
pub(crate) trait InGraph: GetObject + Send + Sync {
    /// The record's `id`.
    fn id(&self) -> &Id;

    /// The Graph whose rights cover this record.
    fn graph_id(&self) -> &Id;

    /// The caller's rights when the record carries them (a Graph does).
    fn own_rights(&self) -> Option<&GraphRights> {
        None
    }
}

impl InGraph for Graph {
    fn id(&self) -> &Id {
        &self.id
    }

    fn graph_id(&self) -> &Id {
        &self.id
    }

    fn own_rights(&self) -> Option<&GraphRights> {
        Some(&self.my_rights)
    }
}

impl InGraph for GraphNode {
    fn id(&self) -> &Id {
        &self.id
    }

    fn graph_id(&self) -> &Id {
        &self.graph_id
    }
}

impl InGraph for GraphNodeOp {
    fn id(&self) -> &Id {
        &self.id
    }

    fn graph_id(&self) -> &Id {
        &self.graph_id
    }
}

impl InGraph for GraphEdge {
    fn id(&self) -> &Id {
        &self.id
    }

    fn graph_id(&self) -> &Id {
        &self.graph_id
    }
}

/// RFC 8620 §5.1 `/get` of `O` that reports a record in a Graph the caller
/// may not read exactly as a missing one: "`notFound` on `/get`"
/// (draft-atwood-jmap-graph-00 §Access Control). A requested id goes to
/// `notFound` in request order; a full fetch (`ids` `null`) leaves it out.
///
/// ponytail: one Graph lookup per distinct `graphId` returned, and an
/// O(ids × records) `notFound` scan bounded by `maxObjectsInGet`.
pub(crate) async fn handle_get_visible<O: InGraph, B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    enforce_max_objects_in_get(&args, backend.max_objects_in_get(caller, &account_id))?;
    let ids: Option<Vec<Id>> = optional_arg(&mut args, "ids", || {
        JmapError::invalid_arguments("ids must be an Id array")
    })?;
    let properties: Option<Vec<String>> = optional_arg(&mut args, "properties", || {
        JmapError::invalid_arguments("properties must be a string array")
    })?;

    let (fetched, _) = backend
        .get_objects::<O>(caller, &account_id, ids.as_deref(), properties.as_deref())
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let mut readable: HashMap<Id, bool> = HashMap::new();
    let mut list = Vec::with_capacity(fetched.len());
    for o in fetched {
        let may_read = match o.own_rights() {
            Some(r) => r.may_read,
            None => match readable.get(o.graph_id()) {
                Some(&r) => r,
                None => {
                    let r = readable_rights(backend, caller, &account_id, o.graph_id())
                        .await
                        .map_err(|_| JmapError::server_fail("backend error"))?
                        .is_some();
                    readable.insert(o.graph_id().clone(), r);
                    r
                }
            },
        };
        if may_read {
            list.push(o);
        }
    }
    let not_found: Vec<Id> = ids
        .unwrap_or_default()
        .into_iter()
        .filter(|id| !list.iter().any(|o| o.id() == id))
        .collect();

    let state = backend
        .get_state::<O>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": serialize_value(&list)?,
        "notFound": not_found_json(&not_found),
    })))
}

/// Reject a `/query` `sort` property the draft does not list: RFC 8620
/// §5.5 `unsupportedSort`. A value that is not an array is left for the
/// typed decode to reject.
pub(crate) fn reject_unsupported_sort<C: serde::de::DeserializeOwned + KnownSort>(
    sort: Option<&Value>,
) -> Result<(), JmapError> {
    if let Some(Value::Array(comparators)) = sort {
        for c in comparators {
            if !<C as serde::Deserialize>::deserialize(c).is_ok_and(|c| c.known()) {
                return Err(JmapError::unsupported_sort());
            }
        }
    }
    Ok(())
}

/// A comparator whose `property` decodes an unknown name to `Other`
/// (bd JMAP-wf92.11), so [`reject_unsupported_sort`] must check it.
pub(crate) trait KnownSort {
    /// `false` when `property` is `Other`, a name the draft does not list.
    fn known(&self) -> bool;
}

macro_rules! known_sort {
    ($($cmp:ty => $prop:path),+ $(,)?) => {$(
        impl KnownSort for $cmp {
            fn known(&self) -> bool {
                !matches!(self.property, $prop(_))
            }
        }
    )+};
}

known_sort!(
    jmap_graph_types::GraphComparator => jmap_graph_types::GraphSortProperty::Other,
    jmap_graph_types::GraphNodeComparator => jmap_graph_types::GraphNodeSortProperty::Other,
    jmap_graph_types::GraphEdgeComparator => jmap_graph_types::GraphEdgeSortProperty::Other,
);

/// The `annotations` checks of a Graph, GraphNode or GraphEdge as it would
/// be stored: "Annotation keys are non-empty strings" (`invalidProperties`
/// naming `annotations`), and "A create or update that would exceed the
/// limit is rejected with a `tooLarge` SetError" for `maxAnnotationSize`,
/// "measured as UTF-8 encoded JSON".
///
/// ponytail: the size is that of the compact serialization; a client that
/// counted its own pretty-printed JSON sees a smaller number here.
pub(crate) fn check_annotations(
    m: &Map<String, Value>,
    cap: &GraphCapability,
) -> Result<(), Value> {
    let Some(a) = m.get("annotations") else {
        return Ok(());
    };
    let Some(map) = a.as_object() else {
        return Err(invalid(
            &["annotations"],
            "annotations must be a JSON object",
        ));
    };
    if map.contains_key("") {
        return Err(invalid(
            &["annotations"],
            "annotation keys are non-empty strings",
        ));
    }
    let size = serde_json::to_vec(a).map_or(usize::MAX, |v| v.len()) as u64;
    if cap.max_annotation_size.is_some_and(|max| size > max) {
        return Err(set_error_value(
            &SetError::new(SetErrorType::TooLarge)
                .with_description("annotations exceed maxAnnotationSize"),
        ));
    }
    Ok(())
}

/// `true` if `s` is an RFC 3986 §3 URI: `scheme ":" hier-part [ "?" query ]
/// [ "#" fragment ]`, with `scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" /
/// "." )`.
///
/// ponytail: after the scheme this checks the character set only (the
/// unreserved and reserved characters and `%` followed by two hex digits),
/// not the hier-part grammar; a full RFC 3986 parser is the upgrade path
/// if a stricter check is ever needed.
pub(crate) fn is_uri(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once(':') else {
        return false;
    };
    let mut sc = scheme.bytes();
    if !sc.next().is_some_and(|c| c.is_ascii_alphabetic())
        || !sc.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
    {
        return false;
    }
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                if !(b.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                    && b.get(i + 2).is_some_and(u8::is_ascii_hexdigit))
                {
                    return false;
                }
                i += 3;
            }
            c if c.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=".contains(&c) => i += 1,
            _ => return false,
        }
    }
    true
}

/// The property names in `keys` that `forbidden_keys` lists, sorted.
pub(crate) fn listed_in<'a>(
    keys: impl Iterator<Item = &'a str>,
    forbidden_keys: &[&str],
) -> Vec<&'a str> {
    let mut found: Vec<&str> = keys.filter(|k| forbidden_keys.contains(k)).collect();
    found.sort_unstable();
    found.dedup();
    found
}

/// The top-level properties a PatchObject touches: `annotations/x` touches
/// `annotations`.
pub(crate) fn patched_properties(patch: &PatchObject) -> Vec<String> {
    let mut keys: Vec<String> = patch
        .as_map()
        .keys()
        .map(|k| k.split('/').next().unwrap_or(k).to_owned())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// Decode a `/set` `update` value as a PatchObject (`invalidPatch`
/// otherwise).
pub(crate) fn decode_patch(patch: Value) -> Result<PatchObject, Value> {
    serde_json::from_value(patch).map_err(|e| invalid_patch(&e.to_string()))
}

/// Apply `patch` to `object`, for validating the object as it would be
/// stored.
pub(crate) fn patched(
    object: &impl serde::Serialize,
    patch: &PatchObject,
) -> Result<Map<String, Value>, Value> {
    let mut m = to_map(object);
    jmap_server::apply_patch_object(&mut m, patch.as_map())
        .map_err(|e| invalid_patch(&e.to_string()))?;
    Ok(m)
}

/// A property name and the JSON type check its value must pass.
pub(crate) type TypeCheck = (&'static str, fn(&Value) -> bool);

/// The properties of `m` that are not of the JSON type the draft gives
/// them: each `(key, check)` must hold when `key` is present.
pub(crate) fn mistyped(m: &Map<String, Value>, checks: &[TypeCheck]) -> Vec<&'static str> {
    checks
        .iter()
        .filter(|(k, ok)| m.get(*k).is_some_and(|v| !ok(v)))
        .map(|(k, _)| *k)
        .collect()
}

/// `String|null`.
pub(crate) fn string_or_null(v: &Value) -> bool {
    v.is_string() || v.is_null()
}

/// The `/set` operations of one Graph data type, run by [`handle_set`].
///
/// Each returns the `created` or `updated` entry, or the SetError JSON.
pub(crate) trait SetOps<B: GraphBackend> {
    /// The data type.
    type Object: JmapObject + Send + Sync;

    /// One create.
    fn create(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        cap: &GraphCapability,
        create_id: &str,
        obj: Value,
    ) -> impl std::future::Future<Output = Result<Value, Value>> + Send;

    /// One update.
    fn update(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        cap: &GraphCapability,
        id: &Id,
        patch: Value,
    ) -> impl std::future::Future<Output = Result<Value, Value>> + Send;

    /// One destroy.
    fn destroy(
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
        id: &Id,
    ) -> impl std::future::Future<Output = Result<(), Value>> + Send;
}

/// RFC 8620 §5.3 `/set`: `ifInState`, then each create, update and
/// destroy through `S`, then the response with `newState` re-read when
/// anything changed. Empty maps and arrays serialize as `null`.
pub(crate) async fn handle_set<S: SetOps<B>, B: GraphBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: an unknown accountId is `accountNotFound`.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state: State = backend
        .get_state::<S::Object>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    if let Some(if_in_state) = args.get("ifInState").and_then(Value::as_str) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }
    let cap = backend
        .graph_capability(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let mut created = Map::new();
    let mut updated = Map::new();
    let mut destroyed = Vec::new();
    let mut not_created = Map::new();
    let mut not_updated = Map::new();
    let mut not_destroyed = Map::new();

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj) in create_map {
            match S::create(backend, caller, &account_id, &cap, &create_id, obj).await {
                Ok(v) => created.insert(create_id, v),
                Err(e) => not_created.insert(create_id, e),
            };
        }
    }
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch) in update_map {
            let id = Id::from(id_str.as_str());
            match S::update(backend, caller, &account_id, &cap, &id, patch).await {
                Ok(v) => updated.insert(id_str, v),
                Err(e) => not_updated.insert(id_str, e),
            };
        }
    }
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of `destroy` is an Id.
        let ids: Vec<&str> = destroy_arr
            .iter()
            .map(|v| v.as_str().ok_or(v))
            .collect::<Result<_, _>>()
            .map_err(|bad| {
                JmapError::invalid_arguments(format!(
                    "destroy: every element must be a string Id; got {bad}"
                ))
            })?;
        for id_str in ids {
            match S::destroy(backend, caller, &account_id, &Id::from(id_str)).await {
                Ok(()) => destroyed.push(Value::String(id_str.to_owned())),
                Err(e) => {
                    not_destroyed.insert(id_str.to_owned(), e);
                }
            }
        }
    }

    let new_state = if created.is_empty() && updated.is_empty() && destroyed.is_empty() {
        old_state.clone()
    } else {
        backend
            .get_state::<S::Object>(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
    };
    let obj = |m: Map<String, Value>| {
        if m.is_empty() {
            Value::Null
        } else {
            Value::Object(m)
        }
    };
    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": obj(created),
        "updated": obj(updated),
        "destroyed": if destroyed.is_empty() { Value::Null } else { Value::Array(destroyed) },
        "notCreated": obj(not_created),
        "notUpdated": obj(not_updated),
        "notDestroyed": obj(not_destroyed),
    })))
}
