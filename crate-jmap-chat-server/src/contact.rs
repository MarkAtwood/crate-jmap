//! ChatContact/* method handlers (JMAP Chat extension §ChatContact).

use jmap_chat_types::ChatContact;
use jmap_types::{Id, Invocation, JmapError, State};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, SetError, SetErrorType};
use crate::helpers::{extract_account_id, not_found_json, ser, set_error_value};

// ---------------------------------------------------------------------------
// ChatContact/get
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/get` method call.
pub async fn handle_contact_get<B: ChatBackend>(
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
        .get_objects::<ChatContact>(&account_id, ids_slice, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<ChatContact>(&account_id)
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
// ChatContact/changes
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/changes` method call (RFC 8620 §5.2).
pub async fn handle_contact_changes<B: ChatBackend>(
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
        .get_changes::<ChatContact>(&account_id, &since_state, max_changes)
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
// ChatContact/set
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/set` method call.
///
/// ChatContact objects are server-managed. Clients MUST NOT create or destroy
/// them; any attempt is rejected with `forbidden`. Only `update` is permitted,
/// and `id`, `firstSeenAt`, `lastSeenAt` are server-set and rejected in updates.
pub async fn handle_contact_set<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let old_state = backend
        .get_state::<ChatContact>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

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

    // -----------------------------------------------------------------------
    // create — forbidden: ChatContact objects are server-managed
    // -----------------------------------------------------------------------
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for create_id in create_map.keys() {
            not_created.insert(
                create_id.clone(),
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(update_map) = args.get("update").and_then(|v| v.as_object()) {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

            // Reject patches that include server-set fields.
            const CONTACT_READONLY: &[&str] = &["id", "firstSeenAt", "lastSeenAt"];
            let bad_props: Vec<&str> = CONTACT_READONLY
                .iter()
                .copied()
                .filter(|&field| patch_val.get(field).is_some())
                .collect();
            if !bad_props.is_empty() {
                not_updated.insert(
                    id_str.clone(),
                    json!({ "type": "invalidProperties", "properties": bad_props }),
                );
                continue;
            }

            match backend
                .update_object::<ChatContact>(&account_id, &id, patch_val.clone())
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(
                        id_str.clone(),
                        serde_json::to_value(&obj).unwrap_or(Value::Null),
                    );
                }
                Ok(None) => {
                    mutated = true;
                    updated.insert(id_str.clone(), Value::Null);
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_updated.insert(id_str.clone(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_updated.insert(
                        id_str.clone(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy — forbidden: ChatContact objects are server-managed
    // -----------------------------------------------------------------------
    if let Some(destroy_arr) = args.get("destroy").and_then(|v| v.as_array()) {
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            not_destroyed.insert(
                id_str.to_owned(),
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<ChatContact>(&account_id)
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

// ---------------------------------------------------------------------------
// ChatContact/query
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/query` method call (RFC 8620 §5.5).
///
/// Filter and sort are passed through to the backend unchanged.
pub async fn handle_contact_query<B: ChatBackend>(
    backend: &B,
    mut args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let limit: Option<u64> = match args["limit"].take() {
        Value::Null => None,
        v => match v.as_u64() {
            Some(n) => Some(n),
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {v}"
                )))
            }
        },
    };

    let position: i64 = match args["position"].take() {
        Value::Null => 0,
        v => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("position: expected an integer, got {v}"))
        })?,
    };

    let filter: Option<serde_json::Value> = match args["filter"].take() {
        Value::Null => None,
        v => Some(v),
    };

    let sort: Option<Vec<serde_json::Value>> = match args["sort"].take() {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("sort must be an array"))?,
        ),
    };

    let result = backend
        .query_objects::<ChatContact>(
            &account_id,
            filter.as_ref(),
            sort.as_deref(),
            limit,
            position,
        )
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": result.query_state.as_ref(),
        "canCalculateChanges": result.can_calculate_changes,
        "position": result.position,
        "ids": result.ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total {
        if let Some(t) = result.total {
            resp["total"] = json!(t);
        }
    }

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// ChatContact/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/queryChanges` method call (RFC 8620 §5.6).
pub async fn handle_contact_query_changes<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let since_query_state: State = match args.get("sinceQueryState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceQueryState is required")),
    };

    let max_changes: Option<u64> = match args.get("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().filter(|&n| n > 0).ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let up_to_id: Option<Id> = match args.get("upToId") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Id::from(s.as_str())),
        Some(_) => {
            return Err(JmapError::invalid_arguments(
                "upToId must be a string Id or null",
            ))
        }
    };

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let result = backend
        .query_changes::<ChatContact>(
            &account_id,
            &since_query_state,
            None,
            None,
            max_changes,
            up_to_id.as_ref(),
            false,
        )
        .await
        .map_err(JmapError::from)?;

    let removed: Vec<&str> = result.removed.iter().map(|id| id.as_ref()).collect();
    let added: Vec<Value> = result
        .added
        .iter()
        .map(|item| {
            json!({
                "id": item.id.as_ref(),
                "index": item.index,
            })
        })
        .collect();

    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "oldQueryState": result.old_query_state.as_ref(),
        "newQueryState": result.new_query_state.as_ref(),
        "removed": removed,
        "added": added,
    });
    if calculate_total {
        if let Some(t) = result.total {
            resp["total"] = json!(t);
        }
    }

    Ok((resp, vec![]))
}
