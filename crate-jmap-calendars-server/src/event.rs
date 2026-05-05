//! CalendarEvent/* method handlers (draft-ietf-jmap-calendars-26 §5).

use jmap_calendars_types::CalendarEvent;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, CalendarsBackend};
use crate::helpers::{extract_account_id, set_error_value};

// ---------------------------------------------------------------------------
// CalendarEvent/get
// ---------------------------------------------------------------------------

/// Handle a `CalendarEvent/get` method call (draft-ietf-jmap-calendars-26 §5.4).
///
/// Extra args (`expandRecurrences`, `reducedParticipants`, `fetchCalendars`)
/// are passed through to the generic handler which forwards them to the
/// backend via the args `Value`.
///
/// If `utcStart` or `utcEnd` appear in the requested `properties` (or all
/// properties are requested), `compute_utc_times` is called per event and
/// the results are injected into the response list
/// (draft-ietf-jmap-calendars-26 §5.2).
pub async fn handle_calendar_event_get<B: CalendarsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    // Determine whether utcStart or utcEnd are requested.
    let want_utc = match args.get("properties") {
        None | Some(Value::Null) => true, // all properties requested
        Some(Value::Array(props)) => props.iter().any(|p| {
            p.as_str()
                .map(|s| s == "utcStart" || s == "utcEnd")
                .unwrap_or(false)
        }),
        _ => false,
    };
    let account_id = extract_account_id(&args)?;

    let (mut response, tail) =
        jmap_server::handlers::handle_get::<CalendarEvent, B>(backend, args).await?;

    if want_utc {
        if let Some(Value::Array(list)) = response.get_mut("list") {
            for item in list.iter_mut() {
                if let Ok(event) = serde_json::from_value::<CalendarEvent>(item.clone()) {
                    let (utc_start, utc_end) =
                        backend.compute_utc_times(&account_id, &event, None).await;
                    if let Some(s) = utc_start {
                        item["utcStart"] = Value::String(s);
                    }
                    if let Some(e) = utc_end {
                        item["utcEnd"] = Value::String(e);
                    }
                }
            }
        }
    }

    Ok((response, tail))
}

// ---------------------------------------------------------------------------
// CalendarEvent/changes
// ---------------------------------------------------------------------------

/// Handle a `CalendarEvent/changes` method call (draft-ietf-jmap-calendars-26 §5.5).
pub async fn handle_calendar_event_changes<B: CalendarsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<CalendarEvent, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// CalendarEvent/set
// ---------------------------------------------------------------------------

/// Handle a `CalendarEvent/set` method call (draft-ietf-jmap-calendars-26 §5.6).
pub async fn handle_calendar_event_set<B: CalendarsBackend>(
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
        .get_state::<CalendarEvent>(&account_id)
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
            // §5.9: client cannot set both utcStart and start simultaneously,
            // or both utcEnd and duration.
            let obj_json = &obj_val;
            let has_utc_start = obj_json
                .get("utcStart")
                .map(|v| !v.is_null())
                .unwrap_or(false);
            let has_start = obj_json.get("start").map(|v| !v.is_null()).unwrap_or(false);
            let has_utc_end = obj_json
                .get("utcEnd")
                .map(|v| !v.is_null())
                .unwrap_or(false);
            let has_duration = obj_json
                .get("duration")
                .map(|v| !v.is_null())
                .unwrap_or(false);

            if has_utc_start && has_start {
                not_created.insert(
                    create_id,
                    json!({ "type": "invalidProperties", "properties": ["utcStart", "start"] }),
                );
                continue;
            }
            if has_utc_end && has_duration {
                not_created.insert(
                    create_id,
                    json!({ "type": "invalidProperties", "properties": ["utcEnd", "duration"] }),
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
            let event: CalendarEvent = match serde_json::from_value(obj_with_id) {
                Ok(e) => e,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };
            match backend
                .create_object::<CalendarEvent>(&account_id, &create_id, event)
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

    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            // §5.9: patch cannot contain both utcStart and start, or both
            // utcEnd and duration.
            let has_utc_start = patch_val
                .get("utcStart")
                .map(|v| !v.is_null())
                .unwrap_or(false);
            let has_start = patch_val
                .get("start")
                .map(|v| !v.is_null())
                .unwrap_or(false);
            let has_utc_end = patch_val
                .get("utcEnd")
                .map(|v| !v.is_null())
                .unwrap_or(false);
            let has_duration = patch_val
                .get("duration")
                .map(|v| !v.is_null())
                .unwrap_or(false);

            if has_utc_start && has_start {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": ["utcStart", "start"] }),
                );
                continue;
            }
            if has_utc_end && has_duration {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": ["utcEnd", "duration"] }),
                );
                continue;
            }

            let id = Id::from(id_str.as_str());
            match backend
                .update_object::<CalendarEvent>(&account_id, &id, patch_val)
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

    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s.to_owned(),
                None => continue,
            };
            let id = Id::from(id_str.as_str());
            match backend
                .destroy_object::<CalendarEvent>(&account_id, &id)
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
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<CalendarEvent>(&account_id)
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
// CalendarEvent/copy
// ---------------------------------------------------------------------------

