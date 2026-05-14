//! Task/* method handlers (draft-tasks-06 §4).
//!
//! Task/set enforces the `isDraft` immutability constraint: once set to false,
//! it cannot be set back to true.

use jmap_tasks_types::Task;
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::backend::{BackendSetError, TasksBackend};
use crate::helpers::{extract_account_id, finalize_set_response, set_error_value, SetAccumulators};
use jmap_server::server_fail_from_backend;

// ---------------------------------------------------------------------------
// Task/get
// ---------------------------------------------------------------------------

/// Handle a `Task/get` method call (draft-tasks-06 §4.5).
///
/// If `"utcStart"` or `"utcDue"` appear in the requested `properties` (or if
/// `properties` is `null` — meaning all fields), [`TasksBackend::compute_task_utc_times`]
/// is called for each returned task and the computed values are merged in
/// (draft-tasks-06 §4, lines 739-772).
pub async fn handle_task_get<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    // Determine whether utcStart / utcDue are requested.
    let want_utc = match args.get("properties") {
        None | Some(Value::Null) => true, // all properties requested
        Some(Value::Array(props)) => props.iter().any(|p| {
            p.as_str()
                .map(|s| s == "utcStart" || s == "utcDue")
                .unwrap_or(false)
        }),
        _ => false,
    };

    // Delegate to the generic get handler.
    let (mut response, tail) =
        jmap_server::handlers::handle_get::<Task, B>(backend, caller, args).await?;

    // If utcStart or utcDue were requested, augment each returned task.
    if want_utc {
        if let Some(Value::Array(list)) = response.get_mut("list") {
            for item in list.iter_mut() {
                if let Ok(task) = Task::deserialize(&*item) {
                    let (utc_start, utc_due) = backend.compute_task_utc_times(&task, None);
                    if let Some(s) = utc_start {
                        item["utcStart"] = Value::String(s.into_inner());
                    }
                    if let Some(d) = utc_due {
                        item["utcDue"] = Value::String(d.into_inner());
                    }
                }
            }
        }
    }

    Ok((response, tail))
}

// ---------------------------------------------------------------------------
// Task/changes
// ---------------------------------------------------------------------------

/// Handle a `Task/changes` method call (draft-tasks-06 §4.6).
pub async fn handle_task_changes<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Task, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Task/set
// ---------------------------------------------------------------------------

