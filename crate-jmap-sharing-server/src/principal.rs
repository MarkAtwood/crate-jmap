//! Principal/* method handlers (RFC 9670 §2).
//!
//! Principal objects represent users, groups, locations, resources, and other
//! entities in a collaborative JMAP environment.  `Principal/set` delegates
//! permission enforcement entirely to the backend — the spec allows any
//! server to restrict creates/updates/destroys with `forbidden` SetErrors.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by RFC 9670 §2. The returned `Value` is the
//! corresponding method-response object per the same section refs.
//!
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `unsupportedFilter`, `unsupportedSort`, `cannotCalculateChanges` —
//! per RFC 8620 §3.6 and §5). Per-target failures inside `/set` surface
//! in the `notCreated` / `notUpdated` / `notDestroyed` maps within
//! `Ok((Value, ...))`, not as `Err`.

use jmap_sharing_types::Principal;
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, SharingBackend};
use crate::helpers::{
    enforce_max_objects_in_set, extract_account_id, finalize_set_response, set_error_value,
    SetAccumulators,
};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend};

// ---------------------------------------------------------------------------
// Principal/get
// ---------------------------------------------------------------------------

/// Handle a `Principal/get` method call (RFC 9670 §2.1).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_principal_get<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<Principal, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Principal/changes
// ---------------------------------------------------------------------------

/// Handle a `Principal/changes` method call (RFC 9670 §2.2).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Backends backed by external read-only directories may return
/// [`BackendChangesError::CannotCalculate`] (bd:JMAP-jfia.31) to
/// signal `cannotCalculateChanges` per RFC 8620 §5.2. The
/// `TooManyChanges { limit: 0 }` magic-zero alias maps to the same
/// wire error via the permanent legacy-alias path (bd:JMAP-jfia.37).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_principal_changes<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Principal, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Principal/set
// ---------------------------------------------------------------------------

/// Handle a `Principal/set` method call (RFC 9670 §2.3).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// All create/update/destroy operations are forwarded to the backend. The
/// backend is responsible for returning `forbidden` SetErrors for operations
/// it does not permit (e.g. a read-only directory backend rejects all writes).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_principal_set<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: an unknown accountId MUST surface as `accountNotFound`.
    // The /get-family handlers in `jmap_server::handlers` perform this check
    // internally; the hand-rolled /set handler must reproduce it explicitly.
    // Mirrors the canonical pattern at
    // `crate-jmap-mail-server/src/mailbox.rs:441-447`.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.8). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<Principal>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

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
                .create_object::<Principal>(caller, &account_id, &create_id, principal)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    created.insert(
                        create_id,
                        serde_json::to_value(&created_obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(create_id, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(create_id, server_fail_value_from_backend(&e));
                }
                Err(_) => {
                    not_created.insert(
                        create_id,
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
            // Id::from: wire-boundary validation deferred to JMAP-k9va; backend rejects unknown IDs.
            let id = Id::from(id_str.as_str());

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let patch = match serde_json::from_value::<PatchObject>(patch_val) {
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
                .update_object::<Principal>(caller, &account_id, &id, patch)
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
                    not_updated.insert(id_str, server_fail_value_from_backend(&e));
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
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        //
        // Single-pass collect: `as_str().ok_or(v)` yields `Ok(&str)` for valid
        // ids and `Err(&Value)` for the first bad element. The resulting
        // `Vec<&str>` lets the consume loop work in `&str` without a
        // never-triggered fallback arm.
        let ids: Vec<&str> = destroy_arr
            .iter()
            .map(|v| v.as_str().ok_or(v))
            .collect::<Result<_, _>>()
            .map_err(|bad| {
                JmapError::invalid_arguments(format!(
                    "destroy: every element must be a string Id; got {bad}"
                ))
            })?;
        for id_str_ref in ids {
            let id_str = id_str_ref.to_owned();
            let id = Id::from(id_str.as_str());

            match backend
                .destroy_object::<Principal>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(id_str, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(id_str, server_fail_value_from_backend(&e));
                }
                Err(_) => {
                    not_destroyed.insert(
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

    finalize_set_response::<B, Principal>(
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

// ---------------------------------------------------------------------------
// Principal/query
// ---------------------------------------------------------------------------

/// Handle a `Principal/query` method call (RFC 9670 §2.4).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`); the
/// returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_principal_query<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<Principal, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Principal/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Principal/queryChanges` method call (RFC 9670 §2.5).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Backends that cannot calculate query changes return
/// [`BackendChangesError::CannotCalculate`] (bd:JMAP-jfia.31) which
/// maps to `cannotCalculateChanges`. The
/// `TooManyChanges { limit: 0 }` magic-zero alias is preserved via the
/// permanent legacy-alias path (bd:JMAP-jfia.37).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_principal_query_changes<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<Principal, B>(backend, caller, args).await
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
        let result = handle_principal_get(&backend, &(), args).await;
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
        let (resp, _) = handle_principal_set(&backend, &(), args)
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

    /// Oracle: Principal/set destroy array with a non-string element must return
    /// a top-level invalidArguments error, not silently skip the element.
    #[tokio::test]
    async fn set_destroy_non_string_element_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": [123]  // integer, not string
        });
        let result = handle_principal_set(&backend, &(), args).await;
        let err = result.expect_err("must return top-level error for non-string destroy element");
        assert_eq!(err.error_type.as_str(), "invalidArguments");
    }

    /// Oracle: Principal/set with unknown accountId must return top-level
    /// accountNotFound (RFC 8620 §3.6.2), NOT silently proceed and build a
    /// fabricated /set response envelope. Regression guard for
    /// bd:JMAP-3t94.1.
    #[tokio::test]
    async fn set_unknown_account_returns_account_not_found() {
        // MockBackend::new() registers no accounts → account_exists returns
        // false for any accountId.
        let backend = MockBackend::new();
        let args = json!({
            "accountId": "unknown",
            "create": {
                "c1": { "type": "individual", "name": "Mallory" }
            }
        });
        let result = handle_principal_set(&backend, &(), args).await;
        let err = result.expect_err("must return top-level error for unknown account");
        assert_eq!(
            err.error_type.as_str(),
            "accountNotFound",
            "unknown accountId must produce accountNotFound; got: {:?}",
            err.error_type
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
        let (resp, _) = handle_principal_set(&backend, &(), args)
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
