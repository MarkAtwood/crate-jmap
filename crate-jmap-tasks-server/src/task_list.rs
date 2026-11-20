//! TaskList/* method handlers (draft-tasks-06 §3).
//!
//! TaskList/set has a special `onDestroyRemoveTasks` argument: if false
//! (default) and the list contains tasks, the destroy is rejected with
//! `taskListHasTask`. If true, the handler destroys every Task in the list
//! through `destroy_object::<Task>` and then destroys the list; the backend
//! has no `onDestroyRemoveTasks` concept.
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
//! - `/changes`: `cannotCalculateChanges`.
//!
//! Per-target failures inside `/set` surface in the `notCreated` / `notUpdated`
//! / `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use jmap_tasks_types::{Task, TaskFilterCondition, TaskList};
use jmap_types::{Filter, Id, JmapError, PatchObject};
use serde_json::{Value, json};

use crate::TasksBackend;
use crate::helpers::{
    SetAccumulators, check_map_key_ids, check_metadata_entry, enforce_max_objects_in_set,
    extract_account_id, finalize_set_response,
};
use jmap_server::{
    HandlerOutput, QueryParams, backend_set_error_value, bool_arg, check_read_only_patch,
    reject_server_set, server_fail_from_backend, server_fail_value_from_backend,
};

// ---------------------------------------------------------------------------
// TaskList/get
// ---------------------------------------------------------------------------

/// Handle a `TaskList/get` method call (draft-tasks-06 §3.5).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]). When the backend lists TaskList for
/// draft-ietf-jmap-metadata-05 ([`JmapAccounts::metadata_info`](jmap_server::JmapAccounts::metadata_info)),
/// `metadata` is returned as an object, by default only with
/// `urn:ietf:params:jmap:metadata` in `using`, and with that capability
/// `properties` may hold `metadata/<namespace>` subselectors (-05 §2,
/// §3.1); see [`jmap_server::handle_get_with_metadata`].
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_list_get<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_get_with_metadata::<TaskList, B>(backend, caller, args, using).await
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
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); with
/// `urn:ietf:params:jmap:metadata` the request may set
/// draft-ietf-jmap-metadata-05 §3.3 `ignoreMetadataOnlyChanges`, and
/// without it metadata-only changes are left out (-05 §3).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_list_changes<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<TaskList, B>(backend, caller, args, using).await
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
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]): a write to `metadata` or
/// `privateMetadata` without `urn:ietf:params:jmap:metadata` is
/// `invalidProperties` (draft-ietf-jmap-metadata-05 §3), as is `null` for
/// either in a create (-05 §3.2). The backend validates the rest.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_list_set<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
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
    let on_destroy_remove_tasks = bool_arg(&args, "onDestroyRemoveTasks", false);

    let old_state = backend
        .get_state::<TaskList>(caller, &account_id)
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
        using,
        on_destroy_remove_tasks,
    };
    let mut acc = SetAccumulators::default();
    let mut mutated = set_create(&ctx, &mut args, &mut acc).await?;
    // RFC 8620 §5.3: update keys and destroy ids may name this call's creates.
    jmap_server::resolve_set_id_refs(&mut args, &std::collections::BTreeMap::new(), &acc.created);
    mutated |= set_update(&ctx, &mut args, &mut acc).await?;
    mutated |= set_destroy(&ctx, &mut args, &mut acc).await?;

    finalize_set_response::<B, TaskList>(backend, caller, &account_id, old_state, mutated, acc)
        .await
}

/// What every `TaskList/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: TasksBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
    /// The request's `using` capabilities.
    using: &'a [String],
    /// The draft-tasks-06 §3.7 `onDestroyRemoveTasks` argument.
    on_destroy_remove_tasks: bool,
}

