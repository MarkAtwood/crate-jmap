//! RFC 8621 §8 VacationResponse/get and VacationResponse/set handlers.
//!
//! VacationResponse is a **singleton**: there is exactly one per account and
//! its `id` is always the string `"singleton"`.  Create and destroy are
//! forbidden; only update of `"singleton"` is permitted.

use jmap_mail_types::VacationResponse;
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, MailBackend, SetError, SetErrorType};
use crate::helpers::{extract_account_id, finalize_set_response, set_error_value, SetAccumulators};

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
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }

    let requested_ids: Option<Vec<String>> = match args.remove("ids").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be a string array"))?,
        ),
    };

    let state = backend
        .get_state::<VacationResponse>(caller, &account_id)
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
                    "notFound": [],
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
        .get_objects::<VacationResponse>(caller, &account_id, Some(&[singleton_id]), None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(|v| serde_json::to_value(v).expect("derive(Serialize) on plain data is infallible"))
        .collect();

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "state": state.as_ref(),
            "list": list_json,
            "notFound": Value::Array(not_found),
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
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }

    // ifInState check.
    let old_state = backend
        .get_state::<VacationResponse>(caller, &account_id)
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
            not_created.insert(create_id.clone(), set_error_value(&err));
        }
    }

    // update — only "singleton" is a valid id.
    let mut updated = serde_json::Map::new();
    if let Some(Value::Object(update)) = args.remove("update") {
        for (id, patch_val) in update {
            if id != SINGLETON_ID {
                let err = SetError::new(SetErrorType::NotFound);
                not_updated.insert(id.clone(), set_error_value(&err));
                continue;
            }

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id.clone(),
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            let singleton_id = Id::from(SINGLETON_ID);
            match backend
                .update_object::<VacationResponse>(
                    caller,
                    &account_id,
                    &singleton_id,
                    patch.clone(),
                )
                .await
            {
                Ok(Some(obj)) => {
                    updated.insert(
                        id.clone(),
                        serde_json::to_value(&obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                    mutated = true;
                }
                Ok(None) => {
                    updated.insert(id.clone(), Value::Null);
                    mutated = true;
                }
                Err(BackendSetError::SetError(ref set_err))
                    if set_err.error_type == SetErrorType::NotFound =>
                {
                    // Singleton does not exist yet — upsert: build a default
                    // VacationResponse, then create it so it is stored under
                    // the "singleton" key.
                    //
                    // Concurrency note: two concurrent requests can both reach
                    // this branch and attempt to create the singleton. Backends
                    // that receive concurrent requests MUST make create_object
                    // idempotent for the singleton key (e.g. via a unique
                    // constraint or a compare-and-swap) to avoid duplicate
                    // creation. The handler layer cannot add locking here
                    // because it holds no shared state.
                    let base = VacationResponse::new(Id::from(SINGLETON_ID), false);
                    match backend
                        .create_object::<VacationResponse>(caller, &account_id, SINGLETON_ID, base)
                        .await
                    {
                        Ok(_) => {
                            // Now apply the patch to the freshly created singleton.
                            match backend
                                .update_object::<VacationResponse>(
                                    caller,
                                    &account_id,
                                    &singleton_id,
                                    patch,
                                )
                                .await
                            {
                                Ok(Some(obj)) => {
                                    updated.insert(
                                        id.clone(),
                                        serde_json::to_value(&obj).expect(
                                            "derive(Serialize) on plain data is infallible",
                                        ),
                                    );
                                    mutated = true;
                                }
                                Ok(None) => {
                                    updated.insert(id.clone(), Value::Null);
                                    mutated = true;
                                }
                                Err(BackendSetError::SetError(e)) => {
                                    // update_object failed after create_object succeeded.
                                    // Roll back the create so the backend is not left with
                                    // a default-state singleton that the client never asked
                                    // for (compensating transaction). Without this, the next
                                    // request would attempt to update a singleton whose state
                                    // is inconsistent with what the client expects.
                                    //
                                    // Production backends should perform the create+update
                                    // atomically (e.g. in a transaction) to avoid the window
                                    // between these two calls entirely.
                                    match backend
                                        .destroy_object::<VacationResponse>(
                                            caller,
                                            &account_id,
                                            &singleton_id,
                                        )
                                        .await
                                    {
                                        Ok(()) => {}
                                        Err(rollback_err) => {
                                            // Rollback failed: the backend now holds an
                                            // orphaned default singleton. Log a warning so
                                            // operators can detect and repair the state.
                                            // This is acceptable for a test/reference backend
                                            // but production backends should use atomic writes.
                                            #[cfg(debug_assertions)]
                                            eprintln!(
                                                "WARN: VacationResponse upsert rollback failed \
                                                 (orphaned singleton in account {:?}): {:?}",
                                                account_id.as_ref(),
                                                rollback_err,
                                            );
                                            #[cfg(not(debug_assertions))]
                                            let _ = &rollback_err;
                                        }
                                    }
                                    not_updated.insert(id.clone(), set_error_value(&e));
                                }
                                Err(BackendSetError::Other(e)) => {
                                    return Err(JmapError::server_fail(e.to_string()));
                                }
                                Err(_) => {
                                    return Err(JmapError::server_fail(
                                        "unhandled backend error variant",
                                    ));
                                }
                            }
                        }
                        Err(BackendSetError::SetError(e)) => {
                            not_updated.insert(id.clone(), set_error_value(&e));
                        }
                        Err(BackendSetError::Other(e)) => {
                            return Err(JmapError::server_fail(e.to_string()));
                        }
                        Err(_) => {
                            return Err(JmapError::server_fail("unhandled backend error variant"));
                        }
                    }
                }
                Err(BackendSetError::SetError(e)) => {
                    not_updated.insert(id.clone(), set_error_value(&e));
                }
                Err(BackendSetError::Other(e)) => {
                    return Err(JmapError::server_fail(e.to_string()));
                }
                Err(_) => {
                    return Err(JmapError::server_fail("unhandled backend error variant"));
                }
            }
        }
    }

    // destroy — always forbidden for singletons.
    if let Some(destroy) = args.get("destroy").and_then(|v| v.as_array()) {
        for id_val in destroy {
            let id = match id_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            let err = SetError::new(SetErrorType::Singleton)
                .with_description("VacationResponse is a singleton; cannot destroy");
            not_destroyed.insert(id.to_owned(), set_error_value(&err));
        }
    }

    // VacationResponse is a singleton: created/destroyed are always empty by
    // construction (see the rejection branches above), so the helper's
    // empty-map → Value::Null conversion produces the same JSON the inline
    // hardcode used to.
    finalize_set_response::<B, VacationResponse>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        SetAccumulators {
            updated,
            not_created,
            not_updated,
            not_destroyed,
            ..Default::default()
        },
    )
    .await
}
