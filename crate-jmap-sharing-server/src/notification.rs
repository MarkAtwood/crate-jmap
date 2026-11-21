//! [`ShareNotification`]/* method handlers (RFC 9670 §3).
//!
//! `ShareNotification`s are server-created immutable records.  Clients may only
//! query and destroy them.  Any attempt to create or update a `ShareNotification`
//! via `/set` MUST be rejected with `forbidden` at the handler layer — the
//! backend never sees create or update calls for this type.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by RFC 9670 §3. The returned `Value` is the
//! corresponding method-response object per the same section refs.
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

use jmap_sharing_types::ShareNotification;
use jmap_types::{Id, JmapError};
use serde_json::Value;

use crate::SharingBackend;
use crate::helpers::{
    SetAccumulators, enforce_max_objects_in_set, extract_account_id, finalize_set_response,
};
use jmap_server::{HandlerOutput, backend_set_error_value, server_fail_from_backend};
use jmap_server::{SetError, SetErrorType};

// ---------------------------------------------------------------------------
// ShareNotification/get
// ---------------------------------------------------------------------------

/// Handle a `ShareNotification/get` method call (RFC 9670 §3.1).
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
pub async fn handle_share_notification_get<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_get::<ShareNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// ShareNotification/changes
// ---------------------------------------------------------------------------

/// Handle a `ShareNotification/changes` method call (RFC 9670 §3.2).
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
pub async fn handle_share_notification_changes<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<ShareNotification, B>(backend, caller, args, &[]).await
}

// ---------------------------------------------------------------------------
// ShareNotification/set
// ---------------------------------------------------------------------------

/// Handle a `ShareNotification/set` method call (RFC 9670 §3.3).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// **Destroy-only enforcement**: RFC 9670 §3.3 states that only `destroy` is
/// supported.  Any entries in the `create` or `update` maps receive an
/// immediate `forbidden` [`SetError`] without touching the backend. The `destroy`
/// list is forwarded to the backend normally.
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
/// A create, update or destroy that fails on its own is a [`SetError`] in
/// the response, not an `Err`.
pub async fn handle_share_notification_set<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: an unknown accountId MUST surface as `accountNotFound`.
    // The /get-family handlers in `jmap-server` perform this check
    // internally; the hand-rolled /set handler must reproduce it explicitly.
    // Mirrors the canonical pattern at
    // `crate-jmap-mail-server/src/mailbox.rs:441-447`.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.8). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<ShareNotification>(caller, &account_id)
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

    finalize_set_response::<B, ShareNotification>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        acc,
    )
    .await
}

/// What every `ShareNotification/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: SharingBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
}