/// The `create` part of `TaskList/set`. Returns whether the backend was
/// written.
async fn set_create<B: TasksBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj_val) in create_map {
            let mut task_list = match task_list_from_create(obj_val, ctx.using) {
                Ok(task_list) => task_list,
                Err(err) => {
                    acc.not_created.insert(create_id, err);
                    continue;
                }
            };

            let build = move |id: &Id| {
                task_list.id = id.clone();
                task_list
            };
            match ctx
                .backend
                .create_object::<TaskList, _>(ctx.caller, ctx.account_id, &create_id, build)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    acc.created
                        .insert(create_id, jmap_server::serialize_value(&created_obj)?);
                }
                Err(e) => {
                    acc.not_created
                        .insert(create_id, backend_set_error_value(e));
                }
            }
        }
    }
    Ok(mutated)
}

/// Check one `TaskList/set` create object, fill in the server-set and
/// defaulted properties, and parse it, or return the SetError value for
/// `notCreated`.
fn task_list_from_create(obj_val: Value, using: &[String]) -> Result<TaskList, Value> {
    if let Some(err) = obj_val.as_object().and_then(reject_server_set::<TaskList>) {
        return Err(serde_json::Value::from(&err));
    }
    if let Some(Err(err)) = obj_val
        .as_object()
        .map(|o| check_metadata_entry(o, true, using))
    {
        return Err(serde_json::Value::from(&*err));
    }
    // draft-ietf-jmap-tasks-06 §3 `shareWith`: Id[TaskRights], keyed
    // by Principal id.
    if let Some(Err(err)) = obj_val
        .as_object()
        .map(|o| check_map_key_ids(o, "shareWith"))
    {
        return Err(serde_json::Value::from(&*err));
    }
    // `TaskList.id` is required, so the parse fills it with
    // `jmap_server::CREATE_ID_PLACEHOLDER`; `build`
    // overwrites it with the id the backend mints.
    let obj_with_id = match obj_val {
        Value::Object(mut m) => {
            m.insert("id".to_owned(), jmap_server::CREATE_ID_PLACEHOLDER.into());
            // RFC 8620 §5.3: the server fills omitted properties that
            // have a default. draft-ietf-jmap-tasks-06 §3:
            // `isSubscribed` SHOULD default to true for any new task
            // list created by the user themselves.
            m.entry("isSubscribed").or_insert(Value::Bool(true));
            // `myRights` is server-set (draft-ietf-jmap-tasks-06 §3)
            // and TaskList requires it, so the server supplies it.
            //
            // ponytail: the creator holds every right, as the owner of
            // a task list in their own account; a backend with finer
            // sharing may replace `myRights` in `create_object`.
            m.insert(
                "myRights".to_owned(),
                json!({
                    "mayReadItems": true,
                    "mayWriteAll": true,
                    "mayWriteOwn": true,
                    "mayUpdatePrivate": true,
                    "mayRSVP": true,
                    "mayAdmin": true,
                    "mayDelete": true,
                }),
            );
            Value::Object(m)
        }
        other => other,
    };

    // RFC 8620 §5.3: a create entry that is not an object has no
    // properties to name, so report it without a `properties` list.
    if !obj_with_id.is_object() {
        return Err(json!({
            "type": "invalidProperties",
            "description": "a create entry must be an object",
        }));
    }

    // draft-ietf-jmap-tasks-06 §3: `name` is required. serde reports a missing
    // field at the root, not on a property path, so name it here
    // (RFC 8620 §5.3 invalidProperties).
    if obj_with_id.get("name").is_none() {
        return Err(json!({
            "type": "invalidProperties",
            "properties": ["name"],
            "description": "name is required on create",
        }));
    }

    serde_path_to_error::deserialize(obj_with_id).map_err(|e| {
        // RFC 8620 §5.3: invalidProperties SHOULD list the
        // invalid properties; the first path segment is the
        // top-level property whose value failed to parse.
        let mut err = json!({ "type": "invalidProperties", "description": e.to_string() });
        if let Some(serde_path_to_error::Segment::Map { key }) = e.path().iter().next() {
            err["properties"] = json!([key]);
        }
        err
    })
}

