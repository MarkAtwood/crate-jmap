//! CustomEmoji/* method handlers (draft-atwood-jmap-chat-00 §CustomEmoji).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by draft-atwood-jmap-chat-00 §CustomEmoji. The
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
//! per RFC 8620 §3.6 and §5). Per-target failures inside `/set`
//! (including the backend-canonical `may_set_custom_emoji` rich-SetError
//! deny path) surface in the `notCreated` / `notUpdated` / `notDestroyed`
//! maps within `Ok((Value, ...))`, not as `Err`.

use std::collections::BTreeMap;

use jmap_chat_types::CustomEmoji;
use jmap_chat_types::{CustomEmojiComparator, CustomEmojiFilter};
use jmap_types::{Id, JmapError, PatchObject, State, UTCDate};
use serde_json::{Value, json};

use crate::helpers::resolved_create;
use crate::helpers::{PropType, mistyped_create_props};
use crate::helpers::{
    SetAccumulators, default_sort, enforce_max_objects_in_get, enforce_max_objects_in_set,
    extract_account_id, finalize_set_response, not_found_json, now_utc_string, optional_arg,
    reject_unsupported_sort, serialize_value, set_error_value,
};
use crate::{ChatBackend, EmojiSetOp};
use jmap_server::{
    HandlerOutput, QueryParams, backend_set_error_value, bool_arg, filter_arg, is_client_updatable,
    query_anchor_args, reject_read_only_patch, reject_server_set, server_fail_from_backend,
    server_fail_value_from_backend,
};

