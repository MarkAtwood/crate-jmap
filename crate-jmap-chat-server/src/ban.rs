//! SpaceBan/* method handlers (JMAP Chat extension §SpaceBan).
//!
//! SpaceBan supports get, changes, and set only (no query, no queryChanges).
//! `bannedBy` is always set server-side from the `accountId`; it is never
//! accepted from client request bodies.

use jmap_chat_types::SpaceBan;
use jmap_types::{Id, Invocation, JmapError, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend};
use crate::helpers::{extract_account_id, not_found_json, now_utc_string, ser, set_error_value};

// ---------------------------------------------------------------------------
// SpaceBan/get
// ---------------------------------------------------------------------------

/// Handle a `SpaceBan/get` method call.
pub async fn handle_ban_get<B: ChatBackend>(
    backend: &B,
    mut args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let ids: Option<Vec<Id>> = match args["ids"].take() {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<SpaceBan>(&account_id, ids_slice, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<SpaceBan>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = list.iter().map(ser).collect::<Result<Vec<_>, _>>()?;

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
// SpaceBan/changes
// ---------------------------------------------------------------------------

/// Handle a `SpaceBan/changes` method call (RFC 8620 §5.2).
pub async fn handle_ban_changes<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

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
        .get_changes::<SpaceBan>(&account_id, &since_state, max_changes)
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
// SpaceBan/set
// ---------------------------------------------------------------------------

/// Handle a `SpaceBan/set` method call.
///
/// Validation enforced here (not in the backend):
/// - `spaceId` and `userId` are required on create.
/// - `bannedBy` is set server-side from `accountId`; never accepted from client.
/// - `id`, `spaceId`, `userId`, `bannedBy`, `createdAt` are server-set/immutable
///   and rejected in updates.
pub async fn handle_ban_set<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    let old_state = backend
        .get_state::<SpaceBan>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

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
            let space_id = match obj_val.get("spaceId").and_then(|v| v.as_str()) {
                Some(s) => Id::from(s),
                None => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["spaceId"] }),
                    );
                    continue;
                }
            };

            let user_id = match obj_val.get("userId").and_then(|v| v.as_str()) {
                Some(s) => Id::from(s),
                None => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["userId"] }),
                    );
                    continue;
                }
            };

            // bannedBy is always the acting account — never from the client body.
            let banned_by = account_id.clone();

            let now_str = now_utc_string();
            let created_at = UTCDate::from(now_str.as_str());

            let mut ban = SpaceBan::new(
                Id::from("placeholder"),
                space_id,
                user_id,
                banned_by,
                created_at,
            );

            if let Some(reason) = obj_val.get("reason").and_then(|v| v.as_str()) {
                if reason.len() > 1000 {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["reason"] }),
                    );
                    continue;
                }
                ban.reason = Some(reason.to_owned());
            }
            if let Some(expires) = obj_val.get("expiresAt").and_then(|v| v.as_str()) {
                ban.expires_at = Some(UTCDate::from(expires));
            }

            match backend
                .create_object::<SpaceBan>(&account_id, create_id, ban)
                .await
            {
                Ok((_server_id, created_obj)) => {
                    mutated = true;
                    created.insert(
                        create_id.clone(),
                        serde_json::to_value(&created_obj).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(create_id.clone(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
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

            // These fields are server-set or immutable after creation.
            const BAN_READONLY: &[&str] = &["id", "spaceId", "userId", "bannedBy", "createdAt"];
            let bad_props: Vec<&str> = BAN_READONLY
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

            const BAN_UPDATE_ALLOWED: &[&str] = &["reason", "expiresAt"];
            let Value::Object(mut patch_map) = patch_val else {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
                );
                continue;
            };
            let mut clean_patch = serde_json::Map::new();
            for &field in BAN_UPDATE_ALLOWED {
                if let Some(v) = patch_map.remove(field) {
                    clean_patch.insert(field.to_owned(), v);
                }
            }
            if clean_patch.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "no updatable fields in patch" }),
                );
                continue;
            }

            match backend
                .update_object::<SpaceBan>(&account_id, &id, Value::Object(clean_patch))
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(id_str, serde_json::to_value(&obj).unwrap_or(Value::Null));
                }
                Ok(None) => {
                    mutated = true;
                    updated.insert(id_str, Value::Null);
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_updated.insert(id_str, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------
    if let Some(destroy_arr) = args.get("destroy").and_then(|v| v.as_array()) {
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            let id = Id::from(id_str);

            match backend.destroy_object::<SpaceBan>(&account_id, &id).await {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str.to_owned()));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(id_str.to_owned(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<SpaceBan>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    } else {
        old_state.clone()
    };

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "oldState": old_state.as_ref(),
            "newState": new_state.as_ref(),
            "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
            "updated": if updated.is_empty() { Value::Null } else { Value::Object(updated) },
            "destroyed": if destroyed_list.is_empty() { Value::Null } else { Value::Array(destroyed_list) },
            "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
            "notUpdated": if not_updated.is_empty() { Value::Null } else { Value::Object(not_updated) },
            "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
        }),
        vec![],
    ))
}
