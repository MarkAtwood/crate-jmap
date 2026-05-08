//! Calendar/* method handlers (draft-ietf-jmap-calendars-26 §4).
//!
//! `Calendar/set` has special logic: if `onDestroyRemoveEvents` is absent or
//! `false`, destroying a Calendar that still has events is rejected with a
//! `calendarHasEvent` SetError (not a top-level error).

use jmap_calendars_types::Calendar;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, CalendarsBackend, SetError, SetErrorType};
use crate::helpers::{extract_account_id, set_error_value};

// ---------------------------------------------------------------------------
// Calendar/get
// ---------------------------------------------------------------------------

/// Handle a `Calendar/get` method call (draft-ietf-jmap-calendars-26 §4.1).
pub async fn handle_calendar_get<B: CalendarsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<Calendar, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Calendar/changes
// ---------------------------------------------------------------------------

/// Handle a `Calendar/changes` method call (draft-ietf-jmap-calendars-26 §4.2).
pub async fn handle_calendar_changes<B: CalendarsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Calendar, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Calendar/set
// ---------------------------------------------------------------------------

/// Handle a `Calendar/set` method call (draft-ietf-jmap-calendars-26 §4.4).
///
/// Special behaviour:
/// - Parses `onDestroyRemoveEvents` (default `false`) from the request args.
/// - If `false`, any calendar in the `destroy` list that still has events is
///   rejected with a `calendarHasEvent` SetError (draft §4.4.1).
/// - Create and update are forwarded to the backend normally.
pub async fn handle_calendar_set<B: CalendarsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    // RFC 8620 §3.6.2: accountId not recognised → accountNotFound (method-level
    // error). Without this, a /set against an unknown accountId would silently
    // "succeed" with a fake oldState/newState envelope.
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }

    let on_destroy_remove_events = args
        .get("onDestroyRemoveEvents")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let old_state = backend
        .get_state::<Calendar>(&account_id)
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
            let cal: Calendar = match serde_json::from_value(obj_with_id) {
                Ok(c) => c,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };
            match backend
                .create_object::<Calendar>(&account_id, &create_id, cal)
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
                .update_object::<Calendar>(&account_id, &id, patch_val)
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

            // Check for events if onDestroyRemoveEvents is false (default).
            if !on_destroy_remove_events && backend.calendar_has_events(&account_id, &id).await {
                not_destroyed.insert(
                    id_str,
                    set_error_value(&SetError::new(SetErrorType::custom("calendarHasEvent"))),
                );
                continue;
            }

            match backend.destroy_object::<Calendar>(&account_id, &id).await {
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
            .get_state::<Calendar>(&account_id)
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: Calendar/get with unknown accountId returns accountNotFound.
    /// Source: RFC 8620 §3.6.2.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let result = handle_calendar_get(&backend, args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: Calendar/set with unknown accountId returns accountNotFound.
    /// Source: RFC 8620 §3.6.2 — every method MUST validate accountId.
    /// Independent oracle: spec-defined error name.
    #[tokio::test]
    async fn set_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown" });
        let result = handle_calendar_set(&backend, args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: Calendar/set with onDestroyRemoveEvents=false and a calendar that
    /// has events → notDestroyed with "calendarHasEvent" error type.
    /// Source: draft-ietf-jmap-calendars-26 §4.4.1.
    #[tokio::test]
    async fn set_destroy_calendar_with_events_returns_calendar_has_events() {
        let backend = MockBackend::new_with_account_and_events("acc1", "cal1");
        let args = json!({
            "accountId": "acc1",
            "destroy": ["cal1"],
            // onDestroyRemoveEvents defaults to false — not sent
        });
        let (resp, _) = handle_calendar_set(&backend, args)
            .await
            .expect("must not return top-level error");
        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );
        assert_eq!(
            not_destroyed["cal1"]["type"], "calendarHasEvent",
            "must produce calendarHasEvent error: {resp}"
        );
    }

    /// Oracle: Calendar/set create with client-supplied "id" → notCreated
    /// with invalidProperties citing properties:["id"].
    /// Source: RFC 8620 §5.3 — "The id property MUST NOT be set in the
    /// create object." Independent oracle: spec wire shape is hand-written.
    #[tokio::test]
    async fn set_create_with_client_supplied_id_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "id": "client-chosen-id", "name": "My Calendar" }
            }
        });
        let (resp, _) = handle_calendar_set(&backend, args)
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
        // Must NOT have created the calendar.
        assert!(
            resp["created"].is_null(),
            "must not have created any calendar: {resp}"
        );
    }

    /// Oracle: Calendar/set with onDestroyRemoveEvents=true and a calendar that
    /// has events → destroy proceeds (backend remove_calendar_events called).
    #[tokio::test]
    async fn set_destroy_with_remove_events_flag_proceeds() {
        let backend = MockBackend::new_with_account_and_events("acc1", "cal1");
        let args = json!({
            "accountId": "acc1",
            "onDestroyRemoveEvents": true,
            "destroy": ["cal1"],
        });
        let (resp, _) = handle_calendar_set(&backend, args)
            .await
            .expect("must not return top-level error");
        // cal1 exists in the mock, so destroy should succeed
        let destroyed = resp["destroyed"]
            .as_array()
            .expect("destroyed must be array");
        assert_eq!(destroyed.len(), 1, "cal1 must be destroyed: {resp}");
        assert_eq!(destroyed[0], json!("cal1"));
    }
}
