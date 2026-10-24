//! EmailSubmission/* method handlers (RFC 8621 §7).
//!
//! Provides handlers for:
//! - `EmailSubmission/get` (§7.1)
//! - `EmailSubmission/changes` (§7.2)
//! - `EmailSubmission/query` (§7.3)
//! - `EmailSubmission/queryChanges` (§7.4)
//! - `EmailSubmission/set` (§7.5) — also handles `onSuccessUpdateEmail`
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by RFC 8621 §7. The returned `Value` is the
//! corresponding method-response object per the same section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3).
//! For the `/get`, `/changes`, `/query`, and `/queryChanges` handlers in
//! this module the vector is **always empty**. `handle_submission_set`
//! is the one exception: it MAY emit one `Email/set` follow-up
//! invocation when `onSuccessUpdateEmail` is present (RFC 8621 §7.5).
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
//! - `/query`, `/queryChanges`: `unsupportedFilter`, `unsupportedSort`;
//! - `/query`: `anchorNotFound`.
//!
//! Per-target failures inside `/set` surface in the `notCreated` /
//! `notUpdated` / `notDestroyed` maps within `Ok((Value, ...))`, not as
//! `Err`.

use std::collections::{BTreeMap, HashMap, HashSet};

use jmap_mail_types::{
    Address, Delivered, DeliveryStatus, Displayed, Email, EmailSubmission,
    EmailSubmissionComparator, EmailSubmissionComparatorProperty, EmailSubmissionFilter, Envelope,
    Identity, UndoStatus,
};
use jmap_types::{Id, Invocation, JmapError, PatchObject, State, UTCDate};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::MailBackend;
use crate::helpers::{
    enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id, not_found_json,
    now_utc_string, resolve_ref, serialize_value,
};
use jmap_server::{BackendSetError, SetError, SetErrorType};
use jmap_server::{
    HandlerOutput, QueryChangesParams, QueryParams, backend_set_error_value, bool_arg, filter_arg,
    limit_or_max, reject_read_only_patch, reject_server_set, resolve_query_start,
    server_fail_from_backend,
};

// ---------------------------------------------------------------------------
// EmailSubmission/get
// ---------------------------------------------------------------------------

/// Handle an `EmailSubmission/get` method call (RFC 8621 §7.1).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_submission_get<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    enforce_max_objects_in_get(&args, backend.max_objects_in_get(caller, &account_id))?;

    let ids: Option<Vec<Id>> = match args.remove("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<EmailSubmission>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<EmailSubmission>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(serialize_value)
        .collect::<Result<Vec<_>, _>>()?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    });

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// EmailSubmission/changes
// ---------------------------------------------------------------------------

/// Handle an `EmailSubmission/changes` method call (RFC 8621 §7.2).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_submission_changes<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<EmailSubmission, B>(backend, caller, args, &[]).await
}

// ---------------------------------------------------------------------------
// EmailSubmission/query
// ---------------------------------------------------------------------------

/// Decode the `sort` argument of `EmailSubmission/query` and
/// `EmailSubmission/queryChanges`.
///
/// RFC 8621 §7.3 defines `emailId`, `threadId` and `sentAt` as the
/// EmailSubmission comparator properties; any other property decodes as
/// [`EmailSubmissionComparatorProperty::Other`] and is unsupportedSort
/// (RFC 8620 §5.5). A malformed comparator is invalidArguments. An absent
/// or null `sort` yields an empty vector.
fn submission_sort_arg(
    args: &serde_json::Map<String, Value>,
) -> Result<Vec<EmailSubmissionComparator>, JmapError> {
    let sort: Vec<EmailSubmissionComparator> = match args.get("sort") {
        None | Some(Value::Null) => return Ok(vec![]),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| JmapError::invalid_arguments(format!("sort: {e}")))?,
    };
    if sort
        .iter()
        .any(|c| matches!(c.property, EmailSubmissionComparatorProperty::Other(_)))
    {
        return Err(JmapError::unsupported_sort());
    }
    Ok(sort)
}

