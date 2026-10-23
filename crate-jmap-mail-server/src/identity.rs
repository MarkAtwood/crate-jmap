//! Identity/get, Identity/changes, and Identity/set method handlers (RFC 8621 §6).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3), with the
//! type-specific arguments defined by RFC 8621 §6. The returned `Value`
//! is the corresponding method-response object per the same section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `cannotCalculateChanges` — per RFC 8620 §3.6 and §5). Per-target
//! failures inside `/set` surface in the `notCreated` / `notUpdated` /
//! `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use std::collections::HashSet;

use jmap_types::{Id, JmapError, PatchObject, State};
use serde_json::{json, Value};

use crate::helpers::{
    enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id, filter_properties,
    finalize_set_response, not_found_json, serialize_value, set_error_value, SetAccumulators,
};
use crate::{BackendSetError, MailBackend};
use jmap_mail_types::Identity;
use jmap_server::{
    reject_read_only_patch, reject_server_set, server_fail_from_backend,
    server_fail_value_from_backend, HandlerOutput,
};

/// Handle an `Identity/get` method call (RFC 8621 §6.1).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_identity_get<B: MailBackend>(
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

    // ids: absent or null means "return all"; Some([]) means "return nothing".
    let ids: Option<Vec<Id>> = match args.remove("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    // RFC 8620 §5.1: when `properties` is specified return only those fields
    // (plus `id` which is always included). `None` means return all fields.
    let properties: Option<Vec<String>> = match args.remove("properties") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<jmap_mail_types::Identity>(
            caller,
            &account_id,
            ids_slice,
            properties.as_deref(),
        )
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<jmap_mail_types::Identity>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let list_json: Vec<Value> = if let Some(ref props) = properties {
        // Build the effective property set once; always include "id" per RFC 8620 §5.1.
        let mut prop_set: HashSet<&str> = props.iter().map(|s| s.as_str()).collect();
        prop_set.insert("id");
        list.iter()
            .map(|obj| {
                let val = serialize_value(obj)?;
                Ok(filter_properties(&val, &prop_set))
            })
            .collect::<Result<Vec<_>, JmapError>>()?
    } else {
        list.iter()
            .map(serialize_value)
            .collect::<Result<Vec<_>, _>>()?
    };

    let resp = json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    });

    Ok(HandlerOutput::new(resp))
}

/// Handle an `Identity/changes` method call (RFC 8621 §6.2).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_identity_changes<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let since_state: State = match args.get("sinceState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceState is required")),
    };

    let max_changes = jmap_server::parse_max_changes(args.get("maxChanges"))?;

    let result = backend
        .get_changes::<jmap_mail_types::Identity>(caller, &account_id, &since_state, max_changes)
        .await
        .map_err(JmapError::from)?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": since_state.as_ref(),
        "newState": result.new_state.as_ref(),
        "hasMoreChanges": result.has_more_changes,
        "updatedProperties": Value::Null,
        "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });

    Ok(HandlerOutput::new(resp))
}

