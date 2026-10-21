//! SceneObject/* method handlers (draft-atwood-jmap-scene-00 §SceneObject
//! Methods).
//!
//! `/get`, `/changes` and `/queryChanges` are the standard RFC 8620 §5.1,
//! §5.2 and §5.6 methods. `/set` applies §Creating an Object, §Updating an
//! Object, §Destroying an Object and §SetError Conditions: `mayCreateObject`,
//! the required `regionId` and `position`, the coordinate, `physicsMode`
//! and `visualRef`/`visualType` rules, the region-scoped, acyclic,
//! depth-limited scene graph, the immutable `regionId` and the
//! owner/region-owner/admin rule on update and destroy. `/query` supplies
//! the "Default sort: `createdAt` ascending" and the §Filter Validation
//! `invalidArguments` checks.

use std::collections::HashMap;

use jmap_scene_types::query::SceneObjectComparator;
use jmap_scene_types::{SceneObject, SceneRegion, IDENTITY_ORIENTATION};
use jmap_types::{Id, JmapError, PatchObject};
use serde_json::{json, Map, Value};

use crate::asset::GLTF_BINARY;
use crate::backend::SceneBackend;
use crate::helpers::{
    backend_set_error_value, check_region_filter, default_sort, enforce_max_objects_in_set,
    extract_account_id, fetch_one, finalize_set_response, finite3, forbidden, handle_get_visible,
    invalid, is_visible, normalized_orientation, not_found, orientation_ok,
    reject_unsupported_sort, set_error_value, SetAccumulators, PLACEHOLDER_ID,
};
use jmap_server::{
    apply_patch_object, now_utc_string, server_fail_from_backend, server_fail_value_from_backend,
    HandlerOutput, SetError, SetErrorType,
};

/// "The server sets `id`, `ownerId`, `createdAt`, and `updatedAt`." None
/// of them may be set or patched.
const SERVER_SET: &[&str] = &["id", "ownerId", "createdAt", "updatedAt"];

/// "`physicsMode` is not one of `\"static\"`, `\"dynamic\"`,
/// `\"kinematic\"`, `\"none\"`" is `invalidProperties` in `/set` and
/// `invalidArguments` in a `/query` filter.
const PHYSICS_MODES: &[&str] = &["static", "dynamic", "kinematic", "none"];

/// ponytail: "Servers MAY impose a maximum hierarchy depth"; this one is
/// fixed at the 16 levels the implementer guide calls "generous for any
/// practical scene graph" (jmap-scene-implementer-guide L339-342). A root
/// object is at depth 1. A deployment that wants another limit registers
/// its own `SceneObject/set`.
const MAX_DEPTH: usize = 16;

/// Handle `SceneObject/get` (draft-atwood-jmap-scene-00 §SceneObject/get):
/// standard RFC 8620 §5.1 `/get`, with an object in a region the caller
/// has no access to ([`SceneBackend::may_access_region`]) or outside its
/// visibility set ([`SceneBackend::is_object_visible`]) reported as
/// `notFound` (§Visibility Contract, §Region Access).
pub async fn handle_scene_object_get<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_get_visible::<SceneObject, B>(backend, caller, args).await
}

/// Handle `SceneObject/changes` (draft-atwood-jmap-scene-00
/// §SceneObject/changes): standard RFC 8620 §5.2 `/changes`.
pub async fn handle_scene_object_changes<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_changes::<SceneObject, B>(backend, caller, args).await
}

/// Handle `SceneObject/query` (draft-atwood-jmap-scene-00
/// §SceneObject/query).
///
/// Supplies the "Default sort: `createdAt` ascending" and refuses any
/// other sort property with `unsupportedSort`. Returns `invalidArguments`
/// when `regionId` is absent, when `physicsMode` is outside the defined
/// set, when `withinRadius.radius` is zero or negative, and when a
/// `withinBounds.max` component is below `min`. An unknown filter
/// property is `unsupportedFilter`. The backend evaluates the filter;
/// [`crate::query::object_matches`], [`crate::query::region_local_position`]
/// and [`crate::query::compare_objects`] implement it.
pub async fn handle_scene_object_query<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_query::<SceneObject, B>(backend, caller, prepare_query(args)?)
        .await
}

