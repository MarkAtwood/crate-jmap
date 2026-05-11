//! Message/* method handlers (JMAP Chat extension §Message).

use jmap_chat_types::{DeliveryState, Message, SenderId};
use jmap_types::{Id, Invocation, JmapError, PatchObject, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend};
use std::collections::HashSet;

use crate::helpers::{
    extract_account_id, filter_properties, finalize_set_response, iso8601_before, not_found_json,
    now_utc_string, ser, set_error_value, SetAccumulators,
};

// ---------------------------------------------------------------------------
// Message/get
// ---------------------------------------------------------------------------

/// Handle a `Message/get` method call.
pub async fn handle_message_get<B: ChatBackend>(
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
        .get_objects::<Message>(caller, &account_id, ids_slice, properties.as_deref())
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<Message>(caller, &account_id)
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
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Message, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Message/query
// ---------------------------------------------------------------------------

/// Handle a `Message/query` method call (RFC 8620 §5.5).
///
/// Filter and sort are passed through to the backend unchanged.
pub async fn handle_message_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

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
            caller,
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
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, args) = extract_account_id(args)?;

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
            caller,
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
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let old_state = backend
        .get_state::<Message>(caller, &account_id)
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

            // senderExpiresAt is a UTCDate per RFC 8620 §1.4 (20-char
            // YYYY-MM-DDTHH:MM:SSZ). Validate the wire shape via
            // UTCDate::new_validated; a malformed value produces
            // invalidProperties rather than silently flowing through to
            // a downstream string compare with undefined ordering.
            let sender_expires_at: Option<UTCDate> =
                match obj_val.get("senderExpiresAt").and_then(|v| v.as_str()) {
                    Some(s) => match UTCDate::new_validated(s) {
                        Ok(d) => Some(d),
                        Err(_) => {
                            not_created.insert(
                                create_id.clone(),
                                json!({
                                    "type": "invalidProperties",
                                    "properties": ["senderExpiresAt"],
                                }),
                            );
                            continue;
                        }
                    },
                    None => None,
                };

            let burn_on_read: Option<bool> = obj_val.get("burnOnRead").and_then(|v| v.as_bool());

            if let Some(ref expires_at) = sender_expires_at {
                let now = now_utc_string();
                if !iso8601_before(now.as_str(), expires_at.as_ref()) {
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
            if let Some(d) = sender_expires_at {
                msg.sender_expires_at = Some(d);
            }
            msg.burn_on_read = burn_on_read;

            match backend
                .create_object::<Message>(caller, &account_id, create_id, msg)
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
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(create_id.clone(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
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
                .update_object::<Message>(caller, &account_id, &id, patch)
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
                    not_updated.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
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
                .destroy_object::<Message>(caller, &account_id, &id)
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

    finalize_set_response::<B, Message>(
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
