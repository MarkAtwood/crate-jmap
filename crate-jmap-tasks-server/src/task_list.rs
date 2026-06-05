//! TaskList/* method handlers (draft-tasks-06 §3).
//!
//! TaskList/set has a special `onDestroyRemoveTasks` argument: if false
//! (default) and the list contains tasks, the destroy is rejected with
//! `taskListHasTask`. If true, the backend destroys the tasks along with
//! the list.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3), with the
//! type-specific arguments defined by draft-tasks-06 §3. The returned
//! `Value` is the corresponding method-response object per the same
//! section refs.
//!
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `cannotCalculateChanges` — per RFC 8620 §3.6 and §5). Per-target
//! failures inside `/set` surface in the `notCreated` / `notUpdated` /
//! `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use jmap_tasks_types::TaskList;
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, TasksBackend};
use crate::helpers::{
    enforce_max_objects_in_set, extract_account_id, finalize_set_response, set_error_value,
    SetAccumulators,
};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend};

// ---------------------------------------------------------------------------
// TaskList/get
// ---------------------------------------------------------------------------

/// Handle a `TaskList/get` method call (draft-tasks-06 §3.5).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_list_get<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<TaskList, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// TaskList/changes
// ---------------------------------------------------------------------------

/// Handle a `TaskList/changes` method call (draft-tasks-06 §3.6).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the §5.2
/// `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_list_changes<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<TaskList, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// TaskList/set
// ---------------------------------------------------------------------------

/// Handle a `TaskList/set` method call (draft-tasks-06 §3.7).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps) plus the
/// draft-tasks-06 §3.7 `onDestroyRemoveTasks` extension argument
/// (default: `false`); the returned `Value` is the §5.3 `/set` response
/// shape (`accountId`, `oldState`, `newState`, plus the per-operation
/// result maps).
///
/// The `onDestroyRemoveTasks` argument (default: `false`) controls whether
/// tasks in a task list are cascade-destroyed when the list is destroyed.
/// If `false` and the list has tasks, the destroy is rejected with a
/// `taskListHasTask` SetError.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_list_set<B: TasksBackend>(
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

    // Parse onDestroyRemoveTasks (default: false)
    let on_destroy_remove_tasks = args
        .get("onDestroyRemoveTasks")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let old_state = backend
        .get_state::<TaskList>(caller, &account_id)
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
            // null) is rejected with invalidProperties:["id"]. Fixed in
            // JMAP-n22t.
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
                .create_object::<TaskList>(caller, &account_id, &create_id, task_list)
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
                    not_created.insert(create_id, server_fail_value_from_backend(&e));
                }
                Err(other) => {
                    not_created.insert(create_id, server_fail_value_from_backend(&other));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            // Id::from: wire-boundary validation deferred to JMAP-k9va; backend rejects unknown IDs.
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
                .update_object::<TaskList>(caller, &account_id, &id, patch)
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
                    not_updated.insert(id_str, server_fail_value_from_backend(&e));
                }
                Err(other) => {
                    not_updated.insert(id_str, server_fail_value_from_backend(&other));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
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

            // Check for tasks in the list if onDestroyRemoveTasks is false.
            // The three-way result distinguishes 'definitely empty',
            // 'definitely has tasks', and 'transient backend failure'
            // (analogous to bd:JMAP-ic0j.4 for calendars).
            if !on_destroy_remove_tasks {
                match backend.task_list_has_tasks(caller, &account_id, &id).await {
                    Ok(true) => {
                        not_destroyed.insert(id_str, json!({ "type": "taskListHasTask" }));
                        continue;
                    }
                    Ok(false) => {
                        // proceed to destroy below
                    }
                    Err(e) => {
                        not_destroyed.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            match backend
                .destroy_object::<TaskList>(caller, &account_id, &id)
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

    finalize_set_response::<B, TaskList>(
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
        let result = handle_task_list_get(&backend, &(), args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: TaskList/set destroy with onDestroyRemoveTasks=false and tasks
    /// in the list → taskListHasTask error (draft-ietf-jmap-tasks-06 §3.4).
    #[tokio::test]
    async fn set_destroy_with_tasks_returns_task_list_has_task() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_task_list_with_task("acc1", "list1");

        let args = json!({
            "accountId": "acc1",
            "destroy": ["list1"],
            "onDestroyRemoveTasks": false
        });
        let (resp, _) = handle_task_list_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        assert_eq!(
            resp["notDestroyed"]["list1"]["type"], "taskListHasTask",
            "must return taskListHasTask: {resp}"
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
        let (resp, _) = handle_task_list_set(&backend, &(), args)
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
        let (resp, _) = handle_task_list_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["destroyed"].is_null());
    }
}
