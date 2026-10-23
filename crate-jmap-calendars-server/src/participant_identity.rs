//! ParticipantIdentity/* method handlers (draft-ietf-jmap-calendars-33 §3).
//!
//! The set of identities, and which is the default, depends on the
//! authenticated user as well as the account, so the backend's
//! caller-scoped reads and the `/changes` state MUST reflect that user's
//! view (§3, §3.2). A server that configures identities out of band
//! rejects every `/set` operation with `forbidden` (§3.3).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3), with the
//! type-specific arguments defined by draft-ietf-jmap-calendars-33 §3.
//! The returned `Value` is the corresponding method-response object per
//! the same section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `cannotCalculateChanges` — per RFC 8620 §3.6 and §5). Per-target
//! failures inside `/set` surface in the `notCreated` / `notUpdated` /
//! `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use jmap_calendars_types::ParticipantIdentity;
use jmap_types::{Id, JmapError, PatchObject};
use serde_json::{Value, json};

use crate::CalendarsBackend;
use crate::helpers::{
    SetAccumulators, apply_default_change_to_response, enforce_max_objects_in_set,
    extract_account_id, finalize_set_response, resolve_on_success_set_is_default, set_error_value,
};
use jmap_server::{
    HandlerOutput, backend_set_error_value, reject_read_only_patch, reject_server_set,
    server_fail_from_backend,
};

// ---------------------------------------------------------------------------
// ParticipantIdentity/get
// ---------------------------------------------------------------------------

/// Handle a `ParticipantIdentity/get` method call
/// (draft-ietf-jmap-calendars-33 §3.1).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_participant_identity_get<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_get::<ParticipantIdentity, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// ParticipantIdentity/changes
// ---------------------------------------------------------------------------

/// Handle a `ParticipantIdentity/changes` method call
/// (draft-ietf-jmap-calendars-33 §3.2).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_participant_identity_changes<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<ParticipantIdentity, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// ParticipantIdentity/set
// ---------------------------------------------------------------------------

