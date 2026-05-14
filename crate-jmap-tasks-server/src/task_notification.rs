//! TaskNotification/* method handlers (draft-tasks-06 §5).
//!
//! TaskNotifications are server-created immutable records. Clients may only
//! query and destroy them. Any attempt to create or update a TaskNotification
//! via `/set` MUST be rejected with `forbidden` at the handler layer — the
//! backend never sees create or update calls for this type.

use jmap_tasks_types::TaskNotification;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, SetError, SetErrorType, TasksBackend};
use crate::helpers::{extract_account_id, finalize_set_response, set_error_value, SetAccumulators};
use jmap_server::server_fail_from_backend;

// ---------------------------------------------------------------------------
// TaskNotification/get
// ---------------------------------------------------------------------------

/// Handle a `TaskNotification/get` method call (draft-tasks-06 §5.2).
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
/// **Destroy-only enforcement**: draft-tasks-06 §5.4 states that only
/// `destroy` is supported. Any entries in the `create` or `update` maps
/// receive an immediate `forbidden` SetError without touching the backend.
/// The `destroy` list is forwarded to the backend normally.
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
                    not_destroyed.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
                Err(other) => {
                    not_destroyed.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": other.to_string() }),
                    );
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