/// Handle an `Identity/set` method call (RFC 8621 §6.3).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_identity_set<B: MailBackend>(
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

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.2). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    // Fetch old state for ifInState check and response.
    let old_state = backend
        .get_state::<jmap_mail_types::Identity>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // ifInState: if provided and does not match, return stateMismatch.
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
        for (create_id, mut obj_val) in create_map {
            if let Some(err) = obj_val.as_object().and_then(reject_server_set::<Identity>) {
                not_created.insert(create_id, set_error_value(&err));
                continue;
            }
            // Validate: email must be present and non-empty.
            let email = match obj_val.get("email").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s.to_owned(),
                _ => {
                    not_created.insert(
                        create_id,
                        json!({
                            "type": "invalidProperties",
                            "properties": ["email"],
                        }),
                    );
                    continue;
                }
            };

            let name = obj_val
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let text_signature = obj_val
                .get("textSignature")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let html_signature = obj_val
                .get("htmlSignature")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let mut reply_to = None;
            if let Some(rt) = obj_val
                .get_mut("replyTo")
                .map(|v| v.take())
                .filter(|v| !v.is_null())
            {
                let Ok(v) = serde_json::from_value(rt) else {
                    not_created.insert(
                        create_id,
                        json!({
                            "type": "invalidProperties",
                            "properties": ["replyTo"],
                        }),
                    );
                    continue;
                };
                reply_to = Some(v);
            }
            let mut bcc = None;
            if let Some(b) = obj_val
                .get_mut("bcc")
                .map(|v| v.take())
                .filter(|v| !v.is_null())
            {
                let Ok(v) = serde_json::from_value(b) else {
                    not_created.insert(
                        create_id,
                        json!({
                            "type": "invalidProperties",
                            "properties": ["bcc"],
                        }),
                    );
                    continue;
                };
                bcc = Some(v);
            }

            // Build identity using the constructor (supplies all defaults), then
            // overlay optional client-supplied fields.
            let build = move |id: &Id| {
                let mut identity = jmap_mail_types::Identity::new(
                    id.clone(),
                    email,
                    true, // server-set: may_delete defaults to true on create
                );
                if let Some(name) = name {
                    identity.name = name;
                }
                if let Some(ts) = text_signature {
                    identity.text_signature = ts;
                }
                if let Some(hs) = html_signature {
                    identity.html_signature = hs;
                }
                if let Some(rt) = reply_to {
                    identity.reply_to = rt;
                }
                if let Some(b) = bcc {
                    identity.bcc = b;
                }
                identity
            };

            match backend
                .create_object::<jmap_mail_types::Identity, _>(
                    caller,
                    &account_id,
                    &create_id,
                    build,
                )
                .await
            {
                Ok((_server_id, created_obj)) => {
                    mutated = true;
                    // create_object guarantees created_obj.id == server_id;
                    // serialize the full object (id is already correct).
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

            // RFC 8621 §6: `email` is immutable; `id` and `mayDelete` are
            // server-set.
            if let Some(err) = reject_read_only_patch::<Identity>(patch.as_map(), None) {
                not_updated.insert(id_str, set_error_value(&err));
                continue;
            }

            match backend
                .update_object::<jmap_mail_types::Identity>(caller, &account_id, &id, patch)
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
    // destroy
    // -----------------------------------------------------------------------
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: the destroy array is Id[]. A non-string element is a
        // malformed request; return invalidArguments for the whole request.
        if let Some(bad) = destroy_arr.iter().find(|v| !v.is_string()) {
            return Err(JmapError::invalid_arguments(format!(
                "destroy array must contain only Id strings; got: {bad}"
            )));
        }
        for id_val in destroy_arr {
            let Value::String(id_str) = id_val else {
                continue; // unreachable: validated above
            };
            let id = Id::from(id_str.as_str());

            // Fetch the identity to check mayDelete.
            let fetch_result = backend
                .get_objects::<jmap_mail_types::Identity>(
                    caller,
                    &account_id,
                    Some(std::slice::from_ref(&id)),
                    None,
                )
                .await
                .map_err(|e| server_fail_from_backend(&e))?;

            let (found, not_found_ids) = fetch_result;

            if !not_found_ids.is_empty() {
                not_destroyed.insert(id_str, json!({ "type": "notFound" }));
                continue;
            }

            // found should have exactly one entry.
            let Some(identity) = found.into_iter().next() else {
                not_destroyed.insert(id_str, json!({ "type": "notFound" }));
                continue;
            };

            if !identity.may_delete {
                not_destroyed.insert(id_str, json!({ "type": "forbidden" }));
                continue;
            }

            match backend
                .destroy_object::<jmap_mail_types::Identity>(caller, &account_id, &id)
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

    finalize_set_response::<B, jmap_mail_types::Identity>(
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
