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
    HandlerOutput, backend_set_error_value, check_read_only_patch, reject_metadata_filter_arg,
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
/// draft-ietf-jmap-metadata-05 ([`JmapAccounts::metadata_info`](jmap_server::JmapAccounts::metadata_info)),
/// `metadata` is returned as an object, by default only with
/// `urn:ietf:params:jmap:metadata` in `using`, and with that capability
/// `properties` may hold `metadata/<namespace>` subselectors (-05 §2,
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
/// draft-ietf-jmap-metadata-05 §3.3 `ignoreMetadataOnlyChanges`, and
/// without it metadata-only changes are left out (-05 §3).
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
/// `invalidProperties` (draft-ietf-jmap-metadata-05 §3), as is `null` for
/// either in a create (-05 §3.2). The backend validates the rest.
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

    // draft-atwood-jmap-updates-00 §3, §4.1: `locale` and `localizations`.
    // The checks that need the patched object run in the backend
    // (`jmap_server::localized_update`).
    let i18n_set = jmap_server::I18nSet::new::<Principal>(
        using,
        backend.i18n_info(caller, &account_id).as_ref(),
    );

    let old_state = backend
        .get_state::<Principal>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    let ctx = SetCtx {
        backend,
        caller,
        account_id: &account_id,
        using,
        i18n_set: &i18n_set,
    };
    let mut acc = SetAccumulators::default();
    let mut mutated = set_create(&ctx, &mut args, &mut acc).await?;
    // RFC 8620 §5.3: update keys and destroy ids may name this call's creates.
    jmap_server::resolve_set_id_refs(&mut args, &std::collections::BTreeMap::new(), &acc.created);
    mutated |= set_update(&ctx, &mut args, &mut acc).await?;
    mutated |= set_destroy(&ctx, &mut args, &mut acc).await?;

    finalize_set_response::<B, Principal>(backend, caller, &account_id, old_state, mutated, acc)
        .await
}

/// What every `Principal/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: SharingBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
    using: &'a [String],
    i18n_set: &'a jmap_server::I18nSet,
}

/// The `create` part of `Principal/set`. Returns whether the backend was
/// written.
async fn set_create<B: SharingBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj_val) in create_map {
            let mut principal = match principal_from_create(ctx, obj_val) {
                Ok(p) => p,
                Err(err) => {
                    acc.not_created.insert(create_id, err);
                    continue;
                }
            };

            let build = move |id: &Id| {
                principal.id = id.clone();
                principal
            };
            match ctx
                .backend
                .create_object::<Principal, _>(ctx.caller, ctx.account_id, &create_id, build)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    let mut entry = jmap_server::serialize_value(&created_obj)?;
                    ctx.i18n_set.echo(&mut entry);
                    acc.created.insert(create_id, entry);
                }
                Err(e) => {
                    acc.not_created
                        .insert(create_id, backend_set_error_value(e));
                }
            }
        }
    }
    Ok(mutated)
}

/// Check one `Principal/set` create object and parse it into the Principal
/// to store, or return the SetError value for `notCreated`.
fn principal_from_create<B: SharingBackend>(
    ctx: &SetCtx<'_, B>,
    obj_val: Value,
) -> Result<Principal, Value> {
    if let Some(err) = obj_val.as_object().and_then(reject_server_set::<Principal>) {
        return Err(serde_json::Value::from(&err));
    }
    if let Some(Err(err)) = obj_val
        .as_object()
        .map(|o| check_metadata_entry(o, true, ctx.using))
    {
        return Err(serde_json::Value::from(&*err));
    }
    let mut obj_val = obj_val;
    if let Value::Object(m) = &mut obj_val
        && let Err(err) = ctx.i18n_set.check_create(m)
    {
        return Err(serde_json::Value::from(&*err));
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

    // RFC 8620 §5.3: a create entry that is not an object has no
    // properties to name, so report it without a `properties` list.
    if !obj_with_id.is_object() {
        return Err(json!({
            "type": "invalidProperties",
            "description": "a create entry must be an object",
        }));
    }

    // RFC 9670 §2: `type` and `name` are required. serde reports a
    // missing field at the root, not on a property path, so name them
    // here (RFC 8620 §5.3 invalidProperties).
    let missing: Vec<&str> = ["type", "name"]
        .into_iter()
        .filter(|p| obj_with_id.get(p).is_none())
        .collect();
    if !missing.is_empty() {
        return Err(json!({
            "type": "invalidProperties",
            "properties": missing,
            "description": "required property missing on create",
        }));
    }

    serde_path_to_error::deserialize(obj_with_id).map_err(|e| {
        // RFC 8620 §5.3: invalidProperties SHOULD list the
        // invalid properties; the first path segment is the
        // top-level property whose value failed to parse.
        let mut err = json!({ "type": "invalidProperties", "description": e.to_string() });
        if let Some(serde_path_to_error::Segment::Map { key }) = e.path().iter().next() {
            err["properties"] = json!([key]);
        }
        err
    })
}

/// The `update` part of `Principal/set`. Returns whether the backend was
/// written.
async fn set_update<B: SharingBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            mutated |= update_one(ctx, id_str, patch_val, acc).await?;
        }
    }
    Ok(mutated)
}

