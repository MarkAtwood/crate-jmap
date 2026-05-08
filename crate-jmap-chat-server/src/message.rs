//! Message/* method handlers (JMAP Chat extension §Message).

use jmap_chat_types::{DeliveryState, Message, SenderId};
use jmap_types::{Id, Invocation, JmapError, PatchObject, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend};
use std::collections::HashSet;

use crate::helpers::{
    extract_account_id, filter_properties, iso8601_before, not_found_json, now_utc_string, ser,
    set_error_value,
};

// ---------------------------------------------------------------------------
// Message/get
// ---------------------------------------------------------------------------

/// Handle a `Message/get` method call.
pub async fn handle_message_get<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    let ids: Option<Vec<Id>> = match args.remove("ids").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    // RFC 8620 §5.1: when `properties` is specified, return only those fields
    // (plus `id` which is always included). `None` means return all fields.
    let properties: Option<Vec<String>> = match args.remove("properties").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<Message>(&account_id, ids_slice, properties.as_deref())
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<Message>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = if let Some(ref props) = properties {
        let mut prop_set: HashSet<&str> = props.iter().map(|s| s.as_str()).collect();
        prop_set.insert("id");
        list.iter()
            .map(|obj| {
                let val = ser(obj)?;
                Ok(filter_properties(&val, &prop_set))
            })
            .collect::<Result<Vec<_>, JmapError>>()?
    } else {
        list.iter().map(ser).collect::<Result<Vec<_>, _>>()?
    };

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
// Message/changes
// ---------------------------------------------------------------------------

/// Handle a `Message/changes` method call (RFC 8620 §5.2).
pub async fn handle_message_changes<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Message, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Message/query
// ---------------------------------------------------------------------------

