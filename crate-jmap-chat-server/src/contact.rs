//! ChatContact/* method handlers (draft-atwood-jmap-chat-00 §ChatContact).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by draft-atwood-jmap-chat-00 §ChatContact. The
//! returned `Value` is the corresponding method-response object per the
//! same section refs.
//!
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `unsupportedFilter`, `unsupportedSort`, `cannotCalculateChanges` —
//! per RFC 8620 §3.6 and §5). Per-target failures inside `/set` surface
//! in the `notCreated` / `notUpdated` / `notDestroyed` maps within
//! `Ok((Value, ...))`, not as `Err`.

use jmap_chat_types::ChatContact;
use jmap_types::{Id, Invocation, JmapError, PatchObject, State};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, SetError, SetErrorType};
use crate::helpers::{
    enforce_max_objects_in_set, extract_account_id, finalize_set_response, not_found_json,
    serialize_value, set_error_value, SetAccumulators,
};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend};

// ---------------------------------------------------------------------------
// ChatContact/get
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/get` method call (draft-atwood-jmap-chat-00 §ChatContact).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_get<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let ids: Option<Vec<Id>> = match args.remove("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<ChatContact>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<ChatContact>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(serialize_value)
        .collect::<Result<Vec<_>, _>>()?;

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "state": state.as_ref(),
            "list": list_json,
            "notFound": not_found_json(&not_found),
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// ChatContact/changes
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/changes` method call (draft-atwood-jmap-chat-00 §ChatContact).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    let since_state: State = match args.get("sinceState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceState is required")),
    };

    let max_changes: Option<u64> = match args.get("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().filter(|&n| n > 0).ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let result = backend
        .get_changes::<ChatContact>(caller, &account_id, &since_state, max_changes)
        .await
        .map_err(JmapError::from)?;

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "oldState": since_state.as_ref(),
            "newState": result.new_state.as_ref(),
            "hasMoreChanges": result.has_more_changes,
            "updatedProperties": Value::Null,
            "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
            "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
            "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// ChatContact/set
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/set` method call (draft-atwood-jmap-chat-00 §ChatContact).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// ChatContact objects are server-managed. Clients MUST NOT create or destroy
/// them; any attempt is rejected with `forbidden`. Only `update` is permitted,
/// and `id`, `firstSeenAt`, `lastSeenAt` are server-set and rejected in updates.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<ChatContact>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create — forbidden: ChatContact objects are server-managed
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
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

            // Reject patches that include server-set fields.
            // MAINTENANCE: when adding a server-set field to ChatContact in
            // jmap-chat-types, add its wire name here too. Drift between
            // this list and the struct fields is a silent client-overwrite
            // bug. Tracked workspace-wide as bd:JMAP-x2gd.87.
            const CONTACT_READONLY: &[&str] = &["id", "firstSeenAt", "lastSeenAt"];
            let bad_props: Vec<&str> = CONTACT_READONLY
                .iter()
                .copied()
                .filter(|&field| patch_val.get(field).is_some())
                .collect();
            if !bad_props.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": bad_props }),
                );
                continue;
            }

            // draft-atwood-jmap-chat-00 §ChatContact/set:
            // "update supports: blocked, displayName."
            //
            // Project the patch onto the allowed-field set so that
            // unknown / not-allowed wire fields (e.g. client-forged
            // `presence`, `lastActiveAt`) are silently dropped rather
            // than reaching `backend.update_object`. Without this
            // projection a client could overwrite server-derived state
            // such as presence, corrupting it for other observers.
            const CONTACT_UPDATE_ALLOWED: &[&str] = &["blocked", "displayName"];
            let Value::Object(mut patch_map) = patch_val else {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
                );
                continue;
            };
            let mut clean_patch = serde_json::Map::new();
            for &field in CONTACT_UPDATE_ALLOWED {
                if let Some(v) = patch_map.remove(field) {
                    clean_patch.insert(field.to_owned(), v);
                }
            }
            if clean_patch.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({
                        "type": "invalidPatch",
                        "description": "no updatable fields in patch",
                    }),
                );
                continue;
            }

            match backend
                .update_object::<ChatContact>(
                    caller,
                    &account_id,
                    &id,
                    PatchObject::from_map(clean_patch),
                )
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
    // destroy — forbidden: ChatContact objects are server-managed
    // -----------------------------------------------------------------------
    if let Some(destroy_arr) = args.get("destroy").and_then(|v| v.as_array()) {
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
                Some(s) => s,
                None => continue, // unreachable: validated above
            };
            not_destroyed.insert(
                id_str.to_owned(),
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    finalize_set_response::<B, ChatContact>(
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
// ChatContact/query
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/query` method call (draft-atwood-jmap-chat-00 §ChatContact).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`); the
/// returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// Filter and sort are passed through to the backend unchanged.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let limit: Option<u64> = match args.remove("limit") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(n) => Some(n),
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {v}"
                )))
            }
        },
    };

    let position: i64 = match args.remove("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("position: expected an integer, got {v}"))
        })?,
    };

    let filter: Option<serde_json::Value> = match args.remove("filter") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v),
    };

    let sort: Option<Vec<serde_json::Value>> = match args.remove("sort") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("sort must be an array"))?,
        ),
    };

    let result = backend
        .query_objects::<ChatContact>(
            caller,
            &account_id,
            filter.as_ref(),
            sort.as_deref(),
            limit,
            position,
        )
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": result.query_state.as_ref(),
        "canCalculateChanges": result.can_calculate_changes,
        "position": result.position,
        "ids": result.ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total {
        if let Some(t) = result.total {
            resp["total"] = json!(t);
        }
    }

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// ChatContact/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `ChatContact/queryChanges` method call (draft-atwood-jmap-chat-00 §ChatContact).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_query_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    let since_query_state: State = match args.get("sinceQueryState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceQueryState is required")),
    };

    let max_changes: Option<u64> = match args.get("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().filter(|&n| n > 0).ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let up_to_id: Option<Id> = match args.get("upToId") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Id::from(s.as_str())),
        Some(_) => {
            return Err(JmapError::invalid_arguments(
                "upToId must be a string Id or null",
            ))
        }
    };

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let result = backend
        .query_changes::<ChatContact>(
            caller,
            &account_id,
            &since_query_state,
            None,
            None,
            max_changes,
            up_to_id.as_ref(),
            false,
        )
        .await
        .map_err(JmapError::from)?;

    let removed: Vec<&str> = result.removed.iter().map(|id| id.as_ref()).collect();
    let added: Vec<Value> = result
        .added
        .iter()
        .map(|item| {
            json!({
                "id": item.id.as_ref(),
                "index": item.index,
            })
        })
        .collect();

    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "oldQueryState": result.old_query_state.as_ref(),
        "newQueryState": result.new_query_state.as_ref(),
        "removed": removed,
        "added": added,
    });
    if calculate_total {
        if let Some(t) = result.total {
            resp["total"] = json!(t);
        }
    }

    Ok((resp, vec![]))
}
