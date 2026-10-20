//! ReadPosition/* method handlers (draft-atwood-jmap-chat-00 §ReadPosition).
//!
//! ReadPosition tracks how far a user has read in a given Chat. There is at
//! most one ReadPosition per (account, chat) pair. The records are managed
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
//! §ReadPosition. The returned `Value` is the corresponding
//! method-response object per the same section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `cannotCalculateChanges` — per RFC 8620 §3.6 and §5). Per-target
//! failures inside `/set` (including the `forbidden` create/destroy
//! rejections) surface in the `notCreated` / `notUpdated` / `notDestroyed`
//! maps within `Ok((Value, ...))`, not as `Err`.

use jmap_chat_types::ReadPosition;
use jmap_types::{Id, JmapError, PatchObject, State};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, SetError, SetErrorType};
use crate::chat::record_unread_count_changes;
use crate::helpers::{
    enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response, not_found_json, now_utc_string, serialize_value, set_error_value,
    SetAccumulators,
};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend, HandlerOutput};

// ---------------------------------------------------------------------------
// ReadPosition/get
// ---------------------------------------------------------------------------

/// Handle a `ReadPosition/get` method call (draft-atwood-jmap-chat-00 §ReadPosition).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_position_get<B: ChatBackend>(
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

/// Handle a `ReadPosition/changes` method call (draft-atwood-jmap-chat-00 §ReadPosition).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_position_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    let since_state: State = match args.get("sinceState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceState is required")),
    };

    let max_changes: Option<u64> = match args.get("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().filter(|&n| n > 0).ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let result = backend
        .get_changes::<ReadPosition>(caller, &account_id, &since_state, max_changes)
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
// ReadPosition/set
// ---------------------------------------------------------------------------

/// Handle a `ReadPosition/set` method call (draft-atwood-jmap-chat-00 §ReadPosition).
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
///   rejected in updates.
/// - Every successful update sets `lastReadAt` to the current time.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_position_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<ReadPosition>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;
    let mut updated_ids: Vec<Id> = Vec::new();

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    // §ReadPosition/set: "`create` and `destroy` are not supported; both
    // MUST return a SetError of type `forbidden`. ReadPosition records are
    // managed by the server." The server creates them on first message
    // delivery (see `crate::message::handle_message_set`).
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for create_id in create_map.keys() {
            not_created.insert(create_id.clone(), forbidden_server_managed());
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

            // id and chatId are immutable; lastReadAt is server-set
            // (§ReadPosition: "`lastReadAt` (UTCDate, server-set)").
            const POSITION_READONLY: &[&str] = &["id", "chatId", "lastReadAt"];
            let bad_props: Vec<&str> = POSITION_READONLY
                .iter()
                .copied()
                .filter(|&field| patch_val.get(field).is_some())
                .collect();
            if !bad_props.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": bad_props }),
                );
                continue;
            }
            // `lastReadMessageId` (String, optional). Checked here because
            // the server-added `lastReadAt` below stops the backend's
            // re-validation from naming the one offending property.
            if patch_val
                .get("lastReadMessageId")
                .is_some_and(|v| !v.is_string() && !v.is_null())
            {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": ["lastReadMessageId"] }),
                );
                continue;
            }

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let mut patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
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
            match backend
                .update_object::<ReadPosition>(caller, &account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated_ids.push(id);
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Ok(None) => {
                    mutated = true;
                    updated_ids.push(id);
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

    if !updated_ids.is_empty() {
        if let Ok((updated_positions, _)) = backend
            .get_objects::<ReadPosition>(caller, &account_id, Some(&updated_ids), None)
            .await
        {
            let chat_ids = updated_positions.into_iter().map(|p| p.chat_id).collect();
            record_unread_count_changes(backend, caller, &account_id, chat_ids).await;
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
            not_destroyed.insert(id_str.to_owned(), forbidden_server_managed());
        }
    }

    finalize_set_response::<B, ReadPosition>(
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

/// The `forbidden` SetError for a client `create` or `destroy` of a
/// server-managed ReadPosition (draft-atwood-jmap-chat-00 §ReadPosition/set).
fn forbidden_server_managed() -> Value {
    set_error_value(
        &SetError::new(SetErrorType::Forbidden)
            .with_description("ReadPosition records are managed by the server"),
    )
}