/// Handle `SceneObject/queryChanges` (draft-atwood-jmap-scene-00
/// §SceneObject/queryChanges). RFC 8620 §5.6: the `filter` and `sort` are
/// those of the original query, so the `/query` rules apply here too.
pub async fn handle_scene_object_query_changes<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_query_changes::<SceneObject, B>(
        backend,
        caller,
        prepare_query(args)?,
    )
    .await
}

fn prepare_query(mut args: Value) -> Result<Value, JmapError> {
    if let Value::Object(map) = &mut args {
        default_sort(
            map,
            json!([{ "property": "createdAt", "isAscending": true }]),
        );
    }
    reject_unsupported_sort::<SceneObjectComparator>(args.get("sort"))?;
    match args.get("filter") {
        None | Some(Value::Null) => {
            Err(JmapError::invalid_arguments("filter.regionId is required"))
        }
        Some(filter) => check_filter(filter),
    }?;
    Ok(args)
}

/// §Filter Validation: the shared region-scoped and spatial checks of
/// [`check_region_filter`], and "If `physicsMode` contains a value not in
/// the defined set, the server MUST return `invalidArguments`."
fn check_filter(filter: &Value) -> Result<(), JmapError> {
    check_region_filter(filter, &|c| {
        match c.get("physicsMode") {
        Some(m) if !m.as_str().is_some_and(|m| PHYSICS_MODES.contains(&m)) => {
            Err(JmapError::invalid_arguments(format!(
                "filter physicsMode must be \"static\", \"dynamic\", \"kinematic\" or \"none\"; got {m}"
            )))
        }
        _ => Ok(()),
    }
    })
}

/// Handle `SceneObject/set` (draft-atwood-jmap-scene-00 §SceneObject/set).
///
/// Create requires `mayCreateObject`, validates the object, its region,
/// its `visualRef` blob ([`SceneBackend::blob_size`]) and its parent, and
/// hands it to [`SceneBackend::create_object`], which sets `ownerId` and
/// applies `maxObjectsPerRegion`. Update and destroy require
/// [`SceneBackend::may_modify_object`]; update validates the patched
/// object as a create, and on destroy the backend removes the object's
/// descendants.
pub async fn handle_scene_object_set<B: SceneBackend>(
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

    let old_state = backend
        .get_state::<SceneObject>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(Value::as_str) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut acc = SetAccumulators::default();
    let mut mutated = false;

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj) in create_map {
            match create_scene_object(backend, caller, &account_id, &create_id, obj).await {
                Ok(created) => {
                    mutated = true;
                    acc.created.insert(create_id, created);
                }
                Err(e) => {
                    acc.not_created.insert(create_id, e);
                }
            }
        }
    }

    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch) in update_map {
            let id = Id::from(id_str.as_str());
            match update_scene_object(backend, caller, &account_id, &id, patch).await {
                Ok(echo) => {
                    mutated = true;
                    acc.updated.insert(id_str, echo);
                }
                Err(e) => {
                    acc.not_updated.insert(id_str, e);
                }
            }
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
            match destroy_scene_object(backend, caller, &account_id, &Id::from(id_str)).await {
                Ok(()) => {
                    mutated = true;
                    acc.destroyed.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    acc.not_destroyed.insert(id_str.to_owned(), e);
                }
            }
        }
    }

    finalize_set_response::<B, SceneObject>(backend, caller, &account_id, old_state, mutated, acc)
        .await
}

