//! [`CalendarEventNotification`]/* method handlers (draft-ietf-jmap-calendars-34 §7).
//!
//! `CalendarEventNotification`s are server-created records. Clients may only
//! query and destroy them. Any attempt to create or update a
//! `CalendarEventNotification` via `/set` MUST be rejected with `forbidden` at
//! the handler layer — the backend never sees create or update calls for
//! this type.
//!
//! The backend owns which notifications a user can see (§7): none for
//! anyone but the calendar's owner when the event is `"private"` or
//! `"secret"` before or after the change; for a user without write access
//! to an event with `hideAttendees`, `event` and `eventPatch` computed from
//! the filtered views, and no notification when that patch would be empty.
//! A notification withheld from a user MUST behave as not existing in the
//! backend's caller-scoped reads.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by draft-ietf-jmap-calendars-34 §7. The returned
//! `Value` is the corresponding method-response object per the same
//! section refs.
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
//! Per-target failures inside `/set` (including the destroy-only
//! create-or-update `forbidden` rejection) surface in the `notCreated` /
//! `notUpdated` / `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use jmap_calendars_types::{CalendarEventNotification, CalendarEventNotificationComparator};
use jmap_types::{Id, JmapError};
use serde_json::Value;

use crate::CalendarsBackend;
use crate::helpers::{
    SetAccumulators, enforce_max_objects_in_set, extract_account_id, finalize_set_response,
    reject_unsupported_sort,
};
use jmap_server::{HandlerOutput, backend_set_error_value, server_fail_from_backend};
use jmap_server::{SetError, SetErrorType};

// ---------------------------------------------------------------------------
// CalendarEventNotification/get
// ---------------------------------------------------------------------------

/// Handle a `CalendarEventNotification/get` method call
/// (draft-ietf-jmap-calendars-34 §7.1).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id, or
///   when `ids` or `properties` is malformed;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `requestTooLarge` when `ids` holds more than `maxObjectsInGet` ids;
/// - `serverFail` when the backend fails.
pub async fn handle_calendar_event_notification_get<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_get::<CalendarEventNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// CalendarEventNotification/changes
// ---------------------------------------------------------------------------

/// Handle a `CalendarEventNotification/changes` method call
/// (draft-ietf-jmap-calendars-34 §7.2).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id,
///   `sinceState` is missing, or `maxChanges` is not a positive integer;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `cannotCalculateChanges` when the backend cannot compute the changes
///   since `sinceState`;
/// - `serverFail` when the backend fails.
pub async fn handle_calendar_event_notification_changes<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<CalendarEventNotification, B>(backend, caller, args, &[]).await
}

// ---------------------------------------------------------------------------
// CalendarEventNotification/set — destroy-only
// ---------------------------------------------------------------------------

/// Handle a `CalendarEventNotification/set` method call
/// (draft-ietf-jmap-calendars-34 §7.3).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// **Destroy-only enforcement**: only `destroy` is supported. Any entries in
/// the `create` or `update` maps receive an immediate `forbidden` [`SetError`]
/// without touching the backend.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id, or
///   when `destroy` is malformed;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `requestTooLarge` when `create`, `update` and `destroy` together hold
///   more than `maxObjectsInSet` objects;
/// - `stateMismatch` when `ifInState` is not the current state;
/// - `serverFail` when the backend fails.
///
/// A create or update is a `forbidden` [`SetError`], and a destroy that
/// fails on its own is a [`SetError`] in `notDestroyed`, not an `Err`.
pub async fn handle_calendar_event_notification_set<B: CalendarsBackend>(
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

    let old_state = backend
        .get_state::<CalendarEventNotification>(caller, &account_id)
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
    set_create(&mut args, &mut acc);
    set_update(&mut args, &mut acc);
    let mutated = set_destroy(&ctx, &mut args, &mut acc).await?;

    finalize_set_response::<B, CalendarEventNotification>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        acc,
    )
    .await
}

/// What every `CalendarEventNotification/set` sub-function needs from the
/// request (docs/design/set-handler-split.md).
struct SetCtx<'a, B: CalendarsBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
}

