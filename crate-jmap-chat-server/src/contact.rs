//! ChatContact/* method handlers (draft-atwood-jmap-chat-00 §ChatContact).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by draft-atwood-jmap-chat-00 §ChatContact. The
//! returned `Value` is the corresponding method-response object per the
//! same section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `unsupportedFilter`, `unsupportedSort`, `cannotCalculateChanges` —
//! per RFC 8620 §3.6 and §5). Per-target failures inside `/set` surface
//! in the `notCreated` / `notUpdated` / `notDestroyed` maps within
//! `Ok((Value, ...))`, not as `Err`.

use jmap_chat_types::ChatContact;
use jmap_chat_types::{ChatContactComparator, ChatContactFilter};
use jmap_types::{Id, JmapError, PatchObject, State, UTCDate};
use serde_json::{Value, json};

use crate::helpers::{
    SetAccumulators, enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response, not_found_json, now_utc_string, optional_arg, reject_unsupported_sort,
    serialize_value, set_error_value,
};
use crate::{ChatBackend, SetError, SetErrorType};
use jmap_server::{
    HandlerOutput, QueryParams, backend_set_error_value, filter_arg, is_client_updatable,
    query_anchor_args, reject_read_only_patch, server_fail_from_backend,
};

// ---------------------------------------------------------------------------
// ChatContact/get
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/get` method call (draft-atwood-jmap-chat-00 §ChatContact).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_get<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    enforce_max_objects_in_get(&args, backend.max_objects_in_get(caller, &account_id))?;

    let ids: Option<Vec<Id>> = match args.remove("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<ChatContact>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<ChatContact>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(serialize_value)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    })))
}

// ---------------------------------------------------------------------------
// ChatContact/changes
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/changes` method call (draft-atwood-jmap-chat-00 §ChatContact).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    let since_state: State = match args.get("sinceState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceState is required")),
    };

    let max_changes = jmap_server::parse_max_changes(args.get("maxChanges"))?;

    let result = backend
        .get_changes::<ChatContact>(caller, &account_id, &since_state, max_changes)
        .await
        .map_err(JmapError::from)?;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "oldState": since_state.as_ref(),
        "newState": result.new_state.as_ref(),
        "hasMoreChanges": result.has_more_changes,
        "updatedProperties": Value::Null,
        "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    })))
}

