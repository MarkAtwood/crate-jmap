//! EmailSubmission/* method handlers (RFC 8621 §7).
//!
//! Provides handlers for:
//! - `EmailSubmission/get` (§7.1)
//! - `EmailSubmission/changes` (§7.2)
//! - `EmailSubmission/query` (§7.3)
//! - `EmailSubmission/queryChanges` (§7.4)
//! - `EmailSubmission/set` (§7.5) — also handles `onSuccessUpdateEmail`

use std::collections::HashMap;

use jmap_mail_types::{
    submission::{Address, Delivered, DeliveryStatus, Displayed, Envelope, UndoStatus},
    Email, EmailSubmission, Identity,
};
use jmap_types::{Id, Invocation, JmapError, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, MailBackend, SetError, SetErrorType};
use crate::helpers::{extract_account_id, now_utc_string};

// ---------------------------------------------------------------------------
// EmailSubmission/get
// ---------------------------------------------------------------------------

/// Handle an `EmailSubmission/get` method call (RFC 8621 §7.1).
///
/// Returns `(response_args, extra_invocations)`. Extra invocations are always empty.
pub async fn handle_submission_get<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

    let ids: Option<Vec<Id>> = match args.remove("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<EmailSubmission>(&account_id, ids_slice, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<EmailSubmission>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(|s| {
            serde_json::to_value(s).expect("type derives Serialize and is always serializable")
        })
        .collect();

    let not_found_json: Option<Vec<Value>> = if not_found.is_empty() {
        None
    } else {
        Some(
            not_found
                .iter()
                .map(|id| Value::String(id.as_ref().to_string()))
                .collect(),
        )
    };

    let resp = json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json,
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// EmailSubmission/changes
// ---------------------------------------------------------------------------