/// The properties of object `m` that break a §Create Errors
/// `invalidProperties` row that needs no backend fact: `regionId` or
/// `position` missing, a non-finite `position` or `scale`, an
/// `orientation` that is not a unit quaternion, an unknown `physicsMode`,
/// `visualRef` without `visualType` or the reverse, and a property of the
/// wrong JSON type.
fn malformed(m: &Map<String, Value>) -> Vec<&'static str> {
    let mut bad = Vec::new();
    let mut check = |key: &'static str, ok: bool| {
        if !ok {
            bad.push(key);
        }
    };
    let optional = |key: &str, ok: fn(&Value) -> bool| m.get(key).is_none_or(ok);

    check("regionId", m.get("regionId").is_some_and(Value::is_string));
    check("position", m.get("position").is_some_and(finite3));
    check("orientation", optional("orientation", orientation_ok));
    check("scale", optional("scale", finite3));
    check(
        "physicsMode",
        optional("physicsMode", |v| {
            v.as_str().is_some_and(|p| PHYSICS_MODES.contains(&p))
        }),
    );
    for key in ["parentId", "name", "visualRef", "visualType", "assetUri"] {
        check(key, optional(key, |v| v.is_null() || v.is_string()));
    }
    for key in ["interactable", "visible"] {
        check(key, optional(key, Value::is_boolean));
    }
    check(
        "worldState",
        optional("worldState", |v| v.is_null() || v.is_object()),
    );
    // "Both MUST be present together or both absent"; `null` is absent
    // ("`visualType` ... MUST be null when `visualRef` is null").
    let present = |key: &str| m.get(key).is_some_and(|v| !v.is_null());
    if present("visualRef") != present("visualType") {
        bad.extend(["visualRef", "visualType"]);
    }
    bad
}

/// "The server MUST return `invalidProperties` naming `visualType` when
/// `visualType` is not in `supportedVisualTypes`." `model/gltf-binary` is
/// always accepted, as in `SceneAsset/set`: §Preferred Asset Format makes
/// it mandatory to implement.
async fn check_visual_type<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    visual_type: &str,
) -> Result<(), Value> {
    let cap = backend
        .scene_capability(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    if visual_type == GLTF_BINARY || cap.supported_visual_types.iter().any(|t| t == visual_type) {
        Ok(())
    } else {
        Err(invalid(
            &["visualType"],
            "visualType is not in supportedVisualTypes",
        ))
    }
}

/// "`visualRef` does not reference a valid blob": `notFound`.
async fn check_visual_ref<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    visual_ref: &str,
) -> Result<(), Value> {
    backend
        .blob_size(caller, account_id, &Id::from(visual_ref))
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
        .map(|_| ())
        .ok_or_else(|| {
            set_error_value(
                &SetError::new(SetErrorType::NotFound)
                    .with_description("visualRef does not reference an uploaded blob"),
            )
        })
}

/// Check `parent_id` as the parent of an object in `region_id` whose own
/// subtree is `height` levels tall, walking the parent chain to the root.
///
/// `invalidProperties` naming `parentId` when "`parentId` does not
/// reference an existing SceneObject", for a parent "in a different
/// region", for a chain that reaches `self_id` ("the object is its own
/// ancestor (directly or transitively)") and for a chain that would put
/// any object deeper than [`MAX_DEPTH`].
async fn check_parent<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    region_id: &Id,
    self_id: Option<&Id>,
    parent_id: &Id,
    height: usize,
) -> Result<(), Value> {
    let mut depth = 0;
    let mut next = Some(parent_id.clone());
    while let Some(id) = next {
        if self_id == Some(&id) {
            return Err(invalid(
                &["parentId"],
                "parentId would make the object its own ancestor",
            ));
        }
        depth += 1;
        if depth + height > MAX_DEPTH {
            return Err(invalid(
                &["parentId"],
                "parentId would make the scene graph deeper than 16 levels",
            ));
        }
        let Some(ancestor): Option<SceneObject> =
            fetch_one(backend, caller, account_id, &id).await?
        else {
            if depth == 1 {
                return Err(invalid(
                    &["parentId"],
                    "parentId does not reference an existing SceneObject",
                ));
            }
            break;
        };
        if ancestor.region_id != *region_id {
            return Err(invalid(
                &["parentId"],
                "parentId references a SceneObject in a different region",
            ));
        }
        next = ancestor.parent_id;
    }
    Ok(())
}

