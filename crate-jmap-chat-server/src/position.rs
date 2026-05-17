//! ReadPosition/* method handlers (draft-atwood-jmap-chat-00 §ReadPosition).
//!
//! ReadPosition tracks how far a user has read in a given Chat. There is at
//! most one ReadPosition per (account, chat) pair. Create and destroy are
//! supported (unlike singletons), but each chat's read position is unique.
//!
//! # Uniqueness contract
//!
//! The handler in this module pre-checks the (account, chatId) uniqueness
//! invariant on every create — see the create branch of
//! [`handle_position_set`] — and rejects sequential and intra-batch
//! duplicates with `alreadyExists`. The pre-check is defense-in-depth only:
//! two concurrent `ReadPosition/set` requests for the same chatId can both
//! pass the pre-check, so backends MUST enforce the uniqueness constraint
//! atomically with the create. See the "Per-type uniqueness contracts"
//! section on [`crate::backend::ChatBackend::create_object`] for the full
//! contract.
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
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `cannotCalculateChanges` — per RFC 8620 §3.6 and §5). Per-target
//! failures inside `/set` (including the uniqueness `alreadyExists`
//! rejection) surface in the `notCreated` / `notUpdated` / `notDestroyed`
//! maps within `Ok((Value, ...))`, not as `Err`.

use jmap_chat_types::ReadPosition;
use jmap_types::{Id, Invocation, JmapError, PatchObject, State};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, SetError, SetErrorType};
use crate::helpers::{
    enforce_max_objects_in_set, extract_account_id, finalize_set_response, not_found_json,
    serialize_value, set_error_value, SetAccumulators,
};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend};

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
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let ids: Option<Vec<Id>> = match args.remove("ids").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
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

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "state": state.as_ref(),
            "list": list_json,
            "notFound": not_found_json(&not_found),
        }),
        vec![],
    ))
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
) -> Result<(Value, Vec<Invocation>), JmapError> {
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

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "oldState": since_state.as_ref(),
            "newState": result.new_state.as_ref(),
            "hasMoreChanges": result.has_more_changes,
            "updatedProperties": Value::Null,
            "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
            "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
            "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        }),
        vec![],
    ))
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
/// - `chatId` is required on create.
/// - `id` and `chatId` are server-set/immutable and rejected in updates.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_position_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
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
        // The (account, chatId) -> ReadPosition uniqueness invariant (this
        // module's top-level doc) is enforced here in the handler. Without
        // this check, a client calling ReadPosition/set create twice for
        // the same chatId — common on retry paths — would produce two
        // ReadPosition records, leaving Chat.unreadCount derivation
        // (draft-atwood-jmap-chat-00 §Chat) ambiguous. The hoisted fetch
        // is paid only when the batch contains at least one create, and
        // the in-batch HashMap covers multiple creates that target the
        // same chatId in a single request. Production backends still need
        // to re-verify atomically on the create_object call to defend
        // against concurrent /set requests racing the pre-check.
        let mut existing_positions: Vec<ReadPosition> = Vec::new();
        if !create_map.is_empty() {
            let (positions, _) = backend
                .get_objects::<ReadPosition>(caller, &account_id, None, None)
                .await
                .map_err(|e| server_fail_from_backend(&e))?;
            existing_positions = positions;
        }
        let mut batch_chat_ids: std::collections::HashMap<String, Id> =
            std::collections::HashMap::new();

        for (create_id, obj_val) in create_map {
            let Some(chat_id_str) = obj_val.get("chatId").and_then(|v| v.as_str()) else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["chatId"] }),
                );
                continue;
            };
            let chat_id = Id::from(chat_id_str);

            // Reject if a ReadPosition already exists for this chatId
            // (either pre-existing or created earlier in this batch). Per
            // the Chat/set Direct dedup pattern, return alreadyExists with
            // the canonical id so the caller can re-target the existing
            // record via ReadPosition/set update.
            if let Some(dup) = existing_positions
                .iter()
                .find(|p| p.chat_id.as_ref() == chat_id.as_ref())
            {
                not_created.insert(
                    create_id.clone(),
                    serde_json::to_value(
                        SetError::new(SetErrorType::AlreadyExists).with_existing_id(dup.id.clone()),
                    )
                    .expect("derive(Serialize) on plain data is infallible"),
                );
                continue;
            }
            if let Some(prior_id) = batch_chat_ids.get(chat_id.as_ref()) {
                not_created.insert(
                    create_id.clone(),
                    serde_json::to_value(
                        SetError::new(SetErrorType::AlreadyExists)
                            .with_existing_id(prior_id.clone()),
                    )
                    .expect("derive(Serialize) on plain data is infallible"),
                );
                continue;
            }

            let mut position = ReadPosition::new(Id::from("placeholder"), chat_id.clone());

            if let Some(msg_id) = obj_val.get("lastReadMessageId").and_then(|v| v.as_str()) {
                position.last_read_message_id = Some(Id::from(msg_id));
            }
            // lastReadAt is a UTCDate per RFC 8620 §1.4 (20-char
            // YYYY-MM-DDTHH:MM:SSZ). Validate the wire shape via
            // UTCDate::new_validated; a malformed value produces
            // invalidProperties rather than silently flowing through
            // to storage with undefined comparison ordering.
            if let Some(at) = obj_val.get("lastReadAt").and_then(|v| v.as_str()) {
                let Ok(d) = jmap_types::UTCDate::new_validated(at) else {
                    not_created.insert(
                        create_id.clone(),
                        json!({
                            "type": "invalidProperties",
                            "properties": ["lastReadAt"],
                        }),
                    );
                    continue;
                };
                position.last_read_at = Some(d);
            }

            match backend
                .create_object::<ReadPosition>(caller, &account_id, create_id, position)
                .await
            {
                Ok((server_id, created_obj)) => {
                    mutated = true;
                    // Record the just-assigned id so later iterations in
                    // this same batch can detect duplicates targeting the
                    // same chatId.
                    batch_chat_ids.insert(chat_id.as_ref().to_owned(), server_id);
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
            let id = Id::from(id_str.as_str());

            // id and chatId are immutable after creation.
            const POSITION_READONLY: &[&str] = &["id", "chatId"];
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

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };
            match backend
                .update_object::<ReadPosition>(caller, &account_id, &id, patch)
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
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue, // unreachable: validated above
            };
            let id = Id::from(id_str);

            match backend
                .destroy_object::<ReadPosition>(caller, &account_id, &id)
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
