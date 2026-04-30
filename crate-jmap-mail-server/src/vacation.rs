//! RFC 8621 §8 VacationResponse/get and VacationResponse/set handlers.
//!
//! VacationResponse is a **singleton**: there is exactly one per account and
//! its `id` is always the string `"singleton"`.  Create and destroy are
//! forbidden; only update of `"singleton"` is permitted.

use jmap_mail_types::VacationResponse;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, MailBackend, SetError, SetErrorType};
use crate::helpers::extract_account_id;

const SINGLETON_ID: &str = "singleton";

// ---------------------------------------------------------------------------
// VacationResponse/get
// ---------------------------------------------------------------------------

/// Handle a `VacationResponse/get` request (RFC 8621 §8.1).
///
/// Accepts `ids = null` or `ids = ["singleton"]` — both return the singleton
/// (if it exists). `ids = []` returns an empty list immediately.  Any id
/// other than `"singleton"` is placed in `notFound`.
pub async fn handle_vacation_get<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let requested_ids: Option<Vec<String>> = match args.get("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v.clone())
                .map_err(|_| JmapError::invalid_arguments("ids must be a string array"))?,
        ),
    };

    let state = backend
        .get_state::<VacationResponse>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // ids=[] — return empty immediately.
    if let Some(ref ids) = requested_ids {
        if ids.is_empty() {
            return Ok((
                json!({
                    "accountId": account_id.as_ref(),
                    "state": state.as_ref(),
                    "list": [],
                    "notFound": null,
                }),
                vec![],
            ));
        }
    }

    // Any requested id that is not "singleton" is notFound.
    let not_found: Vec<Value> = requested_ids
        .iter()
        .flatten()
        .filter(|id| id.as_str() != SINGLETON_ID)
        .map(|id| Value::String(id.clone()))
        .collect();

    // Fetch the singleton from the backend.
    let singleton_id = Id::from(SINGLETON_ID);
    let (list, _) = backend
        .get_objects::<VacationResponse>(&account_id, Some(&[singleton_id]), None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(|v| serde_json::to_value(v).unwrap_or(Value::Null))
        .collect();

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "state": state.as_ref(),
            "list": list_json,
            "notFound": if not_found.is_empty() { Value::Null } else { Value::Array(not_found) },
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// VacationResponse/set
// ---------------------------------------------------------------------------

/// Handle a `VacationResponse/set` request (RFC 8621 §8.2).
///
/// Rules enforced here (not in the backend):
/// - `create` is always rejected with `SetErrorType::Singleton`.
/// - `destroy` is always rejected with `SetErrorType::Singleton`.
/// - `update "singleton"` is the only permitted mutation.  If no
///   VacationResponse exists yet the handler creates it (upsert semantics).
/// - Any update id other than `"singleton"` is rejected with `NotFound`.
pub async fn handle_vacation_set<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    // ifInState check.
    let old_state = backend
        .get_state::<VacationResponse>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;
    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if old_state.as_ref() != if_in_state {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut not_created = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // create — always forbidden for singletons.
    if let Some(create) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, _) in create {
            let err = SetError::new(SetErrorType::Singleton)
                .with_description("VacationResponse is a singleton; use update to modify");
            not_created.insert(
                create_id.clone(),
                serde_json::to_value(&err).map_err(|e| JmapError::server_fail(e.to_string()))?,
            );
        }
    }

    // update — only "singleton" is a valid id.
    let mut updated = serde_json::Map::new();
    if let Some(update) = args.get("update").and_then(|v| v.as_object()) {
        for (id, patch) in update {
            if id != SINGLETON_ID {
                let err = SetError::new(SetErrorType::NotFound);
                not_updated.insert(
                    id.clone(),
                    serde_json::to_value(&err)
                        .map_err(|e| JmapError::server_fail(e.to_string()))?,
                );
                continue;
            }

            let singleton_id = Id::from(SINGLETON_ID);
            match backend
                .update_object::<VacationResponse>(&account_id, &singleton_id, patch.clone())
                .await
            {
                Ok(_) => {
                    updated.insert(id.clone(), Value::Null);
                    mutated = true;
                }
                Err(BackendSetError::SetError(ref set_err))
                    if set_err.error_type == SetErrorType::NotFound =>
                {
                    // Singleton does not exist yet — upsert: build a default
                    // VacationResponse, then create it so it is stored under
                    // the "singleton" key.
                    let base = VacationResponse::new(Id::from(SINGLETON_ID), false);
                    match backend
                        .create_object::<VacationResponse>(&account_id, SINGLETON_ID, base)
                        .await
                    {
                        Ok(_) => {
                            // Now apply the patch to the freshly created singleton.
                            match backend
                                .update_object::<VacationResponse>(
                                    &account_id,
                                    &singleton_id,
                                    patch.clone(),
                                )
                                .await
                            {
                                Ok(_) => {
                                    updated.insert(id.clone(), Value::Null);
                                    mutated = true;
                                }
                                Err(BackendSetError::SetError(e)) => {
                                    not_updated.insert(
                                        id.clone(),
                                        serde_json::to_value(&e)
                                            .map_err(|e| JmapError::server_fail(e.to_string()))?,
                                    );
                                }
                                Err(BackendSetError::Other(e)) => {
                                    return Err(JmapError::server_fail(e.to_string()));
                                }
                            }
                        }
                        Err(BackendSetError::SetError(e)) => {
                            not_updated.insert(
                                id.clone(),
                                serde_json::to_value(&e)
                                    .map_err(|e| JmapError::server_fail(e.to_string()))?,
                            );
                        }
                        Err(BackendSetError::Other(e)) => {
                            return Err(JmapError::server_fail(e.to_string()));
                        }
                    }
                }
                Err(BackendSetError::SetError(e)) => {
                    not_updated.insert(
                        id.clone(),
                        serde_json::to_value(&e)
                            .map_err(|e| JmapError::server_fail(e.to_string()))?,
                    );
                }
                Err(BackendSetError::Other(e)) => {
                    return Err(JmapError::server_fail(e.to_string()));
                }
            }
        }
    }

    // destroy — always forbidden for singletons.
    if let Some(destroy) = args.get("destroy").and_then(|v| v.as_array()) {
        for id_val in destroy {
            let id = id_val.as_str().unwrap_or("");
            let err = SetError::new(SetErrorType::Singleton)
                .with_description("VacationResponse is a singleton; cannot destroy");
            not_destroyed.insert(
                id.to_string(),
                serde_json::to_value(&err).map_err(|e| JmapError::server_fail(e.to_string()))?,
            );
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<VacationResponse>(&account_id)
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
            "created": {},
            "updated": Value::Object(updated),
            "destroyed": [],
            "notCreated": Value::Object(not_created),
            "notUpdated": Value::Object(not_updated),
            "notDestroyed": Value::Object(not_destroyed),
        }),
        vec![],
    ))
}
