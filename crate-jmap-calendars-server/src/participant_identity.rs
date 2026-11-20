//! ParticipantIdentity/* method handlers (draft-ietf-jmap-calendars-34 §3).
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
//! type-specific arguments defined by draft-ietf-jmap-calendars-34 §3.
//! The returned `Value` is the corresponding method-response object per
//! the same section refs.
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
//! - `/changes`: `cannotCalculateChanges`.
//!
//! Per-target failures inside `/set` surface in the `notCreated` / `notUpdated`
//! / `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use jmap_calendars_types::ParticipantIdentity;
use jmap_types::{Id, JmapError, PatchObject};
use serde_json::{Value, json};

use crate::CalendarsBackend;
use crate::helpers::{
    SetAccumulators, apply_default_change_to_response, enforce_max_objects_in_set,
    extract_account_id, finalize_set_response, resolve_on_success_set_is_default,
};
use jmap_server::{
    HandlerOutput, backend_set_error_value, check_read_only_patch, reject_server_set,
    server_fail_from_backend,
};

// ---------------------------------------------------------------------------
// ParticipantIdentity/get
// ---------------------------------------------------------------------------

/// Handle a `ParticipantIdentity/get` method call
/// (draft-ietf-jmap-calendars-34 §3.1).
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
/// (draft-ietf-jmap-calendars-34 §3.2).
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
    jmap_server::handle_changes::<ParticipantIdentity, B>(backend, caller, args, &[]).await
}

// ---------------------------------------------------------------------------
// ParticipantIdentity/set
// ---------------------------------------------------------------------------

/// Handle a `ParticipantIdentity/set` method call
/// (draft-ietf-jmap-calendars-34 §3.3).
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

    let ctx = SetCtx {
        backend,
        caller,
        account_id: &account_id,
    };
    let mut acc = SetAccumulators::default();
    let mut mutated = set_create(&ctx, &mut args, &mut acc).await?;
    // RFC 8620 §5.3: update keys and destroy ids may name this call's creates.
    jmap_server::resolve_set_id_refs(&mut args, &std::collections::BTreeMap::new(), &acc.created);
    mutated |= set_update(&ctx, &mut args, &mut acc).await?;
    mutated |= set_destroy(&ctx, &mut args, &mut acc).await?;
    mutated |= set_on_success_default(&ctx, on_success_set_is_default.as_ref(), &mut acc).await;

    finalize_set_response::<B, ParticipantIdentity>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        acc,
    )
    .await
}

/// What every `ParticipantIdentity/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: CalendarsBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
}

/// The `create` part of `ParticipantIdentity/set`. Returns whether the
/// backend was written.
async fn set_create<B: CalendarsBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, mut obj_val) in create_map {
            if let Some(err) = obj_val
                .as_object()
                .and_then(reject_server_set::<ParticipantIdentity>)
            {
                acc.not_created
                    .insert(create_id, serde_json::Value::from(&err));
                continue;
            }
            // `isDefault` is server-set (draft-ietf-jmap-calendars-34 §3) and
            // `ParticipantIdentity` requires it, so the server supplies it: a
            // new identity is not the default until `onSuccessSetIsDefault`
            // makes it one.
            if let Some(m) = obj_val.as_object_mut() {
                m.insert("isDefault".to_owned(), Value::Bool(false));
            }
            // `ParticipantIdentity.id` is optional, so the create parses without one
            // and `build` sets the id the backend mints.
            // RFC 8620 §5.3: a create entry that is not an object has no
            // properties to name, so report it without a `properties` list.
            if !obj_val.is_object() {
                acc.not_created.insert(
                    create_id,
                    json!({
                        "type": "invalidProperties",
                        "description": "a create entry must be an object",
                    }),
                );
                continue;
            }

            // draft-ietf-jmap-calendars-34 §3: `calendarAddress` is required. serde reports a missing
            // field at the root, not on a property path, so name it here
            // (RFC 8620 §5.3 invalidProperties).
            if obj_val.get("calendarAddress").is_none() {
                acc.not_created.insert(
                    create_id,
                    json!({
                        "type": "invalidProperties",
                        "properties": ["calendarAddress"],
                        "description": "calendarAddress is required on create",
                    }),
                );
                continue;
            }

            let mut pi: ParticipantIdentity = match serde_path_to_error::deserialize(obj_val) {
                Ok(p) => p,
                Err(e) => {
                    // RFC 8620 §5.3: invalidProperties SHOULD list the
                    // invalid properties; the first path segment is the
                    // top-level property whose value failed to parse.
                    let mut err =
                        json!({ "type": "invalidProperties", "description": e.to_string() });
                    if let Some(serde_path_to_error::Segment::Map { key }) = e.path().iter().next()
                    {
                        err["properties"] = json!([key]);
                    }
                    acc.not_created.insert(create_id, err);
                    continue;
                }
            };
            let build = move |id: &Id| {
                pi.id = Some(id.clone());
                pi
            };
            match ctx
                .backend
                .create_object::<ParticipantIdentity, _>(
                    ctx.caller,
                    ctx.account_id,
                    &create_id,
                    build,
                )
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    // ParticipantIdentity uses #[derive(Serialize)] on plain
                    // data; to_value is infallible (JMAP-r3pg.13).
                    acc.created
                        .insert(create_id, jmap_server::serialize_value(&created_obj)?);
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

/// The `update` part of `ParticipantIdentity/set`. Returns whether the
/// backend was written.
async fn set_update<B: CalendarsBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    acc.not_updated
                        .insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };
            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 requires a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let mut patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    acc.not_updated.insert(
                        id_str,
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };
            // draft-ietf-jmap-calendars §3: `id` and `isDefault` are
            // server-set. A changed value is rejected, an identical one
            // dropped (RFC 8620 §5.3); nothing left is a no-op success.
            match check_read_only_patch::<_, ParticipantIdentity>(
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
                    continue;
                }
                Err(err) => {
                    acc.not_updated.insert(id_str, err);
                    continue;
                }
            }
            match ctx
                .backend
                .update_object::<ParticipantIdentity>(ctx.caller, ctx.account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    // See create branch above (JMAP-r3pg.13).
                    acc.updated
                        .insert(id_str, jmap_server::serialize_value(&obj)?);
                }
                Ok(None) => {
                    mutated = true;
                    acc.updated.insert(id_str, Value::Null);
                }
                Err(e) => {
                    acc.not_updated.insert(id_str, backend_set_error_value(e));
                }
            }
        }
    }
    Ok(mutated)
}