// ---------------------------------------------------------------------------
// CustomEmoji/get
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/get` method call (draft-atwood-jmap-chat-00 §CustomEmoji).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_emoji_get<B: ChatBackend>(
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
        .get_objects::<CustomEmoji>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<CustomEmoji>(caller, &account_id)
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
// CustomEmoji/changes
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/changes` method call (draft-atwood-jmap-chat-00 §CustomEmoji).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_emoji_changes<B: ChatBackend>(
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
        .get_changes::<CustomEmoji>(caller, &account_id, &since_state, max_changes)
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
// CustomEmoji/query
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/query` method call (draft-atwood-jmap-chat-00 §CustomEmoji).
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
pub async fn handle_emoji_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let calculate_total: bool = bool_arg(&args, "calculateTotal", false);

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

    // §CustomEmoji/query: "Default sort: `name` ascending."
    default_sort(&mut args, json!([{ "property": "name" }]));
    reject_unsupported_sort::<CustomEmojiComparator>(args.get("sort"))?;
    let filter: Option<CustomEmojiFilter> = filter_arg(&mut args)?;

    let sort: Option<Vec<CustomEmojiComparator>> = optional_arg(&mut args, "sort", || {
        JmapError::invalid_arguments("sort must be an array")
    })?;

    let result = backend
        .query_objects::<CustomEmoji>(
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
// CustomEmoji/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/queryChanges` method call (draft-atwood-jmap-chat-00 §CustomEmoji).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_emoji_query_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    mut args: Value,
) -> Result<HandlerOutput, JmapError> {
    // RFC 8620 §5.6: the `filter` and `sort` are those of the original
    // query, so the §CustomEmoji/query default sort applies here too.
    if let Value::Object(map) = &mut args {
        default_sort(map, json!([{ "property": "name" }]));
    }
    reject_unsupported_sort::<CustomEmojiComparator>(args.get("sort"))?;
    jmap_server::handle_query_changes::<CustomEmoji, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// CustomEmoji/set
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/set` method call (draft-atwood-jmap-chat-00 §CustomEmoji).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// Validation enforced here (not in the backend):
/// - `name` is required on create and must match `/^[a-z0-9_-]+$/`.
/// - `blobId` is required on create.
/// - `id`, `createdBy`, `createdAt`, `spaceId` are server-set or immutable and
///   rejected in updates.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// A `spaceId` that is a `#creationId` reference names a record created earlier in the request and resolves against
/// `created_ids` (pass [`jmap_server::CallContext::created_ids`]; RFC 8620
/// §5.3); an unknown reference is `invalidProperties` naming the property.
pub async fn handle_emoji_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2 accountNotFound (bd:JMAP-fo38).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<CustomEmoji>(caller, &account_id)
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
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, obj_val) in create_map {
            let resolved = match resolved_create(obj_val, &["spaceId"], created_ids) {
                Ok(v) => v,
                Err(err) => {
                    not_created.insert(create_id.clone(), set_error_value(&err));
                    continue;
                }
            };
            let obj_val = &resolved;
            if let Some(err) = obj_val
                .as_object()
                .and_then(reject_server_set::<CustomEmoji>)
            {
                not_created.insert(create_id.clone(), set_error_value(&err));
                continue;
            }
            // Optional properties: a wrongly-typed value is invalidProperties
            // (RFC 8620 §5.3), not a silent absence.
            if let Some(err) = mistyped_create_props(obj_val, &[("spaceId", PropType::Str)]) {
                not_created.insert(create_id.clone(), err);
                continue;
            }
            // Validate name: required, non-empty, matches /^[a-z0-9_-]+$/
            let name = match obj_val.get("name").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s.to_owned(),
                _ => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["name"] }),
                    );
                    continue;
                }
            };
            if !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
            {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["name"] }),
                );
                continue;
            }
            if name.len() > 64 {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["name"] }),
                );
                continue;
            }

            // blobId is required
            let blob_id = match obj_val
                .get("blobId")
                .and_then(|v| v.as_str())
                .and_then(|s| Id::new_validated(s).ok())
            {
                Some(id) => id,
                _ => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["blobId"] }),
                    );
                    continue;
                }
            };

            // spaceId is optional
            let space_id: Option<Id> = match obj_val.get("spaceId").and_then(|v| v.as_str()) {
                Some(s) => match jmap_server::parse_wire_id(s, "spaceId") {
                    Ok(id) => Some(id),
                    Err(err) => {
                        not_created.insert(create_id.clone(), set_error_value(&err));
                        continue;
                    }
                },
                None => None,
            };

            let now_str = now_utc_string();
            let now: UTCDate = UTCDate::from(now_str.as_ref());

            // Authorization gate (draft-atwood-jmap-chat-00 commit
            // `9344aec`). Runs AFTER wire-format validation so
            // malformed creates don't consume an authorization
            // decision, and BEFORE `create_object` so an unauthorized
            // emoji never touches storage. The target scope is
            // whatever spaceId the create payload supplies (None for
            // server-global).
            match backend
                .may_set_custom_emoji(caller, &account_id, space_id.as_ref(), EmojiSetOp::Create)
                .await
            {
                Ok(Ok(())) => {}
                Ok(Err(set_err)) => {
                    not_created.insert(create_id.clone(), set_error_value(&set_err));
                    continue;
                }
                Err(e) => {
                    not_created.insert(create_id.clone(), server_fail_value_from_backend(&e));
                    continue;
                }
            }

            // createdBy is the owner, referenced by the accountId
            // (draft-atwood-jmap-chat-00 §Terminology).
            let emoji = |id: &Id| {
                let mut emoji =
                    CustomEmoji::new(id.clone(), name, blob_id, account_id.clone(), now);
                emoji.space_id = space_id;
                emoji
            };

            match backend
                .create_object::<CustomEmoji, _>(caller, &account_id, create_id, emoji)
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

            // §CustomEmoji: `id`, `createdBy` and `createdAt` are
            // server-set; `spaceId` is set only on create.
            if let Some(err) = patch_val
                .as_object()
                .and_then(|m| reject_read_only_patch::<CustomEmoji>(m, None))
            {
                not_updated.insert(id_str, set_error_value(&err));
                continue;
            }

            // §CustomEmoji/set: "`update` supports: `name`, `blobId`", the
            // client-settable properties that are not immutable.
            let Value::Object(mut patch_map) = patch_val else {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
                );
                continue;
            };
            patch_map.retain(|field, _| is_client_updatable::<CustomEmoji>(field));
            let clean_patch = patch_map;
            if clean_patch.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "no updatable fields in patch" }),
                );
                continue;
            }

            // Authorization gate (draft-atwood-jmap-chat-00 commit
            // `9344aec`). Pre-fetch the existing emoji to learn its
            // `spaceId`. If the pre-fetch reports the id as not
            // found, skip the gate entirely — `update_object` will
            // surface `notFound` and we don't want to consume an
            // authorization decision for a non-existent target. A
            // pre-fetch storage error is surfaced as `serverFail`.
            let (found, _not_found) = match backend
                .get_objects::<CustomEmoji>(
                    caller,
                    &account_id,
                    Some(std::slice::from_ref(&id)),
                    None,
                )
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    not_updated.insert(id_str, server_fail_value_from_backend(&e));
                    continue;
                }
            };
            if let Some(emoji) = found.first() {
                match backend
                    .may_set_custom_emoji(
                        caller,
                        &account_id,
                        emoji.space_id.as_ref(),
                        EmojiSetOp::Update,
                    )
                    .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(set_err)) => {
                        not_updated.insert(id_str, set_error_value(&set_err));
                        continue;
                    }
                    Err(e) => {
                        not_updated.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            match backend
                .update_object::<CustomEmoji>(
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
    // destroy
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
            let id = match jmap_server::parse_wire_id(id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_destroyed.insert(id_str.to_owned(), set_error_value(&err));
                    continue;
                }
            };

            // Authorization gate (draft-atwood-jmap-chat-00 commit
            // `9344aec`). Pre-fetch the existing emoji to learn its
            // `spaceId`. If pre-fetch reports the id as not found,
            // skip the gate so `destroy_object` can surface
            // `notFound` naturally. A pre-fetch storage error is
            // surfaced as `serverFail`.
            let (found, _not_found) = match backend
                .get_objects::<CustomEmoji>(
                    caller,
                    &account_id,
                    Some(std::slice::from_ref(&id)),
                    None,
                )
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    not_destroyed.insert(id_str.to_owned(), server_fail_value_from_backend(&e));
                    continue;
                }
            };
            if let Some(emoji) = found.first() {
                match backend
                    .may_set_custom_emoji(
                        caller,
                        &account_id,
                        emoji.space_id.as_ref(),
                        EmojiSetOp::Destroy,
                    )
                    .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(set_err)) => {
                        not_destroyed.insert(id_str.to_owned(), set_error_value(&set_err));
                        continue;
                    }
                    Err(e) => {
                        not_destroyed.insert(id_str.to_owned(), server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            match backend
                .destroy_object::<CustomEmoji>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    not_destroyed.insert(id_str.to_owned(), backend_set_error_value(e));
                }
            }
        }
    }

    finalize_set_response::<B, CustomEmoji>(
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