// ---------------------------------------------------------------------------
// ChatContact/set
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/set` method call (draft-atwood-jmap-chat-00 §ChatContact).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// `create` accepts only `identity`, which is required; the server sets
/// `id`, `login` (to the `identity`), `firstSeenAt` and `lastSeenAt`
/// (to now) and `blocked` (`false`). A create that omits `identity`,
/// gives it as anything but a string with a non-whitespace character, or
/// carries any other property, `did` included, is rejected with
/// `invalidProperties` naming each offending property: `did` is
/// server-set (draft-atwood-jmap-chat-did-00 §Optional `did` field), so a
/// backend that binds one checks it with
/// [`ChatContact::has_consistent_did`]. A create whose `identity` equals
/// [`ChatBackend::owner_user_id`] is rejected with `invalidProperties`
/// naming `identity`. A create whose `identity` another ChatContact
/// already holds is rejected by the backend with `alreadyExists` and
/// `existingId` (see the `ChatContact` uniqueness contract on
/// [`ChatBackend::create_object`]).
///
/// `destroy` is rejected with `forbidden`. `update` supports `blocked`
/// and `displayName`; `id`, `identity`, `firstSeenAt`, `lastSeenAt` are
/// immutable or server-set and rejected in updates.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<ChatContact>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create — draft-atwood-jmap-chat-00 §ChatContact/set: "`create`
    // accepts only `identity`, which is required; the server assigns `id`
    // and sets the other server-set properties." draft-atwood-jmap-chat-
    // did-00 §Optional `did` field: `did` "is server-set (`ChatContact/set`
    // create accepts only `identity`)".
    // -----------------------------------------------------------------------
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        let owner_user_id = if create_map.is_empty() {
            None
        } else {
            backend
                .owner_user_id(caller, &account_id)
                .await
                .map_err(|e| server_fail_from_backend(&e))?
        };
        for (create_id, obj_val) in create_map {
            // `login` is set to the identity and MUST contain a
            // non-whitespace character (draft §ChatContact `login`).
            let identity = obj_val
                .get("identity")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty());
            let mut bad_props: Vec<&str> = obj_val
                .as_object()
                .into_iter()
                .flat_map(|m| m.keys())
                .map(String::as_str)
                .filter(|&k| k != "identity")
                .collect();
            if identity.is_none() {
                bad_props.push("identity");
            }
            let Some(identity) = identity.filter(|_| bad_props.is_empty()) else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": bad_props }),
                );
                continue;
            };

            // draft §ChatContact/set: "If `identity` equals the owner's
            // identity (the `ownerUserId` Session field), the server MUST
            // reject the create with an `invalidProperties` SetError
            // naming `identity`".
            if owner_user_id.as_deref() == Some(identity) {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["identity"] }),
                );
                continue;
            }

            let now = UTCDate::from(now_utc_string().as_ref());
            let contact =
                |id: &Id| ChatContact::new(id.clone(), identity, identity, now.clone(), now, false);
            match backend
                .create_object::<ChatContact, _>(caller, &account_id, create_id, contact)
                .await
            {
                Ok((_server_id, created_obj)) => {
                    mutated = true;
                    created.insert(
                        create_id.clone(),
                        serde_json::to_value(&created_obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Err(e) => {
                    not_created.insert(create_id.clone(), backend_set_error_value(e));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_updated.insert(id_str, set_error_value(&err));
                    continue;
                }
            };

            // Reject patches to server-set or immutable ChatContact
            // properties (§ChatContact).
            if let Some(err) = patch_val
                .as_object()
                .and_then(|m| reject_read_only_patch::<ChatContact>(m, None))
            {
                not_updated.insert(id_str, set_error_value(&err));
                continue;
            }

            // draft-atwood-jmap-chat-00 §ChatContact/set:
            // "update supports: blocked, displayName.", the client-settable
            // properties that are not immutable.
            //
            // Project the patch onto that set so that unknown wire fields
            // are silently dropped rather than reaching
            // `backend.update_object`.
            let Value::Object(mut patch_map) = patch_val else {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
                );
                continue;
            };
            patch_map.retain(|field, _| is_client_updatable::<ChatContact>(field));
            let clean_patch = patch_map;
            if clean_patch.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({
                        "type": "invalidPatch",
                        "description": "no updatable fields in patch",
                    }),
                );
                continue;
            }

            match backend
                .update_object::<ChatContact>(
                    caller,
                    &account_id,
                    &id,
                    PatchObject::from_map(clean_patch),
                )
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Ok(None) => {
                    mutated = true;
                    updated.insert(id_str, Value::Null);
                }
                Err(e) => {
                    not_updated.insert(id_str, backend_set_error_value(e));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy — draft §ChatContact/set: "`destroy` is not supported and
    // MUST return `forbidden`."
    // -----------------------------------------------------------------------
    if let Some(destroy_arr) = args.get("destroy").and_then(|v| v.as_array()) {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        let id_strs: Vec<&str> = destroy_arr
            .iter()
            .map(|v| {
                v.as_str().ok_or_else(|| {
                    JmapError::invalid_arguments(format!(
                        "destroy: every element must be a string Id; got {}",
                        jmap_server::json_type_name(v)
                    ))
                })
            })
            .collect::<Result<_, _>>()?;
        for id_str in id_strs {
            not_destroyed.insert(
                id_str.to_owned(),
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    finalize_set_response::<B, ChatContact>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        SetAccumulators {
            created,
            updated,
            destroyed: destroyed_list,
            not_created,
            not_updated,
            not_destroyed,
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// ChatContact/query
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/query` method call (draft-atwood-jmap-chat-00 §ChatContact).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`); the
/// returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// Unknown filter clauses and sort properties are refused with
/// `unsupportedFilter` / `unsupportedSort` (RFC 8620 §5.5).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let limit: Option<u64> = match args.remove("limit") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(n) => Some(n),
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {}",
                    jmap_server::json_type_name(&v)
                )));
            }
        },
    };

    let position: i64 = match args.remove("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!(
                "position: expected an integer, got {}",
                jmap_server::json_type_name(&v)
            ))
        })?,
    };

    let (anchor, anchor_offset) = query_anchor_args(&args)?;

    reject_unsupported_sort::<ChatContactComparator>(args.get("sort"))?;
    let filter: Option<ChatContactFilter> = filter_arg(&mut args)?;

    let sort: Option<Vec<ChatContactComparator>> = optional_arg(&mut args, "sort", || {
        JmapError::invalid_arguments("sort must be an array")
    })?;

    let result = backend
        .query_objects::<ChatContact>(
            caller,
            &account_id,
            QueryParams::new()
                .filter(filter.as_ref())
                .sort(sort.as_deref())
                .limit(limit)
                .position(position)
                .anchor(anchor.as_ref())
                .anchor_offset(anchor_offset),
        )
        .await
        .map_err(JmapError::from)?;

    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": result.query_state.as_ref(),
        "canCalculateChanges": result.can_calculate_changes,
        "position": result.position,
        "ids": result.ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total && let Some(t) = result.total {
        resp["total"] = json!(t);
    }

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// ChatContact/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/queryChanges` method call (draft-atwood-jmap-chat-00 §ChatContact).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_query_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    reject_unsupported_sort::<ChatContactComparator>(args.get("sort"))?;
    jmap_server::handle_query_changes::<ChatContact, B>(backend, caller, args).await
}
