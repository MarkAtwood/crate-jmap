//! `SpaceBan`/* method handlers (draft-atwood-jmap-chat-00 §`SpaceBan`).
//!
//! [`SpaceBan`] supports get, changes, and set only (no query, no queryChanges).
//! `bannedBy` is always set server-side from the `accountId`; it is never
//! accepted from client request bodies.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3), with the
//! type-specific arguments defined by draft-atwood-jmap-chat-00 §`SpaceBan`.
//! The returned `Value` is the corresponding method-response object
//! per the same section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures (RFC 8620
//! §3.6.2 and §5):
//!
//! - every handler: `accountNotFound`, `invalidArguments`, `serverFail`;
//! - `/get`, `/set`: `requestTooLarge` past `maxObjectsInGet` /
//!   `maxObjectsInSet`;
//! - `/set`: `stateMismatch`;
//! - `/changes`: `cannotCalculateChanges`.
//!
//! Per-target failures inside `/set` surface in the `notCreated` / `notUpdated`
//! / `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use std::collections::BTreeMap;

use jmap_chat_types::SpaceBan;
use jmap_types::{Id, JmapError, PatchObject, UTCDate};
use serde_json::{Value, json};

use crate::ChatBackend;
use crate::helpers::check_read_only_patch;
use crate::helpers::resolved_create;
use crate::helpers::{PropType, mistyped_create_props};
use crate::helpers::{
    SetAccumulators, enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response, not_found_json, now_utc_string, serialize_value,
};
use jmap_server::{
    HandlerOutput, backend_set_error_value, is_client_updatable, reject_server_set,
    server_fail_from_backend, server_fail_value_from_backend,
};

// ---------------------------------------------------------------------------
// SpaceBan/get
// ---------------------------------------------------------------------------

/// Handle a `SpaceBan/get` method call (draft-atwood-jmap-chat-00 §`SpaceBan`).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Each returned ban must pass [`SpacePolicy::may_get_space_ban`](crate::SpacePolicy::may_get_space_ban); a
/// hidden ban is reported exactly like a missing one.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id, or
///   `ids` is not an Id array;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `requestTooLarge` when `ids` holds more than `maxObjectsInGet` ids;
/// - `serverFail` when the backend fails.
pub async fn handle_ban_get<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2 accountNotFound (bd:JMAP-dtpe).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
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

/// Handle a `SpaceBan/changes` method call (draft-atwood-jmap-chat-00 §`SpaceBan`).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id,
///   `sinceState` is missing, or `maxChanges` is not a positive integer;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `cannotCalculateChanges` when the backend cannot compute the changes
///   since `sinceState`;
/// - `serverFail` when the backend fails.
pub async fn handle_ban_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<SpaceBan, B>(backend, caller, args, &[]).await
}

// ---------------------------------------------------------------------------
// SpaceBan/set
// ---------------------------------------------------------------------------

/// Handle a `SpaceBan/set` method call (draft-atwood-jmap-chat-00 §`SpaceBan`).
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
///   and rejected in updates unless identical to the stored value (RFC 8620 §5.3); an empty patch is a
///   no-op success.
///
/// Create, update and destroy each consult
/// [`SpacePolicy::may_set_space_ban`](crate::SpacePolicy::may_set_space_ban) for the ban's Space.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// A `spaceId` that is a `#creationId` reference names a record created earlier in the request and resolves against
/// `created_ids` (pass [`jmap_server::CallContext::created_ids`]; RFC 8620
/// §5.3); an unknown reference is `invalidProperties` naming the property.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id, or
///   an element of `destroy` is not a string;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `requestTooLarge` when `create`, `update` and `destroy` together hold
///   more than `maxObjectsInSet` objects;
/// - `stateMismatch` when `ifInState` is not the current state;
/// - `serverFail` when the backend fails.
///
/// A create, update or destroy that fails on its own is a
/// [`SetError`](jmap_server::SetError) in the response, not an `Err`.
pub async fn handle_ban_set<B: ChatBackend>(
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
        .get_state::<SpaceBan>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    let ctx = SetCtx {
        backend,
        caller,
        account_id: &account_id,
        created_ids,
    };
    let mut acc = SetAccumulators::default();
    let mut mutated = set_create(&ctx, &mut args, &mut acc).await?;
    // RFC 8620 §5.3: update keys and destroy ids may name this call's creates.
    jmap_server::resolve_set_id_refs(&mut args, created_ids, &acc.created);
    mutated |= set_update(&ctx, &mut args, &mut acc).await?;
    mutated |= set_destroy(&ctx, &mut args, &mut acc).await?;

    finalize_set_response::<B, SpaceBan>(backend, caller, &account_id, old_state, mutated, acc)
        .await
}

