//! TaskNotification/* method handlers (draft-tasks-06 §5).
//!
//! TaskNotifications are server-created immutable records. Clients may only
//! query and destroy them. Any attempt to create or update a TaskNotification
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
//! arguments defined by draft-tasks-06 §5. The returned `Value` is the
//! corresponding method-response object per the same section refs.
//!
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `unsupportedFilter`, `unsupportedSort`, `cannotCalculateChanges` —
//! per RFC 8620 §3.6 and §5). Per-target failures inside `/set` surface
//! in the `notDestroyed` map within `Ok((Value, ...))`, not as `Err`.

use jmap_tasks_types::TaskNotification;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::Value;

use crate::backend::{BackendSetError, SetError, SetErrorType, TasksBackend};
use crate::helpers::{
    enforce_max_objects_in_set, extract_account_id, finalize_set_response, set_error_value,
    SetAccumulators,
};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend};

// ---------------------------------------------------------------------------
// TaskNotification/get
// ---------------------------------------------------------------------------

/// Handle a `TaskNotification/get` method call (draft-tasks-06 §5.2).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_notification_get<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<TaskNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// TaskNotification/changes
// ---------------------------------------------------------------------------

/// Handle a `TaskNotification/changes` method call (draft-tasks-06 §5.3).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the §5.2
/// `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_notification_changes<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<TaskNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// TaskNotification/set
// ---------------------------------------------------------------------------

/// Handle a `TaskNotification/set` method call (draft-tasks-06 §5.4).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation result maps).
///
/// **Destroy-only enforcement**: draft-tasks-06 §5.4 states that only
/// `destroy` is supported. Any entries in the `create` or `update` maps
/// receive an immediate `forbidden` SetError without touching the backend.
/// The `destroy` list is forwarded to the backend normally.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_notification_set<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: accountId not recognised → accountNotFound (method-level
    // error). Without this, a /set against an unknown accountId would silently
    // "succeed" with a fake oldState/newState envelope. Fixed in JMAP-gpt1.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.5). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<TaskNotification>(caller, &account_id)
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
    // create — forbidden: TaskNotification is server-created only
    // -----------------------------------------------------------------------
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for create_id in create_map.keys() {
            not_created.insert(
                create_id.clone(),
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    // -----------------------------------------------------------------------
    // update — forbidden: TaskNotification is immutable
    // -----------------------------------------------------------------------
    if let Some(update_map) = args.get("update").and_then(|v| v.as_object()) {
        for id_str in update_map.keys() {
            not_updated.insert(
                id_str.clone(),
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    // -----------------------------------------------------------------------
    // destroy — the only permitted operation (draft-tasks-06 §5.4)
    // -----------------------------------------------------------------------
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Two-pass validation is intentional: this up-front loop fails the whole
        // request loudly; the `None => continue` arm in the inner match below
        // is unreachable BECAUSE this pre-check ran. Future contributor: do NOT
        // delete this pre-check on the reasoning that the inner match handles
        // non-string elements — silent-skip is the wrong behaviour per the
        // workspace "silent-drop is a data-integrity bug class" rule
        // (JMAP-wlip.1).
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
                .destroy_object::<TaskNotification>(caller, &account_id, &id)
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
                Err(other) => {
                    not_destroyed.insert(id_str, server_fail_value_from_backend(&other));
                }
            }
        }
    }

    finalize_set_response::<B, TaskNotification>(
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
// TaskNotification/query
// ---------------------------------------------------------------------------

/// Handle a `TaskNotification/query` method call (draft-tasks-06 §5.5).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`,
/// optional `filter`, optional `sort`, optional `position`, optional
/// `anchor`, optional `anchorOffset`, optional `limit`, optional
/// `calculateTotal`); the returned `Value` is the §5.5 `/query`
/// response shape (`accountId`, `queryState`, `canCalculateChanges`,
/// `position`, `ids`, optional `total`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_notification_query<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<TaskNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// TaskNotification/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `TaskNotification/queryChanges` method call (draft-tasks-06 §5.6).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_notification_query_changes<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<TaskNotification, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: TaskNotification/set create entries must produce `forbidden` in notCreated.
    /// No backend call is made for create (pure handler-layer enforcement).
    #[tokio::test]
    async fn set_create_returns_forbidden_not_created() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "id": "ignored",
                    "created": "2024-01-01T00:00:00Z",
                    "changedBy": { "@type": "Person", "name": "Alice" },
                    "type": "created",
                    "taskId": "t1"
                }
            }
        });
        let (resp, _) = handle_task_notification_set(&backend, &(), args)
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
        assert!(
            resp["created"].is_null(),
            "created must be null when all creates are forbidden: {resp}"
        );
    }

    /// Oracle: TaskNotification/set update entries must produce `forbidden` in notUpdated.
    #[tokio::test]
    async fn set_update_returns_forbidden_not_updated() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "update": {
                "notif1": { "comment": "new comment" }
            }
        });
        let (resp, _) = handle_task_notification_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        let not_updated = &resp["notUpdated"];
        assert!(
            not_updated.is_object(),
            "notUpdated must be present for update attempts: {resp}"
        );
        assert_eq!(not_updated["notif1"]["type"], "forbidden");
    }

    /// Oracle: destroy of a non-existent notification → notFound in notDestroyed.
    #[tokio::test]
    async fn set_destroy_nonexistent_returns_not_found() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": ["doesnotexist"]
        });
        let (resp, _) = handle_task_notification_set(&backend, &(), args)
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

    /// Oracle: destroy of existing notification → appears in destroyed list.
    #[tokio::test]
    async fn set_destroy_existing_notification_succeeds() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_notification("acc1", "notif1");

        let args = json!({
            "accountId": "acc1",
            "destroy": ["notif1"]
        });
        let (resp, _) = handle_task_notification_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        let destroyed = resp["destroyed"]
            .as_array()
            .expect("destroyed must be array");
        assert_eq!(destroyed.len(), 1);
        assert_eq!(destroyed[0], "notif1");
    }
}