/// The `destroy` part of `ParticipantIdentity/set`. Returns whether the
/// backend was written.
///
/// # Errors
///
/// `invalidArguments` when an element of `destroy` is not a string.
async fn set_destroy<B: CalendarsBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
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
                    acc.not_destroyed
                        .insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };
            match ctx
                .backend
                .destroy_object::<ParticipantIdentity>(ctx.caller, ctx.account_id, &id)
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

/// The draft-ietf-jmap-calendars-34 §3.3 `onSuccessSetIsDefault` phase of
/// `ParticipantIdentity/set`. Returns whether the default changed.
async fn set_on_success_default<B: CalendarsBackend>(
    ctx: &SetCtx<'_, B>,
    on_success_set_is_default: Option<&Value>,
    acc: &mut SetAccumulators,
) -> bool {
    // §3.3: onSuccessSetIsDefault. Apply only if every CRUD attempt
    // succeeded — if any not_* map has entries, the spec's "all creates,
    // updates and destroys (if any) succeed without error" guard fails
    // and the requested default change is skipped silently.
    let all_succeeded =
        acc.not_created.is_empty() && acc.not_updated.is_empty() && acc.not_destroyed.is_empty();
    if all_succeeded
        && let Some(raw) = on_success_set_is_default
        && let Some(target) = resolve_on_success_set_is_default(raw, &acc.created)
    {
        match ctx
            .backend
            .set_default_participant_identity(ctx.caller, ctx.account_id, &target)
            .await
        {
            Ok(result) => {
                return apply_default_change_to_response(
                    &mut acc.created,
                    &mut acc.updated,
                    &result,
                );
            }
            Err(_e) => {
                // §3.3: silently swallow — "No error is returned to
                // the client". Genuine storage errors lose the
                // default change but do not fail the /set.
            }
        }
    }
    false
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

    /// Oracle: RFC 8620 §5.3: for `invalidProperties` "the SetError object
    /// SHOULD also have a property called `properties` of type `String[]`
    /// that lists *all* the properties that were invalid". Each create below
    /// has exactly one hand-picked bad property (`calendarAddress` is a required String, draft-ietf-jmap-calendars-34
    /// §3).
    #[tokio::test]
    async fn set_create_typed_parse_failure_names_property() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "missingAddress": { "name": "A" },
                "numericAddress": { "calendarAddress": 5 },
                "numericName": { "name": 5, "calendarAddress": "mailto:a@example.com" }
            }
        });
        let HandlerOutput { primary: resp, .. } =
            handle_participant_identity_set(&backend, &(), args)
                .await
                .expect("must not return top-level error");
        assert!(resp["created"].is_null(), "nothing may be created: {resp}");
        for (cid, prop) in [
            ("missingAddress", "calendarAddress"),
            ("numericAddress", "calendarAddress"),
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
        let HandlerOutput { primary: resp, .. } =
            handle_participant_identity_set(&backend, &(), args)
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
