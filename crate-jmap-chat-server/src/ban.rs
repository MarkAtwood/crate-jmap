//! SpaceBan/* method handlers (draft-atwood-jmap-chat-00 §SpaceBan).
//!
//! SpaceBan supports get, changes, and set only (no query, no queryChanges).
//! `bannedBy` is always set server-side from the `accountId`; it is never
//! accepted from client request bodies.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3), with the
//! type-specific arguments defined by draft-atwood-jmap-chat-00 §SpaceBan.
//! The returned `Value` is the corresponding method-response object
//! per the same section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `cannotCalculateChanges` — per RFC 8620 §3.6 and §5). Per-target
//! failures inside `/set` surface in the `notCreated` / `notUpdated` /
//! `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use jmap_chat_types::SpaceBan;
use jmap_types::{Id, JmapError, PatchObject, UTCDate};
use serde_json::{json, Value};

use crate::helpers::{
    enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response, not_found_json, now_utc_string, serialize_value, set_error_value,
    SetAccumulators,
};
use crate::helpers::{mistyped_create_props, PropType};
use crate::{BackendSetError, ChatBackend};
use jmap_server::{
    is_client_updatable, reject_read_only_patch, reject_server_set, server_fail_from_backend,
    server_fail_value_from_backend, HandlerOutput,
};

// ---------------------------------------------------------------------------
// SpaceBan/get
// ---------------------------------------------------------------------------

/// Handle a `SpaceBan/get` method call (draft-atwood-jmap-chat-00 §SpaceBan).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Each returned ban must pass [`ChatBackend::may_get_space_ban`]; a
/// hidden ban is reported exactly like a missing one.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_ban_get<B: ChatBackend>(
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
    let (fetched, mut not_found) = backend
        .get_objects::<SpaceBan>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // A hidden ban is indistinguishable from a missing one: a requested
    // id goes to notFound, and a full fetch (`ids: null`) drops it, since
    // RFC 8620 §5.1 notFound lists only requested ids.
    let mut list = Vec::with_capacity(fetched.len());
    for ban in fetched {
        if backend
            .may_get_space_ban(caller, &account_id, &ban)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
        {
            list.push(ban);
        } else if ids_slice.is_some() {
            not_found.push(ban.id);
        }
    }

    let state = backend
        .get_state::<SpaceBan>(caller, &account_id)
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
// SpaceBan/changes
// ---------------------------------------------------------------------------

/// Handle a `SpaceBan/changes` method call (draft-atwood-jmap-chat-00 §SpaceBan).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_ban_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<SpaceBan, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// SpaceBan/set
// ---------------------------------------------------------------------------

