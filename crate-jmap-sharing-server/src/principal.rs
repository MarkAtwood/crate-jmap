//! Principal/* method handlers (RFC 9670 §2).
//!
//! Principal objects represent users, groups, locations, resources, and other
//! entities in a collaborative JMAP environment.  `Principal/set` delegates
//! permission enforcement entirely to the backend — the spec allows any
//! server to restrict creates/updates/destroys with `forbidden` SetErrors.

use jmap_sharing_types::Principal;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, SharingBackend};
use crate::helpers::{extract_account_id, set_error_value};

// ---------------------------------------------------------------------------
// Principal/get
// ---------------------------------------------------------------------------

/// Handle a `Principal/get` method call (RFC 9670 §2.1).
pub async fn handle_principal_get<B: SharingBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<Principal, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Principal/changes
// ---------------------------------------------------------------------------

/// Handle a `Principal/changes` method call (RFC 9670 §2.2).
///
/// Backends backed by external read-only directories may return
/// `BackendChangesError::TooManyChanges { limit: 0 }` to signal
/// `cannotCalculateChanges` per RFC 8620 §5.2.
pub async fn handle_principal_changes<B: SharingBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Principal, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Principal/set
// ---------------------------------------------------------------------------

/// Handle a `Principal/set` method call (RFC 9670 §2.3).
///
/// All create/update/destroy operations are forwarded to the backend. The
/// backend is responsible for returning `forbidden` SetErrors for operations
/// it does not permit (e.g. a read-only directory backend rejects all writes).
pub async fn handle_principal_set<B: SharingBackend>(
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
        .get_state::<Principal>(&account_id)
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
    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj_val) in create_map {
            // Deserialize the Principal from the client-provided object.
            // Missing required fields (id is server-assigned; supply a placeholder)
            // are injected before forwarding to the backend.
            let obj_with_id = match obj_val {
                Value::Object(mut m) => {
                    // Inject a placeholder id — the backend replaces it with the
                    // server-assigned id on success.
                    m.entry("id")
                        .or_insert_with(|| Value::String("placeholder".to_owned()));
                    Value::Object(m)
                }
                other => other,
            };

            let principal: Principal = match serde_json::from_value(obj_with_id) {
                Ok(p) => p,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            match backend
                .create_object::<Principal>(&account_id, &create_id, principal)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    created.insert(
                        create_id,
                        serde_json::to_value(&created_obj).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(create_id, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(
                        create_id,
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

            match backend
                .update_object::<Principal>(&account_id, &id, patch_val)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
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
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s.to_owned(),
                None => continue,
            };
            let id = Id::from(id_str.as_str());

            match backend.destroy_object::<Principal>(&account_id, &id).await {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(id_str, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<Principal>(&account_id)
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
            "created":      if created.is_empty()        { Value::Null } else { Value::Object(created) },
            "updated":      if updated.is_empty()        { Value::Null } else { Value::Object(updated) },
            "destroyed":    if destroyed_list.is_empty() { Value::Null } else { Value::Array(destroyed_list) },
            "notCreated":   if not_created.is_empty()    { Value::Null } else { Value::Object(not_created) },
            "notUpdated":   if not_updated.is_empty()    { Value::Null } else { Value::Object(not_updated) },
            "notDestroyed": if not_destroyed.is_empty()  { Value::Null } else { Value::Object(not_destroyed) },
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// Principal/query
// ---------------------------------------------------------------------------

/// Handle a `Principal/query` method call (RFC 9670 §2.4).
pub async fn handle_principal_query<B: SharingBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<Principal, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Principal/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Principal/queryChanges` method call (RFC 9670 §2.5).
///
/// Backends that cannot calculate query changes return
/// `BackendChangesError::TooManyChanges { limit: 0 }` which maps to
/// `cannotCalculateChanges`.
pub async fn handle_principal_query_changes<B: SharingBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<Principal, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: handle_principal_get with unknown accountId returns accountNotFound.
    ///
    /// Source: RFC 8620 §3.6.2 — accountId unknown to the server → accountNotFound.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        // accountId "unknown" is not registered in the mock → account_exists returns false.
        let args = json!({
            "accountId": "unknown",
            "ids": null
        });
        let result = handle_principal_get(&backend, args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(
            err.error_type.as_str(),
            "accountNotFound",
            "unknown accountId must produce accountNotFound; got: {:?}",
            err.error_type
        );
    }

    /// Oracle: Principal/set forwarding — backend forbidden response surfaces correctly.
    ///
    /// Source: RFC 9670 §2.3 — server rejects changes it doesn't allow with `forbidden`.
    #[tokio::test]
    async fn set_backend_forbidden_on_update_returns_not_updated() {
        let backend = MockBackend::new_with_account("acc1");
        // The mock backend returns `forbidden` for all updates.
        let args = json!({
            "accountId": "acc1",
            "update": {
                "P1": { "name": "New Name" }
            }
        });
        let (resp, _) = handle_principal_set(&backend, args)
            .await
            .expect("must not return top-level error");
        let not_updated = &resp["notUpdated"];
        assert!(
            not_updated.is_object(),
            "notUpdated must be present: {resp}"
        );
        assert_eq!(
            not_updated["P1"]["type"], "forbidden",
            "backend forbidden must appear in notUpdated: {resp}"
        );
    }

    /// Oracle: Principal/set create with invalid JSON → invalidProperties in notCreated.
    #[tokio::test]
    async fn set_create_missing_required_field_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc1");
        // "type" is missing — required by Principal struct.
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "name": "Alice"
                }
            }
        });
        let (resp, _) = handle_principal_set(&backend, args)
            .await
            .expect("must not return top-level error");
        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "invalidProperties",
            "missing required field must produce invalidProperties: {resp}"
        );
    }
}