/// The number of levels in the subtree rooted at `id`: 1 for an object
/// without children.
///
/// ponytail: reads every SceneObject of the account to find the
/// descendants, O(objects) per re-parenting update. A backend with a
/// `parentId` index can serve a faster `SceneObject/set`. The walk stops
/// past [`MAX_DEPTH`] levels, so a corrupt cycle cannot loop.
async fn subtree_height<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<usize, Value> {
    let (all, _) = backend
        .get_objects::<SceneObject>(caller, account_id, None, None)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    let mut children: HashMap<&Id, Vec<&Id>> = HashMap::new();
    for o in &all {
        if let Some(p) = &o.parent_id {
            children.entry(p).or_default().push(&o.id);
        }
    }
    let mut level = vec![id];
    let mut height = 0;
    while !level.is_empty() && height <= MAX_DEPTH {
        height += 1;
        level = level
            .iter()
            .filter_map(|p| children.get(p))
            .flatten()
            .copied()
            .collect();
    }
    Ok(height)
}

fn server_set_in<'a>(keys: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut fixed: Vec<&str> = keys.filter(|k| SERVER_SET.contains(k)).collect();
    fixed.sort_unstable();
    fixed.dedup();
    fixed
}

/// Refuse `bad` (from [`malformed`]) with one `invalidProperties`.
fn refuse_malformed(mut bad: Vec<&'static str>) -> Result<(), Value> {
    if bad.is_empty() {
        return Ok(());
    }
    bad.sort_unstable();
    bad.dedup();
    Err(invalid(
        &bad,
        "regionId and position are required, position and scale finite, orientation a unit \
         quaternion, physicsMode static, dynamic, kinematic or none, visualRef and visualType \
         together",
    ))
}

/// One §Creating an Object. The `Ok` value is the `created` entry: the
/// server-set `id`, `ownerId`, `createdAt` and `updatedAt`.
async fn create_scene_object<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    create_id: &str,
    obj: Value,
) -> Result<Value, Value> {
    let Value::Object(mut m) = obj else {
        return Err(invalid(&[], "a SceneObject create must be a JSON object"));
    };

    let cap = backend
        .scene_capability(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    if !cap.may_create_object {
        return Err(forbidden("mayCreateObject is false"));
    }

    let fixed = server_set_in(m.keys().map(String::as_str));
    if !fixed.is_empty() {
        return Err(invalid(&fixed, "these properties are set by the server"));
    }
    refuse_malformed(malformed(&m))?;
    if let Some(t) = m.get("visualType").and_then(Value::as_str) {
        check_visual_type(backend, caller, account_id, t).await?;
    }

    // "The server MUST return `invalidProperties` naming `regionId` when
    // `regionId` does not exist, and SHOULD return the same error (not
    // `forbidden`) when the caller does not have access to the region".
    let region_id = Id::from(m["regionId"].as_str().unwrap_or_default());
    let inaccessible = || invalid(&["regionId"], "regionId is not an accessible SceneRegion");
    fetch_one::<SceneRegion, B>(backend, caller, account_id, &region_id)
        .await?
        .ok_or_else(inaccessible)?;
    if !backend
        .may_access_region(caller, account_id, &region_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        return Err(inaccessible());
    }
    if let Some(r) = m.get("visualRef").and_then(Value::as_str) {
        check_visual_ref(backend, caller, account_id, r).await?;
    }
    if let Some(p) = m.get("parentId").and_then(Value::as_str) {
        check_parent(
            backend,
            caller,
            account_id,
            &region_id,
            None,
            &Id::from(p),
            1,
        )
        .await?;
    }

    // `orientation` is optional on create and has no stated default; an
    // absent one is "the identity orientation (no rotation)".
    m.entry("orientation")
        .or_insert_with(|| json!(IDENTITY_ORIENTATION));
    if let Some(q) = normalized_orientation(&m) {
        m.insert("orientation".to_owned(), q);
    }
    let now = now_utc_string();
    m.insert("id".to_owned(), json!(PLACEHOLDER_ID));
    m.insert("ownerId".to_owned(), Value::Null);
    m.insert("createdAt".to_owned(), json!(now));
    m.insert("updatedAt".to_owned(), json!(now));
    let object: SceneObject = serde_json::from_value(Value::Object(m))
        .map_err(|e| invalid(&[], &format!("invalid SceneObject: {e}")))?;
    let (id, stored) = backend
        .create_object(caller, account_id, create_id, object)
        .await
        .map_err(backend_set_error_value)?;
    Ok(json!({
        "id": id,
        "ownerId": stored.owner_id,
        "createdAt": stored.created_at,
        "updatedAt": stored.updated_at,
    }))
}

/// The stored object `id` if the caller may modify it
/// ([`SceneBackend::may_modify_object`]), else `forbidden`; `notFound` if
/// it is missing or hidden from the caller: "The server MUST NOT return
/// `forbidden`, as that would confirm the object still exists"
/// (§Visibility Contract).
async fn fetch_modifiable<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<SceneObject, Value> {
    let object: SceneObject = fetch_one(backend, caller, account_id, id)
        .await?
        .ok_or_else(not_found)?;
    if !is_visible(backend, caller, account_id, &object)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        return Err(not_found());
    }
    if backend
        .may_modify_object(caller, account_id, &object)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        Ok(object)
    } else {
        Err(forbidden(
            "Caller is not the object owner, region owner, or an administrator.",
        ))
    }
}