/// Handle a `CalendarEvent/copy` method call (draft-ietf-jmap-calendars-26 §5.7).
///
/// RFC 8620 §5.4: fetches each source event from `fromAccountId`, merges
/// client-supplied property overrides, then creates the result in `accountId`.
/// Supports `ifFromInState`, `ifInState`, and `onSuccessDestroyOriginal`.
pub async fn handle_calendar_event_copy<B: CalendarsBackend>(
    backend: &B,
    args: Value,
    call_id: &str,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    // fromAccountId is required (RFC 8620 §5.4).
    let from_account_id: Id = match args.get("fromAccountId").and_then(|v| v.as_str()) {
        Some(s) => Id::from(s),
        None => return Err(JmapError::invalid_arguments("fromAccountId is required")),
    };

    if !backend
        .account_exists(&from_account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::from_account_not_found());
    }

    // ifFromInState: verify source account state matches (RFC 8620 §5.4).
    if let Some(if_from_in_state) = args.get("ifFromInState").and_then(|v| v.as_str()) {
        let from_state = backend
            .get_state::<CalendarEvent>(&from_account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;
        if if_from_in_state != from_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let old_state = backend
        .get_state::<CalendarEvent>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // ifInState: verify destination account state matches (RFC 8620 §5.4).
    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let on_success_destroy_original: bool = args
        .get("onSuccessDestroyOriginal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    // Track (create_id, source_id) pairs for successful copies.
    let mut copied_pairs: Vec<(String, Id)> = Vec::new();

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, client_val) in create_map {
            // RFC 8620 §5.4: "id" in the create entry is the source object id.
            let source_id: Id = match client_val.get("id").and_then(|v| v.as_str()) {
                Some(s) => Id::from(s),
                None => {
                    not_created.insert(
                        create_id,
                        json!({"type": "invalidProperties", "properties": ["id"]}),
                    );
                    continue;
                }
            };

            // Fetch source event from fromAccountId.
            let source_events: Vec<CalendarEvent> = match backend
                .get_objects::<CalendarEvent>(
                    &from_account_id,
                    Some(std::slice::from_ref(&source_id)),
                    None,
                )
                .await
            {
                Ok((objs, not_found)) => {
                    if !not_found.is_empty() || objs.is_empty() {
                        not_created.insert(create_id, json!({"type": "notFound"}));
                        continue;
                    }
                    objs
                }
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({"type": "serverFail", "description": e.to_string()}),
                    );
                    continue;
                }
            };

            // Merge: serialize source to map, overlay client-supplied properties,
            // then strip "id" so the backend assigns a fresh server id.
            let mut merged: serde_json::Map<String, Value> = match serde_json::to_value(
                &source_events[0],
            ) {
                Ok(Value::Object(m)) => m,
                _ => {
                    not_created.insert(
                            create_id,
                            json!({"type": "serverFail", "description": "failed to serialize source event"}),
                        );
                    continue;
                }
            };
            if let Value::Object(client_props) = client_val {
                for (k, v) in client_props {
                    if k != "id" {
                        merged.insert(k, v);
                    }
                }
            }
            merged.remove("id");

            let event: CalendarEvent = match serde_json::from_value(Value::Object(merged)) {
                Ok(e) => e,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            match backend
                .create_object::<CalendarEvent>(&account_id, &create_id, event)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    let v = serde_json::to_value(&created_obj).unwrap_or_else(
                        |e| json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                    created.insert(create_id.clone(), v);
                    copied_pairs.push((create_id, source_id));
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

    let new_state = if created.is_empty() {
        old_state.clone()
    } else {
        backend
            .get_state::<CalendarEvent>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    };

    let resp = json!({
        "fromAccountId": from_account_id.as_ref(),
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created":    if created.is_empty()    { Value::Null } else { Value::Object(created) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
    });

    // onSuccessDestroyOriginal: generate implicit CalendarEvent/set response
    // against fromAccountId (RFC 8620 §6.3).
    let mut extra: Vec<Invocation> = Vec::new();
    if on_success_destroy_original && !copied_pairs.is_empty() {
        let destroy_old_state = backend
            .get_state::<CalendarEvent>(&from_account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        let mut destroyed_ids: Vec<Value> = Vec::new();
        let mut not_destroyed = serde_json::Map::new();

        for (_, source_id) in &copied_pairs {
            match backend
                .destroy_object::<CalendarEvent>(&from_account_id, source_id)
                .await
            {
                Ok(()) => {
                    destroyed_ids.push(Value::String(source_id.as_ref().to_owned()));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(source_id.as_ref().to_owned(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(
                        source_id.as_ref().to_owned(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }

        let destroy_new_state = backend
            .get_state::<CalendarEvent>(&from_account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        let set_resp = json!({
            "accountId": from_account_id.as_ref(),
            "oldState": destroy_old_state.as_ref(),
            "newState": destroy_new_state.as_ref(),
            "created": Value::Null,
            "updated": Value::Null,
            "destroyed": if destroyed_ids.is_empty() { Value::Null } else { Value::Array(destroyed_ids) },
            "notCreated": Value::Null,
            "notUpdated": Value::Null,
            "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
        });
        extra.push(("CalendarEvent/set".to_owned(), set_resp, call_id.to_owned()));
    }

    Ok((resp, extra))
}

// ---------------------------------------------------------------------------
// CalendarEvent/query
// ---------------------------------------------------------------------------

/// Handle a `CalendarEvent/query` method call (draft-ietf-jmap-calendars-26 §5.11).
pub async fn handle_calendar_event_query<B: CalendarsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<CalendarEvent, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// CalendarEvent/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `CalendarEvent/queryChanges` method call
/// (draft-ietf-jmap-calendars-26 §5.12).
pub async fn handle_calendar_event_query_changes<B: CalendarsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<CalendarEvent, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: CalendarEvent/get with unknown accountId returns accountNotFound.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let result = handle_calendar_event_get(&backend, args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: RFC 8620 §5.4 — fromAccountId that does not exist must return
    /// a top-level `fromAccountNotFound` error.
    #[tokio::test]
    async fn copy_from_account_not_found() {
        let backend = MockBackend::new_with_account("dst");
        let args = json!({
            "accountId": "dst",
            "fromAccountId": "no-such-account",
            "create": {}
        });
        let result = handle_calendar_event_copy(&backend, args, "c0").await;
        let err = result.expect_err("must return error for unknown fromAccountId");
        assert_eq!(
            err.error_type.as_str(),
            "fromAccountNotFound",
            "wrong error type: {err:?}"
        );
    }

    /// Oracle: RFC 8620 §5.4 — when the source id is absent from the create
    /// entry, `notCreated` must contain `invalidProperties` with `properties: ["id"]`.
    #[tokio::test]
    async fn copy_missing_source_id_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc");
        let args = json!({
            "accountId": "acc",
            "fromAccountId": "acc",
            "create": {
                "c1": { "calendarIds": { "cal1": true } }
            }
        });
        let (resp, extra) = handle_calendar_event_copy(&backend, args, "c0")
            .await
            .expect("must not return top-level error");
        assert!(extra.is_empty());
        assert_eq!(
            resp["notCreated"]["c1"]["type"], "invalidProperties",
            "wrong notCreated type: {resp}"
        );
        assert_eq!(
            resp["notCreated"]["c1"]["properties"][0], "id",
            "must cite 'id' in properties: {resp}"
        );
    }

    /// Oracle: RFC 8620 §5.4 — when the source id does not exist in
    /// `fromAccountId`, `notCreated` must contain `{"type":"notFound"}`.
    #[tokio::test]
    async fn copy_source_not_found() {
        let backend = MockBackend::new_with_account("acc");
        let args = json!({
            "accountId": "acc",
            "fromAccountId": "acc",
            "create": {
                "c1": { "id": "no-such-event" }
            }
        });
        let (resp, extra) = handle_calendar_event_copy(&backend, args, "c0")
            .await
            .expect("must not return top-level error");
        assert!(extra.is_empty());
        assert_eq!(
            resp["notCreated"]["c1"]["type"], "notFound",
            "wrong notCreated type: {resp}"
        );
    }

    /// Oracle: RFC 8620 §5.4 — a successful copy merges client-supplied
    /// property overrides onto the source event. `created` contains the new
    /// object; `notCreated` is null.
    #[tokio::test]
    async fn copy_successful_with_overrides() {
        let mut backend = MockBackend::new_with_account("src");
        backend.add_object(
            "src",
            "CalendarEvent",
            "ev1",
            json!({
                "id": "ev1",
                "title": "Original Title",
                "calendarIds": { "cal1": true }
            }),
        );
        // Destination account also exists (same backend, different account key).
        backend.add_object("dst", "CalendarEvent", "_placeholder_", json!({}));
        // Ensure dst account exists in the mock.
        let mut backend = MockBackend::new_with_account("src");
        backend.add_object(
            "src",
            "CalendarEvent",
            "ev1",
            json!({
                "id": "ev1",
                "title": "Original Title",
                "calendarIds": { "cal1": true }
            }),
        );
        // For dst we need a registered account — re-use src account as both src and dst.
        let args = json!({
            "accountId": "src",
            "fromAccountId": "src",
            "create": {
                "c1": {
                    "id": "ev1",
                    "title": "Overridden Title"
                }
            }
        });
        let (resp, extra) = handle_calendar_event_copy(&backend, args, "c0")
            .await
            .expect("must not return top-level error");
        assert!(extra.is_empty(), "no extra invocations expected");
        assert_eq!(
            resp["notCreated"],
            serde_json::Value::Null,
            "notCreated must be null: {resp}"
        );
        assert!(
            resp["created"]["c1"].is_object(),
            "created must contain c1: {resp}"
        );
    }

    /// Oracle: §5.9 — creating with both `utcStart` and `start` set must
    /// produce `notCreated` with `invalidProperties` citing both fields.
    #[tokio::test]
    async fn set_create_utc_start_and_start_conflict_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc");
        let args = json!({
            "accountId": "acc",
            "create": {
                "c1": {
                    "calendarIds": { "cal1": true },
                    "start": "2024-06-01T10:00:00",
                    "utcStart": "2024-06-01T08:00:00Z"
                }
            }
        });
        let (resp, extra) = handle_calendar_event_set(&backend, args)
            .await
            .expect("must not return top-level error");
        assert!(extra.is_empty());
        assert_eq!(
            resp["notCreated"]["c1"]["type"], "invalidProperties",
            "expected invalidProperties: {resp}"
        );
        let props = resp["notCreated"]["c1"]["properties"].as_array().unwrap();
        let prop_strs: Vec<&str> = props.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            prop_strs.contains(&"utcStart") && prop_strs.contains(&"start"),
            "must cite utcStart and start: {resp}"
        );
    }

    /// Oracle: §5.9 — creating with both `utcEnd` and `duration` set must
    /// produce `notCreated` with `invalidProperties` citing both fields.
    #[tokio::test]
    async fn set_create_utc_end_and_duration_conflict_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc");
        let args = json!({
            "accountId": "acc",
            "create": {
                "c1": {
                    "calendarIds": { "cal1": true },
                    "duration": "PT1H",
                    "utcEnd": "2024-06-01T09:00:00Z"
                }
            }
        });
        let (resp, extra) = handle_calendar_event_set(&backend, args)
            .await
            .expect("must not return top-level error");
        assert!(extra.is_empty());
        assert_eq!(
            resp["notCreated"]["c1"]["type"], "invalidProperties",
            "expected invalidProperties: {resp}"
        );
        let props = resp["notCreated"]["c1"]["properties"].as_array().unwrap();
        let prop_strs: Vec<&str> = props.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            prop_strs.contains(&"utcEnd") && prop_strs.contains(&"duration"),
            "must cite utcEnd and duration: {resp}"
        );
    }

    /// Oracle: get without requesting `utcStart`/`utcEnd` must not error.
    #[tokio::test]
    async fn get_without_utc_properties_does_not_error() {
        let backend = MockBackend::new_with_account("acc");
        let args = json!({
            "accountId": "acc",
            "ids": null,
            "properties": ["id", "title"]
        });
        let result = handle_calendar_event_get(&backend, args).await;
        assert!(result.is_ok(), "must not error: {result:?}");
    }

    /// Oracle: get with `utcStart` in properties succeeds; default impl
    /// returns `None` so the field is absent from each item — no error.
    #[tokio::test]
    async fn get_with_utc_start_requested_does_not_error() {
        let backend = MockBackend::new_with_account("acc");
        let args = json!({
            "accountId": "acc",
            "ids": null,
            "properties": ["id", "utcStart"]
        });
        let result = handle_calendar_event_get(&backend, args).await;
        assert!(result.is_ok(), "must not error: {result:?}");
        let (resp, _) = result.unwrap();
        // list is present; utcStart absent from items (default impl returns None).
        if let Some(list) = resp["list"].as_array() {
            for item in list {
                assert!(
                    item.get("utcStart").is_none(),
                    "default impl must not inject utcStart: {item}"
                );
            }
        }
    }

    /// Oracle: RFC 8620 §6.3 — `onSuccessDestroyOriginal: true` generates an
    /// implicit `CalendarEvent/set` invocation appended to the response.
    #[tokio::test]
    async fn copy_on_success_destroy_original_generates_set_invocation() {
        let mut backend = MockBackend::new_with_account("src");
        backend.add_object(
            "src",
            "CalendarEvent",
            "ev1",
            json!({
                "id": "ev1",
                "title": "Event to move",
                "calendarIds": { "cal1": true }
            }),
        );
        let args = json!({
            "accountId": "src",
            "fromAccountId": "src",
            "onSuccessDestroyOriginal": true,
            "create": {
                "c1": { "id": "ev1" }
            }
        });
        let (resp, extra) = handle_calendar_event_copy(&backend, args, "c0")
            .await
            .expect("must not return top-level error");
        assert!(
            resp["created"]["c1"].is_object(),
            "must have created c1: {resp}"
        );
        assert_eq!(
            extra.len(),
            1,
            "must produce exactly one extra CalendarEvent/set invocation"
        );
        let (method, set_resp, call_id) = &extra[0];
        assert_eq!(method, "CalendarEvent/set");
        assert_eq!(call_id, "c0");
        assert_eq!(
            set_resp["accountId"], "src",
            "implicit set targets fromAccountId: {set_resp}"
        );
        assert!(
            set_resp["destroyed"].is_array(),
            "destroyed must be an array: {set_resp}"
        );
    }
}
