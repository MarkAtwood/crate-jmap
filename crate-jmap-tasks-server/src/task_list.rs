//! TaskList/* method handlers (draft-tasks-06 §3).
//!
//! TaskList/set has a special `onDestroyRemoveTasks` argument: if false
//! (default) and the list contains tasks, the destroy is rejected with
//! `taskListHasTasks`. If true, the backend destroys the tasks along with
//! the list.

use jmap_tasks_types::TaskList;
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, TasksBackend};
use crate::helpers::{extract_account_id, finalize_set_response, set_error_value};

// ---------------------------------------------------------------------------
// TaskList/get
// ---------------------------------------------------------------------------

/// Handle a `TaskList/get` method call (draft-tasks-06 §3.5).
pub async fn handle_task_list_get<B: TasksBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<TaskList, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// TaskList/changes
// ---------------------------------------------------------------------------

/// Handle a `TaskList/changes` method call (draft-tasks-06 §3.6).
pub async fn handle_task_list_changes<B: TasksBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<TaskList, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// TaskList/set
// ---------------------------------------------------------------------------

/// Handle a `TaskList/set` method call (draft-tasks-06 §3.7).
///
/// The `onDestroyRemoveTasks` argument (default: `false`) controls whether
/// tasks in a task list are cascade-destroyed when the list is destroyed.
/// If `false` and the list has tasks, the destroy is rejected with a
/// `taskListHasTasks` SetError.
pub async fn handle_task_list_set<B: TasksBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    // Parse onDestroyRemoveTasks (default: false)
    let on_destroy_remove_tasks = args
        .get("onDestroyRemoveTasks")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let old_state = backend
        .get_state::<TaskList>(&account_id)
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
            let obj_with_id = match obj_val {
                Value::Object(mut m) => {
                    m.entry("id")
                        .or_insert_with(|| Value::String("placeholder".to_owned()));
                    Value::Object(m)
                }
                other => other,
            };

            let task_list: TaskList = match serde_json::from_value(obj_with_id) {
                Ok(tl) => tl,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            match backend
                .create_object::<TaskList>(&account_id, &create_id, task_list)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
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
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
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
                .update_object::<TaskList>(&account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
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

            // Check for tasks in the list if onDestroyRemoveTasks is false.
            if !on_destroy_remove_tasks && backend.task_list_has_tasks(&account_id, &id).await {
                not_destroyed.insert(id_str, json!({ "type": "taskListHasTasks" }));
                continue;
            }

            match backend.destroy_object::<TaskList>(&account_id, &id).await {
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

    finalize_set_response::<B, TaskList>(
        backend,
        &account_id,
        old_state,
        mutated,
        created,
        updated,
        destroyed_list,
        not_created,
        not_updated,
        not_destroyed,
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

    /// Oracle: TaskList/get with unknown accountId returns accountNotFound.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let result = handle_task_list_get(&backend, args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: TaskList/set destroy with onDestroyRemoveTasks=false and tasks
    /// in the list → taskListHasTasks error.
    #[tokio::test]
    async fn set_destroy_with_tasks_returns_task_list_has_tasks() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_task_list_with_task("acc1", "list1");

        let args = json!({
            "accountId": "acc1",
            "destroy": ["list1"],
            "onDestroyRemoveTasks": false
        });
        let (resp, _) = handle_task_list_set(&backend, args)
            .await
            .expect("must not return top-level error");

        assert_eq!(
            resp["notDestroyed"]["list1"]["type"], "taskListHasTasks",
            "must return taskListHasTasks: {resp}"
        );
        assert!(
            resp["destroyed"].is_null(),
            "destroyed must be null: {resp}"
        );
    }

    /// Oracle: TaskList/set destroy with onDestroyRemoveTasks=true ignores
    /// task list contents and destroys it.
    #[tokio::test]
    async fn set_destroy_with_on_destroy_remove_tasks_true_succeeds() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_task_list_with_task("acc1", "list1");

        let args = json!({
            "accountId": "acc1",
            "destroy": ["list1"],
            "onDestroyRemoveTasks": true
        });
        let (resp, _) = handle_task_list_set(&backend, args)
            .await
            .expect("must not return top-level error");

        // With onDestroyRemoveTasks=true the list should be destroyed
        // (mock backend's destroy_object will find it and remove it)
        let destroyed = resp["destroyed"].as_array();
        assert!(
            destroyed.is_some(),
            "destroyed array must be present: {resp}"
        );
    }

    /// Oracle: TaskList/set with no operations returns empty /set response.
    #[tokio::test]
    async fn set_empty_returns_valid_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "destroy": [] });
        let (resp, _) = handle_task_list_set(&backend, args)
            .await
            .expect("must not return top-level error");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["destroyed"].is_null());
    }
}