/// Handle a `ParticipantIdentity/set` method call
/// (draft-ietf-jmap-calendars-33 §3.3).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps), augmented
/// with the draft §3.3 `onSuccessSetIsDefault` argument; the returned
/// `Value` is the §5.3 `/set` response shape (`accountId`, `oldState`,
/// `newState`, plus the per-operation `created` / `notCreated` /
/// `updated` / `notUpdated` / `destroyed` / `notDestroyed` maps).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_participant_identity_set<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: accountId not recognised → accountNotFound.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ops7.31). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    // §3.3: onSuccessSetIsDefault — Id|null. Captured here so we can resolve
    // a possible "#createId" reference against the post-create state. The
    // raw value is kept until after all CRUD ops succeed.
    let on_success_set_is_default = args.remove("onSuccessSetIsDefault");

    let old_state = backend
        .get_state::<ParticipantIdentity>(caller, &account_id)
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

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, mut obj_val) in create_map {
            if let Some(err) = obj_val
                .as_object()
                .and_then(reject_server_set::<ParticipantIdentity>)
            {
                not_created.insert(create_id, set_error_value(&err));
                continue;
            }
            // `isDefault` is server-set (draft-ietf-jmap-calendars-33 §3) and
            // `ParticipantIdentity` requires it, so the server supplies it: a
            // new identity is not the default until `onSuccessSetIsDefault`
            // makes it one.
            if let Some(m) = obj_val.as_object_mut() {
                m.insert("isDefault".to_owned(), Value::Bool(false));
            }
            // `ParticipantIdentity.id` is optional, so the create parses without one
            // and `build` sets the id the backend mints.
            let mut pi: ParticipantIdentity = match serde_json::from_value(obj_val) {
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
                pi.id = Some(id.clone());
                pi
            };
            match backend
                .create_object::<ParticipantIdentity, _>(caller, &account_id, &create_id, build)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    // ParticipantIdentity uses #[derive(Serialize)] on plain
                    // data; to_value is infallible (JMAP-r3pg.13).
                    created.insert(
                        create_id,
                        serde_json::to_value(&created_obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                Err(e) => {
                    not_created.insert(create_id, backend_set_error_value(e));
                }
            }
        }
    }

    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_updated.insert(id_str, set_error_value(&err));
                    continue;
                }
            };
            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 requires a PatchObject is a JSON Object; non-object
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
            if let Some(err) = reject_read_only_patch::<ParticipantIdentity>(patch.as_map(), None) {
                not_updated.insert(id_str, set_error_value(&err));
                continue;
            }
            match backend
                .update_object::<ParticipantIdentity>(caller, &account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    // See create branch above (JMAP-r3pg.13).
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
                Err(e) => {
                    not_updated.insert(id_str, backend_set_error_value(e));
                }
            }
        }
    }

    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        let id_strs: Vec<&str> = destroy_arr
            .iter()
            .map(|v| {
                v.as_str().ok_or_else(|| {
                    JmapError::invalid_arguments(format!(
                        "destroy: every element must be a string Id; got {}",
                        jmap_server::json_type_name(v)
                    ))
                })
            })
            .collect::<Result<_, _>>()?;
        for id_str in id_strs {
            let id_str = id_str.to_owned();
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_destroyed.insert(id_str, set_error_value(&err));
                    continue;
                }
            };
            match backend
                .destroy_object::<ParticipantIdentity>(caller, &account_id, &id)
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

    // §3.3: onSuccessSetIsDefault. Apply only if every CRUD attempt
    // succeeded — if any not_* map has entries, the spec's "all creates,
    // updates and destroys (if any) succeed without error" guard fails
    // and the requested default change is skipped silently.
    let all_succeeded =
        not_created.is_empty() && not_updated.is_empty() && not_destroyed.is_empty();
    if all_succeeded
        && let Some(raw) = on_success_set_is_default.as_ref()
        && let Some(target) = resolve_on_success_set_is_default(raw, &created)
    {
        match backend
            .set_default_participant_identity(caller, &account_id, &target)
            .await
        {
            Ok(result) => {
                if apply_default_change_to_response(&mut created, &mut updated, &result) {
                    mutated = true;
                }
            }
            Err(_e) => {
                // §3.3: silently swallow — "No error is returned to
                // the client". Genuine storage errors lose the
                // default change but do not fail the /set.
            }
        }
    }

    finalize_set_response::<B, ParticipantIdentity>(
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: ParticipantIdentity/get with unknown accountId returns accountNotFound.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let result = handle_participant_identity_get(&backend, &(), args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.to_string(), "accountNotFound");
    }

    /// Oracle: ParticipantIdentity/set with unknown accountId returns accountNotFound.
    /// Source: RFC 8620 §3.6.2.
    #[tokio::test]
    async fn set_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown" });
        let result = handle_participant_identity_set(&backend, &(), args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.to_string(), "accountNotFound");
    }

    /// Oracle: ParticipantIdentity/set create with client-supplied "id" →
    /// notCreated with invalidProperties citing properties:["id"].
    /// Source: RFC 8620 §5.3 — "The id property MUST NOT be set in the
    /// create object." Independent oracle: spec wire shape is hand-written.
    #[tokio::test]
    async fn set_create_with_client_supplied_id_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "id": "client-chosen-id", "name": "Alice" }
            }
        });
        let HandlerOutput { primary: resp, .. } =
            handle_participant_identity_set(&backend, &(), args)
                .await
                .expect("must not return top-level error");
        assert_eq!(
            resp["notCreated"]["c1"]["type"], "invalidProperties",
            "must reject client-supplied id with invalidProperties: {resp}"
        );
        assert_eq!(
            resp["notCreated"]["c1"]["properties"][0], "id",
            "must cite 'id' in properties: {resp}"
        );
        assert!(
            resp["created"].is_null(),
            "must not have created any participant identity: {resp}"
        );
    }
}