/// Handle an `EmailSubmission/changes` method call (RFC 8620 §5.2 / RFC 8621 §7.2).
///
/// Returns `(response_args, extra_invocations)`. Extra invocations are always empty.
pub async fn handle_submission_changes<B: MailBackend>(
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
        Some(v) => Some(v.as_u64().ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let result = backend
        .get_changes::<EmailSubmission>(&account_id, &since_state, max_changes)
        .await
        .map_err(JmapError::from)?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": since_state.as_ref(),
        "newState": result.new_state.as_ref(),
        "hasMoreChanges": result.has_more_changes,
        "updatedProperties": Value::Null,
        "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// EmailSubmission/query
// ---------------------------------------------------------------------------

/// Handle an `EmailSubmission/query` method call (RFC 8621 §7.3).
///
/// Returns `(response_args, extra_invocations)`. Extra invocations are always empty.
pub async fn handle_submission_query<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let limit: Option<u64> = match args.get("limit") {
        None | Some(Value::Null) => None,
        Some(v) => v.as_u64(),
    };

    let position: i64 = args.get("position").and_then(|v| v.as_i64()).unwrap_or(0);

    let result = backend
        .query_objects::<EmailSubmission>(&account_id, None, None, limit, position)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": result.query_state.as_ref(),
        "canCalculateChanges": result.can_calculate_changes,
        "position": result.position,
        "total": result.total,
        "ids": result.ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// EmailSubmission/queryChanges
// ---------------------------------------------------------------------------

/// Handle an `EmailSubmission/queryChanges` method call (RFC 8621 §7.4).
///
/// Returns `(response_args, extra_invocations)`. Extra invocations are always empty.
pub async fn handle_submission_query_changes<B: MailBackend>(
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
        Some(v) => Some(v.as_u64().ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let up_to_id: Option<Id> = match args.get("upToId") {
        None | Some(Value::Null) => None,
        Some(v) => v.as_str().map(Id::from),
    };

    let result = backend
        .query_changes::<EmailSubmission>(
            &account_id,
            &since_query_state,
            None,
            None,
            max_changes,
            up_to_id.as_ref(),
        )
        .await
        .map_err(JmapError::from)?;

    let added: Vec<Value> = result
        .added
        .iter()
        .map(|item| json!({ "id": item.id.as_ref(), "index": item.index }))
        .collect();

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldQueryState": result.old_query_state.as_ref(),
        "newQueryState": result.new_query_state.as_ref(),
        "total": result.total,
        "removed": result.removed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "added": added,
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// EmailSubmission/set
// ---------------------------------------------------------------------------

/// Handle an `EmailSubmission/set` method call (RFC 8621 §7.5).
///
/// Returns `(response_args, extra_invocations)`. When `onSuccessUpdateEmail` is
/// present, extra_invocations will contain one `Email/set` invocation.
pub async fn handle_submission_set<B: MailBackend>(
    backend: &B,
    args: Value,
    call_id: &str,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let old_state = backend
        .get_state::<EmailSubmission>(&account_id)
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
    let mut destroyed: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();

    // Pre-fetch email IDs for non-creation-reference submission IDs in onSuccess* args.
    // This must happen before the destroy loop so that email IDs are available
    // even for submissions that will be destroyed in this request.
    // Creation references (keys starting with '#') are populated after the create loop.
    let mut submission_email_id_map: HashMap<String, Id> = HashMap::new();
    {
        let has_on_success = args
            .get("onSuccessUpdateEmail")
            .filter(|v| !v.is_null())
            .is_some()
            || args
                .get("onSuccessDestroyEmail")
                .filter(|v| !v.is_null())
                .is_some();
        if has_on_success {
            let mut non_ref_ids: Vec<Id> = Vec::new();
            if let Some(m) = args.get("onSuccessUpdateEmail").and_then(|v| v.as_object()) {
                for key in m.keys() {
                    if !key.starts_with('#') {
                        non_ref_ids.push(Id::from(key.as_str()));
                    }
                }
            }
            if let Some(arr) = args.get("onSuccessDestroyEmail").and_then(|v| v.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        if !s.starts_with('#') {
                            non_ref_ids.push(Id::from(s));
                        }
                    }
                }
            }
            // Deduplicate before the batch fetch.
            non_ref_ids.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
            non_ref_ids.dedup_by(|a, b| a.as_ref() == b.as_ref());
            if !non_ref_ids.is_empty() {
                let (subs, _) = backend
                    .get_objects::<EmailSubmission>(&account_id, Some(&non_ref_ids), None)
                    .await
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;
                for sub in subs {
                    submission_email_id_map
                        .insert(sub.id.as_ref().to_string(), sub.email_id.clone());
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------

    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, create_args) in create_map {
            match process_create(backend, &account_id, create_id, create_args).await {
                Ok(obj_json) => {
                    // Populate submission → email_id map for onSuccess* processing.
                    if let Some(eid) = obj_json.get("emailId").and_then(|v| v.as_str()) {
                        submission_email_id_map.insert(format!("#{create_id}"), Id::from(eid));
                    }
                    created.insert(create_id.clone(), obj_json);
                }
                Err(err_json) => {
                    not_created.insert(create_id.clone(), err_json);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------

    if let Some(update_map) = args.get("update").and_then(|v| v.as_object()) {
        for (id_str, patch) in update_map {
            let id = Id::from(id_str.as_str());
            match process_update(backend, &account_id, &id, patch).await {
                Ok(()) => {
                    updated.insert(id_str.clone(), Value::Null);
                }
                Err(err_json) => {
                    not_updated.insert(id_str.clone(), err_json);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------

    if let Some(destroy_ids) = args.get("destroy").and_then(|v| v.as_array()) {
        for id_val in destroy_ids {
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            let id = Id::from(id_str);
            match backend
                .destroy_object::<EmailSubmission>(&account_id, &id)
                .await
            {
                Ok(()) => {
                    destroyed.push(Value::String(id_str.to_owned()));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        serde_json::to_value(&set_err)
                            .expect("type derives Serialize and is always serializable"),
                    );
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

    let new_state = backend
        .get_state::<EmailSubmission>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
        "updated": if updated.is_empty() { Value::Null } else { Value::Object(updated) },
        "destroyed": if destroyed.is_empty() { Value::Null } else { Value::Array(destroyed) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
        "notUpdated": if not_updated.is_empty() { Value::Null } else { Value::Object(not_updated) },
        "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
    });

    // -----------------------------------------------------------------------
    // onSuccessUpdateEmail / onSuccessDestroyEmail (RFC 8621 §7.5)
    //
    // Keys are EmailSubmission IDs or creation references ("#<create_id>").
    // Only apply the side effect if the referenced operation succeeded, i.e.
    // the key is present in submission_email_id_map.
    // -----------------------------------------------------------------------

    let mut extra_invocations: Vec<Invocation> = Vec::new();

    let has_on_success = args
        .get("onSuccessUpdateEmail")
        .filter(|v| !v.is_null())
        .is_some()
        || args
            .get("onSuccessDestroyEmail")
            .filter(|v| !v.is_null())
            .is_some();

    if has_on_success {
        let email_old_state = backend
            .get_state::<Email>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        let mut email_updated = serde_json::Map::new();
        let mut email_not_updated = serde_json::Map::new();
        let mut email_destroyed: Vec<Value> = Vec::new();
        let mut email_not_destroyed = serde_json::Map::new();

        // onSuccessUpdateEmail
        if let Some(update_patches) = args.get("onSuccessUpdateEmail").and_then(|v| v.as_object()) {
            for (sub_key, patch) in update_patches {
                let email_id = match submission_email_id_map.get(sub_key.as_str()) {
                    Some(id) => id.clone(),
                    None => continue, // Referenced operation did not succeed; skip.
                };
                match backend
                    .update_object::<Email>(&account_id, &email_id, patch.clone())
                    .await
                {
                    Ok(_) => {
                        email_updated.insert(email_id.as_ref().to_string(), Value::Null);
                    }
                    Err(BackendSetError::SetError(set_err)) => {
                        email_not_updated.insert(
                            email_id.as_ref().to_string(),
                            serde_json::to_value(&set_err)
                                .expect("SetError is always serializable"),
                        );
                    }
                    Err(BackendSetError::Other(e)) => {
                        email_not_updated.insert(
                            email_id.as_ref().to_string(),
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                    }
                }
            }
        }

        // onSuccessDestroyEmail
        if let Some(destroy_keys) = args.get("onSuccessDestroyEmail").and_then(|v| v.as_array()) {
            for key_val in destroy_keys {
                let sub_key = match key_val.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                let email_id = match submission_email_id_map.get(sub_key) {
                    Some(id) => id.clone(),
                    None => continue, // Referenced operation did not succeed; skip.
                };
                match backend
                    .destroy_object::<Email>(&account_id, &email_id)
                    .await
                {
                    Ok(()) => {
                        email_destroyed.push(Value::String(email_id.as_ref().to_string()));
                    }
                    Err(BackendSetError::SetError(set_err)) => {
                        email_not_destroyed.insert(
                            email_id.as_ref().to_string(),
                            serde_json::to_value(&set_err)
                                .expect("SetError is always serializable"),
                        );
                    }
                    Err(BackendSetError::Other(e)) => {
                        email_not_destroyed.insert(
                            email_id.as_ref().to_string(),
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                    }
                }
            }
        }

        let email_new_state = backend
            .get_state::<Email>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        // RFC 8621 §7.5: a single implicit Email/set response is appended after
        // the EmailSubmission/set response. Call-id is "#<parent-call-id>".
        let email_set_resp = json!({
            "accountId": account_id.as_ref(),
            "oldState": email_old_state.as_ref(),
            "newState": email_new_state.as_ref(),
            "created": Value::Null,
            "updated": if email_updated.is_empty() { Value::Null } else { Value::Object(email_updated) },
            "destroyed": if email_destroyed.is_empty() { Value::Null } else { Value::Array(email_destroyed) },
            "notCreated": Value::Null,
            "notUpdated": if email_not_updated.is_empty() { Value::Null } else { Value::Object(email_not_updated) },
            "notDestroyed": if email_not_destroyed.is_empty() { Value::Null } else { Value::Object(email_not_destroyed) },
        });
        extra_invocations.push((
            "Email/set".to_string(),
            email_set_resp,
            format!("#{call_id}"),
        ));
    }

    Ok((resp, extra_invocations))
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Validate that an email address string contains no CR or LF characters.
///
/// Returns `Err` with the offending address on violation.
fn check_no_crlf(email: &str) -> Result<(), &str> {
    if email.contains('\r') || email.contains('\n') {
        Err(email)
    } else {
        Ok(())
    }
}

/// Process a single create entry in an `EmailSubmission/set` request.
///
/// Returns the JSON for the `created` map on success, or the JSON error object
/// (suitable for insertion into `notCreated`) on failure.
async fn process_create<B: MailBackend>(
    backend: &B,
    account_id: &Id,
    create_id: &str,
    create_args: &Value,
) -> Result<Value, Value> {
    // --- identityId ---
    let identity_id_str = create_args
        .get("identityId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            json!({ "type": "invalidProperties", "properties": ["identityId"],
                    "description": "identityId is required" })
        })?;
    let identity_id = Id::from(identity_id_str);

    // Validate identityId references an existing Identity.
    let (identities, _) = backend
        .get_objects::<Identity>(account_id, Some(std::slice::from_ref(&identity_id)), None)
        .await
        .map_err(|e| json!({ "type": "serverFail", "description": e.to_string() }))?;

    if identities.is_empty() {
        return Err(
            json!({ "type": "invalidProperties", "properties": ["identityId"],
                           "description": "identityId does not reference an existing Identity" }),
        );
    }
    let identity = &identities[0];

    // --- emailId ---
    let email_id_str = create_args
        .get("emailId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            json!({ "type": "invalidProperties", "properties": ["emailId"],
                    "description": "emailId is required" })
        })?;
    let email_id = Id::from(email_id_str);

    // Validate emailId references an existing Email and retrieve threadId.
    let (emails, _) = backend
        .get_objects::<Email>(account_id, Some(std::slice::from_ref(&email_id)), None)
        .await
        .map_err(|e| json!({ "type": "serverFail", "description": e.to_string() }))?;

    if emails.is_empty() {
        return Err(
            json!({ "type": "invalidProperties", "properties": ["emailId"],
                           "description": "emailId does not reference an existing Email" }),
        );
    }
    let email = &emails[0];
    let thread_id = email.thread_id.clone();

    // --- envelope (derive if null/absent) ---
    let envelope: Envelope = match create_args.get("envelope") {
        None | Some(Value::Null) => {
            // Derive mailFrom from Identity.email, rcptTo from Email.to + cc + bcc.
            let mail_from = Address::new(identity.email.clone());
            let mut rcpt_to: Vec<Address> = Vec::new();
            for addrs in [&email.to, &email.cc, &email.bcc].into_iter().flatten() {
                for addr in addrs {
                    rcpt_to.push(Address::new(addr.email.clone()));
                }
            }
            Envelope::new(mail_from, rcpt_to)
        }
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
            json!({ "type": "invalidProperties", "properties": ["envelope"],
                    "description": e.to_string() })
        })?,
    };

    // --- SMTP injection defense (RFC 8621 §7.5) ---
    if check_no_crlf(&envelope.mail_from.email).is_err() {
        return Err(json!({ "type": "invalidRecipients",
                           "description": "mailFrom.email contains CR or LF" }));
    }
    for rcpt in &envelope.rcpt_to {
        if check_no_crlf(&rcpt.email).is_err() {
            return Err(json!({ "type": "invalidRecipients",
                               "description": format!("rcptTo address {:?} contains CR or LF",
                                                      rcpt.email) }));
        }
    }

    // --- sendAt (current time if null/absent) ---
    let send_at: UTCDate = match create_args.get("sendAt") {
        None | Some(Value::Null) => UTCDate::from(now_utc_string().as_str()),
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
            json!({ "type": "invalidProperties", "properties": ["sendAt"],
                    "description": e.to_string() })
        })?,
    };

    // --- Build delivery status for each rcptTo ---
    let mut delivery_status: HashMap<String, DeliveryStatus> = HashMap::new();
    for rcpt in &envelope.rcpt_to {
        delivery_status.insert(
            rcpt.email.clone(),
            DeliveryStatus::new("250 OK", Delivered::Yes, Displayed::Unknown),
        );
    }

    // --- Build submission object ---
    // Use a placeholder id; the backend will assign the real one.
    let mut submission = EmailSubmission::new(
        Id::from("placeholder"),
        identity_id.clone(),
        email_id.clone(),
        thread_id,
        send_at,
        UndoStatus::Final,
    );
    submission.envelope = Some(envelope);
    submission.delivery_status = if delivery_status.is_empty() {
        None
    } else {
        Some(delivery_status)
    };

    let (server_id, created_obj) = backend
        .create_object::<EmailSubmission>(account_id, create_id, submission)
        .await
        .map_err(|e| match e {
            BackendSetError::SetError(set_err) => {
                serde_json::to_value(&set_err).expect("SetError is always serializable")
            }
            BackendSetError::Other(inner) => {
                json!({ "type": "serverFail", "description": inner.to_string() })
            }
        })?;

    // SetError is always serializable; a failure here is a programming error.
    let mut obj_json =
        serde_json::to_value(&created_obj).expect("EmailSubmission is always serializable");

    // Ensure the assigned id is in the response.
    if let Value::Object(ref mut map) = obj_json {
        map.insert(
            "id".to_owned(),
            Value::String(server_id.as_ref().to_string()),
        );
    }

    Ok(obj_json)
}

/// Process a single update entry in an `EmailSubmission/set` request.
///
/// RFC 8621 §7.5: only `undoStatus` may be updated. If the current status is
/// already `"final"`, returns `cannotUnsend`.
async fn process_update<B: MailBackend>(
    backend: &B,
    account_id: &Id,
    id: &Id,
    patch: &Value,
) -> Result<(), Value> {
    // RFC 8621 §7.5: only undoStatus may be changed in an update patch.
    if let Some(obj) = patch.as_object() {
        let bad: Vec<&str> = obj
            .keys()
            .filter(|k| k.as_str() != "undoStatus")
            .map(|k| k.as_str())
            .collect();
        if !bad.is_empty() {
            return Err(serde_json::to_value(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(bad.iter().map(|s| s.to_string()).collect())
                    .with_description("only undoStatus may be changed on an EmailSubmission"),
            )
            .expect("SetError is always serializable"));
        }
    }

    // Look up existing submission to check undoStatus.
    let (existing, not_found) = backend
        .get_objects::<EmailSubmission>(account_id, Some(std::slice::from_ref(id)), None)
        .await
        .map_err(|e| json!({ "type": "serverFail", "description": e.to_string() }))?;

    if !not_found.is_empty() || existing.is_empty() {
        return Err(serde_json::to_value(SetError::new(SetErrorType::NotFound))
            .expect("SetError is always serializable"));
    }

    let current = &existing[0];

    // Only allow updating undoStatus; if the submission is already final, reject.
    if current.undo_status == UndoStatus::Final {
        return Err(serde_json::to_value(
            SetError::new(SetErrorType::CannotUnsend)
                .with_description("Submission is already in final state and cannot be undone"),
        )
        .expect("SetError is always serializable"));
    }

    // Apply the patch via the backend.
    backend
        .update_object::<EmailSubmission>(account_id, id, patch.clone())
        .await
        .map(|_| ())
        .map_err(|e| match e {
            BackendSetError::SetError(set_err) => {
                serde_json::to_value(&set_err).expect("SetError is always serializable")
            }
            BackendSetError::Other(inner) => {
                json!({ "type": "serverFail", "description": inner.to_string() })
            }
        })
}