/// Handle a `Message/query` method call (RFC 8620 §5.5).
///
/// Filter and sort are passed through to the backend unchanged.
pub async fn handle_message_query<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let limit: Option<u64> = match args.remove("limit").unwrap_or(Value::Null) {
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

    let position: i64 = match args.remove("position").unwrap_or(Value::Null) {
        Value::Null => 0,
        v => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("position: expected an integer, got {v}"))
        })?,
    };

    let filter: Option<serde_json::Value> = match args.remove("filter").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(v),
    };

    // JMAP Chat spec §Message — chatId filter is required unless hasMention: true.
    let has_chat_id = filter
        .as_ref()
        .and_then(|f| f.get("chatId"))
        .map(|v| !v.is_null())
        .unwrap_or(false);
    let has_mention_true = filter
        .as_ref()
        .and_then(|f| f.get("hasMention"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !has_chat_id && !has_mention_true {
        return Err(JmapError::unsupported_filter());
    }

    let sort: Option<Vec<serde_json::Value>> = match args.remove("sort").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("sort must be an array"))?,
        ),
    };

    let result = backend
        .query_objects::<Message>(
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
// Message/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Message/queryChanges` method call (RFC 8620 §5.6).
pub async fn handle_message_query_changes<B: ChatBackend>(
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
        .query_changes::<Message>(
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

// ---------------------------------------------------------------------------
// Message/set
// ---------------------------------------------------------------------------

/// Handle a `Message/set` method call.
///
/// Validation enforced here (not in the backend):
/// - `chatId` and `body` are required on create.
/// - `id`, `senderMsgId`, `senderId`, `sentAt`, `receivedAt`, `deliveryState`
///   are server-set and rejected in updates.
pub async fn handle_message_set<B: ChatBackend>(
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
        .get_state::<Message>(&account_id)
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
            let chat_id = match obj_val.get("chatId").and_then(|v| v.as_str()) {
                Some(s) => Id::from(s),
                None => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["chatId"] }),
                    );
                    continue;
                }
            };

            let body = match obj_val.get("body").and_then(|v| v.as_str()) {
                Some(s) => s.to_owned(),
                None => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["body"] }),
                    );
                    continue;
                }
            };
            if body.len() > 100_000 {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["body"] }),
                );
                continue;
            }

            let sent_at: UTCDate = match obj_val.get("sentAt").and_then(|v| v.as_str()) {
                Some(s) => UTCDate::from(s),
                None => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["sentAt"] }),
                    );
                    continue;
                }
            };

            let body_type = obj_val
                .get("bodyType")
                .and_then(|v| v.as_str())
                .unwrap_or("text/plain")
                .to_owned();

            let reply_to: Option<Id> = obj_val
                .get("replyTo")
                .and_then(|v| v.as_str())
                .map(Id::from);

            let thread_root_id: Option<Id> = obj_val
                .get("threadRootId")
                .and_then(|v| v.as_str())
                .map(Id::from);

            let sender_expires_at: Option<String> = obj_val
                .get("senderExpiresAt")
                .and_then(|v| v.as_str())
                .map(str::to_owned);

            let burn_on_read: Option<bool> = obj_val.get("burnOnRead").and_then(|v| v.as_bool());

            if let Some(ref expires_str) = sender_expires_at {
                let now = now_utc_string();
                if !iso8601_before(now.as_str(), expires_str.as_str()) {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["senderExpiresAt"] }),
                    );
                    continue;
                }
            }

            let now_str = now_utc_string();
            let received_at: UTCDate = UTCDate::from(now_str.as_str());

            let mut msg = Message::new(
                Id::from("placeholder"),
                Id::from(create_id.as_str()),
                SenderId::Owner,
                chat_id,
                body,
                body_type,
                vec![],
                vec![],
                vec![],
                std::collections::HashMap::new(),
                sent_at,
                received_at,
                DeliveryState::Pending,
            );

            msg.reply_to = reply_to;
            msg.thread_root_id = thread_root_id;
            if let Some(s) = sender_expires_at {
                msg.sender_expires_at = Some(UTCDate::from(s.as_str()));
            }
            msg.burn_on_read = burn_on_read;

            match backend
                .create_object::<Message>(&account_id, create_id, msg)
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

            // Reject patches that include server-set fields.
            // MAINTENANCE: when adding a server-set field to Message in
            // jmap-chat-types, add its wire name here too.  A missing entry
            // silently allows clients to overwrite server-managed state.
            const MESSAGE_READONLY: &[&str] = &[
                "id",
                "senderMsgId",
                "senderId",
                "chatId",
                "sentAt",
                "receivedAt",
                "deliveryState",
            ];
            let bad_props: Vec<&str> = MESSAGE_READONLY
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

            // Build augmented patch with server-side timestamps.
            let mut augmented = patch_val;
            if augmented.get("body").is_some() {
                if let Some(obj) = augmented.as_object_mut() {
                    obj.insert("editedAt".to_owned(), json!(now_utc_string()));
                }
            }
            if augmented.get("deletedForAll").and_then(|v| v.as_bool()) == Some(true) {
                if let Some(obj) = augmented.as_object_mut() {
                    obj.insert("deletedAt".to_owned(), json!(now_utc_string()));
                }
            }
            // Spec §557 + §1029: when readAt is set without readDisposition, default to "displayed".
            if let Some(obj) = augmented.as_object_mut() {
                if obj.contains_key("readAt") && !obj.contains_key("readDisposition") {
                    obj.insert("readDisposition".to_owned(), json!("displayed"));
                }
            }

            // Convert the augmented wire-format Value into a typed
            // PatchObject (RFC 8620 §5.3). Non-object values yield
            // invalidPatch.
            let patch = match serde_json::from_value::<PatchObject>(augmented) {
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
                .update_object::<Message>(&account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(id_str, serde_json::to_value(&obj).unwrap_or_else(|e| serde_json::json!({ "type": "serverFail", "description": e.to_string() })));
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

            match backend.destroy_object::<Message>(&account_id, &id).await {
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
            .get_state::<Message>(&account_id)
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
