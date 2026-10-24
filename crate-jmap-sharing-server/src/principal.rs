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
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures (RFC 8620
//! §3.6.2 and §5):
//!
//! - every handler: `accountNotFound`, `invalidArguments`, `serverFail`;
//! - `/get`, `/set`: `requestTooLarge` past `maxObjectsInGet` /
//!   `maxObjectsInSet`;
//! - `/set`: `stateMismatch`;
//! - `/changes`, `/queryChanges`: `cannotCalculateChanges`;
//! - `/queryChanges`: `tooManyChanges`;
//! - `/query`, `/queryChanges`: `unsupportedFilter`;
//! - `/query`: `unsupportedSort`, `anchorNotFound`.
//!
//! Per-target failures inside `/set` surface in the `notCreated` / `notUpdated`
//! / `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use jmap_sharing_types::{Principal, PrincipalFilterCondition};
use jmap_types::{Id, JmapError, PatchObject};
use serde_json::{Value, json};

use crate::SharingBackend;
use crate::helpers::{
    SetAccumulators, check_metadata_entry, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response,
};
use jmap_server::{
    HandlerOutput, backend_set_error_value, reject_metadata_filter_arg, reject_read_only_patch,
    reject_server_set, server_fail_from_backend,
};

// ---------------------------------------------------------------------------
// Principal/get
// ---------------------------------------------------------------------------

/// Handle a `Principal/get` method call (RFC 9670 §2.1).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]). When the backend lists Principal for
/// draft-ietf-jmap-metadata-04 ([`JmapBackend::metadata_info`](jmap_server::JmapBackend::metadata_info)),
/// `metadata` is returned as an object, by default only with
/// `urn:ietf:params:jmap:metadata` in `using`, and with that capability
/// `properties` may hold `metadata/<namespace>` subselectors (-04 §2,
/// §3.1); see [`jmap_server::handle_get_with_metadata`].
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_principal_get<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_get_with_metadata::<Principal, B>(backend, caller, args, using).await
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
/// [`BackendChangesError::CannotCalculate`](jmap_server::BackendChangesError::CannotCalculate) (bd:JMAP-jfia.31) to
/// signal `cannotCalculateChanges` per RFC 8620 §5.2.
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); with
/// `urn:ietf:params:jmap:metadata` the request may set
/// draft-ietf-jmap-metadata-04 §3.3 `ignoreMetadataOnlyChanges`, and
/// without it metadata-only changes are left out (-04 §3).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_principal_changes<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<Principal, B>(backend, caller, args, using).await
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
/// ## Metadata
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]): a write to `metadata` or
/// `privateMetadata` without `urn:ietf:params:jmap:metadata` is
/// `invalidProperties` (draft-ietf-jmap-metadata-04 §3), as is `null` for
/// either in a create (-04 §3.2). The backend validates the rest.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_principal_set<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: an unknown accountId MUST surface as `accountNotFound`.
    // The /get-family handlers in `jmap-server` perform this check
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

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
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
            if let Some(err) = obj_val.as_object().and_then(reject_server_set::<Principal>) {
                not_created.insert(create_id, serde_json::Value::from(&err));
                continue;
            }
            if let Some(Err(err)) = obj_val
                .as_object()
                .map(|o| check_metadata_entry(o, true, using))
            {
                not_created.insert(create_id, serde_json::Value::from(&*err));
                continue;
            }
            // Deserialize the Principal from the client-provided object.
            // `Principal.id` is required, so the parse fills it with
            // `jmap_server::CREATE_ID_PLACEHOLDER`; `build` overwrites it with the minted id.
            // `capabilities` and `accounts` are server-set (RFC 9670 §2)
            // and required, so the server supplies them: no capabilities
            // and no accounts until the backend grants some.
            let obj_with_id = match obj_val {
                Value::Object(mut m) => {
                    m.insert("id".to_owned(), jmap_server::CREATE_ID_PLACEHOLDER.into());
                    m.insert("capabilities".to_owned(), json!({}));
                    m.insert("accounts".to_owned(), Value::Null);
                    Value::Object(m)
                }
                other => other,
            };

            let mut principal: Principal = match serde_json::from_value(obj_with_id) {
                Ok(p) => p,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            let build = move |id: &Id| {
                principal.id = id.clone();
                principal
            };
            match backend
                .create_object::<Principal, _>(caller, &account_id, &create_id, build)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    created.insert(create_id, jmap_server::serialize_value(&created_obj)?);
                }
                Err(e) => {
                    not_created.insert(create_id, backend_set_error_value(e));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_updated.insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };

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
            if let Some(err) = reject_read_only_patch::<Principal>(patch.as_map(), None) {
                not_updated.insert(id_str, serde_json::Value::from(&err));
                continue;
            }
            if let Err(err) = check_metadata_entry(patch.as_map(), false, using) {
                not_updated.insert(id_str, serde_json::Value::from(&*err));
                continue;
            }

            match backend
                .update_object::<Principal>(caller, &account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(id_str, jmap_server::serialize_value(&obj)?);
                }
                Ok(None) => {
                    mutated = true;
                    updated.insert(id_str, Value::Null);
                }
                Err(e) => {
                    not_updated.insert(id_str, backend_set_error_value(e));
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
                    "destroy: every element must be a string Id; got {}",
                    jmap_server::json_type_name(bad)
                ))
            })?;
        for id_str_ref in ids {
            let id_str = id_str_ref.to_owned();
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_destroyed.insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };

            match backend
                .destroy_object::<Principal>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str));
                }
                Err(e) => {
                    not_destroyed.insert(id_str, backend_set_error_value(e));
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
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); without
/// `urn:ietf:params:jmap:metadata` a draft-ietf-jmap-metadata-04 §3.6
/// filter condition is `unsupportedFilter` (-04 §3).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_principal_query<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    reject_metadata_filter_arg(
        &args,
        PrincipalFilterCondition::has_metadata_condition,
        using,
    )?;
    jmap_server::handle_query::<Principal, B>(backend, caller, args).await
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
/// [`BackendChangesError::CannotCalculate`](jmap_server::BackendChangesError::CannotCalculate) (bd:JMAP-jfia.31) which
/// maps to `cannotCalculateChanges`.
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); without
/// `urn:ietf:params:jmap:metadata` a draft-ietf-jmap-metadata-04 §3.6
/// filter condition is `unsupportedFilter` (-04 §3).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_principal_query_changes<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    reject_metadata_filter_arg(
        &args,
        PrincipalFilterCondition::has_metadata_condition,
        using,
    )?;
    jmap_server::handle_query_changes::<Principal, B>(backend, caller, args).await
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
        let result = handle_principal_get(&backend, &(), args, &[]).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(
            err.error_type.to_string(),
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
        let HandlerOutput { primary: resp, .. } = handle_principal_set(&backend, &(), args, &[])
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
        let result = handle_principal_set(&backend, &(), args, &[]).await;
        let err = result.expect_err("must return top-level error for non-string destroy element");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
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
        let result = handle_principal_set(&backend, &(), args, &[]).await;
        let err = result.expect_err("must return top-level error for unknown account");
        assert_eq!(
            err.error_type.to_string(),
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
        let HandlerOutput { primary: resp, .. } = handle_principal_set(&backend, &(), args, &[])
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

    /// RFC 8620 §5.3 types `destroy` as "Id[]|null", so an object element
    /// is "invalidArguments: One of the arguments is of the wrong type"
    /// (§3.6.2). The description names only the JSON type: §3.6.1 reserves
    /// it for "unstructured English text suitable for displaying to a
    /// developer", so the client-supplied value is never echoed
    /// (bd JMAP-zegc).
    #[tokio::test]
    async fn set_non_string_destroy_element_does_not_echo_value() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "destroy": [{ "note": "CANARY-zegc" }] });
        let err = handle_principal_set(&backend, &(), args, &[])
            .await
            .expect_err("non-string destroy element must be a method error");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
        let desc = err.description.as_deref().unwrap_or_default();
        assert!(desc.contains("got object"), "{desc}");
        assert!(!desc.contains("CANARY"), "{desc}");
    }
}