/// Handle an `EmailSubmission/query` method call (RFC 8621 §7.3).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`); the
/// returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_submission_query<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let calculate_total: bool = bool_arg(&args, "calculateTotal", false);

    let limit: Option<u64> = match args.get("limit") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(n) => Some(n),
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {}",
                    jmap_server::json_type_name(v)
                )));
            }
        },
    };

    let position: i64 = match args.get("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!(
                "position: expected an integer, got {}",
                jmap_server::json_type_name(v)
            ))
        })?,
    };

    // RFC 8620 §5.5: anchor-based pagination overrides position.
    let anchor: Option<Id> = match args.get("anchor") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(
            Id::new_validated(s.as_str())
                .map_err(|e| JmapError::invalid_arguments(format!("anchor: {e}")))?,
        ),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "anchor: expected an Id string or null, got {}",
                jmap_server::json_type_name(v)
            )));
        }
    };
    let anchor_offset: i64 = match args.get("anchorOffset") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!(
                "anchorOffset: expected an integer, got {}",
                jmap_server::json_type_name(v)
            ))
        })?,
    };

    let sort_values = submission_sort_arg(&args)?;
    let sort_slice = (!sort_values.is_empty()).then_some(sort_values.as_slice());

    let filter: Option<EmailSubmissionFilter> = filter_arg(&mut args)?;

    // When anchor is present, fetch the full result set to resolve it.
    // Otherwise, delegate limit/position directly to the backend.
    let (ids, total, query_state, can_calculate_changes, reported_position) =
        if let Some(ref anchor_id) = anchor {
            let all = backend
                .query_objects::<EmailSubmission>(
                    caller,
                    &account_id,
                    QueryParams::new().filter(filter.as_ref()).sort(sort_slice),
                )
                .await
                .map_err(JmapError::from)?;
            let start = resolve_query_start(&all.ids, position, Some(anchor_id), anchor_offset)?;
            let effective_limit = limit_or_max(limit);
            let page: Vec<Id> = all
                .ids
                .into_iter()
                .skip(start)
                .take(effective_limit)
                .collect();
            let total = all.total;
            (
                page,
                total,
                all.query_state,
                all.can_calculate_changes,
                start as u64,
            )
        } else {
            let result = backend
                .query_objects::<EmailSubmission>(
                    caller,
                    &account_id,
                    QueryParams::new()
                        .filter(filter.as_ref())
                        .sort(sort_slice)
                        .limit(limit)
                        .position(position),
                )
                .await
                .map_err(JmapError::from)?;
            let pos = result.position;
            let total = result.total;
            (
                result.ids,
                total,
                result.query_state,
                result.can_calculate_changes,
                pos,
            )
        };

    // RFC 8620 §5.5: total MUST be omitted when calculateTotal is false (default).
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": query_state.as_ref(),
        "canCalculateChanges": can_calculate_changes,
        "position": reported_position,
        "ids": ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total && let Some(t) = total {
        resp["total"] = json!(t);
    }

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// EmailSubmission/queryChanges
// ---------------------------------------------------------------------------

/// Handle an `EmailSubmission/queryChanges` method call (RFC 8621 §7.4).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_submission_query_changes<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let since_query_state: State = match args.get("sinceQueryState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceQueryState is required")),
    };

    let max_changes = jmap_server::parse_max_changes(args.get("maxChanges"))?;

    let up_to_id: Option<Id> = match args.get("upToId") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(
            Id::new_validated(s.as_str())
                .map_err(|e| JmapError::invalid_arguments(format!("upToId: {e}")))?,
        ),
        Some(_) => {
            return Err(JmapError::invalid_arguments(
                "upToId must be a string Id or null",
            ));
        }
    };

    let calculate_total: bool = bool_arg(&args, "calculateTotal", false);

    // RFC 8620 §5.6: `filter` and `sort` are "The filter/sort argument that
    // was used with Foo/query", so they decode and validate exactly as in
    // handle_submission_query.
    let sort_values = submission_sort_arg(&args)?;
    let filter: Option<EmailSubmissionFilter> = filter_arg(&mut args)?;

    let result = backend
        .query_changes::<EmailSubmission>(
            caller,
            &account_id,
            QueryChangesParams::new(&since_query_state)
                .filter(filter.as_ref())
                .sort((!sort_values.is_empty()).then_some(sort_values.as_slice()))
                .max_changes(max_changes)
                .up_to_id(up_to_id.as_ref()),
        )
        .await
        .map_err(JmapError::from)?;

    let added: Vec<Value> = result
        .added
        .iter()
        .map(|item| json!({ "id": item.id.as_ref(), "index": item.index }))
        .collect();

    // RFC 8620 §5.6: total MUST be omitted unless calculateTotal is true.
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "oldQueryState": result.old_query_state.as_ref(),
        "newQueryState": result.new_query_state.as_ref(),
        "removed": result.removed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "added": added,
    });
    if calculate_total && let Some(t) = result.total {
        resp["total"] = json!(t);
    }

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// EmailSubmission/set
// ---------------------------------------------------------------------------

