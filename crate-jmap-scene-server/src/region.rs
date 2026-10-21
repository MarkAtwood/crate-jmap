//! SceneRegion/* method handlers (draft-atwood-jmap-scene-00 §SceneRegion
//! Methods).
//!
//! `/get`, `/changes` and `/queryChanges` are the standard RFC 8620 §5.1,
//! §5.2 and §5.6 methods. `/set` applies §Creating a Region, §Updating a
//! Region, §Destroying a Region and §SetError Conditions: the required
//! `name` and `bounds`, `min <= max`, the `accessPolicy`, `spawnOrientation`
//! and binding rules, `mayCreateRegion`, the owner-or-admin rule on update
//! and the owner rule on destroy. `/query` supplies the "Default sort:
//! `createdAt` descending" and refuses an unknown `accessPolicy` filter
//! value with `invalidArguments`.

use jmap_scene_types::query::SceneRegionComparator;
use jmap_scene_types::{GeoAnchor, SceneRegion};
use jmap_types::{Id, JmapError, PatchObject};
use serde_json::{json, Map, Value};

use crate::backend::{BindingTarget, SceneBackend};
use crate::helpers::{
    backend_set_error_value, default_sort, enforce_max_objects_in_set, extract_account_id,
    fetch_one, finalize_set_response, forbidden, handle_get_visible, invalid, not_found, numbers,
    reject_unsupported_sort, set_error_value, unit_quaternion, SetAccumulators, PLACEHOLDER_ID,
};
use jmap_server::{
    apply_patch_object, now_utc_string, server_fail_from_backend, server_fail_value_from_backend,
    HandlerOutput, SetError, SetErrorType,
};

pub(crate) const JMAP_CHAT_URI: &str = "urn:ietf:params:jmap:chat";
const JMAP_VTC_URI: &str = "urn:ietf:params:jmap:vtc";

/// "The server sets `id`, `accountId`, `ownerId` (to the caller's
/// userId), `activeAvatarCount` (to `0`), `createdAt`, and `updatedAt`."
/// None of them may be set or patched.
const SERVER_SET: &[&str] = &[
    "id",
    "accountId",
    "ownerId",
    "activeAvatarCount",
    "createdAt",
    "updatedAt",
];

/// The binding properties and the companion capability each needs:
/// "present only when the corresponding companion capability is
/// advertised on the same account".
const BINDINGS: &[(&str, &str)] = &[
    ("chatId", JMAP_CHAT_URI),
    ("spaceId", JMAP_CHAT_URI),
    ("channelId", JMAP_CHAT_URI),
    ("activeCallId", JMAP_VTC_URI),
];

/// The `accessPolicy` values: "`accessPolicy` is not one of `\"public\"`,
/// `\"invite\"`, `\"space\"`" is `invalidProperties`.
const ACCESS_POLICIES: &[&str] = &["public", "invite", "space"];

/// Handle `SceneRegion/get` (draft-atwood-jmap-scene-00 §SceneRegion/get):
/// standard RFC 8620 §5.1 `/get`, with a region the caller has no access
/// to ([`SceneBackend::may_access_region`]) reported as `notFound`
/// (§Region Access). Of `invitedUserIds`, "The server returns the list
/// only to callers who may update the region; for every other caller, the
/// server returns `[]`" ([`SceneBackend::may_update_region`]).
pub async fn handle_scene_region_get<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_get_visible::<SceneRegion, B>(backend, caller, args).await
}

/// Handle `SceneRegion/changes` (draft-atwood-jmap-scene-00
/// §SceneRegion/changes): standard RFC 8620 §5.2 `/changes`.
pub async fn handle_scene_region_changes<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_changes::<SceneRegion, B>(backend, caller, args).await
}

/// Handle `SceneRegion/query` (draft-atwood-jmap-scene-00
/// §SceneRegion/query).
///
/// Supplies the "Default sort: `createdAt` descending", refuses any other
/// sort property with `unsupportedSort` and an `accessPolicy` filter value
/// other than `"public"`, `"invite"` or `"space"` with `invalidArguments`;
/// an unknown filter property is `unsupportedFilter`. The backend
/// evaluates the filter; [`crate::query::region_matches`] and
/// [`crate::query::compare_regions`] implement it.
pub async fn handle_scene_region_query<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_query::<SceneRegion, B>(backend, caller, prepare_query(args)?)
        .await
}

/// Handle `SceneRegion/queryChanges` (draft-atwood-jmap-scene-00
/// §SceneRegion/queryChanges). RFC 8620 §5.6: the `filter` and `sort` are
/// those of the original query, so the `/query` rules apply here too.
pub async fn handle_scene_region_query_changes<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_query_changes::<SceneRegion, B>(
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
            json!([{ "property": "createdAt", "isAscending": false }]),
        );
    }
    reject_unsupported_sort::<SceneRegionComparator>(args.get("sort"))?;
    if let Some(filter) = args.get("filter") {
        check_access_policy_filter(filter)?;
    }
    Ok(args)
}

