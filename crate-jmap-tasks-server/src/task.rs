//! Task/* method handlers (draft-tasks-06 §4).
//!
//! The `isDraft` immutability constraint (once false, never true again) is
//! enforced by [`crate::TasksBackend::update_object`], not by Task/set.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3, `/copy` → §5.4,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by draft-tasks-06 §4. The returned `Value` is the
//! corresponding method-response object per the same section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3).
//! For the standard `/get`, `/changes`, `/set`, `/query`, and
//! `/queryChanges` handlers in this module the vector is **always
//! empty**. `handle_task_copy` is the one exception: it MAY emit
//! `Task/set` follow-up invocations when `onSuccessDestroyOriginal` is
//! true (RFC 8620 §5.4).
//!
//! Each handler returns `Err(JmapError)` for method-level failures (RFC 8620
//! §3.6.2 and §5):
//!
//! - every handler: `accountNotFound`, `invalidArguments`, `serverFail`;
//! - `/get`, `/set`: `requestTooLarge` past `maxObjectsInGet` /
//!   `maxObjectsInSet`;
//! - `/set`, `/copy`: `stateMismatch`;
//! - `/copy`: `fromAccountNotFound`;
//! - `/changes`, `/queryChanges`: `cannotCalculateChanges`;
//! - `/queryChanges`: `tooManyChanges`;
//! - `/query`, `/queryChanges`: `unsupportedFilter`;
//! - `/query`: `unsupportedSort`, `anchorNotFound`.
//!
//! Per-target failures inside a `/set` or `/copy` call surface in the
//! `notCreated` / `notUpdated` / `notDestroyed` maps within `Ok((Value, ...))`,
//! not as `Err`.

use std::collections::BTreeMap;

use jmap_tasks_types::{Task, TaskComparator};
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::TasksBackend;
use crate::helpers::{
    SetAccumulators, check_metadata_entry, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response, reject_unsupported_sort, resolve_refs,
};
use jmap_server::{
    HandlerOutput, backend_set_error_value, bool_arg, reject_read_only_patch, reject_server_set,
    server_fail_from_backend, server_fail_value_from_backend,
};

// ---------------------------------------------------------------------------
// Task/get
// ---------------------------------------------------------------------------

/// Handle a `Task/get` method call (draft-tasks-06 §4.5).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// If `"utcStart"` or `"utcDue"` appear in the requested `properties` (or if
/// `properties` is `null` — meaning all fields), [`TasksBackend::compute_utc_times`]
/// is called for each returned task and the computed values are merged in
/// (draft-tasks-06 §4, utcStart/utcDue paragraphs).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]). When the backend lists Task for
/// draft-ietf-jmap-metadata-04 ([`JmapBackend::metadata_info`](jmap_server::JmapBackend::metadata_info)),
/// `metadata` is returned as an object, by default only with
/// `urn:ietf:params:jmap:metadata` in `using`, and with that capability
/// `properties` may hold `metadata/<namespace>` subselectors (-04 §2,
/// §3.1); see [`jmap_server::handle_get_with_metadata`].
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_get<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    // Determine whether utcStart / utcDue are requested.
    let want_utc = match args.get("properties") {
        None | Some(Value::Null) => true, // all properties requested
        Some(Value::Array(props)) => props
            .iter()
            .any(|p| p.as_str().is_some_and(|s| s == "utcStart" || s == "utcDue")),
        _ => false,
    };

    // Peek accountId to thread through compute_utc_times (bd:JMAP-ops7.25).
    // handle_get re-extracts and re-validates idempotently below; a malformed
    // or missing accountId surfaces here as invalidArguments before the
    // delegate call.
    let account_id: Id = args
        .get("accountId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JmapError::invalid_arguments("accountId is required"))
        .and_then(|s| {
            Id::new_validated(s).map_err(|e| {
                JmapError::invalid_arguments(format!("accountId is not a valid Id: {e}"))
            })
        })?;

    // Delegate to the generic get handler.
    let HandlerOutput {
        primary: mut response,
        extra_invocations: tail,
        ..
    } = jmap_server::handle_get_with_metadata::<Task, B>(backend, caller, args, using).await?;

    // If utcStart or utcDue were requested, augment each returned task.
    if want_utc && let Some(Value::Array(list)) = response.get_mut("list") {
        for item in list.iter_mut() {
            if let Ok(task) = Task::deserialize(&*item) {
                let (utc_start, utc_due) = backend
                    .compute_utc_times(caller, &account_id, &task, None)
                    .await;
                if let Some(s) = utc_start {
                    item["utcStart"] = Value::String(s.into_inner());
                }
                if let Some(d) = utc_due {
                    item["utcDue"] = Value::String(d.into_inner());
                }
            }
        }
    }

    Ok(HandlerOutput::new(response).with_extra(tail))
}