/// The `update` part of `TaskList/set`. Returns whether the backend was
/// written.
async fn set_update<B: TasksBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    acc.not_updated
                        .insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let mut patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    acc.not_updated.insert(
                        id_str,
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };
            // draft-ietf-jmap-tasks-06 §3: `id` and `myRights` are server-set. A changed value is rejected, an identical
            // one dropped (RFC 8620 §5.3); nothing left is a no-op success.
            match check_read_only_patch::<_, TaskList>(
                ctx.backend,
                ctx.caller,
                ctx.account_id,
                &id,
                patch.as_map_mut(),
            )
            .await
            {
                Ok(false) => {}
                Ok(true) => {
                    acc.updated.insert(id_str, Value::Null);
                    continue;
                }
                Err(err) => {
                    acc.not_updated.insert(id_str, err);
                    continue;
                }
            }
            if let Err(err) = check_metadata_entry(patch.as_map(), false, ctx.using) {
                acc.not_updated
                    .insert(id_str, serde_json::Value::from(&*err));
                continue;
            }
            if let Err(err) = check_map_key_ids(patch.as_map(), "shareWith") {
                acc.not_updated
                    .insert(id_str, serde_json::Value::from(&*err));
                continue;
            }

            match ctx
                .backend
                .update_object::<TaskList>(ctx.caller, ctx.account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    acc.updated
                        .insert(id_str, jmap_server::serialize_value(&obj)?);
                }
                Ok(None) => {
                    mutated = true;
                    acc.updated.insert(id_str, Value::Null);
                }
                Err(e) => {
                    acc.not_updated.insert(id_str, backend_set_error_value(e));
                }
            }
        }
    }
    Ok(mutated)
}

