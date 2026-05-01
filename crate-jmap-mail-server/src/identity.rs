//! Identity/get, Identity/changes, and Identity/set method handlers (RFC 8621 §6).

use jmap_types::{Id, Invocation, JmapError, State};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, MailBackend, SetErrorType};
use crate::helpers::{extract_account_id, not_found_json, ser};

/// Handle an `Identity/get` method call (RFC 8621 §6.1).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_identity_get<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    // ids: absent or null means "return all"; Some([]) means "return nothing".
    let ids: Option<Vec<Id>> = match args.get("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v.clone())
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<jmap_mail_types::Identity>(&account_id, ids_slice, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<jmap_mail_types::Identity>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(ser)
        .collect::<Result<Vec<_>, _>>()?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    });

    Ok((resp, vec![]))
}

/// Handle an `Identity/changes` method call (RFC 8621 §6.2).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_identity_changes<B: MailBackend>(
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
        .get_changes::<jmap_mail_types::Identity>(&account_id, &since_state, max_changes)
        .await
        .map_err(JmapError::from)?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": since_state.as_ref(),
        "newState": result.new_state.as_ref(),
        "hasMoreChanges": result.has_more_changes,
        "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });

    Ok((resp, vec![]))
}

/// Handle an `Identity/set` method call (RFC 8621 §6.3).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_identity_set<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    // Fetch old state for ifInState check and response.
    let old_state = backend
        .get_state::<jmap_mail_types::Identity>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // ifInState: if provided and does not match, return stateMismatch.
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
            // Validate: email must be present and non-empty.
            let email = match obj_val.get("email").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s.to_owned(),
                _ => {
                    not_created.insert(
                        create_id.clone(),
                        json!({
                            "type": "invalidProperties",
                            "properties": ["email"],
                        }),
                    );
                    continue;
                }
            };

            // Build identity using the constructor (supplies all defaults), then
            // overlay optional client-supplied fields.
            let mut identity = jmap_mail_types::Identity::new(
                Id::from("placeholder"),
                email,
                true, // server-set: may_delete defaults to true on create
            );
            if let Some(name) = obj_val.get("name").and_then(|v| v.as_str()) {
                identity.name = name.to_owned();
            }
            if let Some(ts) = obj_val.get("textSignature").and_then(|v| v.as_str()) {
                identity.text_signature = ts.to_owned();
            }
            if let Some(hs) = obj_val.get("htmlSignature").and_then(|v| v.as_str()) {
                identity.html_signature = hs.to_owned();
            }
            if let Some(rt) = obj_val.get("replyTo") {
                if !rt.is_null() {
                    match serde_json::from_value(rt.clone()) {
                        Ok(v) => identity.reply_to = v,
                        Err(_) => {
                            not_created.insert(
                                create_id.clone(),
                                json!({
                                    "type": "invalidProperties",
                                    "properties": ["replyTo"],
                                }),
                            );
                            continue;
                        }
                    }
                }
            }
            if let Some(bcc) = obj_val.get("bcc") {
                if !bcc.is_null() {
                    match serde_json::from_value(bcc.clone()) {
                        Ok(v) => identity.bcc = v,
                        Err(_) => {
                            not_created.insert(
                                create_id.clone(),
                                json!({
                                    "type": "invalidProperties",
                                    "properties": ["bcc"],
                                }),
                            );
                            continue;
                        }
                    }
                }
            }

            match backend
                .create_object::<jmap_mail_types::Identity>(&account_id, create_id, identity)
                .await
            {
                Ok((_server_id, created_obj)) => {
                    mutated = true;
                    // create_object guarantees created_obj.id == server_id;
                    // serialize the full object (id is already correct).
                    created.insert(
                        create_id.clone(),
                        serde_json::to_value(&created_obj)
                            .unwrap_or_else(|e| json!({ "type": "serverFail", "description": e.to_string() })),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(
                        create_id.clone(),
                        serde_json::to_value(&set_err)
                            .unwrap_or_else(|e| json!({ "type": "serverFail", "description": e.to_string() })),
                    );
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
    if let Some(update_map) = args.get("update").and_then(|v| v.as_object()) {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

            // Reject patches that include immutable or server-set fields.
            // RFC 8621 §6.3: email is immutable; id and mayDelete are server-set.
            const IDENTITY_READONLY: &[&str] = &["email", "id", "mayDelete"];
            let bad_props: Vec<&str> = IDENTITY_READONLY
                .iter()
                .copied()
                .filter(|&field| patch_val.get(field).is_some())
                .collect();
            if !bad_props.is_empty() {
                not_updated.insert(
                    id_str.clone(),
                    json!({
                        "type": "invalidProperties",
                        "properties": bad_props,
                    }),
                );
                continue;
            }

            match backend
                .update_object::<jmap_mail_types::Identity>(&account_id, &id, patch_val.clone())
                .await
            {
                Ok(_) => {
                    mutated = true;
                    updated.insert(id_str.clone(), Value::Null);
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_updated.insert(
                        id_str.clone(),
                        serde_json::to_value(&set_err)
                            .unwrap_or_else(|e| json!({ "type": "serverFail", "description": e.to_string() })),
                    );
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
    // destroy
    // -----------------------------------------------------------------------
    if let Some(destroy_arr) = args.get("destroy").and_then(|v| v.as_array()) {
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            let id = Id::from(id_str);

            // Fetch the identity to check mayDelete.
            let fetch_result = backend
                .get_objects::<jmap_mail_types::Identity>(
                    &account_id,
                    Some(std::slice::from_ref(&id)),
                    None,
                )
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;

            let (found, not_found_ids) = fetch_result;

            if !not_found_ids.is_empty() {
                not_destroyed.insert(id_str.to_owned(), json!({ "type": "notFound" }));
                continue;
            }

            // found should have exactly one entry.
            let identity = match found.into_iter().next() {
                Some(i) => i,
                None => {
                    not_destroyed.insert(id_str.to_owned(), json!({ "type": "notFound" }));
                    continue;
                }
            };

            if !identity.may_delete {
                not_destroyed.insert(id_str.to_owned(), json!({ "type": "forbidden" }));
                continue;
            }

            match backend
                .destroy_object::<jmap_mail_types::Identity>(&account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str.to_owned()));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    let type_str = match set_err.error_type {
                        SetErrorType::NotFound => "notFound",
                        SetErrorType::Forbidden => "forbidden",
                        _ => "serverFail",
                    };
                    not_destroyed.insert(id_str.to_owned(), json!({ "type": type_str }));
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

    // Fetch new state if anything changed.
    let new_state = if mutated {
        backend
            .get_state::<jmap_mail_types::Identity>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    } else {
        old_state.clone()
    };

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
        "updated": if updated.is_empty() { Value::Null } else { Value::Object(updated) },
        "destroyed": if destroyed_list.is_empty() { Value::Null } else { Value::Array(destroyed_list) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
        "notUpdated": if not_updated.is_empty() { Value::Null } else { Value::Object(not_updated) },
        "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
    });

    Ok((resp, vec![]))
}
