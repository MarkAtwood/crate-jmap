//! Calendar/* method handlers (draft-ietf-jmap-calendars-33 §4).
//!
//! `Calendar/set` has special logic: if `onDestroyRemoveEvents` is absent or
//! `false`, destroying a Calendar that still has events is rejected with a
//! `calendarHasEvent` SetError (not a top-level error).
//!
//! A calendar on which the caller holds only `mayReadFreeBusy` MUST NOT be
//! returned by the backend's caller-scoped `Calendar/get` or
//! `Calendar/changes` reads (§4).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3), with the
//! type-specific arguments defined by draft-ietf-jmap-calendars-33 §4.
//! The returned `Value` is the corresponding method-response object
//! per the same section refs.
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

use jmap_calendars_types::{Calendar, CalendarEvent, CalendarEventFilterCondition};
use jmap_types::{Filter, Id, JmapError, PatchObject};
use serde_json::{Value, json};

use crate::helpers::{
    SetAccumulators, apply_default_change_to_response, enforce_max_objects_in_set,
    extract_account_id, finalize_set_response, resolve_on_success_set_is_default, set_error_value,
};
use crate::{CalendarsBackend, SetError, SetErrorType};
use jmap_server::{
    HandlerOutput, QueryParams, backend_set_error_value, bool_arg, reject_read_only_patch,
    reject_server_set, server_fail_from_backend, server_fail_value_from_backend,
};

// ---------------------------------------------------------------------------
// Calendar/get
// ---------------------------------------------------------------------------

/// Handle a `Calendar/get` method call (draft-ietf-jmap-calendars-33 §4.1).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_calendar_get<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_get::<Calendar, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Calendar/changes
// ---------------------------------------------------------------------------

/// Handle a `Calendar/changes` method call (draft-ietf-jmap-calendars-33 §4.2).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_calendar_changes<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<Calendar, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Calendar/set
// ---------------------------------------------------------------------------

