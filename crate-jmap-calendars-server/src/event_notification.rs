//! CalendarEventNotification/* method handlers (draft-ietf-jmap-calendars-26 §7).
//!
//! CalendarEventNotifications are server-created records. Clients may only
//! query and destroy them. Any attempt to create or update a
//! CalendarEventNotification via `/set` MUST be rejected with `forbidden` at
//! the handler layer — the backend never sees create or update calls for
//! this type.

use jmap_calendars_types::CalendarEventNotification;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, CalendarsBackend, SetError, SetErrorType};
use crate::helpers::{extract_account_id, finalize_set_response, set_error_value, SetAccumulators};
use jmap_server::server_fail_from_backend;

// ---------------------------------------------------------------------------
// CalendarEventNotification/get
// ---------------------------------------------------------------------------

/// Handle a `CalendarEventNotification/get` method call
/// (draft-ietf-jmap-calendars-26 §7.1).
pub async fn handle_calendar_event_notification_get<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<CalendarEventNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// CalendarEventNotification/changes
// ---------------------------------------------------------------------------

/// Handle a `CalendarEventNotification/changes` method call
/// (draft-ietf-jmap-calendars-26 §7.2).
pub async fn handle_calendar_event_notification_changes<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<CalendarEventNotification, B>(backend, caller, args)
        .await
}

// ---------------------------------------------------------------------------
// CalendarEventNotification/set — destroy-only
// ---------------------------------------------------------------------------

/// Handle a `CalendarEventNotification/set` method call
/// (draft-ietf-jmap-calendars-26 §7.3).
///
/// **Destroy-only enforcement**: only `destroy` is supported. Any entries in
/// the `create` or `update` maps receive an immediate `forbidden` SetError
/// without touching the backend.
pub async fn handle_calendar_event_notification_set<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: accountId not recognised → accountNotFound.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let old_state = backend
        .get_state::<CalendarEventNotification>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // create — forbidden
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for create_id in create_map.keys() {
            not_created.insert(
                create_id.clone(),
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    // update — forbidden
    if let Some(update_map) = args.get("update").and_then(|v| v.as_object()) {
        for id_str in update_map.keys() {
            not_updated.insert(
                id_str.clone(),
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    // destroy — the only permitted operation
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
                .destroy_object::<CalendarEventNotification>(caller, &account_id, &id)
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

    finalize_set_response::<B, CalendarEventNotification>(
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
// CalendarEventNotification/query
// ---------------------------------------------------------------------------

/// Handle a `CalendarEventNotification/query` method call
/// (draft-ietf-jmap-calendars-26 §7.4).
pub async fn handle_calendar_event_notification_query<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<CalendarEventNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// CalendarEventNotification/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `CalendarEventNotification/queryChanges` method call
/// (draft-ietf-jmap-calendars-26 §7.5).
pub async fn handle_calendar_event_notification_query_changes<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<CalendarEventNotification, B>(
        backend, caller, args,
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

    /// Oracle: create entries must produce `forbidden` in notCreated.
    /// Source: draft-ietf-jmap-calendars-26 §7.3 — notifications are server-created.
    #[tokio::test]
    async fn set_create_returns_forbidden_not_created() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "id": "n1",
                    "created": "2024-01-01T00:00:00Z",
                    "changedBy": { "name": "Alice", "email": null, "principalId": null },
                    "type": "created",
                    "calendarEventId": "ev1",
                    "event": {}
                }
            }
        });
        let (resp, _) = handle_calendar_event_notification_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "forbidden",
            "create must be forbidden: {resp}"
        );
    }

    /// Oracle: update entries must produce `forbidden` in notUpdated.
    #[tokio::test]
    async fn set_update_returns_forbidden_not_updated() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "update": {
                "n1": { "comment": "changed" }
            }
        });
        let (resp, _) = handle_calendar_event_notification_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert_eq!(resp["notUpdated"]["n1"]["type"], "forbidden");
    }

    /// Oracle: CalendarEventNotification/set with unknown accountId returns
    /// accountNotFound. Source: RFC 8620 §3.6.2.
    #[tokio::test]
    async fn set_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown" });
        let result = handle_calendar_event_notification_set(&backend, &(), args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: destroy of a non-existent notification → notFound in notDestroyed.
    #[tokio::test]
    async fn set_destroy_nonexistent_returns_not_found() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": ["doesnotexist"]
        });
        let (resp, _) = handle_calendar_event_notification_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert_eq!(
            resp["notDestroyed"]["doesnotexist"]["type"], "notFound",
            "missing id must produce notFound: {resp}"
        );
    }
}