/// One §Updating an Object. `Ok` is the `updated` echo: the server-set
/// `updatedAt` and a normalized `orientation` (RFC 8620 §5.3: "any
/// properties that changed in a way not explicitly requested by the
/// PatchObject").
async fn update_scene_object<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    patch: Value,
) -> Result<Value, Value> {
    let mut patch: PatchObject = serde_json::from_value(patch).map_err(|e| {
        set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;
    let object = fetch_modifiable(backend, caller, account_id, id).await?;

    let top = |k: &str| k.split('/').next().unwrap_or(k).to_owned();
    let patched: Vec<String> = patch.as_map().keys().map(|k| top(k)).collect();
    let fixed = server_set_in(patched.iter().map(String::as_str));
    if !fixed.is_empty() {
        return Err(invalid(&fixed, "these properties are set by the server"));
    }

    let Value::Object(mut result) =
        serde_json::to_value(&object).expect("derive(Serialize) on plain data is infallible")
    else {
        unreachable!("a SceneObject serializes as a JSON object")
    };
    apply_patch_object(&mut result, patch.as_map()).map_err(|e| {
        set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;
    // "`regionId` is immutable and cannot be changed via update."
    if result.get("regionId") != Some(&json!(object.region_id)) {
        return Err(invalid(&["regionId"], "regionId is immutable"));
    }
    // "Same field-level validations as create", on the object as the
    // patch leaves it.
    refuse_malformed(malformed(&result))?;
    let touched = |k: &str| patched.iter().any(|p| p == k);
    if touched("visualType") {
        if let Some(t) = result.get("visualType").and_then(Value::as_str) {
            check_visual_type(backend, caller, account_id, t).await?;
        }
    }
    if touched("visualRef") {
        if let Some(r) = result.get("visualRef").and_then(Value::as_str) {
            check_visual_ref(backend, caller, account_id, r).await?;
        }
    }
    if touched("parentId") {
        if let Some(p) = result.get("parentId").and_then(Value::as_str) {
            let height = subtree_height(backend, caller, account_id, id).await?;
            check_parent(
                backend,
                caller,
                account_id,
                &object.region_id,
                Some(id),
                &Id::from(p),
                height,
            )
            .await?;
        }
    }

    let now = now_utc_string();
    let mut echo = Map::new();
    echo.insert("updatedAt".to_owned(), json!(now));
    let map = patch.as_map_mut();
    if touched("orientation") {
        if let Some(q) = normalized_orientation(&result) {
            map.retain(|k, _| top(k) != "orientation");
            map.insert("orientation".to_owned(), q.clone());
            echo.insert("orientation".to_owned(), q);
        }
    }
    map.insert("updatedAt".to_owned(), json!(now));
    match backend
        .update_object::<SceneObject>(caller, account_id, id, patch)
        .await
        .map_err(backend_set_error_value)?
    {
        Some(object) => Ok(
            serde_json::to_value(object).expect("derive(Serialize) on plain data is infallible")
        ),
        None => Ok(Value::Object(echo)),
    }
}

/// One §Destroying an Object: forbidden unless
/// [`SceneBackend::may_modify_object`]; the backend removes the
/// descendants.
async fn destroy_scene_object<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<(), Value> {
    fetch_modifiable(backend, caller, account_id, id).await?;
    backend
        .destroy_object::<SceneObject>(caller, account_id, id)
        .await
        .map_err(backend_set_error_value)
}