/// The `create` part of `CalendarEventNotification/set`: every entry is
/// `forbidden`.
fn set_create(args: &mut serde_json::Map<String, Value>, acc: &mut SetAccumulators) {
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for create_id in create_map.keys() {
            acc.not_created.insert(
                create_id.clone(),
                serde_json::Value::from(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }
}

/// The `update` part of `CalendarEventNotification/set`: every entry is
/// `forbidden`.
fn set_update(args: &mut serde_json::Map<String, Value>, acc: &mut SetAccumulators) {
    if let Some(update_map) = args.get("update").and_then(|v| v.as_object()) {
        for id_str in update_map.keys() {
            acc.not_updated.insert(
                id_str.clone(),
                serde_json::Value::from(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }
}

/// The `destroy` part of `CalendarEventNotification/set`, the only
/// permitted operation. Returns whether the backend was written.
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
                .destroy_object::<CalendarEventNotification>(ctx.caller, ctx.account_id, &id)
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
// CalendarEventNotification/query
// ---------------------------------------------------------------------------

/// Handle a `CalendarEventNotification/query` method call
/// (draft-ietf-jmap-calendars-34 §7.4).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`); the
/// returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id, or
///   when `filter`, `sort`, `position`, `anchor`, `anchorOffset` or `limit`
///   is malformed;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `unsupportedFilter` when the filter does not decode as a `CalendarEventNotification` filter;
/// - `unsupportedSort` when `sort` names a property that is not supported;
/// - `serverFail` when the backend fails, or any other RFC 8620 §5.5 error
///   the backend returns, such as `anchorNotFound`.
pub async fn handle_calendar_event_notification_query<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    reject_unsupported_sort::<CalendarEventNotificationComparator>(args.get("sort"))?;
    jmap_server::handle_query::<CalendarEventNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// CalendarEventNotification/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `CalendarEventNotification/queryChanges` method call
/// (draft-ietf-jmap-calendars-34 §7.5).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// # Errors
///
/// Returns a method-level [`JmapError`]:
///
/// - `invalidArguments` when `accountId` is missing or not a valid Id,
///   `sinceQueryState` is missing, or `filter`, `sort`, `maxChanges` or
///   `upToId` is malformed;
/// - `accountNotFound` when the backend does not know `accountId`;
/// - `unsupportedFilter` or `unsupportedSort` as for
///   [`handle_calendar_event_notification_query`];
/// - `cannotCalculateChanges` when `sinceQueryState` was computed in another
///   language or the backend cannot compute the changes since it;
/// - `tooManyChanges` when there are more changes than `maxChanges`;
/// - `serverFail` when the backend fails.
pub async fn handle_calendar_event_notification_query_changes<B: CalendarsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    reject_unsupported_sort::<CalendarEventNotificationComparator>(args.get("sort"))?;
    jmap_server::handle_query_changes::<CalendarEventNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: RFC 8620 §5.5 `unsupportedSort`: "The sort is syntactically
    /// valid, but includes a property the server does not support sorting
    /// on". draft-ietf-jmap-calendars-34 §7.4.2: "The \"created\" property MUST be
    /// supported for sorting." and lists no other; `calendarEventId` is not among them.
    #[tokio::test]
    async fn query_unlisted_sort_property_returns_unsupported_sort() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sort": [{ "property": "calendarEventId" }] });
        let err = handle_calendar_event_notification_query(&backend, &(), args)
            .await
            .expect_err("unlisted sort property must be rejected");
        assert_eq!(err.error_type.to_string(), "unsupportedSort");
    }

    /// Oracle: as above; `CalendarEventNotification/queryChanges` carries the same `sort`
    /// (RFC 8620 §5.6).
    #[tokio::test]
    async fn query_changes_unlisted_sort_property_returns_unsupported_sort() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "sinceQueryState": "0",
            "sort": [{ "property": "created" }, { "property": "calendarEventId" }]
        });
        let err = handle_calendar_event_notification_query_changes(&backend, &(), args)
            .await
            .expect_err("unlisted sort property must be rejected");
        assert_eq!(err.error_type.to_string(), "unsupportedSort");
    }

    /// Oracle: draft-ietf-jmap-calendars-34 §7.4.2: "The \"created\" property MUST be
    /// supported for sorting." and lists no other, so a `created` sort is accepted.
    #[tokio::test]
    async fn query_listed_sort_property_is_accepted() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sort": [{ "property": "created" }] });
        handle_calendar_event_notification_query(&backend, &(), args)
            .await
            .expect("listed sort property must not be rejected");
    }

    /// Oracle: create entries must produce `forbidden` in notCreated.
    /// Source: draft-ietf-jmap-calendars-34 §7.3 — notifications are server-created.
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
        let HandlerOutput { primary: resp, .. } =
            handle_calendar_event_notification_set(&backend, &(), args)
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
        let HandlerOutput { primary: resp, .. } =
            handle_calendar_event_notification_set(&backend, &(), args)
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
        assert_eq!(err.error_type.to_string(), "accountNotFound");
    }

    /// Oracle: destroy of a non-existent notification → notFound in notDestroyed.
    #[tokio::test]
    async fn set_destroy_nonexistent_returns_not_found() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": ["doesnotexist"]
        });
        let HandlerOutput { primary: resp, .. } =
            handle_calendar_event_notification_set(&backend, &(), args)
                .await
                .expect("must not return top-level error");
        assert_eq!(
            resp["notDestroyed"]["doesnotexist"]["type"], "notFound",
            "missing id must produce notFound: {resp}"
        );
    }
}