/// Handle an `EmailSubmission/set` method call (RFC 8621 §7.5).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps),
/// augmented with the RFC 8621 §7.5 `onSuccessUpdateEmail` and
/// `onSuccessDestroyEmail` arguments; the returned `Value` is the §5.3
/// `/set` response shape (`accountId`, `oldState`, `newState`, plus the
/// per-operation `created` / `notCreated` / `updated` / `notUpdated` /
/// `destroyed` / `notDestroyed` maps).
///
/// Returns `(response_args, extra_invocations)`. When `onSuccessUpdateEmail` is
/// present, extra_invocations will contain one `Email/set` invocation.
///
/// A `#creationId` `identityId` or `emailId` on a create, and a
/// `#creationId` Mailbox in an `onSuccessUpdateEmail` `mailboxIds` patch,
/// resolves against `created_ids` (pass
/// [`jmap_server::CallContext::created_ids`]; RFC 8620 §5.3).
pub async fn handle_submission_set<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    call_id: &str,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.2). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<EmailSubmission>(caller, &account_id)
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
    let mut destroyed: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();

    // Pre-fetch email IDs for non-creation-reference submission IDs in onSuccess* args.
    // This must happen before the destroy loop so that email IDs are available
    // even for submissions that will be destroyed in this request.
    // Creation references (keys starting with '#') are populated after the create loop.
    let has_on_success = args
        .get("onSuccessUpdateEmail")
        .filter(|v| !v.is_null())
        .is_some()
        || args
            .get("onSuccessDestroyEmail")
            .filter(|v| !v.is_null())
            .is_some();
    let mut submission_email_id_map: HashMap<String, Id> = HashMap::new();
    if has_on_success {
        let mut non_ref_id_set: HashSet<Id> = HashSet::new();
        if let Some(m) = args.get("onSuccessUpdateEmail").and_then(|v| v.as_object()) {
            // `#` creation refs and malformed ids match no stored
            // submission, so only valid literal ids are prefetched.
            non_ref_id_set.extend(m.keys().filter_map(|k| Id::new_validated(k.as_str()).ok()));
        }
        if let Some(arr) = args.get("onSuccessDestroyEmail").and_then(|v| v.as_array()) {
            non_ref_id_set.extend(
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| Id::new_validated(s).ok()),
            );
        }
        let non_ref_ids: Vec<Id> = non_ref_id_set.into_iter().collect();
        if !non_ref_ids.is_empty() {
            let (subs, _) = backend
                .get_objects::<EmailSubmission>(caller, &account_id, Some(&non_ref_ids), None)
                .await
                .map_err(|e| server_fail_from_backend(&e))?;
            for sub in subs {
                submission_email_id_map.insert(sub.id.as_ref().to_owned(), sub.email_id.clone());
            }
        }
    }

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------

    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, create_args) in create_map {
            match process_create(
                backend,
                caller,
                &account_id,
                create_id,
                create_args,
                created_ids,
            )
            .await
            {
                Ok(obj_json) => {
                    // Populate submission → email_id map for onSuccess* processing.
                    if let Some(eid) = obj_json.get("emailId").and_then(|v| v.as_str()) {
                        submission_email_id_map.insert(format!("#{create_id}"), Id::from(eid));
                    }
                    created.insert(create_id.clone(), obj_json);
                }
                Err(err) => {
                    let err_json = match err {
                        CreateError::SetError(se) => serde_json::Value::from(&*se),
                        CreateError::Server(jmap_err) => {
                            // server_fail_from_backend already stripped any
                            // backend Display text at the source call site;
                            // we just need the wire shape that matches
                            // the JmapError's type + description.
                            let mut obj = serde_json::Map::new();
                            obj.insert(
                                "type".to_owned(),
                                Value::String(jmap_err.error_type.to_string()),
                            );
                            if let Some(desc) = &jmap_err.description {
                                obj.insert("description".to_owned(), Value::String(desc.clone()));
                            }
                            Value::Object(obj)
                        }
                    };
                    not_created.insert(create_id.clone(), err_json);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------

    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_updated.insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };
            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id_str.clone(),
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };
            match process_update(backend, caller, &account_id, &id, patch).await {
                Ok(Some(obj)) => {
                    updated.insert(id_str.clone(), jmap_server::serialize_value(&obj)?);
                }
                Ok(None) => {
                    updated.insert(id_str.clone(), Value::Null);
                }
                Err(e) => {
                    not_updated.insert(id_str.clone(), backend_set_error_value(e));
                }
            }
        }
    }
    // e53.46: remove failed updates from the onSuccess map so their side
    // effects are not applied.
    for id_str in not_updated.keys() {
        submission_email_id_map.remove(id_str);
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------

    if let Some(destroy_ids) = args.get("destroy").and_then(|v| v.as_array()) {
        // RFC 8620 §5.3: the destroy array is Id[]. A non-string element is a
        // malformed request; return invalidArguments for the whole request.
        let id_strs: Vec<&str> = destroy_ids
            .iter()
            .map(|v| {
                v.as_str().ok_or_else(|| {
                    JmapError::invalid_arguments(format!(
                        "destroy array must contain only Id strings; got {}",
                        jmap_server::json_type_name(v)
                    ))
                })
            })
            .collect::<Result<_, _>>()?;
        for id_str in id_strs {
            let id = match jmap_server::parse_wire_id(id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_destroyed.insert(id_str.to_owned(), serde_json::Value::from(&*err));
                    continue;
                }
            };
            match backend
                .destroy_object::<EmailSubmission>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    destroyed.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    not_destroyed.insert(id_str.to_owned(), backend_set_error_value(e));
                }
            }
        }
        // e53.46: remove failed destroys from the onSuccess map so their side
        // effects are not applied.
        for id_str in not_destroyed.keys() {
            submission_email_id_map.remove(id_str);
        }
    }

    let mutated = !created.is_empty() || !updated.is_empty() || !destroyed.is_empty();
    let new_state = if mutated {
        backend
            .get_state::<EmailSubmission>(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
    } else {
        old_state.clone()
    };

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
        "updated": if updated.is_empty() { Value::Null } else { Value::Object(updated) },
        "destroyed": if destroyed.is_empty() { Value::Null } else { Value::Array(destroyed) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
        "notUpdated": if not_updated.is_empty() { Value::Null } else { Value::Object(not_updated) },
        "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
    });

    // -----------------------------------------------------------------------
    // onSuccessUpdateEmail / onSuccessDestroyEmail (RFC 8621 §7.5)
    //
    // Keys are EmailSubmission IDs or creation references ("#<create_id>").
    // Only apply the side effect if the referenced operation succeeded, i.e.
    // the key is present in submission_email_id_map.
    // -----------------------------------------------------------------------

    let mut extra_invocations: Vec<Invocation> = Vec::new();

    if has_on_success {
        let email_old_state = backend
            .get_state::<Email>(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;

        let mut email_updated = serde_json::Map::new();
        let mut email_not_updated = serde_json::Map::new();
        let mut email_destroyed: Vec<Value> = Vec::new();
        let mut email_not_destroyed = serde_json::Map::new();

        // onSuccessUpdateEmail
        // patch_val.clone() is required here: args is borrowed as &Value throughout
        // handle_submission_set (used in multiple subsequent blocks), so we cannot
        // take ownership of the nested map via remove(). The clone is bounded to
        // the patch object, not the full args.
        if let Some(update_patches) = args.get("onSuccessUpdateEmail").and_then(|v| v.as_object()) {
            for (sub_key, patch_val) in update_patches {
                let email_id = match submission_email_id_map.get(sub_key.as_str()) {
                    Some(id) => id.clone(),
                    None => continue, // Referenced operation did not succeed; skip.
                };
                // RFC 8620 §5.3: `mailboxIds/#m` paths and `mailboxIds`
                // keys may name a Mailbox created earlier in the request.
                let mut patch_val = patch_val.clone();
                if let Err(err) = crate::email::resolve_mailbox_id_refs(&mut patch_val, created_ids)
                {
                    email_not_updated.insert(email_id.as_ref().to_owned(), Value::from(&*err));
                    continue;
                }
                // Convert wire-format Value into a typed PatchObject before
                // the immutable-field guard. RFC 8620 §5.3.
                let patch = match PatchObject::deserialize(&patch_val) {
                    Ok(p) => p,
                    Err(e) => {
                        email_not_updated.insert(
                            email_id.as_ref().to_owned(),
                            json!({
                                "type": "invalidPatch",
                                "description": e.to_string()
                            }),
                        );
                        continue;
                    }
                };
                // Apply same immutable-field guard as handle_email_set patches.
                if let Some(err) = reject_read_only_patch::<Email>(patch.as_map(), None) {
                    email_not_updated
                        .insert(email_id.as_ref().to_owned(), serde_json::Value::from(&err));
                    continue;
                }
                match backend
                    .update_object::<Email>(caller, &account_id, &email_id, patch)
                    .await
                {
                    Ok(Some(obj)) => {
                        email_updated.insert(
                            email_id.as_ref().to_owned(),
                            jmap_server::serialize_value(&obj)?,
                        );
                    }
                    Ok(None) => {
                        email_updated.insert(email_id.as_ref().to_owned(), Value::Null);
                    }
                    Err(e) => {
                        email_not_updated
                            .insert(email_id.as_ref().to_owned(), backend_set_error_value(e));
                    }
                }
            }
        }

        // onSuccessDestroyEmail
        if let Some(destroy_keys) = args.get("onSuccessDestroyEmail").and_then(|v| v.as_array()) {
            for key_val in destroy_keys {
                let Some(sub_key) = key_val.as_str() else {
                    continue;
                };
                let email_id = match submission_email_id_map.get(sub_key) {
                    Some(id) => id.clone(),
                    None => continue, // Referenced operation did not succeed; skip.
                };
                match backend
                    .destroy_object::<Email>(caller, &account_id, &email_id)
                    .await
                {
                    Ok(()) => {
                        email_destroyed.push(Value::String(email_id.as_ref().to_owned()));
                    }
                    Err(e) => {
                        email_not_destroyed
                            .insert(email_id.as_ref().to_owned(), backend_set_error_value(e));
                    }
                }
            }
        }

        // RFC 8621 §7.5: only emit the implicit Email/set if at least one
        // email operation was attempted. If all referenced creates failed, the
        // map is empty and there is nothing to report.
        let any_email_ops = !email_updated.is_empty()
            || !email_not_updated.is_empty()
            || !email_destroyed.is_empty()
            || !email_not_destroyed.is_empty();

        if any_email_ops {
            let email_new_state = backend
                .get_state::<Email>(caller, &account_id)
                .await
                .map_err(|e| server_fail_from_backend(&e))?;

            // RFC 8621 §7.5: a single implicit Email/set response is appended
            // after the EmailSubmission/set response. Call-id is the same as
            // the originating EmailSubmission/set call (RFC 8620 §3.2).
            let email_set_resp = json!({
                "accountId": account_id.as_ref(),
                "oldState": email_old_state.as_ref(),
                "newState": email_new_state.as_ref(),
                "created": Value::Null,
                "updated": if email_updated.is_empty() { Value::Null } else { Value::Object(email_updated) },
                "destroyed": if email_destroyed.is_empty() { Value::Null } else { Value::Array(email_destroyed) },
                "notCreated": Value::Null,
                "notUpdated": if email_not_updated.is_empty() { Value::Null } else { Value::Object(email_not_updated) },
                "notDestroyed": if email_not_destroyed.is_empty() { Value::Null } else { Value::Object(email_not_destroyed) },
            });
            extra_invocations.push(Invocation::new(
                "Email/set",
                email_set_resp,
                call_id.to_owned(),
            ));
        }
    }

    Ok(HandlerOutput::new(resp).with_extra(extra_invocations))
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Validate that an email address string contains no CR or LF characters.
pub(crate) fn check_no_crlf(s: &str) -> bool {
    !s.contains('\r') && !s.contains('\n')
}

/// Typed error for [`process_create`] — avoids `Result<Value, Value>`.
///
/// `Server` carries a fully-formed [`JmapError`] so backend `Display`
/// text never leaks into the wire — call sites use
/// [`server_fail_from_backend`] (which drops backend text and returns a
/// generic serverFail) before wrapping in this variant.
enum CreateError {
    SetError(Box<SetError>),
    Server(JmapError),
}

impl From<SetError> for CreateError {
    fn from(e: SetError) -> Self {
        Self::SetError(Box::new(e))
    }
}

/// Process a single create entry in an `EmailSubmission/set` request.
///
/// Returns the JSON for the `created` map on success, or a typed
/// [`CreateError`] (converted to JSON at the call site) on failure.
async fn process_create<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    create_id: &str,
    create_args: &Value,
    created_ids: &BTreeMap<Id, Id>,
) -> Result<Value, CreateError> {
    if let Some(err) = create_args
        .as_object()
        .and_then(reject_server_set::<EmailSubmission>)
    {
        return Err(CreateError::SetError(Box::new(err)));
    }
    // --- identityId ---
    let identity_id_str = create_args
        .get("identityId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            CreateError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(["identityId"])
                    .with_description("identityId is required"),
            ))
        })?;
    let identity_id =
        resolve_ref(identity_id_str, "identityId", created_ids).map_err(CreateError::SetError)?;

    // Validate identityId references an existing Identity.
    let (identities, _) = backend
        .get_objects::<Identity>(
            caller,
            account_id,
            Some(std::slice::from_ref(&identity_id)),
            None,
        )
        .await
        .map_err(|e| CreateError::Server(server_fail_from_backend(&e)))?;

    let identity = identities.into_iter().next().ok_or_else(|| {
        CreateError::SetError(Box::new(
            SetError::new(SetErrorType::InvalidProperties)
                .with_properties(["identityId"])
                .with_description("identityId does not reference an existing Identity"),
        ))
    })?;

    // --- emailId ---
    let email_id_str = create_args
        .get("emailId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            CreateError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(["emailId"])
                    .with_description("emailId is required"),
            ))
        })?;
    let email_id =
        resolve_ref(email_id_str, "emailId", created_ids).map_err(CreateError::SetError)?;

    // Validate emailId references an existing Email and retrieve threadId.
    let (emails, _) = backend
        .get_objects::<Email>(
            caller,
            account_id,
            Some(std::slice::from_ref(&email_id)),
            None,
        )
        .await
        .map_err(|e| CreateError::Server(server_fail_from_backend(&e)))?;

    let email = emails.into_iter().next().ok_or_else(|| {
        CreateError::SetError(Box::new(
            SetError::new(SetErrorType::InvalidProperties)
                .with_properties(["emailId"])
                .with_description("emailId does not reference an existing Email"),
        ))
    })?;
    let thread_id = email.thread_id.clone();

    // --- envelope (derive if null/absent) ---
    let envelope: Envelope = match create_args.get("envelope") {
        None | Some(Value::Null) => {
            // Derive mailFrom from Identity.email, rcptTo from Email.to + cc + bcc.
            let mail_from = Address::new(identity.email.clone());
            let mut rcpt_to: Vec<Address> = Vec::new();
            for addrs in [&email.to, &email.cc, &email.bcc].into_iter().flatten() {
                for addr in addrs {
                    rcpt_to.push(Address::new(addr.email.clone()));
                }
            }
            Envelope::new(mail_from, rcpt_to)
        }
        Some(v) => Envelope::deserialize(v).map_err(|e| {
            CreateError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(["envelope"])
                    .with_description(e.to_string()),
            ))
        })?,
    };

    // --- noRecipients check (RFC 8621 §7.5) ---
    // Applies whether the envelope was derived or supplied by the client.
    if envelope.rcpt_to.is_empty() {
        return Err(CreateError::SetError(Box::new(SetError::new(
            SetErrorType::NoRecipients,
        ))));
    }

    // --- SMTP injection defense (RFC 8621 §7.5) ---
    // Collect *all* invalid addresses; RFC 8621 §7.5 requires the complete list.
    {
        let mut invalid: Vec<&str> = Vec::new();
        if !check_no_crlf(&envelope.mail_from.email) {
            invalid.push(&envelope.mail_from.email);
        }
        for rcpt in &envelope.rcpt_to {
            if !check_no_crlf(&rcpt.email) {
                invalid.push(&rcpt.email);
            }
        }
        if !invalid.is_empty() {
            return Err(CreateError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidRecipients)
                    .with_invalid_recipients(invalid)
                    .with_description("one or more addresses contain CR or LF"),
            )));
        }
    }

    // --- sendAt is server-set (RFC 8621 §7.2: sendAt is set by the server) ---
    // Any client-supplied sendAt is ignored.
    let send_at: UTCDate = now_utc_string();

    // --- Build delivery status for each rcptTo ---
    // Delivery has not yet occurred; reflect queued state.
    let mut delivery_status: BTreeMap<String, DeliveryStatus> = BTreeMap::new();
    for rcpt in &envelope.rcpt_to {
        delivery_status.insert(
            rcpt.email.clone(),
            DeliveryStatus::new("queued", Delivered::Queued, Displayed::Unknown),
        );
    }

    // --- Build submission object ---
    let identity_id = identity_id.clone();
    let email_id = email_id.clone();
    let build = move |id: &Id| {
        let mut submission = EmailSubmission::new(
            id.clone(),
            identity_id,
            email_id,
            thread_id,
            send_at,
            UndoStatus::Final,
        );
        submission.envelope = Some(envelope);
        submission.delivery_status = if delivery_status.is_empty() {
            None
        } else {
            Some(delivery_status)
        };
        submission
    };

    let (_server_id, created_obj) = backend
        .create_object::<EmailSubmission, _>(caller, account_id, create_id, build)
        .await
        .map_err(|e| match e {
            BackendSetError::SetError(set_err) => CreateError::SetError(set_err),
            other => CreateError::Server(server_fail_from_backend(&other)),
        })?;

    // create_object guarantees created_obj.id == server_id; serialize as-is.
    jmap_server::serialize_value(&created_obj).map_err(CreateError::Server)
}