/// "If a client provides an invalid value for `accessPolicy` (not one of
/// the defined values), the server MUST return `invalidArguments`", in
/// every condition of a FilterOperator tree.
fn check_access_policy_filter(filter: &Value) -> Result<(), JmapError> {
    if let Some(Value::Array(conditions)) = filter.get("conditions") {
        return conditions.iter().try_for_each(check_access_policy_filter);
    }
    match filter.get("accessPolicy") {
        Some(v) if !v.as_str().is_some_and(|p| ACCESS_POLICIES.contains(&p)) => {
            Err(JmapError::invalid_arguments(format!(
                "filter accessPolicy must be \"public\", \"invite\" or \"space\"; got {v}"
            )))
        }
        _ => Ok(()),
    }
}

/// Handle `SceneRegion/set` (draft-atwood-jmap-scene-00 §SceneRegion/set).
///
/// Create requires `mayCreateRegion`, validates the region and its
/// bindings ([`SceneBackend::has_capability`],
/// [`SceneBackend::region_binding`]) and hands it to
/// [`SceneBackend::create_object`], which applies `maxRegionsPerAccount`.
/// Update and destroy of a region the caller has no access to
/// ([`SceneBackend::may_access_region`]) are `notFound`, never
/// `forbidden`. Update requires [`SceneBackend::may_update_region`] and
/// validates the patched region as a create; destroy requires
/// [`SceneBackend::may_destroy_region`], and the backend removes the
/// contained objects and avatars.
pub async fn handle_scene_region_set<B: SceneBackend>(
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
        .get_state::<SceneRegion>(caller, &account_id)
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
            match create_region(backend, caller, &account_id, &create_id, obj).await {
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
            match update_region(backend, caller, &account_id, &id, patch).await {
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
            match destroy_region(backend, caller, &account_id, &Id::from(id_str)).await {
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

    finalize_set_response::<B, SceneRegion>(backend, caller, &account_id, old_state, mutated, acc)
        .await
}

/// SceneBounds: `min` and `max` are "a 3-element numeric array" and
/// "Each component of `min` MUST be less than or equal to the
/// corresponding component of `max`."
fn bounds_ok(v: &Value) -> bool {
    match (
        v.get("min").and_then(|m| numbers(m, 3)),
        v.get("max").and_then(|m| numbers(m, 3)),
    ) {
        (Some(min), Some(max)) => min.iter().zip(&max).all(|(lo, hi)| lo <= hi),
        _ => false,
    }
}

/// The properties of region `m` that break a §Create Errors
/// `invalidProperties` row that needs no backend fact: `name` "missing or
/// empty", `bounds` missing or malformed or `min > max`, an unknown
/// `accessPolicy`, a `spawnPosition` that is not 3 Numbers, a
/// `spawnOrientation` that is not a unit quaternion, "`invitedUserIds` is
/// not an array of Strings", and a property of the wrong JSON type.
fn malformed(m: &Map<String, Value>) -> Vec<&'static str> {
    let mut bad = Vec::new();
    let mut check = |key: &'static str, ok: bool| {
        if !ok {
            bad.push(key);
        }
    };
    let optional = |key: &str, ok: fn(&Value) -> bool| m.get(key).is_none_or(ok);

    check(
        "name",
        m.get("name")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty()),
    );
    check("bounds", m.get("bounds").is_some_and(bounds_ok));
    for key in [
        "description",
        "simulationUri",
        "inputUri",
        "viewHint",
        "chatId",
        "spaceId",
        "channelId",
        "activeCallId",
    ] {
        check(key, optional(key, |v| v.is_null() || v.is_string()));
    }
    for key in ["environment", "worldState"] {
        check(key, optional(key, |v| v.is_null() || v.is_object()));
    }
    check(
        "geoAnchor",
        optional("geoAnchor", |v| {
            v.is_null() || serde_json::from_value::<GeoAnchor>(v.clone()).is_ok()
        }),
    );
    check(
        "spawnPosition",
        optional("spawnPosition", |v| numbers(v, 3).is_some()),
    );
    check(
        "spawnOrientation",
        optional("spawnOrientation", unit_quaternion),
    );
    check(
        "accessPolicy",
        optional("accessPolicy", |v| {
            v.as_str().is_some_and(|p| ACCESS_POLICIES.contains(&p))
        }),
    );
    check(
        "invitedUserIds",
        optional("invitedUserIds", |v| {
            v.as_array().is_some_and(|a| a.iter().all(Value::is_string))
        }),
    );
    bad
}