/// One entry of the `Principal/set` `update` map. Returns whether the
/// backend was written.
async fn update_one<B: SharingBackend>(
    ctx: &SetCtx<'_, B>,
    id_str: String,
    patch_val: Value,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let id = match jmap_server::parse_wire_id(&id_str, "id") {
        Ok(id) => id,
        Err(err) => {
            acc.not_updated
                .insert(id_str, serde_json::Value::from(&*err));
            return Ok(false);
        }
    };

    // Convert wire-format Value into a typed PatchObject. RFC 8620
    // §5.3 mandates a PatchObject is a JSON Object; non-object
    // values produce an `invalidPatch` SetError.
    let mut patch = match serde_json::from_value::<PatchObject>(patch_val) {
        Ok(p) => p,
        Err(e) => {
            acc.not_updated.insert(
                id_str,
                json!({ "type": "invalidPatch", "description": e.to_string() }),
            );
            return Ok(false);
        }
    };
    // RFC 9670 §2: `id`, `capabilities` and `accounts` are server-set. A changed value is rejected, an identical
    // one dropped (RFC 8620 §5.3); nothing left is a no-op success.
    match check_read_only_patch::<_, Principal>(
        ctx.backend,
        ctx.caller,
        ctx.account_id,
        &id,
        patch.as_map_mut(),
    )
    .await
    {
        Ok(false) => {}
        Ok(true) => {
            acc.updated.insert(id_str, Value::Null);
            return Ok(false);
        }
        Err(err) => {
            acc.not_updated.insert(id_str, err);
            return Ok(false);
        }
    }
    if let Err(err) = check_metadata_entry(patch.as_map(), false, ctx.using)
        .and_then(|()| ctx.i18n_set.check_patch(patch.as_map()))
    {
        acc.not_updated
            .insert(id_str, serde_json::Value::from(&*err));
        return Ok(false);
    }

    match ctx
        .backend
        .update_object::<Principal>(ctx.caller, ctx.account_id, &id, patch)
        .await
    {
        Ok(Some(obj)) => {
            let mut entry = jmap_server::serialize_value(&obj)?;
            ctx.i18n_set.echo(&mut entry);
            acc.updated.insert(id_str, entry);
        }
        Ok(None) => {
            acc.updated.insert(id_str, Value::Null);
        }
        Err(e) => {
            acc.not_updated.insert(id_str, backend_set_error_value(e));
            return Ok(false);
        }
    }
    Ok(true)
}

/// The `destroy` part of `Principal/set`, including the whole-request
/// `invalidArguments` for a non-string element. Returns whether the
/// backend was written.
async fn set_destroy<B: SharingBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
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
                    acc.not_destroyed
                        .insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };

            match ctx
                .backend
                .destroy_object::<Principal>(ctx.caller, ctx.account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    acc.destroyed.push(Value::String(id_str));
                }
                Err(e) => {
                    acc.not_destroyed.insert(id_str, backend_set_error_value(e));
                }
            }
        }
    }
    Ok(mutated)
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
/// `urn:ietf:params:jmap:metadata` a draft-ietf-jmap-metadata-05 §3.6
/// filter condition is `unsupportedFilter` (-05 §3).
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
    // draft-atwood-jmap-updates-00 §4.2.2: `language` on Principal.
    jmap_server::handle_query_with_i18n::<Principal, B>(backend, caller, args, using).await
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
/// `urn:ietf:params:jmap:metadata` a draft-ietf-jmap-metadata-05 §3.6
/// filter condition is `unsupportedFilter` (-05 §3).
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
    // draft-atwood-jmap-updates-00 §4.2.3: `language` on Principal.
    jmap_server::handle_query_changes_with_i18n::<Principal, B>(backend, caller, args, using).await
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

    /// Oracle: RFC 8620 §5.3: for `invalidProperties` "the SetError object
    /// SHOULD also have a property called `properties` of type `String[]`
    /// that lists *all* the properties that were invalid". Each create below
    /// has exactly one hand-picked bad property (`type` and `name` are required Strings, RFC 9670 §2).
    #[tokio::test]
    async fn set_create_typed_parse_failure_names_property() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "missingName": { "type": "individual" },
                "missingType": { "name": "A" },
                "numericType": { "type": 5, "name": "A" },
                "numericName": { "type": "individual", "name": 5 }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_principal_set(&backend, &(), args, &[])
            .await
            .expect("must not return top-level error");
        assert!(resp["created"].is_null(), "nothing may be created: {resp}");
        for (cid, prop) in [
            ("missingName", "name"),
            ("missingType", "type"),
            ("numericType", "type"),
            ("numericName", "name"),
        ] {
            let err = &resp["notCreated"][cid];
            assert_eq!(err["type"], "invalidProperties", "{cid}: {resp}");
            assert_eq!(err["properties"], json!([prop]), "{cid}: {resp}");
        }
    }

    /// Oracle: RFC 8620 §5.3: `create` is "A map of a creation id ... to a
    /// Foo object"; an entry that is not a JSON object is invalid. It has
    /// no properties, so the SetError must not name one it does not have
    /// (hand-written non-object entries below).
    #[tokio::test]
    async fn set_create_non_object_entry_names_no_property() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": { "str": "x", "arr": ["x"], "nul": null, "num": 5 }
        });
        let HandlerOutput { primary: resp, .. } = handle_principal_set(&backend, &(), args, &[])
            .await
            .expect("must not return top-level error");
        assert!(resp["created"].is_null(), "nothing may be created: {resp}");
        for cid in ["str", "arr", "nul", "num"] {
            let err = &resp["notCreated"][cid];
            assert_eq!(err["type"], "invalidProperties", "{cid}: {resp}");
            assert!(err.get("properties").is_none(), "{cid}: {resp}");
        }
    }
}