/// Process a single update entry in an `EmailSubmission/set` request.
///
/// RFC 8621 §7.5: only `undoStatus` may be updated. If the current status is
/// already `"final"`, returns `cannotUnsend`.
async fn process_update<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    patch: PatchObject,
) -> Result<Option<EmailSubmission>, BackendSetError<B::Error>> {
    // RFC 8621 §7.5: only undoStatus may be changed in an update patch.
    {
        let bad: Vec<&str> = patch
            .as_map()
            .keys()
            .filter(|k| k.as_str() != "undoStatus")
            .map(|k| k.as_str())
            .collect();
        if !bad.is_empty() {
            return Err(BackendSetError::SetError(Box::new(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(bad)
                    .with_description("only undoStatus may be changed on an EmailSubmission"),
            )));
        }
    }

    // Look up existing submission to check undoStatus.
    let (existing, not_found) = backend
        .get_objects::<EmailSubmission>(caller, account_id, Some(std::slice::from_ref(id)), None)
        .await
        .map_err(BackendSetError::Other)?;

    if !not_found.is_empty() || existing.is_empty() {
        return Err(BackendSetError::SetError(Box::new(SetError::new(
            SetErrorType::NotFound,
        ))));
    }

    let current = &existing[0];

    // Only allow updating undoStatus; if the submission is already final, reject.
    if current.undo_status == UndoStatus::Final {
        return Err(BackendSetError::SetError(Box::new(
            SetError::new(SetErrorType::CannotUnsend)
                .with_description("Submission is already in final state and cannot be undone"),
        )));
    }

    // Apply the patch via the backend.
    backend
        .update_object::<EmailSubmission>(caller, account_id, id, patch)
        .await
}