/// The `destroy` part of `TaskList/set`, with the draft-tasks-06 §3.7
/// `onDestroyRemoveTasks` check or cascade. Returns whether the backend
/// was written.
///
/// # Errors
///
/// `invalidArguments` when an element of `destroy` is not a string.
async fn set_destroy<B: TasksBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Two-pass validation is intentional: this up-front loop fails the whole
        // request loudly; the `None => continue` arm in the inner match below
        // is unreachable BECAUSE this pre-check ran. Future contributor: do NOT
        // delete this pre-check on the reasoning that the inner match handles
        // non-string elements — silent-skip is the wrong behaviour per the
        // workspace "silent-drop is a data-integrity bug class" rule
        // (JMAP-wlip.1).
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

            // Check for tasks in the list if onDestroyRemoveTasks is false.
            // The three-way result distinguishes 'definitely empty',
            // 'definitely has tasks', and 'transient backend failure'
            // (analogous to bd:JMAP-ic0j.4 for calendars).
            if !ctx.on_destroy_remove_tasks {
                match ctx
                    .backend
                    .task_list_has_tasks(ctx.caller, ctx.account_id, &id)
                    .await
                {
                    Ok(true) => {
                        acc.not_destroyed
                            .insert(id_str, json!({ "type": "taskListHasTask" }));
                        continue;
                    }
                    Ok(false) => {
                        // proceed to destroy below
                    }
                    Err(e) => {
                        acc.not_destroyed
                            .insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            // draft-tasks-06 §3.4: "If true, any Tasks that were in the
            // TaskList will be removed from it, and they will be destroyed."
            // A failed cascade leaves the list in place rather than
            // orphaning the Tasks that remain.
            if ctx.on_destroy_remove_tasks
                && let Err(value) =
                    destroy_task_list_tasks(ctx.backend, ctx.caller, ctx.account_id, &id).await
            {
                acc.not_destroyed.insert(id_str, value);
                continue;
            }

            match ctx
                .backend
                .destroy_object::<TaskList>(ctx.caller, ctx.account_id, &id)
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

/// Destroy every Task whose `taskListId` is `task_list_id`
/// (draft-tasks-06 §3.4 `onDestroyRemoveTasks: true`).
///
/// A Task belongs to exactly one TaskList, so each one is destroyed
/// outright; there is no membership to patch out as in the Calendar and
/// Mailbox cascades.
///
/// # Errors
///
/// Returns the redacted wire-format SetError [`Value`] for the TaskList's
/// `notDestroyed` entry on the first failed query, fetch or destroy. A
/// typed [`BackendSetError::SetError`] keeps its JMAP type; every other
/// failure becomes `serverFail`.
async fn destroy_task_list_tasks<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    task_list_id: &Id,
) -> Result<(), Value> {
    let mut condition = TaskFilterCondition::default();
    condition.in_task_lists = Some(vec![task_list_id.clone()]);
    let filter = Filter::Condition(condition);
    let ids = backend
        .query_objects::<Task>(caller, account_id, QueryParams::new().filter(Some(&filter)))
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
        .ids;
    if ids.is_empty() {
        return Ok(());
    }

    // Re-check membership on the fetched Tasks so a backend whose filter
    // over-matches cannot destroy Tasks belonging to another list.
    let (tasks, _not_found): (Vec<Task>, _) = backend
        .get_objects::<Task>(caller, account_id, Some(&ids), None)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    for task in tasks {
        if task.task_list_id.as_ref() != Some(task_list_id) {
            continue;
        }
        let Some(task_id) = task.id.as_ref() else {
            return Err(server_fail_value_from_backend(&"missing task id"));
        };
        backend
            .destroy_object::<Task>(caller, account_id, task_id)
            .await
            .map_err(backend_set_error_value)?;
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

    /// Oracle: TaskList/get with unknown accountId returns accountNotFound.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let result = handle_task_list_get(&backend, &(), args, &[]).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.to_string(), "accountNotFound");
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
        let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), args, &[])
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
        let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), args, &[])
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
        let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), args, &[])
            .await
            .expect("must not return top-level error");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["destroyed"].is_null());
    }

    /// Oracle: RFC 8620 §5.3: for `invalidProperties` "the SetError object
    /// SHOULD also have a property called `properties` of type `String[]`
    /// that lists *all* the properties that were invalid". Each create below
    /// has exactly one hand-picked bad property (`name` is a required String, draft-ietf-jmap-tasks-06 §3).
    #[tokio::test]
    async fn set_create_typed_parse_failure_names_property() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "missingName": {},
                "nullName": { "name": null },
                "badSortOrder": { "name": "l", "sortOrder": "x" }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), args, &[])
            .await
            .expect("must not return top-level error");
        assert!(resp["created"].is_null(), "nothing may be created: {resp}");
        for (cid, prop) in [
            ("missingName", "name"),
            ("nullName", "name"),
            ("badSortOrder", "sortOrder"),
        ] {
            let err = &resp["notCreated"][cid];
            assert_eq!(err["type"], "invalidProperties", "{cid}: {resp}");
            assert_eq!(err["properties"], json!([prop]), "{cid}: {resp}");
        }
    }

    /// Oracle: RFC 8620 §5.3: `create` is "A map of a creation id ... to a
    /// Foo object"; an entry that is not a JSON object is invalid. It has
    /// no properties, so the SetError must not name one it does not have
    /// (hand-written non-object entries below).
    #[tokio::test]
    async fn set_create_non_object_entry_names_no_property() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": { "str": "x", "arr": ["x"], "nul": null, "num": 5 }
        });
        let HandlerOutput { primary: resp, .. } = handle_task_list_set(&backend, &(), args, &[])
            .await
            .expect("must not return top-level error");
        assert!(resp["created"].is_null(), "nothing may be created: {resp}");
        for cid in ["str", "arr", "nul", "num"] {
            let err = &resp["notCreated"][cid];
            assert_eq!(err["type"], "invalidProperties", "{cid}: {resp}");
            assert!(err.get("properties").is_none(), "{cid}: {resp}");
        }
    }
}