/// What every `SpaceBan/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: ChatBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
    /// The request-wide `#creationId` map (RFC 8620 §5.3).
    created_ids: &'a BTreeMap<Id, Id>,
}

/// The `create` part of `SpaceBan/set`. Returns whether the backend was
/// written.
async fn set_create<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, obj_val) in create_map {
            mutated |= create_one(ctx, create_id, obj_val, acc).await?;
        }
    }
    Ok(mutated)
}

/// One `create` entry of `SpaceBan/set`. Returns whether the backend was
/// written.
async fn create_one<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    create_id: &str,
    obj_val: &Value,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let resolved = match resolved_create(obj_val, &["spaceId"], ctx.created_ids) {
        Ok(v) => v,
        Err(err) => {
            acc.not_created
                .insert(create_id.to_owned(), serde_json::Value::from(&*err));
            return Ok(false);
        }
    };
    let fields = match create_fields(&resolved) {
        Ok(fields) => fields,
        Err(err) => {
            acc.not_created.insert(create_id.to_owned(), err);
            return Ok(false);
        }
    };
    let CreateFields {
        space_id,
        user_id,
        reason,
        expires_at,
    } = fields;

    // bannedBy is always the acting caller's resolved identity
    // (foundation seam) — never from the client body. See
    // draft-atwood-jmap-chat-00 §SpaceBan.bannedBy.
    // draft-atwood-jmap-chat-00 §SpaceBan.bannedBy: "The
    // ChatContact.id of the Space member who issued this ban". The
    // issuer is the owner, referenced by the accountId
    // (§Terminology).
    let banned_by = ctx.account_id.clone();

    let now_str = now_utc_string();
    let created_at = UTCDate::from(now_str.as_ref());

    if let Some(err) = ban_denied(ctx.backend, ctx.caller, ctx.account_id, &space_id).await {
        acc.not_created.insert(create_id.to_owned(), err);
        return Ok(false);
    }

    let build = move |id: &Id| {
        let mut ban = SpaceBan::new(id.clone(), space_id, user_id, banned_by, created_at);
        ban.reason = reason;
        ban.expires_at = expires_at;
        ban
    };
    match ctx
        .backend
        .create_object::<SpaceBan, _>(ctx.caller, ctx.account_id, create_id, build)
        .await
    {
        Ok((_server_id, created_obj)) => {
            ctx.backend
                .on_space_ban_created(ctx.caller, ctx.account_id, &created_obj)
                .await;
            acc.created.insert(
                create_id.to_owned(),
                jmap_server::serialize_value(&created_obj)?,
            );
            Ok(true)
        }
        Err(e) => {
            acc.not_created
                .insert(create_id.to_owned(), backend_set_error_value(e));
            Ok(false)
        }
    }
}

/// The client-supplied properties of a `SpaceBan/set` create, checked.
struct CreateFields {
    space_id: Id,
    user_id: Id,
    reason: Option<String>,
    expires_at: Option<UTCDate>,
}

/// Check one `SpaceBan/set` create object (with `#creationId` references
/// resolved) and return its properties, or the `SetError` value for
/// `notCreated`.
fn create_fields(obj_val: &Value) -> Result<CreateFields, Value> {
    if let Some(err) = obj_val.as_object().and_then(reject_server_set::<SpaceBan>) {
        return Err(serde_json::Value::from(&err));
    }
    // Optional properties: a wrongly-typed value is invalidProperties
    // (RFC 8620 §5.3), not a silent absence.
    if let Some(err) = mistyped_create_props(
        obj_val,
        &[("reason", PropType::Str), ("expiresAt", PropType::Str)],
    ) {
        return Err(err);
    }
    let Some(space_id_str) = obj_val.get("spaceId").and_then(|v| v.as_str()) else {
        return Err(json!({ "type": "invalidProperties", "properties": ["spaceId"] }));
    };
    let space_id = jmap_server::parse_wire_id(space_id_str, "spaceId")
        .map_err(|err| serde_json::Value::from(&*err))?;

    let Some(user_id_str) = obj_val.get("userId").and_then(|v| v.as_str()) else {
        return Err(json!({ "type": "invalidProperties", "properties": ["userId"] }));
    };
    let user_id = jmap_server::parse_wire_id(user_id_str, "userId")
        .map_err(|err| serde_json::Value::from(&*err))?;

    let mut reason = None;
    if let Some(r) = obj_val.get("reason").and_then(|v| v.as_str()) {
        if r.len() > 1000 {
            return Err(json!({ "type": "invalidProperties", "properties": ["reason"] }));
        }
        reason = Some(r.to_owned());
    }
    // expiresAt is a UTCDate per RFC 8620 §1.4. Validate the wire
    // shape via UTCDate::new_validated; a malformed value produces
    // invalidProperties rather than storing an unparseable string.
    let mut expires_at = None;
    if let Some(expires) = obj_val.get("expiresAt").and_then(|v| v.as_str()) {
        let Ok(d) = UTCDate::new_validated(expires) else {
            return Err(json!({ "type": "invalidProperties", "properties": ["expiresAt"] }));
        };
        expires_at = Some(d);
    }
    Ok(CreateFields {
        space_id,
        user_id,
        reason,
        expires_at,
    })
}

