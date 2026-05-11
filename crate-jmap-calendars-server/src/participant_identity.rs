//! ParticipantIdentity/* method handlers (draft-ietf-jmap-calendars-26 §3).

use jmap_calendars_types::ParticipantIdentity;
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, CalendarsBackend};
use crate::helpers::{
    apply_default_change_to_response, extract_account_id, finalize_set_response,
    resolve_on_success_set_is_default, set_error_value, SetAccumulators,
};

// ---------------------------------------------------------------------------
// ParticipantIdentity/get
// ---------------------------------------------------------------------------

/// Handle a `ParticipantIdentity/get` method call
/// (draft-ietf-jmap-calendars-26 §3.1).
pub async fn handle_participant_identity_get<B: CalendarsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<ParticipantIdentity, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// ParticipantIdentity/changes
// ---------------------------------------------------------------------------

/// Handle a `ParticipantIdentity/changes` method call
/// (draft-ietf-jmap-calendars-26 §3.2).
pub async fn handle_participant_identity_changes<B: CalendarsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<ParticipantIdentity, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// ParticipantIdentity/set
// ---------------------------------------------------------------------------

/// Handle a `ParticipantIdentity/set` method call
/// (draft-ietf-jmap-calendars-26 §3.3).
pub async fn handle_participant_identity_set<B: CalendarsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    // RFC 8620 §3.6.2: accountId not recognised → accountNotFound.
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }

    // §3.3: onSuccessSetIsDefault — Id|null. Captured here so we can resolve
    // a possible "#createId" reference against the post-create state. The
    // raw value is kept until after all CRUD ops succeed.
    let on_success_set_is_default = args.remove("onSuccessSetIsDefault");

    let old_state = backend
        .get_state::<ParticipantIdentity>(&account_id)
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

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj_val) in create_map {
            // RFC 8620 §5.3: "The id property MUST NOT be set in the create
            // object" — id is server-assigned. Any present "id" key (even
            // null) is rejected with invalidProperties:["id"].
            if obj_val.get("id").is_some() {
                not_created.insert(
                    create_id,
                    json!({"type": "invalidProperties", "properties": ["id"]}),
                );
                continue;
            }
            let obj_with_id = match obj_val {
                Value::Object(mut m) => {
                    m.entry("id")
                        .or_insert_with(|| Value::String("placeholder".to_owned()));
                    Value::Object(m)
                }
                other => other,
            };
            let pi: ParticipantIdentity = match serde_json::from_value(obj_with_id) {
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
                .create_object::<ParticipantIdentity>(&account_id, &create_id, pi)
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
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(create_id, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
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

    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());
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
            match backend
                .update_object::<ParticipantIdentity>(&account_id, &id, patch)
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
                Err(BackendSetError::SetError(set_err)) => {
                    not_updated.insert(id_str, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
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

    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        if let Some(bad) = destroy_arr.iter().find(|v| !v.is_string()) {
            return Err(JmapError::invalid_arguments(format!(
                "destroy: every element must be a string Id; got {bad}"
            )));
        }
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s.to_owned(),
                None => continue, // unreachable: validated above
            };
            let id = Id::from(id_str.as_str());
            match backend
                .destroy_object::<ParticipantIdentity>(&account_id, &id)
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
                    not_destroyed.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
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

    // §3.3: onSuccessSetIsDefault. Apply only if every CRUD attempt
    // succeeded — if any not_* map has entries, the spec's "all creates,
    // updates and destroys (if any) succeed without error" guard fails
    // and the requested default change is skipped silently.
    let all_succeeded =
        not_created.is_empty() && not_updated.is_empty() && not_destroyed.is_empty();
    if all_succeeded {
        if let Some(raw) = on_success_set_is_default.as_ref() {
            if let Some(target) = resolve_on_success_set_is_default(raw, &created) {
                match backend
                    .set_default_participant_identity(&account_id, &target)
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
        }
    }

    finalize_set_response::<B, ParticipantIdentity>(
        backend,
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
        let result = handle_participant_identity_get(&backend, args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: ParticipantIdentity/set with unknown accountId returns accountNotFound.
    /// Source: RFC 8620 §3.6.2.
    #[tokio::test]
    async fn set_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown" });
        let result = handle_participant_identity_set(&backend, args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
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
        let (resp, _) = handle_participant_identity_set(&backend, args)
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