/// Handle a `Task/set` method call (draft-tasks-06 §4.7).
///
/// **isDraft immutability**: If a patch contains `"isDraft": true` and the
/// current stored object has `isDraft = false`, the update is rejected with
/// `invalidProperties`. The current object must be fetched from the backend
/// to check this.
///
/// Note: The backend returns the current object via `get_objects` for the
/// isDraft check. This is not done here to avoid coupling to the stored state;
/// instead, backends are expected to enforce this constraint and return
/// `invalidProperties` from `update_object` when the patch violates it.
/// The handler also does a best-effort check on the patch itself.
pub async fn handle_task_set<B: TasksBackend>(
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
        .get_state::<Task>(caller, &account_id)
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
            // JMAP-n22t. (Task/copy is a separate handler and accepts an
            // id from the source account; this rejection applies to /set
            // only.)
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

            let task: Task = match serde_json::from_value(obj_with_id) {
                Ok(t) => t,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            match backend
                .create_object::<Task>(caller, &account_id, &create_id, task)
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
            // isDraft immutability check (draft-tasks-06 §4, lines 733-737):
            // once set to false, isDraft MUST NOT be updated back to true.
            // We enforce this at the handler layer by fetching the current task.
            //
            // Fast-path: if the backend reports that it enforces this invariant
            // atomically in update_object (via enforce_is_draft_atomically()),
            // skip the pre-fetch get_objects call — the backend will return
            // InvalidProperties directly.  This saves one backend round-trip per
            // update that sets isDraft:true when the backend self-enforces.
            if patch_val.get("isDraft").and_then(|v| v.as_bool()) == Some(true)
                && !backend.enforce_is_draft_atomically()
            {
                let task_id = Id::from(id_str.as_str());
                match backend
                    .get_objects::<Task>(caller, &account_id, Some(&[task_id]), None)
                    .await
                {
                    Ok((tasks, _)) => {
                        if tasks.first().and_then(|t| t.is_draft) == Some(false) {
                            // Task is already published; reverting to draft is forbidden.
                            not_updated.insert(
                                id_str,
                                json!({
                                    "type": "invalidProperties",
                                    "properties": ["isDraft"]
                                }),
                            );
                            continue;
                        }
                        // is_draft == Some(true) or None (no info): allow the
                        // patch through and let the backend handle it.
                    }
                    Err(e) => {
                        not_updated.insert(
                            id_str,
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                        continue;
                    }
                }
            }

            let id = Id::from(id_str.as_str());

            // Route to the per-user update path when every patch key targets
            // a per-user Task property (draft-tasks-06 §4.5.1).
            //
            // PatchObject keys are RFC 6901 JSON Pointers (RFC 8620 §5.3), so a
            // per-user keyword toggle is sent as `keywords/done`, not bare
            // `keywords`. Classify on the FIRST pointer segment (the substring
            // before the first '/'), not the whole key.
            //
            // The null short-circuit that was here previously (`v.is_null() ||
            // ...`) misclassified a delete-shared-property patch like
            // `{"title": null}` as per-user-only, and an empty patch object as
            // per-user-only via vacuous-truth on `iter().all`. Both go to
            // `update_object` now: deletions are shared mutations, and an
            // empty patch is a no-op that should not bump per-user state.
            //
            // Per-user property names (`keywords`, `color`, `freeBusyStatus`,
            // `useDefaultAlerts`, `alerts`) contain no `/` or `~`, so a plain
            // `split('/').next()` is correct without RFC 6901 unescaping.
            let is_per_user_only = patch_val
                .as_object()
                .map(|m| {
                    !m.is_empty()
                        && m.keys().all(|k| {
                            let head = k.split('/').next().unwrap_or(k);
                            B::is_per_user_property(head)
                        })
                })
                .unwrap_or(false);

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError. Conversion runs
            // after the isDraft and per-user-routing introspection above so
            // those checks operate on the raw wire shape.
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

            let update_result = if is_per_user_only {
                backend
                    .update_task_per_user(caller, &account_id, &id, patch)
                    .await
            } else {
                backend
                    .update_object::<Task>(caller, &account_id, &id, patch)
                    .await
            };

            match update_result {
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

            match backend
                .destroy_object::<Task>(caller, &account_id, &id)
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

    finalize_set_response::<B, Task>(
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
// Task/copy
// ---------------------------------------------------------------------------

/// Handle a `Task/copy` method call (draft-tasks-06 §4.8).
///
/// Copies tasks from `fromAccountId` into the current account. The `create`
/// map keys are client-side creation ids; the backend assigns new server-side
/// ids.
pub async fn handle_task_copy<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (to_account_id, mut args) = extract_account_id(args)?;

    if !backend
        .account_exists(caller, &to_account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let from_account_id_str = args
        .remove("fromAccountId")
        .and_then(|v| v.as_str().map(|s| s.to_owned()))
        .ok_or_else(|| JmapError::invalid_arguments("fromAccountId is required"))?;
    let from_account_id = Id::from(from_account_id_str.as_str());

    // RFC 8620 §5.4: fromAccountId MUST differ from accountId. The canonical
    // sibling jmap-mail-server enforces this in handle_email_copy; without it
    // a same-account "copy" silently produces a duplicate within the source
    // account, contradicting the meaning of /copy.
    if from_account_id == to_account_id {
        return Err(JmapError::invalid_arguments(
            "fromAccountId must be different from accountId",
        ));
    }

    // RFC 8620 §5.4: validate fromAccountId resolves to a real account;
    // emit the spec-canonical `fromAccountNotFound` (not the generic
    // `accountNotFound`) when it does not.
    if !backend
        .account_exists(caller, &from_account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::from_account_not_found());
    }

    // RFC 8620 §5.4 `ifFromInState`: if supplied, the source-account Task
    // state MUST match; otherwise abort with stateMismatch. Probed before
    // the destination state so a stale source guard fails fast.
    if let Some(if_from_in_state) = args.get("ifFromInState").and_then(|v| v.as_str()) {
        let from_state = backend
            .get_state::<Task>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        if if_from_in_state != from_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let old_state = backend
        .get_state::<Task>(caller, &to_account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // RFC 8620 §5.4 `ifInState`: if supplied, the destination-account Task
    // state MUST match; otherwise abort with stateMismatch.
    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut mutated = false;

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

            let task: Task = match serde_json::from_value(obj_with_id) {
                Ok(t) => t,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            match backend
                .copy_task(caller, &from_account_id, &to_account_id, task)
                .await
            {
                Ok((_new_id, copied_task)) => {
                    mutated = true;
                    created.insert(
                        create_id,
                        serde_json::to_value(&copied_task)
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

    let new_state = if mutated {
        backend
            .get_state::<Task>(caller, &to_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
    } else {
        old_state.clone()
    };

    Ok((
        json!({
            "fromAccountId": from_account_id.as_ref(),
            "accountId": to_account_id.as_ref(),
            "oldState": old_state.as_ref(),
            "newState": new_state.as_ref(),
            "created":    if created.is_empty()     { Value::Null } else { Value::Object(created) },
            "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// Task/query
// ---------------------------------------------------------------------------

/// Handle a `Task/query` method call (draft-tasks-06 §4.13).
pub async fn handle_task_query<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<Task, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Task/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Task/queryChanges` method call (draft-tasks-06 §4.14).
pub async fn handle_task_query_changes<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<Task, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: Task/get with unknown accountId returns accountNotFound.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let result = handle_task_get(&backend, &(), args).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: Task/copy missing fromAccountId returns invalidArguments.
    #[tokio::test]
    async fn copy_missing_from_account_id_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {}
        });
        let result = handle_task_copy(&backend, &(), args).await;
        let err = result.expect_err("must return error when fromAccountId missing");
        assert_eq!(err.error_type.as_str(), "invalidArguments");
    }

    /// Oracle: RFC 8620 §5.4 — fromAccountId MUST be different from accountId.
    /// A same-account request must be rejected with invalidArguments before
    /// any per-create work. Matches the canonical sibling jmap-mail-server
    /// at email.rs:2263-2268.
    #[tokio::test]
    async fn copy_same_account_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "acc1",
            "create": {}
        });
        let result = handle_task_copy(&backend, &(), args).await;
        let err = result.expect_err("must reject same-account Task/copy per RFC 8620 §5.4");
        assert_eq!(err.error_type.as_str(), "invalidArguments");
    }

    /// Oracle: RFC 8620 §5.4 — `fromAccountId` that does not resolve to a
    /// real account returns the spec-canonical `fromAccountNotFound` error,
    /// not the generic `accountNotFound`.
    #[tokio::test]
    async fn copy_unknown_from_account_returns_from_account_not_found() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "no-such-account",
            "create": {}
        });
        let result = handle_task_copy(&backend, &(), args).await;
        let err = result.expect_err("must reject unknown fromAccountId");
        assert_eq!(err.error_type.as_str(), "fromAccountNotFound");
    }

    /// Oracle: RFC 8620 §5.4 — `ifFromInState` mismatch on the source
    /// account returns `stateMismatch`.
    #[tokio::test]
    async fn copy_if_from_in_state_mismatch_returns_state_mismatch() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "acc2",
            "ifFromInState": "stale-source-state",
            "create": {}
        });
        let result = handle_task_copy(&backend, &(), args).await;
        let err = result.expect_err("must reject stale ifFromInState");
        assert_eq!(err.error_type.as_str(), "stateMismatch");
    }

    /// Oracle: RFC 8620 §5.4 — `ifInState` mismatch on the destination
    /// account returns `stateMismatch`.
    #[tokio::test]
    async fn copy_if_in_state_mismatch_returns_state_mismatch() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "acc2",
            "ifInState": "stale-destination-state",
            "create": {}
        });
        let result = handle_task_copy(&backend, &(), args).await;
        let err = result.expect_err("must reject stale ifInState");
        assert_eq!(err.error_type.as_str(), "stateMismatch");
    }

    /// Oracle: Task/set empty destroy returns valid response.
    #[tokio::test]
    async fn set_empty_destroy_returns_valid_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "destroy": [] });
        let (resp, _) = handle_task_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert_eq!(resp["accountId"], "acc1");
    }

    // ── isDraft immutability enforcement (draft-tasks-06 §4, lines 733-737) ─

    /// Oracle: draft-tasks-06 §4 — "Once set to false, the value [isDraft]
    /// cannot be updated to true."
    /// A patch of {"isDraft": true} on a task that already has isDraft=false
    /// MUST be rejected at the handler level with invalidProperties.
    #[tokio::test]
    async fn set_is_draft_revert_rejected_when_current_is_false() {
        let mut backend = MockBackend::new_with_account("acc1");
        // Pre-seed a published (isDraft=false) task.
        backend.seed_task("acc1", "t1", false);

        let args = json!({
            "accountId": "acc1",
            "update": {
                "t1": { "isDraft": true }
            }
        });
        let (resp, _) = handle_task_set(&backend, &(), args)
            .await
            .expect("must not return a top-level error");

        let not_updated = resp["notUpdated"]
            .as_object()
            .expect("notUpdated must be an object");
        assert!(
            not_updated.contains_key("t1"),
            "t1 must appear in notUpdated: {resp}"
        );
        assert_eq!(
            not_updated["t1"]["type"].as_str(),
            Some("invalidProperties"),
            "error type must be invalidProperties"
        );
        assert_eq!(
            not_updated["t1"]["properties"][0].as_str(),
            Some("isDraft"),
            "properties must list isDraft"
        );
    }

    /// Oracle: draft-tasks-06 §4 — patching {"isDraft": true} on a task that
    /// is already a draft (isDraft=true) is allowed (no state transition).
    #[tokio::test]
    async fn set_is_draft_true_on_draft_task_passes_to_backend() {
        let mut backend = MockBackend::new_with_account("acc1");
        // Pre-seed a draft task (isDraft=true).
        backend.seed_task("acc1", "t2", true);

        let args = json!({
            "accountId": "acc1",
            "update": {
                "t2": { "isDraft": true }
            }
        });
        let (resp, _) = handle_task_set(&backend, &(), args)
            .await
            .expect("must not return a top-level error");

        // The handler passes the patch to the backend (which returns Forbidden
        // in the mock). The important thing is it is NOT pre-rejected here —
        // it must NOT appear in notUpdated with type=="invalidProperties".
        if let Some(not_updated) = resp["notUpdated"].as_object() {
            if let Some(err) = not_updated.get("t2") {
                assert_ne!(
                    err["type"].as_str(),
                    Some("invalidProperties"),
                    "isDraft:true on an existing draft must not produce invalidProperties"
                );
            }
        }
    }

    /// Oracle: draft-tasks-06 §4 — patching {"isDraft": false} is always allowed
    /// (moving from draft to published is a one-way door going the right way).
    #[tokio::test]
    async fn set_is_draft_false_passes_to_backend() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.seed_task("acc1", "t3", true);

        let args = json!({
            "accountId": "acc1",
            "update": {
                "t3": { "isDraft": false }
            }
        });
        let (resp, _) = handle_task_set(&backend, &(), args)
            .await
            .expect("must not return a top-level error");

        // Like above: the patch must NOT be pre-rejected with invalidProperties.
        if let Some(not_updated) = resp["notUpdated"].as_object() {
            if let Some(err) = not_updated.get("t3") {
                assert_ne!(
                    err["type"].as_str(),
                    Some("invalidProperties"),
                    "isDraft:false must never produce invalidProperties"
                );
            }
        }
    }

    // ── compute_task_utc_times / utcStart wiring (draft-tasks-06 §4 lines 739-772) ─

    /// Oracle: draft-tasks-06 §4 — utcStart is not returned unless explicitly
    /// requested in `properties`.  The default impl returns None so it must be
    /// absent from the response when not requested.
    #[tokio::test]
    async fn get_without_utc_properties_omits_utc_fields() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "ids": null,
            "properties": ["id", "title"]  // utcStart and utcDue NOT listed
        });
        let (resp, _) = handle_task_get(&backend, &(), args)
            .await
            .expect("must succeed");
        // The list is empty (no tasks seeded), but the response itself must be valid.
        assert_eq!(resp["accountId"], "acc1");
        // No utcStart or utcDue should appear in response items (list is empty so
        // this is vacuously satisfied, but the path is exercised without error).
        let list = resp["list"].as_array().expect("list must be an array");
        for item in list {
            assert!(
                item.get("utcStart").is_none(),
                "utcStart must not appear when not in properties"
            );
        }
    }

    /// Oracle: draft-tasks-06 §4 — when utcStart is in properties, the handler
    /// calls compute_task_utc_times. The default impl returns (None, None) so
    /// no utcStart key is injected, but no error is raised either.
    #[tokio::test]
    async fn get_with_utc_start_in_properties_does_not_error() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "ids": null,
            "properties": ["id", "utcStart", "utcDue"]
        });
        let (resp, _) = handle_task_get(&backend, &(), args)
            .await
            .expect("must succeed with utcStart in properties");
        assert_eq!(resp["accountId"], "acc1");
    }

    // ── per-user property routing (draft-tasks-06 §4.5.1) ──────────────────
    //
    // Routing is observable via MockBackend::per_user_calls — a counter that
    // increments inside `update_task_per_user` and stays at 0 for paths that
    // go to `update_object`. The earlier shape of these tests only asserted
    // that no `invalidProperties` error came back, which would pass even if
    // routing were inverted; tightened here to inspect the counter directly.

    /// Oracle: draft-tasks-06 §4.5.1 — a patch with only per-user properties
    /// (bare keys, no JSON Pointer sub-paths) must route to update_task_per_user.
    #[tokio::test]
    async fn set_per_user_only_patch_routes_to_per_user_update() {
        use std::sync::atomic::Ordering;
        let backend = MockBackend::new_with_account("acc1");
        // color is a per-user property (§4.5.1).
        let args = json!({
            "accountId": "acc1",
            "update": { "t1": { "color": "#ff0000" } }
        });
        let _ = handle_task_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert_eq!(
            backend.per_user_calls.load(Ordering::Relaxed),
            1,
            "per-user-only patch must route to update_task_per_user"
        );
    }

    /// Oracle: draft-tasks-06 §4.5.1 + RFC 8620 §5.3 — JSON-pointer patches on
    /// per-user properties (e.g. `keywords/done: true` to toggle a single
    /// keyword, `alerts/0/offset: null` to clear an alert sub-field) must still
    /// route to update_task_per_user. The first pointer segment is the
    /// property name; the implementation MUST classify on that segment, not
    /// on the whole key.
    #[tokio::test]
    async fn set_per_user_json_pointer_patch_routes_to_per_user_update() {
        use std::sync::atomic::Ordering;
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "update": {
                "t1": {
                    "keywords/done": true,
                    "alerts/0/offset": null
                }
            }
        });
        let _ = handle_task_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert_eq!(
            backend.per_user_calls.load(Ordering::Relaxed),
            1,
            "JSON-pointer patch on per-user properties must route to update_task_per_user"
        );
    }

    /// Oracle: draft-tasks-06 §4.5.1 — a patch on a shared property (title)
    /// must route to update_object.
    #[tokio::test]
    async fn set_shared_property_patch_routes_to_update_object() {
        use std::sync::atomic::Ordering;
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "update": { "t1": { "title": "New title" } }
        });
        let _ = handle_task_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert_eq!(
            backend.per_user_calls.load(Ordering::Relaxed),
            0,
            "shared-property patch must NOT route to update_task_per_user"
        );
    }

    /// Oracle: RFC 7396 / RFC 8620 §5.3 — `{"title": null}` is a delete-shared-
    /// property mutation, NOT a per-user mutation. The previous routing
    /// short-circuited on null values, misclassifying this as per-user-only.
    #[tokio::test]
    async fn set_shared_property_null_patch_routes_to_update_object() {
        use std::sync::atomic::Ordering;
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "update": { "t1": { "title": null } }
        });
        let _ = handle_task_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert_eq!(
            backend.per_user_calls.load(Ordering::Relaxed),
            0,
            "delete-shared-property patch must NOT route to update_task_per_user"
        );
    }

    /// Oracle: draft-tasks-06 §4.5.1 — a mixed patch (per-user + shared)
    /// routes to update_object, never update_task_per_user.
    #[tokio::test]
    async fn set_mixed_patch_routes_to_update_object() {
        use std::sync::atomic::Ordering;
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "update": { "t1": { "color": "#ff0000", "title": "New" } }
        });
        let _ = handle_task_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert_eq!(
            backend.per_user_calls.load(Ordering::Relaxed),
            0,
            "mixed patch must route to update_object"
        );
    }

    /// Oracle: RFC 8620 §5.3 — an empty PatchObject is a no-op, not a per-user
    /// mutation. The previous routing classified it as per-user-only via
    /// vacuous-truth on `iter().all`.
    #[tokio::test]
    async fn set_empty_patch_routes_to_update_object() {
        use std::sync::atomic::Ordering;
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "update": { "t1": {} }
        });
        let _ = handle_task_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert_eq!(
            backend.per_user_calls.load(Ordering::Relaxed),
            0,
            "empty patch must NOT route to update_task_per_user"
        );
    }

    // ── isDraft fast-path (enforce_is_draft_atomically) ──────────────────────

    /// Oracle: when enforce_is_draft_atomically() returns true, the handler
    /// skips the get_objects pre-fetch and delegates isDraft enforcement to
    /// update_object.  The backend here accepts the patch (no InvalidProperties),
    /// so the handler must not inject a pre-rejection.
    ///
    /// This test uses MockBackend which has enforce_is_draft_atomically() = false
    /// by default.  We verify the fast-path flag is wired correctly by confirming
    /// the existing behaviour is unchanged when the flag is false (pre-fetch runs).
    #[tokio::test]
    async fn set_is_draft_revert_rejected_with_prefetch() {
        // Verify the pre-fetch path still works (enforce_is_draft_atomically = false).
        let mut backend = MockBackend::new_with_account("acc1");
        backend.seed_task("acc1", "t1", false); // published task

        let args = json!({
            "accountId": "acc1",
            "update": { "t1": { "isDraft": true } }
        });
        let (resp, _) = handle_task_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        // Pre-fetch detected isDraft=false → rejects with invalidProperties.
        let not_updated = resp["notUpdated"].as_object().expect("notUpdated");
        assert_eq!(
            not_updated["t1"]["type"], "invalidProperties",
            "pre-fetch path must reject isDraft revert: {resp}"
        );
        assert_eq!(not_updated["t1"]["properties"][0], "isDraft");
    }
}