/// The `update` part of `SpaceBan/set`. Returns whether the backend was
/// written.
async fn set_update<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            mutated |= update_one(ctx, id_str, patch_val, acc).await?;
        }
    }
    Ok(mutated)
}

/// One `update` entry of `SpaceBan/set`. Returns whether the backend was
/// written.
async fn update_one<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    id_str: String,
    mut patch_val: Value,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let id = match jmap_server::parse_wire_id(&id_str, "id") {
        Ok(id) => id,
        Err(err) => {
            acc.not_updated
                .insert(id_str, serde_json::Value::from(&*err));
            return Ok(false);
        }
    };

    // The permission gate runs before the read-only and no-op
    // checks, so a caller who may not update the ban gets
    // `forbidden` even for `{}`.
    if let Some(err) = stored_ban_denied(ctx.backend, ctx.caller, ctx.account_id, &id).await {
        acc.not_updated.insert(id_str, err);
        return Ok(false);
    }

    // §SpaceBan: `id`, `bannedBy` and `createdAt` are server-set,
    // `spaceId` and `userId` immutable.
    match check_read_only_patch::<_, SpaceBan>(
        ctx.backend,
        ctx.caller,
        ctx.account_id,
        &id,
        &mut patch_val,
    )
    .await
    {
        Ok(false) => {}
        Ok(true) => {
            acc.updated.insert(id_str, Value::Null);
            return Ok(false);
        }
        Err(err) => {
            acc.not_updated.insert(id_str, err);
            return Ok(false);
        }
    }

    // §SpaceBan/set: "`update` supports: `reason`, `expiresAt`", the
    // client-settable properties that are not immutable.
    let Value::Object(mut patch_map) = patch_val else {
        acc.not_updated.insert(
            id_str,
            json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
        );
        return Ok(false);
    };
    patch_map.retain(|field, _| is_client_updatable::<SpaceBan>(field));
    let clean_patch = patch_map;
    if clean_patch.is_empty() {
        acc.not_updated.insert(
            id_str,
            json!({ "type": "invalidPatch", "description": "no updatable fields in patch" }),
        );
        return Ok(false);
    }

    match ctx
        .backend
        .update_object::<SpaceBan>(
            ctx.caller,
            ctx.account_id,
            &id,
            PatchObject::from_map(clean_patch),
        )
        .await
    {
        Ok(Some(obj)) => {
            acc.updated
                .insert(id_str, jmap_server::serialize_value(&obj)?);
            Ok(true)
        }
        Ok(None) => {
            acc.updated.insert(id_str, Value::Null);
            Ok(true)
        }
        Err(e) => {
            acc.not_updated.insert(id_str, backend_set_error_value(e));
            Ok(false)
        }
    }
}

/// The `destroy` part of `SpaceBan/set`. Returns whether the backend was
/// written.
async fn set_destroy<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
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
                    acc.not_destroyed
                        .insert(id_str.to_owned(), serde_json::Value::from(&*err));
                    continue;
                }
            };

            if let Some(err) = stored_ban_denied(ctx.backend, ctx.caller, ctx.account_id, &id).await
            {
                acc.not_destroyed.insert(id_str.to_owned(), err);
                continue;
            }

            match ctx
                .backend
                .destroy_object::<SpaceBan>(ctx.caller, ctx.account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    acc.destroyed.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    acc.not_destroyed
                        .insert(id_str.to_owned(), backend_set_error_value(e));
                }
            }
        }
    }

    Ok(mutated)
}

/// draft-atwood-jmap-chat-00 §SpaceBan/set: create, update and destroy
/// each "Requires `"ban"` permission in the Space"; the backend decides.
/// Returns the `SetError` value to report when the op is denied.
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
        Ok(Err(set_err)) => Some(serde_json::Value::from(&set_err)),
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