/// Handle a `Calendar/set` method call (draft-ietf-jmap-calendars-33 §4.4).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps), augmented
/// with the draft §4.4 `onDestroyRemoveEvents` and
/// `onSuccessSetIsDefault` arguments; the returned `Value` is the §5.3
/// `/set` response shape (`accountId`, `oldState`, `newState`, plus the
/// per-operation `created` / `notCreated` / `updated` / `notUpdated` /
/// `destroyed` / `notDestroyed` maps).
///
/// Special behaviour:
/// - Parses `onDestroyRemoveEvents` (default `false`) from the request args.
/// - If `false`, any calendar in the `destroy` list that still has events is
///   rejected with a `calendarHasEvent` SetError (draft §4.4.1).
/// - Create and update are forwarded to the backend normally.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_calendar_set<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
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

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.4). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let on_destroy_remove_events = bool_arg(&args, "onDestroyRemoveEvents", false);

    // §4.3: onSuccessSetIsDefault — Id|null. Captured here so we can resolve
    // a possible "#createId" reference against the post-create state. The
    // raw value is kept until after all CRUD ops succeed.
    let on_success_set_is_default = args.remove("onSuccessSetIsDefault");

    let old_state = backend
        .get_state::<Calendar>(caller, &account_id)
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
        for (create_id, mut obj_val) in create_map {
            if let Some(err) = obj_val.as_object().and_then(reject_server_set::<Calendar>) {
                not_created.insert(create_id, set_error_value(&err));
                continue;
            }
            // `isDefault` and `myRights` are server-set (draft-ietf-jmap-
            // calendars-33 §4) and `Calendar` requires both, so the server
            // supplies them. A new calendar is not the default until
            // `onSuccessSetIsDefault` makes it one.
            //
            // ponytail: the creator holds every right, as the owner of a
            // calendar in their own account; a backend with finer sharing may
            // replace `myRights` in `create_object`.
            if let Some(m) = obj_val.as_object_mut() {
                m.insert("isDefault".to_owned(), Value::Bool(false));
                m.insert(
                    "myRights".to_owned(),
                    json!({
                        "mayReadFreeBusy": true,
                        "mayReadItems": true,
                        "mayWriteAll": true,
                        "mayWriteOwn": true,
                        "mayUpdatePrivate": true,
                        "mayRSVP": true,
                        "mayShare": true,
                        "mayDelete": true,
                    }),
                );
            }
            // `Calendar.id` is optional, so the create parses without one
            // and `build` sets the id the backend mints.
            let mut cal: Calendar = match serde_json::from_value(obj_val) {
                Ok(c) => c,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };
            let build = move |id: &Id| {
                cal.id = Some(id.clone());
                cal
            };
            match backend
                .create_object::<Calendar, _>(caller, &account_id, &create_id, build)
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
                    not_updated.insert(id_str, set_error_value(&err));
                    continue;
                }
            };
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
            if let Some(err) = reject_read_only_patch::<Calendar>(patch.as_map(), None) {
                not_updated.insert(id_str, set_error_value(&err));
                continue;
            }
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
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_destroyed.insert(id_str, set_error_value(&err));
                    continue;
                }
            };

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
                            set_error_value(&SetError::new(SetErrorType::from("calendarHasEvent"))),
                        );
                        continue;
                    }
                    Ok(false) => {
                        // proceed to destroy below
                    }
                    Err(e) => {
                        // bd:JMAP-q2wa.7.2: backend Display text never
                        // reaches the wire.
                        not_destroyed.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            // PLAN.md §5 / draft-ietf-jmap-calendars-33 §4.4: when
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
                    Err(value) => {
                        // bd:JMAP-ic0j.68 — `cleanup_calendar_events` returns
                        // the already-redacted wire-format SetError Value so
                        // backend Display text never reaches the wire.
                        not_destroyed.insert(id_str, value);
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
                Err(e) => {
                    not_destroyed.insert(id_str, backend_set_error_value(e));
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
    if all_succeeded
        && let Some(raw) = on_success_set_is_default.as_ref()
        && let Some(target) = resolve_on_success_set_is_default(raw, &created)
    {
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
/// Implements PLAN.md §5 (draft-ietf-jmap-calendars-33 §4.4 onDestroyRemoveEvents):
///   - Events whose `calendarIds` is just this calendar are destroyed.
///   - Events with multiple calendarIds are patched to remove this calendar id.
///
/// Returns `Err(value)` on the first sub-step failure where `value` is the
/// already-redacted wire-format SetError [`Value`] for the calendar's
/// `notDestroyed` entry (bd:JMAP-ic0j.68). Partial cleanup is not acceptable:
/// failing fast keeps the data store consistent.
///
/// The helper owns its own error wire shape end-to-end so that the
/// `JmapBackend::Error` Display contract (workspace AGENTS.md "Backend trait
/// rule") is honoured at a single chokepoint rather than at every call site.
/// Inner `BackendSetError::SetError(set_err)` failures preserve the typed
/// JMAP error variant via [`crate::helpers::set_error_value`]; inner
/// `BackendSetError::Other(_)` and `JmapBackend::Error` failures collapse to
/// the static [`server_fail_value_from_backend`] wire shape.
async fn cleanup_calendar_events<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    calendar_id: &Id,
) -> Result<(), Value> {
    // Step 1: query event ids whose calendarIds include this calendar.
    // CalendarEventFilterCondition is #[non_exhaustive], so construct via
    // Default + field assignment rather than a struct literal.
    let mut condition = CalendarEventFilterCondition::default();
    condition.in_calendar = Some(calendar_id.clone());
    let filter = Filter::Condition(condition);
    let event_ids: Vec<Id> = backend
        .query_objects::<CalendarEvent>(
            caller,
            account_id,
            QueryParams::new().filter(Some(&filter)),
        )
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
        .ids;

    if event_ids.is_empty() {
        return Ok(());
    }

    // Step 2: fetch full event objects to inspect calendar_ids count.
    let (events, _not_found): (Vec<CalendarEvent>, _) = backend
        .get_objects::<CalendarEvent>(caller, account_id, Some(&event_ids), None)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;

    // Step 3: for each event, destroy if single-calendar, else patch out this id.
    for event in events {
        let n_calendars = event.calendar_ids.as_ref().map(|m| m.len()).unwrap_or(0);

        // The CalendarEvent's `id` is required for /set semantics — it should
        // always be Some on a real backend. If it's missing here we cannot
        // address the event, so abort with a redacted serverFail (the
        // "missing id" condition is a server-internal-state observation; do
        // not surface internal cleanup state to the wire).
        let event_id = match event.id.as_ref() {
            Some(id) => id.clone(),
            None => return Err(server_fail_value_from_backend(&"missing event id")),
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
                .map_err(backend_set_error_value)?;
        } else {
            // Single-calendar (this one): destroy the event outright.
            backend
                .destroy_object::<CalendarEvent>(caller, account_id, &event_id)
                .await
                .map_err(backend_set_error_value)?;
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
        assert_eq!(err.error_type.to_string(), "accountNotFound");
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
        assert_eq!(err.error_type.to_string(), "accountNotFound");
    }

    /// Oracle: Calendar/set with onDestroyRemoveEvents=false and a calendar that
    /// has events → notDestroyed with "calendarHasEvent" error type.
    /// Source: draft-ietf-jmap-calendars-33 §4.4.1.
    #[tokio::test]
    async fn set_destroy_calendar_with_events_returns_calendar_has_events() {
        let backend = MockBackend::new_with_account_and_events("acc1", "cal1");
        let args = json!({
            "accountId": "acc1",
            "destroy": ["cal1"],
            // onDestroyRemoveEvents defaults to false — not sent
        });
        let HandlerOutput { primary: resp, .. } = handle_calendar_set(&backend, &(), args)
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
            BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject,
            JmapBackend, JmapObject, QueryChangesParams, QueryChangesResult, QueryObject,
            QueryResult, SetError as JsSetError, SetErrorType as JsSetErrorType, SetObject,
        };
        use jmap_types::{Id, PatchObject, State};

        // Canary for bd:JMAP-q2wa.7.2. Oracle: the workspace contract that
        // backend Display text never reaches the wire, pinned by
        // jmap-server `server_fail_value_from_backend_drops_display_text`.
        const CANARY: &str = "TOKEN-DO-NOT-LEAK-ca1e";

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
                _params: QueryParams<'_, O>,
            ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
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
                params: QueryChangesParams<'_, O>,
            ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
                let QueryChangesParams {
                    since_query_state, ..
                } = params;
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
            async fn create_object<O, F>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _create_id: &str,
                build: F,
            ) -> Result<(Id, O), BackendSetError<Self::Error>>
            where
                O: SetObject + Send + Sync,
                F: FnOnce(&Id) -> O + Send,
            {
                let id = Id::from("mock-id");
                let obj = build(&id);
                Ok((id, obj))
            }

            async fn update_object<O: SetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _id: &Id,
                _patch: O::Patch,
            ) -> Result<Option<O>, BackendSetError<Self::Error>> {
                Err(BackendSetError::SetError(Box::new(JsSetError::new(
                    JsSetErrorType::NotFound,
                ))))
            }

            async fn update_per_user_properties(
                &self,
                _caller: &(),
                _account_id: &Id,
                _id: &Id,
                _patch: PatchObject,
            ) -> Result<Option<jmap_calendars_types::CalendarEvent>, BackendSetError<Self::Error>>
            {
                Err(BackendSetError::SetError(Box::new(JsSetError::new(
                    JsSetErrorType::NotFound,
                ))))
            }

            async fn destroy_object<O: SetObject + Send + Sync>(
                &self,
                _caller: &(),
                _account_id: &Id,
                _id: &Id,
            ) -> Result<(), BackendSetError<Self::Error>> {
                Ok(())
            }

            fn supports_type<O: JmapObject>(
                &self,
                _caller: &Self::CallerCtx,
                _account_id: &jmap_types::Id,
            ) -> bool {
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
                Err(FaultyError(CANARY))
            }

            async fn parse_calendar_event_blobs(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _blob_ids: &[jmap_types::Id],
                _properties: Option<&[String]>,
            ) -> Result<crate::CalendarEventParseResult, Self::Error> {
                Err(FaultyError(CANARY))
            }

            async fn get_availability(
                &self,
                _caller: &(),
                _account_id: &jmap_types::Id,
                _principal_id: &jmap_types::Id,
                _utc_start: &jmap_types::UTCDate,
                _utc_end: &jmap_types::UTCDate,
                _show_details: bool,
                _event_properties: Option<&[String]>,
            ) -> Result<
                Vec<jmap_calendars_types::BusyPeriod>,
                jmap_server::BackendQueryError<Self::Error>,
            > {
                Err(jmap_server::BackendQueryError::Other(FaultyError(CANARY)))
            }
        }

        let backend = FaultyBackend;
        let args = json!({
            "accountId": "acc1",
            "destroy": ["cal1"],
            // onDestroyRemoveEvents defaults to false → handler calls
            // calendar_has_events first.
        });
        let HandlerOutput { primary: resp, .. } = handle_calendar_set(&backend, &(), args)
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
        assert!(
            !resp.to_string().contains(CANARY),
            "backend Display text must not reach the wire: {resp}"
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
        let HandlerOutput { primary: resp, .. } = handle_calendar_set(&backend, &(), args)
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
        let HandlerOutput { primary: resp, .. } = handle_calendar_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        // cal1 exists in the mock, so destroy should succeed
        let destroyed = resp["destroyed"]
            .as_array()
            .expect("destroyed must be array");
        assert_eq!(destroyed.len(), 1, "cal1 must be destroyed: {resp}");
        assert_eq!(destroyed[0], json!("cal1"));
    }

    /// Oracle: PLAN.md §5 / draft-ietf-jmap-calendars-33 §4.4 —
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
        let HandlerOutput { primary: resp, .. } = handle_calendar_set(&backend, &(), args)
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

    /// Oracle: PLAN.md §5 / draft-ietf-jmap-calendars-33 §4.4 —
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
        let HandlerOutput { primary: resp, .. } = handle_calendar_set(&backend, &(), args)
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
