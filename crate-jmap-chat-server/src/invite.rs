//! SpaceInvite/* method handlers (JMAP Chat extension §SpaceInvite).
//!
//! Methods: get, changes, set only (no query, no queryChanges per spec).
//! Updates are forbidden — the spec treats SpaceInvite as write-once.

use jmap_chat_types::SpaceInvite;
use jmap_types::{Id, Invocation, JmapError, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, SetError, SetErrorType};
use crate::helpers::{extract_account_id, not_found_json, now_utc_string, ser, set_error_value};

// ---------------------------------------------------------------------------
// SpaceInvite/get
// ---------------------------------------------------------------------------

/// Handle a `SpaceInvite/get` method call.
pub async fn handle_invite_get<B: ChatBackend>(
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
        .get_objects::<SpaceInvite>(&account_id, ids_slice, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<SpaceInvite>(&account_id)
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
// SpaceInvite/changes
// ---------------------------------------------------------------------------

/// Handle a `SpaceInvite/changes` method call (RFC 8620 §5.2).
pub async fn handle_invite_changes<B: ChatBackend>(
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
        .get_changes::<SpaceInvite>(&account_id, &since_state, max_changes)
        .await
        .map_err(JmapError::from)?;

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "oldState": since_state.as_ref(),
            "newState": result.new_state.as_ref(),
            "hasMoreChanges": result.has_more_changes,
            "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
            "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
            "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// SpaceInvite/set
// ---------------------------------------------------------------------------

/// Handle a `SpaceInvite/set` method call.
///
/// Per spec, SpaceInvite objects are write-once:
/// - create: accepted (`spaceId` required; server sets `code`, `createdBy`,
///   `uses`, `createdAt`).
/// - update: always rejected with `forbidden`.
/// - destroy: allowed.
pub async fn handle_invite_set<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let old_state = backend
        .get_state::<SpaceInvite>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, obj_val) in create_map {
            // spaceId is required on create.
            let space_id = match obj_val.get("spaceId").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => Id::from(s),
                _ => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["spaceId"] }),
                    );
                    continue;
                }
            };

            let default_channel_id: Option<Id> = obj_val
                .get("defaultChannelId")
                .and_then(|v| v.as_str())
                .map(Id::from);

            let expires_at: Option<UTCDate> = obj_val
                .get("expiresAt")
                .and_then(|v| v.as_str())
                .map(UTCDate::from);

            let max_uses: Option<u64> = obj_val.get("maxUses").and_then(|v| v.as_u64());
            if max_uses == Some(0) {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["maxUses"] }),
                );
                continue;
            }

            let now_str = now_utc_string();
            let now: UTCDate = UTCDate::from(now_str.as_str());

            // Generate a short URL-safe invite code from the current nanosecond
            // timestamp.  Not cryptographically random but sufficient for a
            // MemoryBackend / test context.
            let code = format!(
                "{:012x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
                    & 0xffff_ffff_ffff
            );

            // Security: createdBy MUST be set server-side from accountId,
            // never accepted from the client body.
            let invite = SpaceInvite::new(
                Id::from("placeholder"),
                code,
                space_id,
                account_id.clone(),
                0,
                now,
                default_channel_id,
                expires_at,
                max_uses,
            );

            match backend
                .create_object::<SpaceInvite>(&account_id, create_id, invite)
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
    // update — forbidden: SpaceInvite objects are write-once per spec
    // -----------------------------------------------------------------------
    if let Some(update_map) = args.get("update").and_then(|v| v.as_object()) {
        for id_str in update_map.keys() {
            not_updated.insert(
                id_str.clone(),
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
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

            match backend
                .destroy_object::<SpaceInvite>(&account_id, &id)
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
            .get_state::<SpaceInvite>(&account_id)
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
