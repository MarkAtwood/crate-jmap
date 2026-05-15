//! Calendar/* method handlers (draft-ietf-jmap-calendars-26 §4).
//!
//! `Calendar/set` has special logic: if `onDestroyRemoveEvents` is absent or
//! `false`, destroying a Calendar that still has events is rejected with a
//! `calendarHasEvent` SetError (not a top-level error).

use jmap_calendars_types::{Calendar, CalendarEvent, CalendarEventFilterCondition};
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, CalendarsBackend, SetError, SetErrorType};
use crate::helpers::{
    apply_default_change_to_response, extract_account_id, finalize_set_response,
    resolve_on_success_set_is_default, set_error_value, SetAccumulators,
};
use jmap_server::{bool_arg, server_fail_from_backend};

// ---------------------------------------------------------------------------
// Calendar/get
// ---------------------------------------------------------------------------

/// Handle a `Calendar/get` method call (draft-ietf-jmap-calendars-26 §4.1).
pub async fn handle_calendar_get<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<Calendar, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Calendar/changes
// ---------------------------------------------------------------------------

/// Handle a `Calendar/changes` method call (draft-ietf-jmap-calendars-26 §4.2).
pub async fn handle_calendar_changes<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Calendar, B>(backend, caller, args).await
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
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: accountId not recognised → accountNotFound (method-level
    // error). Without this, a /set against an unknown accountId would silently
    // "succeed" with a fake oldState/newState envelope.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let on_destroy_remove_events = bool_arg(&args, "onDestroyRemoveEvents", false);

    // §4.3: onSuccessSetIsDefault — Id|null. Captured here so we can resolve
    // a possible "#createId" reference against the post-create state. The
    // raw value is kept until after all CRUD ops succeed.
    let on_success_set_is_default = args.remove("onSuccessSetIsDefault");

    let old_state = backend
        .get_state::<Calendar>(caller, &account_id)
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
                .create_object::<Calendar>(caller, &account_id, &create_id, cal)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    // Calendar uses #[derive(Serialize)] on plain data; to_value
                    // is infallible. Asserting rather than masking would-be
                    // failures as `serverFail` (per JMAP-r3pg.13).
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

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());
            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object values
            // produce an `invalidPatch` SetError. The newtype's transparent
            // deserialize enforces this at the boundary.
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
                .update_object::<Calendar>(caller, &account_id, &id, patch)
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

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------
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

            // Check for events if onDestroyRemoveEvents is false (default).
            // The three-way result distinguishes 'definitely empty',
            // 'definitely has events', and 'transient backend failure'
            // (bd:JMAP-ic0j.4) — the last must surface as serverFail so
            // the client knows to retry, not as a deterministic
            // calendarHasEvent SetError.
            if !on_destroy_remove_events {
                match backend.calendar_has_events(caller, &account_id, &id).await {
                    Ok(true) => {
                        not_destroyed.insert(
                            id_str,
                            set_error_value(&SetError::new(SetErrorType::custom(
                                "calendarHasEvent",
                            ))),
                        );
                        continue;
                    }
                    Ok(false) => {
                        // proceed to destroy below
                    }
                    Err(e) => {
                        not_destroyed.insert(
                            id_str,
                            json!({
                                "type": "serverFail",
                                "description": e.to_string(),
                            }),
                        );
                        continue;
                    }
                }
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
                match cleanup_calendar_events(backend, caller, &account_id, &id).await {
                    Ok(()) => {
                        // proceed to destroy the calendar below
                    }
                    Err(e) => {
                        not_destroyed
                            .insert(id_str, json!({"type": "serverFail", "description": e}));
                        continue;
                    }
                }
            }

            match backend
                .destroy_object::<Calendar>(caller, &account_id, &id)
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

    // §4.3: onSuccessSetIsDefault. Apply only if every CRUD attempt
    // succeeded — if any not_* map has entries, the spec's "all creates,
    // updates and destroys (if any) succeed without error" guard fails
    // and the requested default change is skipped silently.
    let all_succeeded =
        not_created.is_empty() && not_updated.is_empty() && not_destroyed.is_empty();
    if all_succeeded {
        if let Some(raw) = on_success_set_is_default.as_ref() {
            if let Some(target) = resolve_on_success_set_is_default(raw, &created) {
                match backend
                    .set_default_calendar(caller, &account_id, &target)
                    .await
                {
                    Ok(result) => {
                        if apply_default_change_to_response(&mut created, &mut updated, &result) {
                            mutated = true;
                        }
                    }
                    Err(_e) => {
                        // §4.3: silently swallow — "No error is returned to
                        // the client". Genuine storage errors lose the
                        // default change but do not fail the /set.
                    }
                }
            }
        }
    }

    finalize_set_response::<B, Calendar>(
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
    caller: &B::CallerCtx,
    account_id: &Id,
    calendar_id: &Id,
) -> Result<(), String> {
    // Step 1: query event ids whose calendarIds include this calendar.
    // CalendarEventFilterCondition is #[non_exhaustive], so construct via
    // Default + field assignment rather than a struct literal.
    let mut filter = CalendarEventFilterCondition::default();
    filter.in_calendar = Some(calendar_id.clone());
    let event_ids: Vec<Id> = backend
        .query_objects::<CalendarEvent>(caller, account_id, Some(&filter), None, None, 0)
        .await
        .map_err(|e| e.to_string())?
        .ids;

    if event_ids.is_empty() {
        return Ok(());
    }

    // Step 2: fetch full event objects to inspect calendar_ids count.
    let (events, _not_found): (Vec<CalendarEvent>, _) = backend
        .get_objects::<CalendarEvent>(caller, account_id, Some(&event_ids), None)
        .await
        .map_err(|e| e.to_string())?;

    // Step 3: for each event, destroy if single-calendar, else patch out this id.
    for event in events {
        let n_calendars = event.calendar_ids.as_ref().map(|m| m.len()).unwrap_or(0);

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
                    caller,
                    account_id,
                    &event_id,
                    PatchObject::from_map(patch_obj),
                )
                .await
                .map_err(|e| match e {
                    BackendSetError::SetError(set_err) => {
                        format!("update_object failed: {}", set_err.error_type)
                    }
                    BackendSetError::Other(err) => err.to_string(),
                    _ => "update_object failed: unhandled backend error variant".to_owned(),
                })?;
        } else {
            // Single-calendar (this one): destroy the event outright.
            backend
                .destroy_object::<CalendarEvent>(caller, account_id, &event_id)
                .await
                .map_err(|e| match e {
                    BackendSetError::SetError(set_err) => {
                        format!("destroy_object failed: {}", set_err.error_type)
                    }
                    BackendSetError::Other(err) => err.to_string(),
                    _ => "destroy_object failed: unhandled backend error variant".to_owned(),
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
        let result = handle_calendar_get(&backend, &(), args).await;
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
        let result = handle_calendar_set(&backend, &(), args).await;
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
        let (resp, _) = handle_calendar_set(&backend, &(), args)
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

    /// Regression for bd:JMAP-ic0j.4: when `calendar_has_events` returns
    /// `Err(_)`, the handler must NOT silently treat that as
    /// `Ok(false)` and proceed with the destroy — doing so would
    /// orphan any events that actually existed in the calendar. The
    /// transient backend failure surfaces as `serverFail` on the
    /// destroy entry so the client knows to retry.
    ///
    /// Oracle: the three-way-result rationale documented on
    /// [`CalendarsBackend::calendar_has_events`] (mirrors the canonical
    /// `MailBackend::blob_exists` shape).
    #[tokio::test]
    async fn set_destroy_calendar_has_events_error_returns_server_fail() {
        use jmap_server::{
            BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
            JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError as JsSetError,
            SetErrorType as JsSetErrorType, SetObject,
        };
        use jmap_types::{Id, PatchObject, State};

        #[derive(Debug)]
        struct FaultyError(&'static str);
        impl std::fmt::Display for FaultyError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
        impl std::error::Error for FaultyError {}

        #[derive(Clone)]
        struct FaultyBackend;

        impl JmapBackend for FaultyBackend {
            type Error = FaultyError;
            type CallerCtx = ();

            async fn account_exists(
                &self,
                _caller: &(),
                _account_id: &Id,
            ) -> Result<bool, Self::Error> {
                Ok(true)
            }

            async fn get_objects<O: GetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _ids: Option<&[Id]>,
                _properties: Option<&[String]>,
            ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
                Ok((vec![], vec![]))
            }

            async fn get_state<O: JmapObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
            ) -> Result<State, Self::Error> {
                Ok(State::from("0"))
            }

            async fn get_changes<O: JmapObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _since_state: &State,
                _max_changes: Option<u64>,
            ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
                Ok(ChangesResult::new(
                    vec![],
                    vec![],
                    vec![],
                    false,
                    State::from("0"),
                ))
            }

            async fn query_objects<O: QueryObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _filter: Option<&O::Filter>,
                _sort: Option<&[O::Comparator]>,
                _limit: Option<u64>,
                _position: i64,
            ) -> Result<QueryResult, Self::Error> {
                Ok(QueryResult::new(
                    vec![],
                    0,
                    Some(0),
                    State::from("0"),
                    false,
                ))
            }

            async fn query_changes<O: QueryObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                since_query_state: &State,
                _filter: Option<&O::Filter>,
                _sort: Option<&[O::Comparator]>,
                _max_changes: Option<u64>,
                _up_to_id: Option<&Id>,
                _collapse_threads: bool,
            ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
                Ok(QueryChangesResult::new(
                    since_query_state.clone(),
                    State::from("0"),
                    Some(0),
                    vec![],
                    vec![],
                ))
            }
        }

        impl CalendarsBackend for FaultyBackend {
            async fn create_object<O: SetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _create_id: &str,
                obj: O,
            ) -> Result<(Id, O), BackendSetError<Self::Error>> {
                Ok((Id::from("mock-id"), obj))
            }

            async fn update_object<O: SetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _id: &Id,
                _patch: O::Patch,
            ) -> Result<Option<O>, BackendSetError<Self::Error>> {
                Err(BackendSetError::SetError(JsSetError::new(
                    JsSetErrorType::NotFound,
                )))
            }

            async fn update_per_user_properties(
                &self,
                _caller: &(),
                _account_id: &Id,
                _id: &Id,
                _patch: PatchObject,
            ) -> Result<Option<jmap_calendars_types::CalendarEvent>, BackendSetError<Self::Error>>
            {
                Err(BackendSetError::SetError(JsSetError::new(
                    JsSetErrorType::NotFound,
                )))
            }

            async fn destroy_object<O: SetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _id: &Id,
            ) -> Result<(), BackendSetError<Self::Error>> {
                Ok(())
            }

            fn supports_type<O: JmapObject>(&self) -> bool {
                true
            }

            async fn calendar_has_events(
                &self,
                _caller: &(),
                _account_id: &Id,
                _calendar_id: &Id,
            ) -> Result<bool, Self::Error> {
                // Simulate a transient backend failure — the handler
                // must NOT silently collapse this to Ok(false).
                Err(FaultyError("transient backend error"))
            }
        }

        let backend = FaultyBackend;
        let args = json!({
            "accountId": "acc1",
            "destroy": ["cal1"],
            // onDestroyRemoveEvents defaults to false → handler calls
            // calendar_has_events first.
        });
        let (resp, _) = handle_calendar_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );
        assert_eq!(
            not_destroyed["cal1"]["type"], "serverFail",
            "transient backend error must surface as serverFail, not as a deterministic \
             'calendarHasEvent' or silently-successful destroy: {resp}"
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
        let (resp, _) = handle_calendar_set(&backend, &(), args)
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
        let (resp, _) = handle_calendar_set(&backend, &(), args)
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
        let (resp, _) = handle_calendar_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        // Calendar destroyed
        let destroyed = resp["destroyed"]
            .as_array()
            .expect("destroyed must be array");
        assert_eq!(
            destroyed[0],
            json!("cal1"),
            "cal1 must be destroyed: {resp}"
        );

        // Event also destroyed — query MockBackend's CalendarEvent store directly.
        use jmap_server::JmapBackend;
        let (events, not_found) = backend
            .get_objects::<jmap_calendars_types::CalendarEvent>(
                &(),
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
        let (resp, _) = handle_calendar_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        // Calendar destroyed.
        let destroyed = resp["destroyed"]
            .as_array()
            .expect("destroyed must be array");
        assert_eq!(
            destroyed[0],
            json!("cal1"),
            "cal1 must be destroyed: {resp}"
        );

        // Event still exists, but cal1 must be gone from calendarIds and cal2
        // must remain.
        use jmap_server::JmapBackend;
        let (events, _) = backend
            .get_objects::<jmap_calendars_types::CalendarEvent>(
                &(),
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

    /// JMAP-r3pg.18 — RFC 8620 §5.3: every element of the destroy array MUST
    /// be a string Id. A non-string element (here `null`) must be rejected
    /// with an `invalidArguments` method-error rather than silently skipped.
    ///
    /// Oracle: the response is a `JmapError`, not a successful set response;
    /// the error type is `invalidArguments` per RFC 8620 §3.6.1.
    #[tokio::test]
    async fn set_destroy_rejects_non_string_entry() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": ["valid-id", null, "another-id"],
        });
        let err = handle_calendar_set(&backend, &(), args)
            .await
            .expect_err("destroy with non-string entry must error");
        let err_json = serde_json::to_value(&err).expect("serialize JmapError");
        assert_eq!(
            err_json["type"], "invalidArguments",
            "non-string destroy entry must yield invalidArguments: {err_json}"
        );
    }
}
