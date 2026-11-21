//! `ReadPosition`/* method handlers (draft-atwood-jmap-chat-00 §`ReadPosition`).
//!
//! [`ReadPosition`] tracks how far a user has read in a given Chat. There is at
//! most one `ReadPosition` per (account, chat) pair. The records are managed
//! by the server: `ReadPosition/set` supports only `update`; `create` and
//! `destroy` return `forbidden`. The record itself is created on first
//! message delivery by [`crate::message::handle_message_set`] or, for
//! channel Chats, on [`crate::space::handle_space_join`], and is destroyed
//! with its Chat by the backend.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3), with the
//! type-specific arguments defined by draft-atwood-jmap-chat-00
//! §`ReadPosition`. The returned `Value` is the corresponding
//! method-response object per the same section refs.
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
//! Per-target failures inside `/set` (including the `forbidden` create/destroy
//! rejections) surface in the `notCreated` / `notUpdated` / `notDestroyed` maps
//! within `Ok((Value, ...))`, not as `Err`.

use std::collections::BTreeMap;

use jmap_chat_types::ReadPosition;
use jmap_types::{Id, JmapError, PatchObject, State};
use serde_json::{Value, json};

use crate::ChatBackend;
use crate::chat::record_unread_count_changes;
use crate::helpers::check_read_only_patch;
use crate::helpers::resolve_refs;
use crate::helpers::{
    SetAccumulators, enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response, not_found_json, now_utc_string, serialize_value,
};
use jmap_server::{HandlerOutput, backend_set_error_value, server_fail_from_backend};
use jmap_server::{SetError, SetErrorType};

// ---------------------------------------------------------------------------
// ReadPosition/get
// ---------------------------------------------------------------------------

/// Handle a `ReadPosition/get` method call (draft-atwood-jmap-chat-00 §`ReadPosition`).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
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
pub async fn handle_position_get<B: ChatBackend>(
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
    let (list, not_found) = backend
        .get_objects::<ReadPosition>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<ReadPosition>(caller, &account_id)
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
// ReadPosition/changes
// ---------------------------------------------------------------------------

/// Handle a `ReadPosition/changes` method call (draft-atwood-jmap-chat-00 §`ReadPosition`).
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
pub async fn handle_position_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2 accountNotFound (bd:JMAP-dtpe).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let since_state: State = match args.get("sinceState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceState is required")),
    };

    let max_changes = jmap_server::parse_max_changes(args.get("maxChanges"))?;

    let result = backend
        .get_changes::<ReadPosition>(
            caller,
            &account_id,
            jmap_server::ChangesParams::new(&since_state).max_changes(max_changes),
        )
        .await
        .map_err(jmap_server::changes_error_from_backend)?;

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
// ReadPosition/set
// ---------------------------------------------------------------------------

/// Handle a `ReadPosition/set` method call (draft-atwood-jmap-chat-00 §`ReadPosition`).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// Validation enforced here (not in the backend):
/// - `create` and `destroy` are rejected with `forbidden`.
/// - `id`, `chatId` and `lastReadAt` are server-set/immutable and
///   rejected in updates unless identical to the stored value (RFC 8620 §5.3).
/// - Every successful update sets `lastReadAt` to the current time,
///   except an empty patch, which is a no-op success.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// A `lastReadMessageId` that is a `#creationId` reference names a record created earlier in the request and resolves against
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
/// An update that fails on its own, and every create and destroy, is a
/// [`SetError`] in the response, not an `Err`.
pub async fn handle_position_set<B: ChatBackend>(
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
        .get_state::<ReadPosition>(caller, &account_id)
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
    set_create(&args, &mut acc);
    let mutated = set_update(&ctx, &mut args, &mut acc).await?;
    set_destroy(&args, &mut acc)?;

    finalize_set_response::<B, ReadPosition>(backend, caller, &account_id, old_state, mutated, acc)
        .await
}

/// What every `ReadPosition/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: ChatBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
    /// The request-wide `#creationId` map (RFC 8620 §5.3).
    created_ids: &'a BTreeMap<Id, Id>,
}

/// The `create` part of `ReadPosition/set`: forbidden.
///
/// §ReadPosition/set: "`create` and `destroy` are not supported; both
/// MUST return a [`SetError`] of type `forbidden`. [`ReadPosition`] records are
/// managed by the server." The server creates them on first message
/// delivery (see `crate::message::handle_message_set`).
fn set_create(args: &serde_json::Map<String, Value>, acc: &mut SetAccumulators) {
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for create_id in create_map.keys() {
            acc.not_created
                .insert(create_id.clone(), forbidden_server_managed());
        }
    }
}

