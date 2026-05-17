//! ShareNotification/* method handlers (RFC 9670 §3).
//!
//! ShareNotifications are server-created immutable records.  Clients may only
//! query and destroy them.  Any attempt to create or update a ShareNotification
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
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `unsupportedFilter`, `unsupportedSort`, `cannotCalculateChanges` —
//! per RFC 8620 §3.6 and §5). Per-target failures inside `/set`
//! (including the destroy-only create-or-update `forbidden` rejection)
//! surface in the `notCreated` / `notUpdated` / `notDestroyed` maps
//! within `Ok((Value, ...))`, not as `Err`.

use jmap_sharing_types::ShareNotification;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, SetError, SetErrorType, SharingBackend};
use crate::helpers::{extract_account_id, finalize_set_response, set_error_value, SetAccumulators};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend};

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
pub async fn handle_share_notification_get<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<ShareNotification, B>(backend, caller, args).await
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
pub async fn handle_share_notification_changes<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<ShareNotification, B>(backend, caller, args).await
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
/// immediate `forbidden` SetError without touching the backend. The `destroy`
/// list is forwarded to the backend normally.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_share_notification_set<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: an unknown accountId MUST surface as `accountNotFound`.
    // The /get-family handlers in `jmap_server::handlers` perform this check
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

    let old_state = backend
        .get_state::<ShareNotification>(caller, &account_id)
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

    // -----------------------------------------------------------------------
    // create — forbidden: ShareNotification is server-created only
    // -----------------------------------------------------------------------
    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, _) in create_map {
            not_created.insert(
                create_id,
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    // -----------------------------------------------------------------------
    // update — forbidden: ShareNotification is immutable
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, _) in update_map {
            not_updated.insert(
                id_str,
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    // -----------------------------------------------------------------------
    // destroy — the only permitted operation (RFC 9670 §3.3)
    // -----------------------------------------------------------------------
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
                    "destroy: every element must be a string Id; got {bad}"
                ))
            })?;
        for id_str_ref in ids {
            let id_str = id_str_ref.to_owned();
            let id = Id::from(id_str.as_str());

            match backend
                .destroy_object::<ShareNotification>(caller, &account_id, &id)
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
                    not_destroyed.insert(id_str, server_fail_value_from_backend(&e));
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

    finalize_set_response::<B, ShareNotification>(
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
pub async fn handle_share_notification_query<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<ShareNotification, B>(backend, caller, args).await
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
pub async fn handle_share_notification_query_changes<B: SharingBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<ShareNotification, B>(backend, caller, args).await
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
        let (resp, _) = handle_share_notification_set(&backend, &(), args)
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
        let (resp, _) = handle_share_notification_set(&backend, &(), args)
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
        let (resp, _) = handle_share_notification_set(&backend, &(), args)
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
        assert_eq!(err.error_type.as_str(), "invalidArguments");
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
            err.error_type.as_str(),
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
        let (resp, _) = handle_share_notification_set(&backend, &(), args)
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