/// The capability-dependent §Create Errors of region `m`: a non-null
/// binding in `touched` without its companion capability, and
/// "`accessPolicy` is `\"space\"` but `urn:ietf:params:jmap:chat` is not in
/// the server's capabilities" (naming `accessPolicy`) or "but `spaceId` is
/// null" (naming `spaceId`, as in §Updating a Region: "an update that sets
/// `spaceId` to `null` while `accessPolicy` is `\"space\"` fails with
/// `invalidProperties` naming `spaceId`").
fn capability_errors<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    m: &Map<String, Value>,
    touched: &impl Fn(&str) -> bool,
) -> Vec<&'static str> {
    let has = |uri| backend.has_capability(caller, account_id, uri);
    let mut bad: Vec<&'static str> = BINDINGS
        .iter()
        .filter(|(key, uri)| touched(key) && m.get(*key).is_some_and(|v| !v.is_null()) && !has(uri))
        .map(|(key, _)| *key)
        .collect();
    if m.get("accessPolicy").and_then(Value::as_str) == Some("space") {
        if !has(JMAP_CHAT_URI) {
            bad.push("accessPolicy");
        } else if m.get("spaceId").is_none_or(Value::is_null) {
            bad.push("spaceId");
        }
    }
    bad
}

/// Ask [`SceneBackend::region_binding`] about each non-null binding in
/// `touched` and map the answer to the draft's SetError: `invalidProperties`
/// naming the property, except `forbidden` for an `activeCallId` the
/// caller lacks access to. Then ask [`SceneBackend::channel_in_space`]
/// whether a non-null `channelId` "is a channel of the Space named by
/// `spaceId`", naming `channelId` if not.
async fn check_bindings<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    m: &Map<String, Value>,
    touched: &impl Fn(&str) -> bool,
) -> Result<(), Value> {
    for (key, _) in BINDINGS.iter().filter(|(key, _)| touched(key)) {
        let Some(id) = m.get(*key).and_then(Value::as_str) else {
            continue;
        };
        let target = backend
            .region_binding(caller, account_id, key, &Id::from(id))
            .await
            .map_err(|e| server_fail_value_from_backend(&e))?;
        match (target, *key) {
            (BindingTarget::Inaccessible, "activeCallId") => {
                return Err(forbidden(
                    "the caller is not a participant in or moderator of the VTCCall",
                ));
            }
            (BindingTarget::Missing | BindingTarget::Inaccessible, _) => {
                return Err(invalid(
                    &[key],
                    "the referenced object does not exist, is not accessible, or is an ended call",
                ));
            }
            _ => {}
        }
    }
    if touched("channelId") || touched("spaceId") {
        if let (Some(channel), Some(space)) = (
            m.get("channelId").and_then(Value::as_str),
            m.get("spaceId").and_then(Value::as_str),
        ) {
            let in_space = backend
                .channel_in_space(caller, account_id, &Id::from(channel), &Id::from(space))
                .await
                .map_err(|e| server_fail_value_from_backend(&e))?;
            if !in_space {
                return Err(invalid(
                    &["channelId"],
                    "channelId is not a channel of the Space named by spaceId",
                ));
            }
        }
    }
    Ok(())
}

/// Every validation shared by create and update, on the whole region `m`.
async fn validate<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    m: &Map<String, Value>,
    touched: &impl Fn(&str) -> bool,
) -> Result<(), Value> {
    let mut bad = malformed(m);
    if bad.is_empty() {
        bad = capability_errors(backend, caller, account_id, m, touched);
    }
    if !bad.is_empty() {
        bad.sort_unstable();
        bad.dedup();
        return Err(invalid(
            &bad,
            "name and bounds are required, bounds.min <= bounds.max, spawnOrientation a unit \
             quaternion, accessPolicy public, invite or space (space needs \
             urn:ietf:params:jmap:chat and spaceId), bindings need their companion capability",
        ));
    }
    check_bindings(backend, caller, account_id, m, touched).await
}

fn server_set_in<'a>(keys: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut fixed: Vec<&str> = keys.filter(|k| SERVER_SET.contains(k)).collect();
    fixed.sort_unstable();
    fixed.dedup();
    fixed
}

