//! Calendar/* method handlers (draft-ietf-jmap-calendars-26 §4).
//!
//! `Calendar/set` has special logic: if `onDestroyRemoveEvents` is absent or
//! `false`, destroying a Calendar that still has events is rejected with a
//! `calendarHasEvent` SetError (not a top-level error).

use jmap_calendars_types::{Calendar, CalendarEvent, CalendarEventFilterCondition};
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

            // PLAN.md §5 / draft-ietf-jmap-calendars-26 §4.4: when
            // onDestroyRemoveEvents is true, the handler is responsible for
            // cleaning up events in this calendar before destroying the
            // calendar itself. The backend is type-agnostic and has no
            // onDestroyRemoveEvents concept.
            //
            // Steps:
            //   1. Query CalendarEvents with calendarIds containing this id.
            //   2. Fetch full event objects to inspect calendar_ids count.
            //   3. For each event in only this calendar: destroy.
            //      For each event in multiple calendars: patch out this id.
            //   4. Destroy the Calendar.
            //
            // Any sub-step failure aborts with a serverFail SetError on the
            // calendar destroy entry — a partial cleanup must not leave the
            // calendar destroyed with dangling events.
            if on_destroy_remove_events {
                match cleanup_calendar_events(backend, &account_id, &id).await {
                    Ok(()) => {
                        // proceed to destroy the calendar below
                    }
                    Err(e) => {
                        not_destroyed.insert(
                            id_str,
                            json!({"type": "serverFail", "description": e}),
                        );
                        continue;
                    }
                }
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

/// Clean up CalendarEvents in a Calendar before destroying the Calendar.
///
/// Implements PLAN.md §5 (draft-ietf-jmap-calendars-26 §4.4 onDestroyRemoveEvents):
///   - Events whose `calendarIds` is just this calendar are destroyed.
///   - Events with multiple calendarIds are patched to remove this calendar id.
///
/// Returns `Err(message)` on the first sub-step failure so the calling handler
/// can surface a `serverFail` SetError on the calendar destroy entry. Partial
/// cleanup is not acceptable: failing fast keeps the data store consistent.
async fn cleanup_calendar_events<B: CalendarsBackend>(
    backend: &B,
    account_id: &Id,
    calendar_id: &Id,
) -> Result<(), String> {
    // Step 1: query event ids whose calendarIds include this calendar.
    // CalendarEventFilterCondition is #[non_exhaustive], so construct via
    // Default + field assignment rather than a struct literal.
    let mut filter = CalendarEventFilterCondition::default();
    filter.in_calendar = Some(calendar_id.clone());
    let event_ids: Vec<Id> = backend
        .query_objects::<CalendarEvent>(account_id, Some(&filter), None, None, 0)
        .await
        .map_err(|e| e.to_string())?
        .ids;

    if event_ids.is_empty() {
        return Ok(());
    }

    // Step 2: fetch full event objects to inspect calendar_ids count.
    let (events, _not_found): (Vec<CalendarEvent>, _) = backend
        .get_objects::<CalendarEvent>(account_id, Some(&event_ids), None)
        .await
        .map_err(|e| e.to_string())?;

    // Step 3: for each event, destroy if single-calendar, else patch out this id.
    for event in events {
        let n_calendars = event
            .calendar_ids
            .as_ref()
            .map(|m| m.len())
            .unwrap_or(0);

        // The CalendarEvent's `id` is required for /set semantics — it should
        // always be Some on a real backend. If it's missing here we cannot
        // address the event, so abort.
        let event_id = match event.id.as_ref() {
            Some(id) => id.clone(),
            None => return Err("event missing id field during cleanup".to_owned()),
        };

        if n_calendars > 1 {
            // Multi-calendar: PatchObject path "calendarIds/<id>" = null
            // removes that key from the calendarIds map (RFC 8620 §5.3).
            let patch_key = format!("calendarIds/{}", calendar_id.as_ref());
            let mut patch_obj = serde_json::Map::new();
            patch_obj.insert(patch_key, Value::Null);
            backend
                .update_object::<CalendarEvent>(
                    account_id,
                    &event_id,
                    Value::Object(patch_obj),
                )
                .await
                .map_err(|e| match e {
                    BackendSetError::SetError(set_err) => {
                        format!("update_object failed: {}", set_err.error_type)
                    }
                    BackendSetError::Other(err) => err.to_string(),
                })?;
        } else {
            // Single-calendar (this one): destroy the event outright.
            backend
                .destroy_object::<CalendarEvent>(account_id, &event_id)
                .await
                .map_err(|e| match e {
                    BackendSetError::SetError(set_err) => {
                        format!("destroy_object failed: {}", set_err.error_type)
                    }
                    BackendSetError::Other(err) => err.to_string(),
                })?;
        }
    }

    Ok(())
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

    /// Oracle: PLAN.md §5 / draft-ietf-jmap-calendars-26 §4.4 —
    /// onDestroyRemoveEvents:true must destroy events whose only calendar is
    /// the one being deleted.
    ///
    /// Independent oracle: stored event count is observable via get_objects;
    /// the assertion checks the post-condition (event gone) without ever
    /// re-running the cleanup helper.
    #[tokio::test]
    async fn set_destroy_with_remove_events_destroys_single_calendar_event() {
        let mut backend = MockBackend::new_with_account_and_events("acc1", "cal1");
        // Seed a CalendarEvent that lives only in cal1.
        backend.add_object(
            "acc1",
            "CalendarEvent",
            "ev-only-cal1",
            json!({
                "id": "ev-only-cal1",
                "calendarIds": { "cal1": true },
                "title": "Only in cal1"
            }),
        );

        let args = json!({
            "accountId": "acc1",
            "onDestroyRemoveEvents": true,
            "destroy": ["cal1"],
        });
        let (resp, _) = handle_calendar_set(&backend, args)
            .await
            .expect("must not return top-level error");

        // Calendar destroyed
        let destroyed = resp["destroyed"]
            .as_array()
            .expect("destroyed must be array");
        assert_eq!(destroyed[0], json!("cal1"), "cal1 must be destroyed: {resp}");

        // Event also destroyed — query MockBackend's CalendarEvent store directly.
        use jmap_server::JmapBackend;
        let (events, not_found) = backend
            .get_objects::<jmap_calendars_types::CalendarEvent>(
                &Id::from("acc1"),
                Some(&[Id::from("ev-only-cal1")]),
                None,
            )
            .await
            .expect("get_objects succeeds");
        assert!(events.is_empty(), "event must have been destroyed");
        assert_eq!(
            not_found.len(),
            1,
            "ev-only-cal1 must appear in not_found: {not_found:?}"
        );
    }

    /// Oracle: PLAN.md §5 / draft-ietf-jmap-calendars-26 §4.4 —
    /// onDestroyRemoveEvents:true must NOT destroy events that live in
    /// multiple calendars; instead, it must remove only the destroyed
    /// calendar's id from each such event's `calendarIds` map.
    ///
    /// Independent oracle: stored event after the call must still exist with
    /// `calendarIds` containing the *other* calendar id, but not the
    /// destroyed one.
    #[tokio::test]
    async fn set_destroy_with_remove_events_unsets_multi_calendar_event() {
        let mut backend = MockBackend::new_with_account_and_events("acc1", "cal1");
        // Seed a CalendarEvent that lives in BOTH cal1 (destroyed) and cal2.
        backend.add_object(
            "acc1",
            "CalendarEvent",
            "ev-multi",
            json!({
                "id": "ev-multi",
                "calendarIds": { "cal1": true, "cal2": true },
                "title": "Lives in both"
            }),
        );

        let args = json!({
            "accountId": "acc1",
            "onDestroyRemoveEvents": true,
            "destroy": ["cal1"],
        });
        let (resp, _) = handle_calendar_set(&backend, args)
            .await
            .expect("must not return top-level error");

        // Calendar destroyed.
        let destroyed = resp["destroyed"]
            .as_array()
            .expect("destroyed must be array");
        assert_eq!(destroyed[0], json!("cal1"), "cal1 must be destroyed: {resp}");

        // Event still exists, but cal1 must be gone from calendarIds and cal2
        // must remain.
        use jmap_server::JmapBackend;
        let (events, _) = backend
            .get_objects::<jmap_calendars_types::CalendarEvent>(
                &Id::from("acc1"),
                Some(&[Id::from("ev-multi")]),
                None,
            )
            .await
            .expect("get_objects succeeds");
        assert_eq!(events.len(), 1, "ev-multi must survive destroy");
        let event = &events[0];
        let calendar_ids = event
            .calendar_ids
            .as_ref()
            .expect("calendarIds must remain");
        assert!(
            !calendar_ids.contains_key(&Id::from("cal1")),
            "cal1 must be removed from calendarIds: {calendar_ids:?}"
        );
        assert!(
            calendar_ids.contains_key(&Id::from("cal2")),
            "cal2 must still be in calendarIds: {calendar_ids:?}"
        );
    }
}