// ---------------------------------------------------------------------------
// Task/changes
// ---------------------------------------------------------------------------

/// Handle a `Task/changes` method call (draft-tasks-06 §4.6).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the §5.2
/// `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); with
/// `urn:ietf:params:jmap:metadata` the request may set
/// draft-ietf-jmap-metadata-04 §3.3 `ignoreMetadataOnlyChanges`, and
/// without it metadata-only changes are left out (-04 §3).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_changes<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<Task, B>(backend, caller, args, using).await
}

// ---------------------------------------------------------------------------
// Task/set
// ---------------------------------------------------------------------------

/// The Task properties that hold the id of another record, which a client
/// may set to a `#creationId` reference (RFC 8620 §5.3).
const TASK_ID_PROPERTIES: &[&str] = &["taskListId"];

/// Handle a `Task/set` method call (draft-tasks-06 §4.7).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation result maps).
///
/// **isDraft immutability**: this handler does not check `isDraft`. The
/// backend's [`crate::TasksBackend::update_object`] MUST reject a patch that
/// sets `isDraft: true` on a Task whose stored value is not `true` with
/// `invalidProperties`, atomically with the write; the rejection is reported
/// in `notUpdated`.
///
/// **`taskListId` references**: a `#creationId` value on create or update
/// names a TaskList created earlier in the request and resolves against
/// `created_ids` (pass [`jmap_server::CallContext::created_ids`]; RFC 8620
/// §5.3). An unknown reference is `invalidProperties` naming `taskListId`.
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]): a write to `metadata` or
/// `privateMetadata` without `urn:ietf:params:jmap:metadata` is
/// `invalidProperties` (draft-ietf-jmap-metadata-04 §3), as is `null` for
/// either in a create (-04 §3.2). The backend validates the rest.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_set<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
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

    let old_state = backend
        .get_state::<Task>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
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
        for (create_id, mut obj_val) in create_map {
            if let Some(err) = obj_val.as_object().and_then(reject_server_set::<Task>) {
                not_created.insert(create_id, serde_json::Value::from(&err));
                continue;
            }
            if let Some(Err(err)) = obj_val
                .as_object()
                .map(|o| check_metadata_entry(o, true, using))
            {
                not_created.insert(create_id, serde_json::Value::from(&*err));
                continue;
            }
            if let Err(err) = resolve_refs(&mut obj_val, TASK_ID_PROPERTIES, created_ids) {
                not_created.insert(create_id, serde_json::Value::from(&*err));
                continue;
            }
            // `Task.id` is optional, so the create parses without one and
            // `build` sets the id the backend mints.
            let mut task: Task = match serde_json::from_value(obj_val) {
                Ok(t) => t,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            let build = move |id: &Id| {
                task.id = Some(id.clone());
                task
            };
            match backend
                .create_object::<Task, _>(caller, &account_id, &create_id, build)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    created.insert(create_id, jmap_server::serialize_value(&created_obj)?);
                }
                Err(e) => {
                    not_created.insert(create_id, backend_set_error_value(e));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, mut patch_val) in update_map {
            // isDraft immutability (draft-tasks-06 §4 isDraft paragraph) is
            // enforced by the backend in update_object, atomically with the
            // write (TasksBackend::update_object, bd:JMAP-h47t.29).
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_updated.insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };

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
            let is_per_user_only = patch_val.as_object().is_some_and(|m| {
                !m.is_empty()
                    && m.keys().all(|k| {
                        let head = k.split('/').next().unwrap_or(k);
                        jmap_tasks_types::is_per_user_task_property(head)
                    })
            });

            if let Err(err) = resolve_refs(&mut patch_val, TASK_ID_PROPERTIES, created_ids) {
                not_updated.insert(id_str, serde_json::Value::from(&*err));
                continue;
            }

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
            if let Some(err) = reject_read_only_patch::<Task>(patch.as_map(), None) {
                not_updated.insert(id_str, serde_json::Value::from(&err));
                continue;
            }
            if let Err(err) = check_metadata_entry(patch.as_map(), false, using) {
                not_updated.insert(id_str, serde_json::Value::from(&*err));
                continue;
            }

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
                    updated.insert(id_str, jmap_server::serialize_value(&obj)?);
                }
                Ok(None) => {
                    mutated = true;
                    updated.insert(id_str, Value::Null);
                }
                Err(e) => {
                    not_updated.insert(id_str, backend_set_error_value(e));
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
                    not_destroyed.insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };

            match backend
                .destroy_object::<Task>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str));
                }
                Err(e) => {
                    not_destroyed.insert(id_str, backend_set_error_value(e));
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
/// `args` is the RFC 8620 §5.4 `/copy` request shape (`fromAccountId`,
/// optional `ifFromInState`, `accountId`, optional `ifInState`, `create`
/// map, optional `onSuccessDestroyOriginal`, optional
/// `destroyFromIfInState`); the returned `Value` is the §5.4 `/copy`
/// response shape (`fromAccountId`, `accountId`, optional `oldState`,
/// `newState`, optional `created`, optional `notCreated`).
///
/// `call_id` is the request's method-call id, used to construct any
/// follow-up `Task/set` invocations injected when
/// `onSuccessDestroyOriginal` is true (RFC 8620 §5.4 + §6.3).
///
/// Copies tasks from `fromAccountId` into the current account. The `create`
/// map keys are client-side creation ids; the backend assigns new server-side
/// ids.
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]). draft-ietf-jmap-metadata-04 §3.4:
/// the copy keeps the source's `metadata`, and its `privateMetadata` only
/// with `urn:ietf:params:jmap:metadata` in `using` and when the
/// destination supports it; neither is copied when the destination does
/// not list Task ([`JmapBackend::metadata_info`](jmap_server::JmapBackend::metadata_info)).
/// The create object "MAY include metadata and privateMetadata to override
/// the copied values, with the same rules as a create in Foo/set": the
/// handler rejects them without the capability and rejects `null`, and
/// the backend's `create_object` validates the rest.
///
/// Returns `(response_args, extra_invocations)`. The extra invocation list
/// is populated with a follow-up `Task/set` invocation when
/// `onSuccessDestroyOriginal` is true (RFC 8620 §5.4); otherwise it is
/// empty.
pub async fn handle_task_copy<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    call_id: &str,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
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
    let from_account_id = Id::new_validated(from_account_id_str)
        .map_err(|e| JmapError::invalid_arguments(format!("fromAccountId: {e}")))?;

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
    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    // RFC 8620 §5.4 `onSuccessDestroyOriginal` / `destroyFromIfInState`:
    // parsed here so the request payload is consumed in handler-local order
    // and so a clearly-shaped per-feature value is available below.
    let on_success_destroy_original: bool = bool_arg(&args, "onSuccessDestroyOriginal", false);
    let destroy_from_if_in_state: Option<String> = args
        .get("destroyFromIfInState")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut mutated = false;
    // (create_id, source_id) pairs for entries that succeeded — fed to the
    // implicit Task/set destroy below when onSuccessDestroyOriginal is true.
    let mut copied_source_ids: Vec<(String, Id)> = Vec::new();

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, client_val) in create_map {
            // RFC 8620 §5.4: the Foo object MUST contain an `id` property,
            // which is the id (in the fromAccount) of the record to be copied.
            // The earlier handler silently invented a "placeholder" id when
            // the client omitted one, masking the missing-id case the spec
            // treats as a hard requirement.
            let Some(source_id_str) = client_val.get("id").and_then(|v| v.as_str()) else {
                not_created.insert(
                    create_id,
                    json!({"type": "invalidProperties", "properties": ["id"]}),
                );
                continue;
            };
            let source_id: Id = match jmap_server::parse_wire_id(source_id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_created.insert(create_id, serde_json::Value::from(&*err));
                    continue;
                }
            };

            // Fetch the source Task from `fromAccountId`. RFC 8620 §5.4 frames
            // /copy as a read-from-source-then-create-in-destination operation;
            // unknown source ids surface as `notFound` per-entry SetErrors.
            let mut source_tasks: Vec<Task> = match backend
                .get_objects::<Task>(
                    caller,
                    &from_account_id,
                    Some(std::slice::from_ref(&source_id)),
                    None,
                )
                .await
            {
                Ok((objs, not_found)) => {
                    if !not_found.is_empty() || objs.is_empty() {
                        not_created.insert(create_id, json!({"type": "notFound"}));
                        continue;
                    }
                    objs
                }
                Err(e) => {
                    not_created.insert(create_id, server_fail_value_from_backend(&e));
                    continue;
                }
            };

            // RFC 8620 §5.4: "any other properties included [in the create
            // entry] are used instead of the current value for that property
            // on the original." Serialize the source, overlay client-supplied
            // properties, strip the source id so the backend assigns a fresh
            // server id. Pattern mirrors jmap-calendars-server event.rs:610-642.
            // draft-ietf-jmap-metadata-04 §3.4: metadata overrides follow the
            // Foo/set create rules (§3, §3.2).
            if let Some(Err(err)) = client_val
                .as_object()
                .map(|o| check_metadata_entry(o, true, using))
            {
                not_created.insert(create_id, Value::from(&*err));
                continue;
            }
            // -04 §3.4: "When the request does not use this capability, only
            // metadata is copied", "privateMetadata is not copied when the
            // destination does not support it, and neither property is
            // copied when the data type is not listed in the destination
            // account". The overrides are merged in below.
            let info = backend.metadata_info::<Task>(caller, &to_account_id);
            if !jmap_server::uses_metadata(using)
                || !info.as_ref().is_some_and(|i| i.supports_private)
            {
                source_tasks[0].private_metadata = None;
            }
            if info.is_none() {
                source_tasks[0].metadata = None;
            }
            let Ok(Value::Object(mut merged)) = serde_json::to_value(&source_tasks[0]) else {
                not_created.insert(
                    create_id,
                    json!({
                        "type": "serverFail",
                        "description": "failed to serialize source Task",
                    }),
                );
                continue;
            };
            if let Value::Object(client_props) = client_val {
                for (k, v) in client_props {
                    if k != "id" {
                        merged.insert(k, v);
                    }
                }
            }
            merged.remove("id");

            let mut task: Task = match serde_json::from_value(Value::Object(merged)) {
                Ok(t) => t,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            let build = move |id: &Id| {
                task.id = Some(id.clone());
                task
            };
            match backend
                .create_object::<Task, _>(caller, &to_account_id, &create_id, build)
                .await
            {
                Ok((_new_id, created_task)) => {
                    mutated = true;
                    created.insert(
                        create_id.clone(),
                        jmap_server::serialize_value(&created_task)?,
                    );
                    copied_source_ids.push((create_id, source_id));
                }
                Err(e) => {
                    not_created.insert(create_id, backend_set_error_value(e));
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

    let resp = json!({
        "fromAccountId": from_account_id.as_ref(),
        "accountId": to_account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created":    if created.is_empty()     { Value::Null } else { Value::Object(created) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
    });

    // RFC 8620 §5.4 `onSuccessDestroyOriginal`: after emitting the Task/copy
    // response, the server MUST make a single implicit Task/set call against
    // fromAccountId destroying each successfully copied source record.
    // `destroyFromIfInState`, if supplied, is passed through as the ifInState
    // argument on that implicit call. The dispatcher returns the extra
    // Invocation vec to the caller (RFC 8620 §6.3 chained responses).
    // Pattern mirrors jmap-mail-server email.rs:2432-2575.
    let mut extra: Vec<Invocation> = Vec::new();
    if on_success_destroy_original && !copied_source_ids.is_empty() {
        let from_old_state = backend
            .get_state::<Task>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;

        // Honour destroyFromIfInState on the implicit Task/set call. A
        // mismatch yields a stateMismatch top-level error on the synthesized
        // response (§5.4 inheriting §5.3 semantics on the chained call).
        let mut destroy_state_mismatch = false;
        if let Some(if_state) = destroy_from_if_in_state.as_deref()
            && if_state != from_old_state.as_ref()
        {
            destroy_state_mismatch = true;
        }

        let mut destroyed_arr: Vec<Value> = Vec::new();
        let mut not_destroyed = serde_json::Map::new();

        if !destroy_state_mismatch {
            for (_, source_id) in &copied_source_ids {
                match backend
                    .destroy_object::<Task>(caller, &from_account_id, source_id)
                    .await
                {
                    Ok(()) => {
                        destroyed_arr.push(Value::String(source_id.as_ref().to_owned()));
                    }
                    Err(e) => {
                        not_destroyed
                            .insert(source_id.as_ref().to_owned(), backend_set_error_value(e));
                    }
                }
            }
        }

        let from_new_state = backend
            .get_state::<Task>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;

        let set_resp = if destroy_state_mismatch {
            json!({
                "type": "stateMismatch",
            })
        } else {
            json!({
                "accountId": from_account_id.as_ref(),
                "oldState": from_old_state.as_ref(),
                "newState": from_new_state.as_ref(),
                "created": Value::Null,
                "updated": Value::Null,
                "destroyed": if destroyed_arr.is_empty() { Value::Null } else { Value::Array(destroyed_arr) },
                "notCreated": Value::Null,
                "notUpdated": Value::Null,
                "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
            })
        };

        let method_name = if destroy_state_mismatch {
            "error".to_owned()
        } else {
            "Task/set".to_owned()
        };
        extra.push(Invocation::new(method_name, set_resp, call_id.to_owned()));
    }

    Ok(HandlerOutput::new(resp).with_extra(extra))
}

// ---------------------------------------------------------------------------
// Task/query
// ---------------------------------------------------------------------------

/// Handle a `Task/query` method call (draft-tasks-06 §4.13).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`,
/// optional `filter`, optional `sort`, optional `position`, optional
/// `anchor`, optional `anchorOffset`, optional `limit`, optional
/// `calculateTotal`); the returned `Value` is the §5.5 `/query`
/// response shape (`accountId`, `queryState`, `canCalculateChanges`,
/// `position`, `ids`, optional `total`).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); without
/// `urn:ietf:params:jmap:metadata` a draft-ietf-jmap-metadata-04 §3.6
/// filter condition is `unsupportedFilter` (-04 §3).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_query<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::reject_metadata_filter_arg(
        &args,
        jmap_tasks_types::TaskFilterCondition::has_metadata_condition,
        using,
    )?;
    reject_unsupported_sort::<TaskComparator>(args.get("sort"))?;
    jmap_server::handle_query::<Task, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Task/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Task/queryChanges` method call (draft-tasks-06 §4.14).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); without
/// `urn:ietf:params:jmap:metadata` a draft-ietf-jmap-metadata-04 §3.6
/// filter condition is `unsupportedFilter` (-04 §3).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_task_query_changes<B: TasksBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::reject_metadata_filter_arg(
        &args,
        jmap_tasks_types::TaskFilterCondition::has_metadata_condition,
        using,
    )?;
    reject_unsupported_sort::<TaskComparator>(args.get("sort"))?;
    jmap_server::handle_query_changes::<Task, B>(backend, caller, args).await
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
    /// on". draft-ietf-jmap-tasks-06 §4.13 defers to CalendarEvent/query, whose sort
    /// properties are start, uid, recurrenceId, created and updated; `title` is not among them.
    #[tokio::test]
    async fn query_unlisted_sort_property_returns_unsupported_sort() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sort": [{ "property": "title" }] });
        let err = handle_task_query(&backend, &(), args, &[])
            .await
            .expect_err("unlisted sort property must be rejected");
        assert_eq!(err.error_type.to_string(), "unsupportedSort");
    }

    /// Oracle: as above; `Task/queryChanges` carries the same `sort`
    /// (RFC 8620 §5.6).
    #[tokio::test]
    async fn query_changes_unlisted_sort_property_returns_unsupported_sort() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "sinceQueryState": "0",
            "sort": [{ "property": "start" }, { "property": "title" }]
        });
        let err = handle_task_query_changes(&backend, &(), args, &[])
            .await
            .expect_err("unlisted sort property must be rejected");
        assert_eq!(err.error_type.to_string(), "unsupportedSort");
    }

    /// Oracle: draft-ietf-jmap-tasks-06 §4.13 defers to CalendarEvent/query, whose sort
    /// properties are start, uid, recurrenceId, created and updated, so a `start` sort is accepted.
    #[tokio::test]
    async fn query_listed_sort_property_is_accepted() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sort": [{ "property": "start" }] });
        handle_task_query(&backend, &(), args, &[])
            .await
            .expect("listed sort property must not be rejected");
    }

    /// Oracle: Task/get with unknown accountId returns accountNotFound.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let result = handle_task_get(&backend, &(), args, &[]).await;
        let err = result.expect_err("must return error for unknown account");
        assert_eq!(err.error_type.to_string(), "accountNotFound");
    }

    /// Oracle: Task/copy missing fromAccountId returns invalidArguments.
    #[tokio::test]
    async fn copy_missing_from_account_id_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {}
        });
        let result = handle_task_copy(&backend, &(), args, "c0", &[]).await;
        let err = result.expect_err("must return error when fromAccountId missing");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
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
        let result = handle_task_copy(&backend, &(), args, "c0", &[]).await;
        let err = result.expect_err("must reject same-account Task/copy per RFC 8620 §5.4");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
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
        let result = handle_task_copy(&backend, &(), args, "c0", &[]).await;
        let err = result.expect_err("must reject unknown fromAccountId");
        assert_eq!(err.error_type.to_string(), "fromAccountNotFound");
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
        let result = handle_task_copy(&backend, &(), args, "c0", &[]).await;
        let err = result.expect_err("must reject stale ifFromInState");
        assert_eq!(err.error_type.to_string(), "stateMismatch");
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
        let result = handle_task_copy(&backend, &(), args, "c0", &[]).await;
        let err = result.expect_err("must reject stale ifInState");
        assert_eq!(err.error_type.to_string(), "stateMismatch");
    }

    /// Oracle: RFC 8620 §5.4 — a create entry without an `id` property must
    /// produce a per-entry `invalidProperties` SetError naming `id`. The
    /// previous handler silently substituted a "placeholder" id, masking
    /// this hard requirement.
    #[tokio::test]
    async fn copy_create_entry_without_id_returns_invalid_properties() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "acc2",
            "create": { "c1": {} }
        });
        let HandlerOutput { primary: resp, .. } = handle_task_copy(&backend, &(), args, "c0", &[])
            .await
            .expect("top-level call must succeed");
        let nc = resp["notCreated"].as_object().expect("notCreated present");
        let err = nc.get("c1").expect("c1 in notCreated");
        assert_eq!(err["type"], "invalidProperties");
        assert_eq!(err["properties"][0], "id");
    }

    /// Oracle: RFC 8620 §5.4 — when the source id does not resolve to a
    /// Task in `fromAccountId`, the per-entry result is `notFound`.
    #[tokio::test]
    async fn copy_unknown_source_id_returns_not_found() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "acc2",
            "create": { "c1": { "id": "no-such-task" } }
        });
        let HandlerOutput { primary: resp, .. } = handle_task_copy(&backend, &(), args, "c0", &[])
            .await
            .expect("top-level call must succeed");
        let nc = resp["notCreated"].as_object().expect("notCreated present");
        assert_eq!(nc["c1"]["type"], "notFound");
    }

    /// Oracle: RFC 8620 §5.4 — the create entry's `id` selects WHAT to copy;
    /// client-supplied properties OTHER than `id` overlay the source values.
    /// Verify the handler actually round-trips through the source: seed a
    /// Task into `from_account`, request a copy that overrides `title`, and
    /// confirm the response carries the override.
    #[tokio::test]
    async fn copy_fetches_source_and_overlays_client_overrides() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.seed_task("acc2", "src1", true); // source task in acc2

        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "acc2",
            "create": {
                "c1": { "id": "src1", "title": "Overridden title" }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_task_copy(&backend, &(), args, "c0", &[])
            .await
            .expect("top-level call must succeed");

        // Source was fetched and the override applied: the created entry
        // carries the new title, not the source value (which had none).
        let created = resp["created"].as_object().expect("created present");
        let c1 = created.get("c1").expect("c1 in created");
        assert_eq!(
            c1["title"], "Overridden title",
            "client-supplied title must override source value: {resp}"
        );
    }

    /// Oracle: RFC 8620 §5.4 — when `onSuccessDestroyOriginal` is true, the
    /// server emits a single implicit Task/set call against fromAccountId
    /// destroying each successfully copied source. The synthesized response
    /// appears in the extra Vec<Invocation> with the same callId as the
    /// triggering Task/copy. Mirrors canonical jmap-mail-server
    /// email.rs:2432-2575.
    #[tokio::test]
    async fn copy_on_success_destroy_original_emits_implicit_task_set() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.seed_task("acc2", "src1", true);

        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "acc2",
            "onSuccessDestroyOriginal": true,
            "create": {
                "c1": { "id": "src1" }
            }
        });
        let HandlerOutput {
            extra_invocations: extra,
            ..
        } = handle_task_copy(&backend, &(), args, "c0", &[])
            .await
            .expect("top-level call must succeed");

        // Exactly one synthesized invocation: ("Task/set", response, "c0").
        assert_eq!(extra.len(), 1, "must emit one implicit Task/set: {extra:?}");
        let Invocation {
            name: method,
            arguments: val,
            call_id: ci,
        } = &extra[0];
        assert_eq!(method, "Task/set");
        assert_eq!(ci, "c0");
        assert_eq!(val["accountId"], "acc2", "implicit set targets fromAccount");
        let destroyed = val["destroyed"]
            .as_array()
            .expect("destroyed must be array");
        assert_eq!(destroyed.len(), 1);
        assert_eq!(destroyed[0], "src1");
        // Both state fields are present on the synthesized response. Their
        // bump semantics depend on the underlying backend's get_state — the
        // MockBackend here always returns "0" so we only assert presence;
        // state-advance semantics are covered by stateful integration tests.
        assert!(val.get("oldState").is_some(), "oldState must be present");
        assert!(val.get("newState").is_some(), "newState must be present");
    }

    /// Oracle: RFC 8620 §5.4 — `destroyFromIfInState` mismatch on the implicit
    /// Task/set call yields a `stateMismatch` error response in place of the
    /// normal Task/set body. The destroy MUST NOT proceed.
    #[tokio::test]
    async fn copy_destroy_from_if_in_state_mismatch_aborts_destroy() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.seed_task("acc2", "src1", true);

        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "acc2",
            "onSuccessDestroyOriginal": true,
            "destroyFromIfInState": "stale-source-state",
            "create": {
                "c1": { "id": "src1" }
            }
        });
        let HandlerOutput {
            extra_invocations: extra,
            ..
        } = handle_task_copy(&backend, &(), args, "c0", &[])
            .await
            .expect("top-level call must succeed");

        assert_eq!(extra.len(), 1);
        let Invocation { arguments: val, .. } = &extra[0];
        assert_eq!(val["type"], "stateMismatch");
    }

    /// Oracle: RFC 8620 §5.4 — when `onSuccessDestroyOriginal` is false (or
    /// absent), no implicit Task/set call is synthesized.
    #[tokio::test]
    async fn copy_without_on_success_emits_no_extra_invocations() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.seed_task("acc2", "src1", true);

        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "acc2",
            "create": {
                "c1": { "id": "src1" }
            }
        });
        let HandlerOutput {
            extra_invocations: extra,
            ..
        } = handle_task_copy(&backend, &(), args, "c0", &[])
            .await
            .expect("top-level call must succeed");
        assert!(
            extra.is_empty(),
            "no extra invocations without onSuccessDestroyOriginal: {extra:?}"
        );
    }

    /// Oracle: Task/set empty destroy returns valid response.
    #[tokio::test]
    async fn set_empty_destroy_returns_valid_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "destroy": [] });
        let HandlerOutput { primary: resp, .. } =
            handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
                .await
                .expect("must not return top-level error");
        assert_eq!(resp["accountId"], "acc1");
    }

    // ── isDraft immutability enforcement (draft-tasks-06 §4 (isDraft paragraph)) ─

    /// Oracle: draft-tasks-06 §4 — "Once set to false, the value [isDraft]
    /// cannot be updated to true."
    /// A patch of {"isDraft": true} on a task that already has isDraft=false
    /// MUST be rejected with invalidProperties. The handler does no isDraft
    /// check, so this exercises the backend rejection reaching `notUpdated`.
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
        let HandlerOutput { primary: resp, .. } =
            handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
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
        let HandlerOutput { primary: resp, .. } =
            handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
                .await
                .expect("must not return a top-level error");

        // The handler passes the patch to the backend (which returns Forbidden
        // in the mock). The important thing is it is NOT pre-rejected here —
        // it must NOT appear in notUpdated with type=="invalidProperties".
        if let Some(not_updated) = resp["notUpdated"].as_object()
            && let Some(err) = not_updated.get("t2")
        {
            assert_ne!(
                err["type"].as_str(),
                Some("invalidProperties"),
                "isDraft:true on an existing draft must not produce invalidProperties"
            );
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
        let HandlerOutput { primary: resp, .. } =
            handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
                .await
                .expect("must not return a top-level error");

        // Like above: the patch must NOT be pre-rejected with invalidProperties.
        if let Some(not_updated) = resp["notUpdated"].as_object()
            && let Some(err) = not_updated.get("t3")
        {
            assert_ne!(
                err["type"].as_str(),
                Some("invalidProperties"),
                "isDraft:false must never produce invalidProperties"
            );
        }
    }

    // ── compute_utc_times / utcStart wiring (draft-tasks-06 §4 (utcStart/utcDue paragraphs)) ─

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
        let HandlerOutput { primary: resp, .. } = handle_task_get(&backend, &(), args, &[])
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
    /// calls compute_utc_times. The default impl returns (None, None) so
    /// no utcStart key is injected, but no error is raised either.
    #[tokio::test]
    async fn get_with_utc_start_in_properties_does_not_error() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "ids": null,
            "properties": ["id", "utcStart", "utcDue"]
        });
        let HandlerOutput { primary: resp, .. } = handle_task_get(&backend, &(), args, &[])
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
        let _ = handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
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
        let _ = handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
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
        let _ = handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
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
        let _ = handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
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
        let _ = handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
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
        let _ = handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
            .await
            .expect("must not return top-level error");
        assert_eq!(
            backend.per_user_calls.load(Ordering::Relaxed),
            0,
            "empty patch must NOT route to update_task_per_user"
        );
    }

    /// RFC 8620 §5.3 types `destroy` as "Id[]|null", so an object element
    /// is "invalidArguments: One of the arguments is of the wrong type"
    /// (§3.6.2). The description names only the JSON type: §3.6.1 reserves
    /// it for "unstructured English text suitable for displaying to a
    /// developer", so the client-supplied value is never echoed
    /// (bd JMAP-zegc).
    #[tokio::test]
    async fn set_non_string_destroy_element_does_not_echo_value() {
        let backend = MockBackend::new_with_account("acc");
        let args = json!({ "accountId": "acc", "destroy": [{ "note": "CANARY-zegc" }] });
        let err = handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
            .await
            .expect_err("non-string destroy element must be a method error");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
        let desc = err.description.as_deref().unwrap_or_default();
        assert!(desc.contains("got object"), "{desc}");
        assert!(!desc.contains("CANARY"), "{desc}");
    }
}