/// The `create` part of `ShareNotification/set`: forbidden, because a
/// [`ShareNotification`] is server-created only.
fn set_create(args: &mut serde_json::Map<String, Value>, acc: &mut SetAccumulators) {
    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, _) in create_map {
            acc.not_created.insert(
                create_id,
                serde_json::Value::from(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }
}

/// The `update` part of `ShareNotification/set`: forbidden, because a
/// [`ShareNotification`] is immutable.
fn set_update(args: &mut serde_json::Map<String, Value>, acc: &mut SetAccumulators) {
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, _) in update_map {
            acc.not_updated.insert(
                id_str,
                serde_json::Value::from(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }
}

/// The `destroy` part of `ShareNotification/set`, the only permitted
/// operation (RFC 9670 §3.3), including the whole-request
/// `invalidArguments` for a non-string element. Returns whether the
/// backend was written.
async fn set_destroy<B: SharingBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        //
        // Single-pass collect: `as_str().ok_or(v)` yields `Ok(&str)` for valid
        // ids and `Err(&Value)` for the first bad element. The resulting
        // `Vec<&str>` lets the consume loop work in `&str` without a
        // never-triggered fallback arm.
        let ids: Vec<&str> = destroy_arr
            .iter()
            .map(|v| v.as_str().ok_or(v))
            .collect::<Result<_, _>>()
            .map_err(|bad| {
                JmapError::invalid_arguments(format!(
                    "destroy: every element must be a string Id; got {}",
                    jmap_server::json_type_name(bad)
                ))
            })?;
        for id_str_ref in ids {
            let id_str = id_str_ref.to_owned();
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
                .destroy_object::<ShareNotification>(ctx.caller, ctx.account_id, &id)
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
// ShareNotification/query
// ---------------------------------------------------------------------------

/// Handle a `ShareNotification/query` method call (RFC 9670 §3.4).
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
/// - `unsupportedFilter` when the filter does not decode as a
///   `ShareNotification` filter or the backend cannot process it;
/// - `unsupportedSort` when the backend does not support the sort;
/// - `anchorNotFound` when `anchor` is not in the results;
/// - `serverFail` when the backend fails, or any other RFC 8620 §5.5 error
///   the backend returns.
pub async fn handle_share_notification_query<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_query::<ShareNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// ShareNotification/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `ShareNotification/queryChanges` method call (RFC 9670 §3.5).
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
///   [`handle_share_notification_query`];
/// - `cannotCalculateChanges` when the backend cannot compute the changes
///   since `sinceQueryState`;
/// - `tooManyChanges` when there are more changes than `maxChanges`;
/// - `serverFail` when the backend fails.
pub async fn handle_share_notification_query_changes<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_query_changes::<ShareNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: RFC 9670 §3.3 — create entries must produce `forbidden` in notCreated.
    /// No backend call is made for create (pure handler-layer enforcement).
    #[tokio::test]
    async fn set_create_returns_forbidden_not_created() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "id": "ignored", "created": "2024-01-01T00:00:00Z",
                         "changedBy": { "name": "Alice", "email": null, "principalId": null },
                         "objectType": "Mailbox", "objectAccountId": "acc2",
                         "objectId": "mb1", "oldRights": null, "newRights": null,
                         "name": "Team Inbox" },
                "c2": { "id": "ignored2", "created": "2024-01-02T00:00:00Z",
                         "changedBy": { "name": "Bob", "email": null, "principalId": null },
                         "objectType": "Calendar", "objectAccountId": "acc3",
                         "objectId": "cal1", "oldRights": null, "newRights": null,
                         "name": "Calendar" }
            }
        });
        let HandlerOutput { primary: resp, .. } =
            handle_share_notification_set(&backend, &(), args)
                .await
                .expect("must not return top-level error");

        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present for create attempts: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "forbidden",
            "c1 create must be forbidden: {resp}"
        );
        assert_eq!(
            not_created["c2"]["type"], "forbidden",
            "c2 create must be forbidden: {resp}"
        );
        // created must be null — nothing was actually created
        assert!(
            resp["created"].is_null(),
            "created must be null when all creates are forbidden: {resp}"
        );
    }

    /// Oracle: RFC 9670 §3.3 — update entries must produce `forbidden` in notUpdated.
    #[tokio::test]
    async fn set_update_returns_forbidden_not_updated() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "update": {
                "notif1": { "name": "Renamed" },
                "notif2": { "objectType": "Calendar" }
            }
        });
        let HandlerOutput { primary: resp, .. } =
            handle_share_notification_set(&backend, &(), args)
                .await
                .expect("must not return top-level error");

        let not_updated = &resp["notUpdated"];
        assert!(
            not_updated.is_object(),
            "notUpdated must be present for update attempts: {resp}"
        );
        assert_eq!(not_updated["notif1"]["type"], "forbidden");
        assert_eq!(not_updated["notif2"]["type"], "forbidden");
    }

    /// Oracle: RFC 9670 §3.3 — destroy proceeds normally even when create/update are
    /// also present (they get forbidden but destroy is forwarded to backend).
    #[tokio::test]
    async fn set_mixed_create_and_destroy_enforces_destroy_only() {
        let backend = MockBackend::new_with_account("acc1");
        // Pre-populate the mock with a notification to destroy.
        backend.add_notification("acc1", "notif1");

        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "id": "x", "created": "2024-01-01T00:00:00Z",
                         "changedBy": { "name": "Alice", "email": null, "principalId": null },
                         "objectType": "Mailbox", "objectAccountId": "a",
                         "objectId": "m1", "oldRights": null, "newRights": null,
                         "name": "N" }
            },
            "destroy": ["notif1"]
        });
        let HandlerOutput { primary: resp, .. } =
            handle_share_notification_set(&backend, &(), args)
                .await
                .expect("must not return top-level error");

        // create → forbidden
        assert_eq!(resp["notCreated"]["c1"]["type"], "forbidden");
        // destroy → succeeded
        let destroyed = resp["destroyed"]
            .as_array()
            .expect("destroyed must be array");
        assert_eq!(destroyed.len(), 1);
        assert_eq!(destroyed[0], "notif1");
    }

    /// Oracle: ShareNotification/set destroy array with null element must return
    /// a top-level invalidArguments error.
    #[tokio::test]
    async fn set_destroy_null_element_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": [null]
        });
        let result = handle_share_notification_set(&backend, &(), args).await;
        let err = result.expect_err("must return top-level error for null destroy element");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
    }

    /// Oracle: ShareNotification/set with unknown accountId must return
    /// top-level accountNotFound (RFC 8620 §3.6.2). Regression guard for
    /// bd:JMAP-3t94.1.
    #[tokio::test]
    async fn set_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({
            "accountId": "unknown",
            "destroy": ["notif1"]
        });
        let result = handle_share_notification_set(&backend, &(), args).await;
        let err = result.expect_err("must return top-level error for unknown account");
        assert_eq!(
            err.error_type.to_string(),
            "accountNotFound",
            "unknown accountId must produce accountNotFound; got: {:?}",
            err.error_type
        );
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
            handle_share_notification_set(&backend, &(), args)
                .await
                .expect("must not return top-level error");

        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );
        assert_eq!(
            not_destroyed["doesnotexist"]["type"], "notFound",
            "missing id must produce notFound: {resp}"
        );
    }
}