/// The `update` part of `ReadPosition/set`, then the `Chat.unreadCount`
/// recompute for the updated positions. Returns whether the backend was
/// written.
async fn set_update<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    let mut updated_ids: Vec<Id> = Vec::new();
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            if let Some(id) = update_one(ctx, id_str, patch_val, acc).await? {
                mutated = true;
                updated_ids.push(id);
            }
        }
    }

    if !updated_ids.is_empty()
        && let Ok((updated_positions, _)) = ctx
            .backend
            .get_objects::<ReadPosition>(ctx.caller, ctx.account_id, Some(&updated_ids), None)
            .await
    {
        let chat_ids = updated_positions.into_iter().map(|p| p.chat_id).collect();
        record_unread_count_changes(ctx.backend, ctx.caller, ctx.account_id, chat_ids).await;
    }
    Ok(mutated)
}

/// One `update` entry of `ReadPosition/set`. Returns the id when the
/// backend was written.
async fn update_one<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    id_str: String,
    mut patch_val: Value,
    acc: &mut SetAccumulators,
) -> Result<Option<Id>, JmapError> {
    let id = match jmap_server::parse_wire_id(&id_str, "id") {
        Ok(id) => id,
        Err(err) => {
            acc.not_updated
                .insert(id_str, serde_json::Value::from(&*err));
            return Ok(None);
        }
    };
    if let Err(err) = resolve_refs(&mut patch_val, &["lastReadMessageId"], ctx.created_ids) {
        acc.not_updated
            .insert(id_str, serde_json::Value::from(&*err));
        return Ok(None);
    }

    // §ReadPosition: `id` and `lastReadAt` are server-set, `chatId`
    // is immutable.
    match check_read_only_patch::<_, ReadPosition>(
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
            return Ok(None);
        }
        Err(err) => {
            acc.not_updated.insert(id_str, err);
            return Ok(None);
        }
    }
    // `lastReadMessageId` (String, optional). Checked here because
    // the server-added `lastReadAt` below stops the backend's
    // re-validation from naming the one offending property.
    if patch_val
        .get("lastReadMessageId")
        .is_some_and(|v| !v.is_string() && !v.is_null())
    {
        acc.not_updated.insert(
            id_str,
            json!({ "type": "invalidProperties", "properties": ["lastReadMessageId"] }),
        );
        return Ok(None);
    }

    // Convert wire-format Value into a typed PatchObject. RFC 8620
    // §5.3 mandates a PatchObject is a JSON Object; non-object
    // values produce an `invalidPatch` SetError.
    let mut patch = match serde_json::from_value::<PatchObject>(patch_val) {
        Ok(p) => p,
        Err(e) => {
            acc.not_updated.insert(
                id_str,
                json!({ "type": "invalidPatch", "description": e.to_string() }),
            );
            return Ok(None);
        }
    };
    // §ReadPosition/set: "The server sets `lastReadAt` to the
    // current time and recomputes `Chat.unreadCount` for the
    // affected Chat." The count is written to the Chat after the
    // update loop (`record_unread_count_changes`).
    patch.as_map_mut().insert(
        "lastReadAt".to_owned(),
        Value::String(now_utc_string().into_inner()),
    );
    match ctx
        .backend
        .update_object::<ReadPosition>(ctx.caller, ctx.account_id, &id, patch)
        .await
    {
        Ok(Some(obj)) => {
            acc.updated
                .insert(id_str, jmap_server::serialize_value(&obj)?);
            Ok(Some(id))
        }
        Ok(None) => {
            acc.updated.insert(id_str, Value::Null);
            Ok(Some(id))
        }
        Err(e) => {
            acc.not_updated.insert(id_str, backend_set_error_value(e));
            Ok(None)
        }
    }
}

/// The `destroy` part of `ReadPosition/set`: forbidden (see
/// [`set_create`]).
fn set_destroy(
    args: &serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<(), JmapError> {
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
            acc.not_destroyed
                .insert(id_str.to_owned(), forbidden_server_managed());
        }
    }
    Ok(())
}

/// The `forbidden` [`SetError`] for a client `create` or `destroy` of a
/// server-managed [`ReadPosition`] (draft-atwood-jmap-chat-00 §ReadPosition/set).
fn forbidden_server_managed() -> Value {
    serde_json::Value::from(
        &SetError::new(SetErrorType::Forbidden)
            .with_description("ReadPosition records are managed by the server"),
    )
}
