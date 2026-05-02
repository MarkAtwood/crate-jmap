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
    query::EmailSubmissionFilter,
    submission::{Address, Delivered, DeliveryStatus, Displayed, Envelope, UndoStatus},
    Email, EmailSubmission, Identity,
};
use jmap_types::{Id, Invocation, JmapError, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, MailBackend, SetError, SetErrorType};
use crate::helpers::{extract_account_id, not_found_json, now_utc_string, ser};

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

    let list_json: Vec<Value> = list.iter().map(ser).collect::<Result<Vec<_>, _>>()?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
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
        Some(v) => Some(v.as_u64().filter(|&n| n > 0).ok_or_else(|| {
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

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let limit: Option<u64> = match args.get("limit") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(n) => Some(n),
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {v}"
                )))
            }
        },
    };

    let position: i64 = match args.get("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("position: expected an integer, got {v}"))
        })?,
    };

    // RFC 8620 §5.5: anchor-based pagination overrides position.
    let anchor: Option<Id> = match args.get("anchor") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Id::from(s.as_str())),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "anchor: expected an Id string or null, got {v}"
            )))
        }
    };
    let anchor_offset: i64 = match args.get("anchorOffset") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("anchorOffset: expected an integer, got {v}"))
        })?,
    };

    let filter: Option<EmailSubmissionFilter> = match args.get("filter") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v.clone())
                .map_err(|e| JmapError::invalid_arguments(e.to_string()))?,
        ),
    };

    // When anchor is present, fetch the full result set to resolve it.
    // Otherwise, delegate limit/position directly to the backend.
    let (ids, total, query_state, can_calculate_changes, reported_position) =
        if let Some(ref anchor_id) = anchor {
            let all = backend
                .query_objects::<EmailSubmission>(&account_id, filter.as_ref(), None, None, 0)
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;
            let anchor_idx = all
                .ids
                .iter()
                .position(|id| id == anchor_id)
                .ok_or_else(JmapError::anchor_not_found)?;
            let raw = anchor_idx as i64 + anchor_offset;
            let start = raw.max(0).min(all.ids.len() as i64) as usize;
            let effective_limit = limit.map_or(usize::MAX, |n| n as usize);
            let page: Vec<Id> = all
                .ids
                .into_iter()
                .skip(start)
                .take(effective_limit)
                .collect();
            let total = all.total;
            (
                page,
                total,
                all.query_state,
                all.can_calculate_changes,
                start as i64,
            )
        } else {
            let result = backend
                .query_objects::<EmailSubmission>(
                    &account_id,
                    filter.as_ref(),
                    None,
                    limit,
                    position,
                )
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;
            let pos = result.position;
            let total = result.total;
            (
                result.ids,
                total,
                result.query_state,
                result.can_calculate_changes,
                pos,
            )
        };

    // RFC 8620 §5.5: total MUST be omitted when calculateTotal is false (default).
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": query_state.as_ref(),
        "canCalculateChanges": can_calculate_changes,
        "position": reported_position,
        "ids": ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total {
        if let Some(t) = total {
            resp["total"] = json!(t);
        }
    }

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
        .query_changes::<EmailSubmission>(
            &account_id,
            &since_query_state,
            None,
            None,
            max_changes,
            up_to_id.as_ref(),
            false, // collapseThreads does not apply to EmailSubmission
        )
        .await
        .map_err(JmapError::from)?;

    let added: Vec<Value> = result
        .added
        .iter()
        .map(|item| json!({ "id": item.id.as_ref(), "index": item.index }))
        .collect();

    // RFC 8620 §5.6: total MUST be omitted unless calculateTotal is true.
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "oldQueryState": result.old_query_state.as_ref(),
        "newQueryState": result.new_query_state.as_ref(),
        "removed": result.removed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
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
            let mut non_ref_id_set: std::collections::HashSet<Id> =
                std::collections::HashSet::new();
            if let Some(m) = args.get("onSuccessUpdateEmail").and_then(|v| v.as_object()) {
                for key in m.keys() {
                    if !key.starts_with('#') {
                        non_ref_id_set.insert(Id::from(key.as_str()));
                    }
                }
            }
            if let Some(arr) = args.get("onSuccessDestroyEmail").and_then(|v| v.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        if !s.starts_with('#') {
                            non_ref_id_set.insert(Id::from(s));
                        }
                    }
                }
            }
            let non_ref_ids: Vec<Id> = non_ref_id_set.into_iter().collect();
            if !non_ref_ids.is_empty() {
                let (subs, _) = backend
                    .get_objects::<EmailSubmission>(&account_id, Some(&non_ref_ids), None)
                    .await
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;
                for sub in subs {
                    submission_email_id_map
                        .insert(sub.id.as_ref().to_owned(), sub.email_id.clone());
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
                Err(err) => {
                    let err_json = match err {
                        CreateError::SetError(se) => serde_json::to_value(se).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
                        CreateError::Server(msg) => {
                            json!({ "type": "serverFail", "description": msg })
                        }
                    };
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
                Ok(Some(obj)) => {
                    updated.insert(
                        id_str.clone(),
                        serde_json::to_value(&obj).unwrap_or(Value::Null),
                    );
                }
                Ok(None) => {
                    updated.insert(id_str.clone(), Value::Null);
                }
                Err(BackendSetError::SetError(se)) => {
                    not_updated.insert(
                        id_str.clone(),
                        ser(&se).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
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
    // e53.46: remove failed updates from the onSuccess map so their side
    // effects are not applied.
    for id_str in not_updated.keys() {
        submission_email_id_map.remove(id_str);
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------

    if let Some(destroy_ids) = args.get("destroy").and_then(|v| v.as_array()) {
        // RFC 8620 §5.3: the destroy array is Id[]. A non-string element is a
        // malformed request; return invalidArguments for the whole request.
        if let Some(bad) = destroy_ids.iter().find(|v| !v.is_string()) {
            return Err(JmapError::invalid_arguments(format!(
                "destroy array must contain only Id strings; got: {bad}"
            )));
        }
        for id_val in destroy_ids {
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue, // unreachable: validated above
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
                        serde_json::to_value(&set_err).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
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
        // e53.46: remove failed destroys from the onSuccess map so their side
        // effects are not applied.
        for id_str in not_destroyed.keys() {
            submission_email_id_map.remove(id_str);
        }
    }

    let mutated = !created.is_empty() || !updated.is_empty() || !destroyed.is_empty();
    let new_state = if mutated {
        backend
            .get_state::<EmailSubmission>(&account_id)
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
                // Apply same immutable-field guard as handle_email_set patches.
                if let Some(bad_field) = crate::email::find_immutable_patch_key(patch) {
                    email_not_updated.insert(
                        email_id.as_ref().to_owned(),
                        json!({
                            "type": "invalidProperties",
                            "properties": [bad_field],
                        }),
                    );
                    continue;
                }
                match backend
                    .update_object::<Email>(&account_id, &email_id, patch.clone())
                    .await
                {
                    Ok(_) => {
                        email_updated.insert(email_id.as_ref().to_owned(), Value::Null);
                    }
                    Err(BackendSetError::SetError(set_err)) => {
                        email_not_updated.insert(
                            email_id.as_ref().to_owned(),
                            serde_json::to_value(&set_err).unwrap_or_else(
                                |e| json!({ "type": "serverFail", "description": e.to_string() }),
                            ),
                        );
                    }
                    Err(BackendSetError::Other(e)) => {
                        email_not_updated.insert(
                            email_id.as_ref().to_owned(),
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
                        email_destroyed.push(Value::String(email_id.as_ref().to_owned()));
                    }
                    Err(BackendSetError::SetError(set_err)) => {
                        email_not_destroyed.insert(
                            email_id.as_ref().to_owned(),
                            serde_json::to_value(&set_err).unwrap_or_else(
                                |e| json!({ "type": "serverFail", "description": e.to_string() }),
                            ),
                        );
                    }
                    Err(BackendSetError::Other(e)) => {
                        email_not_destroyed.insert(
                            email_id.as_ref().to_owned(),
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                    }
                }
            }
        }

        // RFC 8621 §7.5: only emit the implicit Email/set if at least one
        // email operation was attempted. If all referenced creates failed, the
        // map is empty and there is nothing to report.
        let any_email_ops = !email_updated.is_empty()
            || !email_not_updated.is_empty()
            || !email_destroyed.is_empty()
            || !email_not_destroyed.is_empty();

        if any_email_ops {
            let email_new_state = backend
                .get_state::<Email>(&account_id)
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;

            // RFC 8621 §7.5: a single implicit Email/set response is appended
            // after the EmailSubmission/set response. Call-id is the same as
            // the originating EmailSubmission/set call (RFC 8620 §3.2).
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
            extra_invocations.push(("Email/set".to_owned(), email_set_resp, call_id.to_owned()));
        }
    }

    Ok((resp, extra_invocations))
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Validate that an email address string contains no CR or LF characters.
fn check_no_crlf(email: &str) -> bool {
    !email.contains('\r') && !email.contains('\n')
}

/// Typed error for [`process_create`] — avoids `Result<Value, Value>`.
enum CreateError {
    SetError(SetError),
    Server(String),
}

impl From<SetError> for CreateError {
    fn from(e: SetError) -> Self {
        Self::SetError(e)
    }
}

/// Process a single create entry in an `EmailSubmission/set` request.
///
/// Returns the JSON for the `created` map on success, or a typed
/// [`CreateError`] (converted to JSON at the call site) on failure.
async fn process_create<B: MailBackend>(
    backend: &B,
    account_id: &Id,
    create_id: &str,
    create_args: &Value,
) -> Result<Value, CreateError> {
    // --- identityId ---
    let identity_id_str = create_args
        .get("identityId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            CreateError::SetError(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(["identityId"])
                    .with_description("identityId is required"),
            )
        })?;
    let identity_id = Id::from(identity_id_str);

    // Validate identityId references an existing Identity.
    let (identities, _) = backend
        .get_objects::<Identity>(account_id, Some(std::slice::from_ref(&identity_id)), None)
        .await
        .map_err(|e| CreateError::Server(e.to_string()))?;

    if identities.is_empty() {
        return Err(CreateError::SetError(
            SetError::new(SetErrorType::InvalidProperties)
                .with_properties(["identityId"])
                .with_description("identityId does not reference an existing Identity"),
        ));
    }
    let identity = &identities[0];

    // --- emailId ---
    let email_id_str = create_args
        .get("emailId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            CreateError::SetError(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(["emailId"])
                    .with_description("emailId is required"),
            )
        })?;
    let email_id = Id::from(email_id_str);

    // Validate emailId references an existing Email and retrieve threadId.
    let (emails, _) = backend
        .get_objects::<Email>(account_id, Some(std::slice::from_ref(&email_id)), None)
        .await
        .map_err(|e| CreateError::Server(e.to_string()))?;

    if emails.is_empty() {
        return Err(CreateError::SetError(
            SetError::new(SetErrorType::InvalidProperties)
                .with_properties(["emailId"])
                .with_description("emailId does not reference an existing Email"),
        ));
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
            CreateError::SetError(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(["envelope"])
                    .with_description(e.to_string()),
            )
        })?,
    };

    // --- noRecipients check (RFC 8621 §7.5) ---
    // Applies whether the envelope was derived or supplied by the client.
    if envelope.rcpt_to.is_empty() {
        return Err(CreateError::SetError(SetError::new(
            SetErrorType::NoRecipients,
        )));
    }

    // --- SMTP injection defense (RFC 8621 §7.5) ---
    // Collect *all* invalid addresses; RFC 8621 §7.5 requires the complete list.
    {
        let mut invalid: Vec<&str> = Vec::new();
        if !check_no_crlf(&envelope.mail_from.email) {
            invalid.push(&envelope.mail_from.email);
        }
        for rcpt in &envelope.rcpt_to {
            if !check_no_crlf(&rcpt.email) {
                invalid.push(&rcpt.email);
            }
        }
        if !invalid.is_empty() {
            return Err(CreateError::SetError(
                SetError::new(SetErrorType::InvalidRecipients)
                    .with_invalid_recipients(invalid)
                    .with_description("one or more addresses contain CR or LF"),
            ));
        }
    }

    // --- sendAt is server-set (RFC 8621 §7.2: sendAt is set by the server) ---
    // Any client-supplied sendAt is ignored.
    let send_at: UTCDate = UTCDate::from(now_utc_string().as_str());

    // --- Build delivery status for each rcptTo ---
    // Delivery has not yet occurred; reflect queued state.
    let mut delivery_status: HashMap<String, DeliveryStatus> = HashMap::new();
    for rcpt in &envelope.rcpt_to {
        delivery_status.insert(
            rcpt.email.clone(),
            DeliveryStatus::new("queued", Delivered::Queued, Displayed::Unknown),
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

    let (_server_id, created_obj) = backend
        .create_object::<EmailSubmission>(account_id, create_id, submission)
        .await
        .map_err(|e| match e {
            BackendSetError::SetError(set_err) => CreateError::SetError(set_err),
            BackendSetError::Other(inner) => CreateError::Server(inner.to_string()),
        })?;

    // create_object guarantees created_obj.id == server_id; serialize as-is.
    Ok(serde_json::to_value(&created_obj)
        .unwrap_or_else(|e| json!({ "type": "serverFail", "description": e.to_string() })))
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
) -> Result<Option<EmailSubmission>, BackendSetError<B::Error>> {
    // RFC 8621 §7.5: only undoStatus may be changed in an update patch.
    if let Some(obj) = patch.as_object() {
        let bad: Vec<&str> = obj
            .keys()
            .filter(|k| k.as_str() != "undoStatus")
            .map(|k| k.as_str())
            .collect();
        if !bad.is_empty() {
            return Err(BackendSetError::SetError(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(bad)
                    .with_description("only undoStatus may be changed on an EmailSubmission"),
            ));
        }
    }

    // Look up existing submission to check undoStatus.
    let (existing, not_found) = backend
        .get_objects::<EmailSubmission>(account_id, Some(std::slice::from_ref(id)), None)
        .await
        .map_err(BackendSetError::Other)?;

    if !not_found.is_empty() || existing.is_empty() {
        return Err(BackendSetError::SetError(SetError::new(
            SetErrorType::NotFound,
        )));
    }

    let current = &existing[0];

    // Only allow updating undoStatus; if the submission is already final, reject.
    if current.undo_status == UndoStatus::Final {
        return Err(BackendSetError::SetError(
            SetError::new(SetErrorType::CannotUnsend)
                .with_description("Submission is already in final state and cannot be undone"),
        ));
    }

    // Apply the patch via the backend.
    backend
        .update_object::<EmailSubmission>(account_id, id, patch.clone())
        .await
}