/// One §Creating a Region. The `Ok` value is the `created` entry: the
/// server-set `id`, `accountId`, `ownerId` (from
/// [`SceneBackend::caller_user_id`]), `activeAvatarCount`, `createdAt` and
/// `updatedAt`.
async fn create_region<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    create_id: &str,
    obj: Value,
) -> Result<Value, Value> {
    let Value::Object(mut m) = obj else {
        return Err(invalid(&[], "a SceneRegion create must be a JSON object"));
    };

    let cap = backend
        .scene_capability(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    if !cap.may_create_region {
        return Err(forbidden("mayCreateRegion is false"));
    }

    // `activeCallId` is a binding `update` supports; `create` does not
    // list it.
    let mut fixed = server_set_in(m.keys().map(String::as_str));
    if m.contains_key("activeCallId") {
        fixed.push("activeCallId");
    }
    if !fixed.is_empty() {
        return Err(invalid(
            &fixed,
            "these properties are set by the server or cannot be set on create",
        ));
    }

    validate(backend, caller, account_id, &m, &|_| true).await?;
    let owner_id = backend
        .caller_user_id(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;

    for (key, uri) in BINDINGS {
        if backend.has_capability(caller, account_id, uri) {
            m.entry(*key).or_insert(Value::Null);
        }
    }
    let now = now_utc_string();
    m.insert("id".to_owned(), json!(PLACEHOLDER_ID));
    m.insert("accountId".to_owned(), json!(account_id));
    m.insert("ownerId".to_owned(), json!(owner_id));
    m.insert("activeAvatarCount".to_owned(), json!(0));
    m.insert("createdAt".to_owned(), json!(now));
    m.insert("updatedAt".to_owned(), json!(now));
    let region: SceneRegion = serde_json::from_value(Value::Object(m))
        .map_err(|e| invalid(&[], &format!("invalid SceneRegion: {e}")))?;
    let (id, stored) = backend
        .create_object(caller, account_id, create_id, region)
        .await
        .map_err(backend_set_error_value)?;
    Ok(json!({
        "id": id,
        "accountId": stored.account_id,
        "ownerId": stored.owner_id,
        "activeAvatarCount": stored.active_avatar_count,
        "createdAt": stored.created_at,
        "updatedAt": stored.updated_at,
    }))
}

/// One §Updating a Region. `Ok` is the `updated` echo: the server-set
/// `updatedAt` (RFC 8620 §5.3: "any properties that changed in a way not
/// explicitly requested by the PatchObject").
async fn update_region<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    patch: Value,
) -> Result<Value, Value> {
    let mut patch: PatchObject = serde_json::from_value(patch).map_err(|e| {
        set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;
    let region = fetch_accessible(backend, caller, account_id, id).await?;
    if !backend
        .may_update_region(caller, account_id, &region)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        return Err(forbidden(
            "only the region owner or an admin may modify this region",
        ));
    }

    let top = |k: &str| k.split('/').next().unwrap_or(k).to_owned();
    let patched: Vec<String> = patch.as_map().keys().map(|k| top(k)).collect();
    let fixed = server_set_in(patched.iter().map(String::as_str));
    if !fixed.is_empty() {
        return Err(invalid(&fixed, "these properties are set by the server"));
    }

    // "Same field-level validations as create apply", to the region as
    // the patch leaves it.
    let Value::Object(mut result) =
        serde_json::to_value(&region).expect("derive(Serialize) on plain data is infallible")
    else {
        unreachable!("a SceneRegion serializes as a JSON object")
    };
    apply_patch_object(&mut result, patch.as_map()).map_err(|e| {
        set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;
    let touched = |k: &str| patched.iter().any(|p| p == k);
    validate(backend, caller, account_id, &result, &touched).await?;

    let now = now_utc_string();
    patch
        .as_map_mut()
        .insert("updatedAt".to_owned(), json!(now));
    match backend
        .update_object::<SceneRegion>(caller, account_id, id, patch)
        .await
        .map_err(backend_set_error_value)?
    {
        Some(region) => Ok(
            serde_json::to_value(region).expect("derive(Serialize) on plain data is infallible")
        ),
        None => Ok(json!({ "updatedAt": now })),
    }
}

/// The region `id` that update or destroy acts on: `notFound` when "The
/// region id does not exist or the caller lacks access"
/// ([`SceneBackend::may_access_region`]), "the server returns `notFound`
/// rather than `forbidden` to prevent probing" (§Update Errors, §Destroy
/// Errors).
async fn fetch_accessible<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<SceneRegion, Value> {
    let region: SceneRegion = fetch_one(backend, caller, account_id, id)
        .await?
        .ok_or_else(not_found)?;
    if backend
        .may_access_region(caller, account_id, &region.id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        Ok(region)
    } else {
        Err(not_found())
    }
}

/// One §Destroying a Region: "The server SHOULD return `forbidden` when
/// the caller is not the region owner and does not have
/// deployment-defined administrative privileges."
async fn destroy_region<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<(), Value> {
    let region = fetch_accessible(backend, caller, account_id, id).await?;
    if !backend
        .may_destroy_region(caller, account_id, &region)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        return Err(forbidden(
            "only the region owner or an admin may destroy this region",
        ));
    }
    backend
        .destroy_object::<SceneRegion>(caller, account_id, id)
        .await
        .map_err(backend_set_error_value)
}