/// Handle a `SpaceBan/set` method call (draft-atwood-jmap-chat-00 §SpaceBan).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// Validation enforced here (not in the backend):
/// - `spaceId` and `userId` are required on create.
/// - `bannedBy` is set server-side from `accountId`; never accepted from client.
/// - `id`, `spaceId`, `userId`, `bannedBy`, `createdAt` are server-set/immutable
///   and rejected in updates.
///
/// Create, update and destroy each consult
/// [`ChatBackend::may_set_space_ban`] for the ban's Space.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_ban_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<SpaceBan>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
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
            if let Some(err) = obj_val.as_object().and_then(reject_server_set::<SpaceBan>) {
                not_created.insert(create_id.clone(), set_error_value(&err));
                continue;
            }
            // Optional properties: a wrongly-typed value is invalidProperties
            // (RFC 8620 §5.3), not a silent absence.
            if let Some(err) = mistyped_create_props(
                obj_val,
                &[("reason", PropType::Str), ("expiresAt", PropType::Str)],
            ) {
                not_created.insert(create_id.clone(), err);
                continue;
            }
            let Some(space_id_str) = obj_val.get("spaceId").and_then(|v| v.as_str()) else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["spaceId"] }),
                );
                continue;
            };
            let space_id = match jmap_server::parse_wire_id(space_id_str, "spaceId") {
                Ok(id) => id,
                Err(err) => {
                    not_created.insert(create_id.clone(), set_error_value(&err));
                    continue;
                }
            };

            let Some(user_id_str) = obj_val.get("userId").and_then(|v| v.as_str()) else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["userId"] }),
                );
                continue;
            };
            let user_id = match jmap_server::parse_wire_id(user_id_str, "userId") {
                Ok(id) => id,
                Err(err) => {
                    not_created.insert(create_id.clone(), set_error_value(&err));
                    continue;
                }
            };

            // bannedBy is always the acting caller's resolved identity
            // (foundation seam) — never from the client body. See
            // draft-atwood-jmap-chat-00 §SpaceBan.bannedBy.
            // draft-atwood-jmap-chat-00 §SpaceBan.bannedBy: "The
            // ChatContact.id of the Space member who issued this ban". The
            // issuer is the owner, referenced by the accountId
            // (§Terminology).
            let banned_by = account_id.clone();

            let now_str = now_utc_string();
            let created_at = UTCDate::from(now_str.as_ref());

            let mut reason = None;
            if let Some(r) = obj_val.get("reason").and_then(|v| v.as_str()) {
                if r.len() > 1000 {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["reason"] }),
                    );
                    continue;
                }
                reason = Some(r.to_owned());
            }
            // expiresAt is a UTCDate per RFC 8620 §1.4. Validate the wire
            // shape via UTCDate::new_validated; a malformed value produces
            // invalidProperties rather than storing an unparseable string.
            let mut expires_at = None;
            if let Some(expires) = obj_val.get("expiresAt").and_then(|v| v.as_str()) {
                let Ok(d) = UTCDate::new_validated(expires) else {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["expiresAt"] }),
                    );
                    continue;
                };
                expires_at = Some(d);
            }

            if let Some(err) = ban_denied(backend, caller, &account_id, &space_id).await {
                not_created.insert(create_id.clone(), err);
                continue;
            }

            let build = move |id: &Id| {
                let mut ban = SpaceBan::new(id.clone(), space_id, user_id, banned_by, created_at);
                ban.reason = reason;
                ban.expires_at = expires_at;
                ban
            };
            match backend
                .create_object::<SpaceBan, _>(caller, &account_id, create_id, build)
                .await
            {
                Ok((_server_id, created_obj)) => {
                    mutated = true;
                    backend
                        .on_space_ban_created(caller, &account_id, &created_obj)
                        .await;
                    created.insert(
                        create_id.clone(),
                        serde_json::to_value(&created_obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(create_id.clone(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(create_id.clone(), server_fail_value_from_backend(&e));
                }
                Err(_) => {
                    not_created.insert(
                        create_id.clone(),
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
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

            // §SpaceBan: `id`, `bannedBy` and `createdAt` are server-set,
            // `spaceId` and `userId` immutable.
            if let Some(err) = patch_val
                .as_object()
                .and_then(|m| reject_read_only_patch::<SpaceBan>(m, None))
            {
                not_updated.insert(id_str, set_error_value(&err));
                continue;
            }

            // §SpaceBan/set: "`update` supports: `reason`, `expiresAt`", the
            // client-settable properties that are not immutable.
            let Value::Object(mut patch_map) = patch_val else {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
                );
                continue;
            };
            patch_map.retain(|field, _| is_client_updatable::<SpaceBan>(field));
            let clean_patch = patch_map;
            if clean_patch.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "no updatable fields in patch" }),
                );
                continue;
            }

            if let Some(err) = stored_ban_denied(backend, caller, &account_id, &id).await {
                not_updated.insert(id_str, err);
                continue;
            }

            match backend
                .update_object::<SpaceBan>(
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
                Err(BackendSetError::SetError(set_err)) => {
                    not_updated.insert(id_str, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_updated.insert(id_str, server_fail_value_from_backend(&e));
                }
                Err(_) => {
                    not_updated.insert(
                        id_str,
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
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
        if let Some(bad) = destroy_arr.iter().find(|v| !v.is_string()) {
            return Err(JmapError::invalid_arguments(format!(
                "destroy: every element must be a string Id; got {bad}"
            )));
        }
        for id_val in destroy_arr {
            let Some(id_str) = id_val.as_str() else {
                continue; // unreachable: validated above
            };
            let id = match jmap_server::parse_wire_id(id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_destroyed.insert(id_str.to_owned(), set_error_value(&err));
                    continue;
                }
            };

            if let Some(err) = stored_ban_denied(backend, caller, &account_id, &id).await {
                not_destroyed.insert(id_str.to_owned(), err);
                continue;
            }

            match backend
                .destroy_object::<SpaceBan>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str.to_owned()));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(id_str.to_owned(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(id_str.to_owned(), server_fail_value_from_backend(&e));
                }
                Err(_) => {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
                }
            }
        }
    }

    finalize_set_response::<B, SpaceBan>(
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

/// draft-atwood-jmap-chat-00 §SpaceBan/set: create, update and destroy
/// each "Requires `"ban"` permission in the Space"; the backend decides.
/// Returns the SetError value to report when the op is denied.
async fn ban_denied<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    space_id: &Id,
) -> Option<Value> {
    match backend
        .may_set_space_ban(caller, account_id, space_id)
        .await
    {
        Ok(Ok(())) => None,
        Ok(Err(set_err)) => Some(set_error_value(&set_err)),
        Err(e) => Some(server_fail_value_from_backend(&e)),
    }
}

/// [`ban_denied`] for the Space of the stored ban `id`. A ban that does
/// not resolve is not gated, so the backend surfaces `notFound`.
async fn stored_ban_denied<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Option<Value> {
    match backend
        .get_objects::<SpaceBan>(caller, account_id, Some(std::slice::from_ref(id)), None)
        .await
    {
        Ok((bans, _)) => match bans.first() {
            Some(ban) => ban_denied(backend, caller, account_id, &ban.space_id).await,
            None => None,
        },
        Err(e) => Some(server_fail_value_from_backend(&e)),
    }
}
