//! ContactCard/* method handlers (RFC 9610 §3).
//!
//! Provides handlers for:
//! - `ContactCard/get`
//! - `ContactCard/changes`
//! - `ContactCard/set`
//! - `ContactCard/copy`
//! - `ContactCard/query`
//! - `ContactCard/queryChanges`
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3, `/copy` → §5.4,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by RFC 9610 §3. The returned `Value` is the
//! corresponding method-response object per the same section refs.
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

use jmap_contacts_types::{ContactCard, ContactCardComparator};
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde_json::{Value, json};

use crate::ContactsBackend;
use crate::helpers::{
    SetAccumulators, check_metadata_entry, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response, reject_unsupported_sort, resolve_map_key_refs,
};
use jmap_server::{
    HandlerOutput, apply_patch_object, backend_set_error_value, bool_arg, reject_read_only_patch,
    reject_server_set, server_fail_from_backend,
};

// ---------------------------------------------------------------------------
// ContactCard/get
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/get` method call (RFC 9610 §3.1).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_card_get<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_get::<ContactCard, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// ContactCard/changes
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/changes` method call (RFC 9610 §3.2).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); with
/// `urn:ietf:params:jmap:metadata` the request may set
/// draft-ietf-jmap-metadata-04 §3.3 `ignoreMetadataOnlyChanges`, and
/// without it metadata-only changes are left out (-04 §3).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_card_changes<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<ContactCard, B>(backend, caller, args, using).await
}

// ---------------------------------------------------------------------------
// ContactCard/set
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/set` method call (RFC 9610 §3.3).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// An `addressBookIds` key, or the id segment of an `addressBookIds/<id>`
/// patch path, that is a `#creationId` reference names an AddressBook
/// created earlier in the request and resolves against `created_ids` (pass
/// [`jmap_server::CallContext::created_ids`]; RFC 8620 §5.3). An unknown
/// reference is `invalidProperties` naming `addressBookIds`.
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]): a write to `metadata` or
/// `privateMetadata` without `urn:ietf:params:jmap:metadata` is
/// `invalidProperties` (draft-ietf-jmap-metadata-04 §3), as is `null` for
/// either in a create (-04 §3.2). The backend validates the rest.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_card_set<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: accountId not recognised → accountNotFound (method-level
    // error). Without this, a /set against an unknown accountId would silently
    // "succeed" with a fake oldState/newState envelope.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.6). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<ContactCard>(caller, &account_id)
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
            if let Some(err) = obj_val
                .as_object()
                .and_then(reject_server_set::<ContactCard>)
            {
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
            if let Err(err) = resolve_map_key_refs(&mut obj_val, "addressBookIds", created_ids) {
                not_created.insert(create_id, serde_json::Value::from(&*err));
                continue;
            }
            // `ContactCard.id` is optional, so the create parses without
            // one and `build` sets the id the backend mints.
            let mut card: ContactCard = match serde_json::from_value(obj_val) {
                Ok(c) => c,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            let build = move |id: &Id| {
                card.id = Some(id.clone());
                card
            };
            match backend
                .create_object::<ContactCard, _>(caller, &account_id, &create_id, build)
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
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_updated.insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };
            if let Err(err) = resolve_map_key_refs(&mut patch_val, "addressBookIds", created_ids) {
                not_updated.insert(id_str, serde_json::Value::from(&*err));
                continue;
            }

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
            if let Some(err) = reject_read_only_patch::<ContactCard>(patch.as_map(), None) {
                not_updated.insert(id_str, serde_json::Value::from(&err));
                continue;
            }
            if let Err(err) = check_metadata_entry(patch.as_map(), false, using) {
                not_updated.insert(id_str, serde_json::Value::from(&*err));
                continue;
            }

            match backend
                .update_object::<ContactCard>(caller, &account_id, &id, patch)
                .await
            {
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
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
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
        for s in id_strs {
            let id_str = s.to_owned();
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_destroyed.insert(id_str, serde_json::Value::from(&*err));
                    continue;
                }
            };

            match backend
                .destroy_object::<ContactCard>(caller, &account_id, &id)
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

    finalize_set_response::<B, ContactCard>(
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
// ContactCard/copy
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/copy` method call (RFC 9610 §3.4 / RFC 8620 §5.4).
///
/// `args` is the RFC 8620 §5.4 `/copy` request shape (`fromAccountId`,
/// optional `ifFromInState`, `accountId`, optional `ifInState`, `create`
/// map of creationId → object, optional `onSuccessDestroyOriginal`,
/// optional `destroyFromIfInState`); the returned `Value` is the §5.4
/// `/copy` response shape (`fromAccountId`, `accountId`, `oldState`,
/// `newState`, `created` / `notCreated` maps).
///
/// Fetches cards from `fromAccountId`, delegates copy to the backend, and
/// returns `copied`/`notCopied` maps.
///
/// Implements the RFC 8620 §5.4 mandates inherited by ContactCard/copy:
/// `fromAccountId` MUST differ from `accountId` (rejected with
/// `invalidArguments`); `ifFromInState` is checked against the source
/// account's current state; `onSuccessDestroyOriginal` triggers an
/// implicit `ContactCard/set destroy` against the source account and
/// emits a synthetic `ContactCard/set` invocation per RFC 8620 §6.3;
/// `destroyFromIfInState` (if supplied) gates the implicit destroy
/// against the source state at destroy time.
///
/// Returns `(response_args, extra_invocations)`. Extra invocations are
/// generated when `onSuccessDestroyOriginal: true`, per RFC 8620 §6.3. If
/// `destroyFromIfInState` does not match the source account's ContactCard
/// state, the single extra invocation is an `error` of type `stateMismatch`
/// and the originals are left untouched.
pub async fn handle_contact_card_copy<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    call_id: &str,
) -> Result<HandlerOutput, JmapError> {
    let (to_account_id, mut args) = extract_account_id(args)?;

    let from_account_id = match args.get("fromAccountId").and_then(|v| v.as_str()) {
        Some(s) => Id::new_validated(s)
            .map_err(|e| JmapError::invalid_arguments(format!("fromAccountId: {e}")))?,
        None => return Err(JmapError::invalid_arguments("fromAccountId is required")),
    };

    // RFC 8620 §5.4: fromAccountId MUST differ from accountId. Without
    // this check, a same-account /copy fetches and re-inserts the same
    // card, producing a duplicate id in one account (silent data
    // integrity bug — see bd:JMAP-qz9v.2).
    if from_account_id == to_account_id {
        return Err(JmapError::invalid_arguments(
            "fromAccountId must be different from accountId",
        ));
    }

    // Verify both accounts exist.
    let to_exists = backend
        .account_exists(caller, &to_account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    if !to_exists {
        return Err(JmapError::account_not_found());
    }

    let from_exists = backend
        .account_exists(caller, &from_account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    if !from_exists {
        return Err(JmapError::from_account_not_found());
    }

    let on_success_destroy_original: bool = bool_arg(&args, "onSuccessDestroyOriginal", false);

    let destroy_from_if_in_state: Option<String> = args
        .get("destroyFromIfInState")
        .and_then(|v| v.as_str())
        .map(String::from);

    // RFC 8620 §5.4: ifFromInState — check source account state at the
    // start of the method. Mismatch aborts the whole /copy with
    // stateMismatch.
    if let Some(if_from_in_state) = args.get("ifFromInState").and_then(|v| v.as_str()) {
        let from_state = backend
            .get_state::<ContactCard>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        if if_from_in_state != from_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let old_state = backend
        .get_state::<ContactCard>(caller, &to_account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    let mut copied = serde_json::Map::new();
    let mut not_copied = serde_json::Map::new();
    let mut mutated = false;
    // (copy_id, source_id) for each successfully copied entry — used to
    // drive the implicit destroy when onSuccessDestroyOriginal is true.
    let mut copied_source_ids: Vec<(String, Id)> = Vec::new();

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, spec_val) in create_map {
            // The copy spec is an object with an "id" of the source card and
            // optionally patch fields to apply after copy (RFC 8620 §6.3).
            let Some(source_id_str) = spec_val.get("id").and_then(|v| v.as_str()) else {
                not_copied.insert(
                    create_id,
                    json!({ "type": "invalidProperties", "description": "id is required in copy spec" }),
                );
                continue;
            };
            let source_id = match jmap_server::parse_wire_id(source_id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_copied.insert(create_id, serde_json::Value::from(&*err));
                    continue;
                }
            };

            // Fetch the source card.
            let (mut cards, not_found) = backend
                .get_objects::<ContactCard>(
                    caller,
                    &from_account_id,
                    Some(std::slice::from_ref(&source_id)),
                    None,
                )
                .await
                .map_err(|e| server_fail_from_backend(&e))?;

            if !not_found.is_empty() || cards.is_empty() {
                not_copied.insert(create_id, json!({ "type": "notFound" }));
                continue;
            }

            let mut card = cards.remove(0);

            // Apply any patch fields from the copy spec (RFC 8620 §6.3).
            // Keys are RFC 8620 §5.3 PatchObject paths; a path the shared
            // `apply_patch_object` rejects surfaces as invalidPatch.
            //
            // serialize-roundtrip-then-patch: serialize the source card to
            // JSON, mutate the JSON via apply_patch_object, then deserialize
            // back to ContactCard. Failure to deserialize the patched JSON
            // signals that the patch produced a structurally invalid card
            // (e.g. wrong type for a typed field); surface as an
            // invalidProperties SetError per RFC 8620 §5.3 rather than
            // silently using the un-patched card and claiming success.
            if let Value::Object(spec_obj) = &spec_val {
                let mut card_val = jmap_server::serialize_value(&card)?;
                if let Value::Object(ref mut merged_map) = card_val {
                    let mut patch = spec_obj.clone();
                    patch.remove("id");
                    if let Err(e) = apply_patch_object(merged_map, &patch) {
                        not_copied.insert(
                            create_id,
                            json!({ "type": "invalidPatch", "description": e.to_string() }),
                        );
                        continue;
                    }
                }
                card = match serde_json::from_value(card_val) {
                    Ok(c) => c,
                    Err(e) => {
                        not_copied.insert(
                            create_id,
                            json!({
                                "type": "invalidProperties",
                                "description":
                                    format!("patch produced invalid ContactCard: {e}"),
                            }),
                        );
                        continue;
                    }
                };
            }

            match backend
                .copy_object(caller, &from_account_id, &to_account_id, card)
                .await
            {
                Ok((_new_id, copied_obj)) => {
                    mutated = true;
                    copied.insert(
                        create_id.clone(),
                        jmap_server::serialize_value(&copied_obj)?,
                    );
                    copied_source_ids.push((create_id, source_id));
                }
                Err(e) => {
                    not_copied.insert(create_id, backend_set_error_value(e));
                }
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<ContactCard>(caller, &to_account_id)
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
        "copied":    if copied.is_empty()     { Value::Null } else { Value::Object(copied) },
        "notCopied": if not_copied.is_empty() { Value::Null } else { Value::Object(not_copied) },
    });

    // RFC 8620 §5.4: onSuccessDestroyOriginal — destroy each successfully
    // copied source card and emit a single implicit ContactCard/set
    // response. The dispatcher appends extra invocations verbatim to
    // methodResponses, so the full response object is built here.
    let mut extra: Vec<Invocation> = Vec::new();

    if on_success_destroy_original && !copied_source_ids.is_empty() {
        let from_old_state = backend
            .get_state::<ContactCard>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;

        let mut destroyed: Vec<Value> = Vec::new();
        let mut not_destroyed = serde_json::Map::new();

        // RFC 8620 §5.4: `destroyFromIfInState` "is passed on as the
        // "ifInState" argument to the implicit "Foo/set" call"; per §5.3 a
        // mismatch aborts that whole call with a method-level stateMismatch
        // error in place of the ContactCard/set response. Nothing is
        // destroyed; the /copy itself already succeeded.
        if destroy_from_if_in_state
            .as_deref()
            .is_some_and(|expected| expected != from_old_state.as_ref())
        {
            extra.push(Invocation::new(
                "error",
                json!({ "type": "stateMismatch" }),
                call_id.to_owned(),
            ));
            return Ok(HandlerOutput::new(resp).with_extra(extra));
        }

        for (_, source_id) in &copied_source_ids {
            match backend
                .destroy_object::<ContactCard>(caller, &from_account_id, source_id)
                .await
            {
                Ok(()) => {
                    destroyed.push(Value::String(source_id.as_ref().to_owned()));
                }
                Err(e) => {
                    not_destroyed.insert(source_id.as_ref().to_owned(), backend_set_error_value(e));
                }
            }
        }

        let from_new_state = backend
            .get_state::<ContactCard>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;

        let set_resp = json!({
            "accountId": from_account_id.as_ref(),
            "oldState": from_old_state.as_ref(),
            "newState": from_new_state.as_ref(),
            "created": Value::Null,
            "updated": Value::Null,
            "destroyed": if destroyed.is_empty() { Value::Null } else { Value::Array(destroyed) },
            "notCreated": Value::Null,
            "notUpdated": Value::Null,
            "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
        });
        extra.push(Invocation::new(
            "ContactCard/set",
            set_resp,
            call_id.to_owned(),
        ));
    }

    Ok(HandlerOutput::new(resp).with_extra(extra))
}

// ---------------------------------------------------------------------------
// ContactCard/query
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/query` method call (RFC 9610 §3.3).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`); the
/// returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_card_query<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    reject_unsupported_sort::<ContactCardComparator>(args.get("sort"))?;
    jmap_server::handle_query::<ContactCard, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// ContactCard/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/queryChanges` method call (RFC 9610 §3.4).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_contact_card_query_changes<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    reject_unsupported_sort::<ContactCardComparator>(args.get("sort"))?;
    jmap_server::handle_query_changes::<ContactCard, B>(backend, caller, args).await
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
    /// on". RFC 9610 §3.3.2 lists created and updated (MUST) and name/given,
    /// name/surname and name/surname2 (SHOULD); `kind` is not among them.
    #[tokio::test]
    async fn query_unlisted_sort_property_returns_unsupported_sort() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sort": [{ "property": "kind" }] });
        let err = handle_contact_card_query(&backend, &(), args)
            .await
            .expect_err("unlisted sort property must be rejected");
        assert_eq!(err.error_type.to_string(), "unsupportedSort");
    }

    /// Oracle: as above; `ContactCard/queryChanges` carries the same `sort`
    /// (RFC 8620 §5.6).
    #[tokio::test]
    async fn query_changes_unlisted_sort_property_returns_unsupported_sort() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "sinceQueryState": "0",
            "sort": [{ "property": "name/surname" }, { "property": "kind" }]
        });
        let err = handle_contact_card_query_changes(&backend, &(), args)
            .await
            .expect_err("unlisted sort property must be rejected");
        assert_eq!(err.error_type.to_string(), "unsupportedSort");
    }

    /// Oracle: RFC 9610 §3.3.2 lists created and updated (MUST) and name/given,
    /// name/surname and name/surname2 (SHOULD), so a `name/surname` sort is accepted.
    #[tokio::test]
    async fn query_listed_sort_property_is_accepted() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sort": [{ "property": "name/surname" }] });
        handle_contact_card_query(&backend, &(), args)
            .await
            .expect("listed sort property must not be rejected");
    }

    /// Oracle: RFC 8620 §5.5 Comparator: "property: `String`" is required, so
    /// a comparator without it is not syntactically valid and is
    /// `invalidArguments` (RFC 8620 §3.6.2), not `unsupportedSort`.
    #[tokio::test]
    async fn query_comparator_without_property_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sort": [{ "isAscending": false }] });
        let err = handle_contact_card_query(&backend, &(), args)
            .await
            .expect_err("comparator without property must be rejected");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
    }

    /// Oracle: ContactCard/get with unknown account → accountNotFound.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let err = handle_contact_card_get(&backend, &(), args)
            .await
            .expect_err("must return error for unknown account");
        assert_eq!(err.error_type.to_string(), "accountNotFound");
    }

    /// Oracle: ContactCard/changes with known account returns valid response.
    #[tokio::test]
    async fn changes_known_account_returns_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sinceState": "0" });
        let HandlerOutput { primary: resp, .. } =
            handle_contact_card_changes(&backend, &(), args, &[])
                .await
                .expect("must not error");
        assert_eq!(resp["accountId"], "acc1");
    }

    /// Oracle: ContactCard/query with known account returns valid response.
    #[tokio::test]
    async fn query_known_account_returns_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "filter": null, "sort": null });
        let HandlerOutput { primary: resp, .. } = handle_contact_card_query(&backend, &(), args)
            .await
            .expect("must not error");
        assert_eq!(resp["accountId"], "acc1");
    }

    /// Oracle: ContactCard/queryChanges with known account returns valid response.
    #[tokio::test]
    async fn query_changes_known_account_returns_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sinceQueryState": "0" });
        let HandlerOutput { primary: resp, .. } =
            handle_contact_card_query_changes(&backend, &(), args)
                .await
                .expect("must not error");
        assert_eq!(resp["accountId"], "acc1");
    }

    /// Oracle: ContactCard/copy with unknown fromAccountId → fromAccountNotFound.
    #[tokio::test]
    async fn copy_unknown_from_account_returns_from_account_not_found() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "unknown",
            "create": { "c1": { "id": "card1" } }
        });
        let err = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect_err("must return error for unknown fromAccountId");
        assert_eq!(err.error_type.to_string(), "fromAccountNotFound");
    }

    /// Oracle: ContactCard/copy with unknown source id returns notCopied with
    /// type "notFound" (RFC 8620 §6.3).
    ///
    /// Uses the dispatcher path (register_contacts_handlers) so that the full
    /// handler dispatch stack is exercised.
    #[tokio::test]
    async fn copy_source_not_found_returns_not_found() {
        use std::sync::Arc;

        use jmap_server::{Dispatcher, JmapRequest, State};

        use crate::JMAP_CONTACTS_URI;
        use crate::register_contacts_handlers;

        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        // Do NOT seed the source card — get_objects will report not_found.

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_contacts_handlers(&mut dispatcher, Arc::new(backend));

        let req = JmapRequest::new(
            vec![JMAP_CONTACTS_URI.into()],
            vec![Invocation::new(
                "ContactCard/copy",
                json!({
                    "accountId": "acc2",
                    "fromAccountId": "acc1",
                    "create": { "c1": { "id": "nonexistent" } }
                }),
                "c0",
            )],
            None,
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let Invocation {
            arguments: args, ..
        } = &resp.method_responses[0];

        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        assert!(
            args["copied"].is_null(),
            "copied must be null when source not found: {args}"
        );
        assert_eq!(
            args["notCopied"]["c1"]["type"], "notFound",
            "unknown source id must yield notFound: {args}"
        );
    }

    /// Oracle: ContactCard/copy with a patch override correctly applies the
    /// patch to the copied card (RFC 8620 §6.3 / §5.4 patch semantics).
    ///
    /// Seeds a card with addressBookIds: {ab1:true}; copies it with override
    /// addressBookIds: {ab2:true}; verifies the copied result has ab2, not ab1.
    ///
    /// Uses the dispatcher path (register_contacts_handlers).
    #[tokio::test]
    async fn copy_applies_flat_patch_correctly() {
        use std::sync::Arc;

        use jmap_server::{Dispatcher, JmapRequest, State};

        use crate::JMAP_CONTACTS_URI;
        use crate::register_contacts_handlers;

        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_contacts_handlers(&mut dispatcher, Arc::new(backend));

        let req = JmapRequest::new(
            vec![JMAP_CONTACTS_URI.into()],
            vec![Invocation::new(
                "ContactCard/copy",
                json!({
                    "accountId": "acc2",
                    "fromAccountId": "acc1",
                    "create": {
                        "c1": {
                            "id": "card1",
                            "addressBookIds": { "ab2": true }
                        }
                    }
                }),
                "c0",
            )],
            None,
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let Invocation {
            arguments: args, ..
        } = &resp.method_responses[0];

        assert!(
            args.get("type").is_none(),
            "must not be a top-level error: {args}"
        );
        let copied = &args["copied"];
        assert!(copied.is_object(), "copied must be present: {args}");
        let c1 = &copied["c1"];
        assert!(c1.is_object(), "c1 must appear in copied: {args}");
        assert_eq!(
            c1["addressBookIds"],
            json!({ "ab2": true }),
            "patch must have replaced addressBookIds: {c1}"
        );
        assert_ne!(
            c1["addressBookIds"],
            json!({ "ab1": true }),
            "old addressBookIds must have been replaced: {c1}"
        );
    }

    /// Oracle: ContactCard/copy calls copy_object on the backend.
    ///
    /// Source: RFC 9610 §3.4 — copy must succeed when both accounts exist
    /// and the source card is found.
    #[tokio::test]
    async fn copy_calls_backend_copy_object() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "create": { "c1": { "id": "card1" } }
        });
        let HandlerOutput { primary: resp, .. } =
            handle_contact_card_copy(&backend, &(), args, "c0")
                .await
                .expect("must not return top-level error");

        let copied = &resp["copied"];
        assert!(
            copied.is_object(),
            "copied must be present when copy succeeds: {resp}"
        );
        assert!(copied["c1"].is_object(), "c1 must appear in copied: {resp}");
    }

    /// Oracle: ContactCard/set with create returns created entry on success.
    #[tokio::test]
    async fn set_create_returns_created_entry() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "addressBookIds": { "ab1": true }
                }
            }
        });
        let HandlerOutput { primary: resp, .. } =
            handle_contact_card_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
                .await
                .expect("must not return top-level error");
        let created = &resp["created"];
        assert!(
            created.is_object(),
            "created must be present on successful create: {resp}"
        );
        assert!(
            created["c1"].is_object(),
            "c1 must appear in created: {resp}"
        );
    }

    /// Oracle: ContactCard/copy with a flat (top-level) patch key replaces the
    /// field correctly and does not leave a literal slash-containing key.
    ///
    /// Source: RFC 8620 §5.4 — a patch with key "addressBookIds" (no slash)
    /// must replace the top-level field, not nest it.
    #[tokio::test]
    async fn copy_with_flat_patch_applies_correctly() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        // Spec overrides addressBookIds from {"ab1":true} → {"ab2":true}.
        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "create": {
                "c1": {
                    "id": "card1",
                    "addressBookIds": { "ab2": true }
                }
            }
        });
        let HandlerOutput { primary: resp, .. } =
            handle_contact_card_copy(&backend, &(), args, "c0")
                .await
                .expect("must not return top-level error");

        let copied = &resp["copied"];
        assert!(copied.is_object(), "copied must be present: {resp}");
        let c1 = &copied["c1"];
        assert!(c1.is_object(), "c1 must appear in copied: {resp}");

        // The merged card must have addressBookIds == {"ab2":true}.
        assert_eq!(
            c1["addressBookIds"],
            json!({ "ab2": true }),
            "flat patch must replace addressBookIds: {c1}"
        );
        // Must NOT have a literal top-level key called "addressBookIds" with the
        // old value still sitting alongside the new one.
        assert_ne!(
            c1["addressBookIds"],
            json!({ "ab1": true }),
            "old addressBookIds must have been replaced: {c1}"
        );
    }

    /// Oracle: ContactCard/copy with a source id not found in fromAccountId
    /// returns notCopied with type "notFound".
    ///
    /// Source: RFC 8620 §6.3 — unknown source ids must appear in notCopied.
    #[tokio::test]
    async fn copy_source_not_found() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        // Do NOT seed "nonexistent" into acc1 — get_objects will return not_found.

        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "create": {
                "c1": { "id": "nonexistent" }
            }
        });
        let HandlerOutput { primary: resp, .. } =
            handle_contact_card_copy(&backend, &(), args, "c0")
                .await
                .expect("must not return top-level error");

        assert!(
            resp["copied"].is_null(),
            "copied must be null when source not found: {resp}"
        );
        assert_eq!(
            resp["notCopied"]["c1"]["type"], "notFound",
            "unknown source id must yield notFound: {resp}"
        );
    }

    // -----------------------------------------------------------------------
    // RFC 8620 §5.4 mandates for ContactCard/copy (bd:JMAP-qz9v.2)
    // -----------------------------------------------------------------------

    /// Oracle (RFC 8620 §5.4): `fromAccountId` MUST differ from `accountId`.
    /// Same-account /copy is rejected with invalidArguments — previously it
    /// silently proceeded and produced a duplicate id in one account.
    #[tokio::test]
    async fn copy_same_account_rejected_with_invalid_arguments() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_contact_card("acc1", "card1");

        let args = json!({
            "accountId": "acc1",
            "fromAccountId": "acc1",
            "create": { "c1": { "id": "card1" } }
        });
        let err = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect_err("same-account copy must error");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
    }

    /// Oracle (RFC 8620 §5.4): `ifFromInState` checks the SOURCE account
    /// state. Mismatch aborts the /copy with stateMismatch.
    #[tokio::test]
    async fn copy_if_from_in_state_mismatch_rejected_with_state_mismatch() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        // MockBackend.get_state always returns "0"; provide a different
        // expected value to trigger the mismatch.
        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "ifFromInState": "stale-state-value",
            "create": { "c1": { "id": "card1" } }
        });
        let err = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect_err("ifFromInState mismatch must error");
        assert_eq!(err.error_type.to_string(), "stateMismatch");
    }

    /// Oracle (RFC 8620 §5.4): `ifFromInState` matching the source state
    /// allows the /copy to proceed normally.
    #[tokio::test]
    async fn copy_if_from_in_state_match_proceeds() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "ifFromInState": "0", // MockBackend.get_state always returns "0"
            "create": { "c1": { "id": "card1" } }
        });
        let HandlerOutput { primary: resp, .. } =
            handle_contact_card_copy(&backend, &(), args, "c0")
                .await
                .expect("ifFromInState match must succeed");
        assert!(
            resp["copied"]["c1"].is_object(),
            "copy must succeed when ifFromInState matches: {resp}"
        );
    }

    /// Oracle (RFC 8620 §5.4): `onSuccessDestroyOriginal: true` emits a
    /// synthetic `ContactCard/set` invocation per RFC 8620 §6.3. The
    /// invocation carries `accountId == fromAccountId`, the source
    /// account's old and new states, and one entry per copied source id
    /// (either in `destroyed` or `notDestroyed`).
    #[tokio::test]
    async fn copy_on_success_destroy_original_emits_synthetic_set_invocation() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "onSuccessDestroyOriginal": true,
            "create": { "c1": { "id": "card1" } }
        });
        let HandlerOutput {
            extra_invocations: extra,
            ..
        } = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect("/copy must succeed");

        assert_eq!(
            extra.len(),
            1,
            "onSuccessDestroyOriginal must emit exactly one synthetic invocation"
        );
        let Invocation {
            name: method,
            arguments: set_resp,
            call_id: returned_call_id,
        } = &extra[0];
        assert_eq!(method, "ContactCard/set");
        assert_eq!(returned_call_id, "c0", "call_id must be echoed");
        assert_eq!(
            set_resp["accountId"], "acc1",
            "synthetic /set targets fromAccountId: {set_resp}"
        );
        // MockBackend.destroy_object always returns NotFound, so the
        // source id ends up in notDestroyed rather than destroyed. The
        // important assertion is that the destroy was attempted.
        assert!(
            set_resp["notDestroyed"]["card1"].is_object(),
            "source id must appear in synthetic /set notDestroyed: {set_resp}"
        );
    }

    /// Oracle (RFC 8620 §5.4): `onSuccessDestroyOriginal: false` (default)
    /// is a no-op — no synthetic invocation is emitted.
    #[tokio::test]
    async fn copy_on_success_destroy_original_default_false_emits_no_extra() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "create": { "c1": { "id": "card1" } }
        });
        let HandlerOutput {
            extra_invocations: extra,
            ..
        } = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect("/copy must succeed");
        assert!(
            extra.is_empty(),
            "default onSuccessDestroyOriginal=false must emit no extra invocations"
        );
    }

    /// Oracle (RFC 8620 §5.4): `onSuccessDestroyOriginal: true` with NO
    /// successful copies (e.g. all sources missing) emits no synthetic
    /// invocation — there are no source ids to destroy.
    #[tokio::test]
    async fn copy_on_success_destroy_original_no_successful_copies_emits_no_extra() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        // Do NOT seed any card — source not found.

        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "onSuccessDestroyOriginal": true,
            "create": { "c1": { "id": "missing-card" } }
        });
        let HandlerOutput {
            extra_invocations: extra,
            ..
        } = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect("/copy must succeed");
        assert!(
            extra.is_empty(),
            "no successful copies → no synthetic destroy invocation"
        );
    }

    /// Oracle: RFC 8620 §5.4 — `destroyFromIfInState` "is passed on as the
    /// "ifInState" argument to the implicit "Foo/set" call", and §5.3
    /// `ifInState`: "If supplied, the string must match the current state;
    /// otherwise, the method will be aborted and a "stateMismatch" error
    /// returned." A stale value therefore replaces the implicit
    /// ContactCard/set with a stateMismatch error and destroys nothing.
    /// The /copy itself is unaffected (it already succeeded).
    #[tokio::test]
    async fn copy_destroy_from_if_in_state_mismatch_aborts_destroy() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "onSuccessDestroyOriginal": true,
            "destroyFromIfInState": "stale-state-value",
            "create": { "c1": { "id": "card1" } }
        });
        let HandlerOutput {
            primary: resp,
            extra_invocations: extra,
            ..
        } = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect("/copy must succeed (mismatch only affects implicit destroy)");

        assert!(
            resp["copied"]["c1"].is_object(),
            "copy must succeed despite destroyFromIfInState mismatch: {resp}"
        );

        assert_eq!(extra.len(), 1, "one implicit response: {extra:?}");
        let Invocation {
            name,
            arguments,
            call_id,
        } = &extra[0];
        assert_eq!(name, "error");
        assert_eq!(arguments, &json!({ "type": "stateMismatch" }));
        assert_eq!(call_id, "c0");

        let after = handle_contact_card_get(
            &backend,
            &(),
            json!({ "accountId": "acc1", "ids": ["card1"] }),
        )
        .await
        .expect("get must succeed");
        assert_eq!(
            after.primary["list"].as_array().map(Vec::len),
            Some(1),
            "source card must be kept: {}",
            after.primary
        );
    }

    /// Oracle (bd:JMAP-qz9v.54): a patch that produces a structurally
    /// invalid ContactCard (e.g. addressBookIds set to a non-Object
    /// value) must surface as `invalidProperties` in notCopied, not be
    /// silently swallowed via `unwrap_or(card)` while claiming success.
    #[tokio::test]
    async fn copy_patch_producing_invalid_card_yields_invalid_properties() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        // addressBookIds expects Option<HashMap<Id, bool>>; a string
        // value cannot be deserialized into that type.
        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "create": {
                "c1": {
                    "id": "card1",
                    "addressBookIds": "not-an-object"
                }
            }
        });
        let HandlerOutput { primary: resp, .. } =
            handle_contact_card_copy(&backend, &(), args, "c0")
                .await
                .expect("/copy must not return a top-level error");

        assert!(
            resp["copied"].is_null(),
            "invalid-patch copy must not appear in copied: {resp}"
        );
        assert_eq!(
            resp["notCopied"]["c1"]["type"], "invalidProperties",
            "structurally invalid patched card must surface as invalidProperties: {resp}"
        );
    }

    /// Run a single-entry ContactCard/copy of the fixture card `card1`
    /// from `acc1` to `acc2` with `patch` merged into the copy spec, and
    /// return the response together with the backend for inspection.
    async fn copy_card1_with_patch(patch: Value) -> (Value, MockBackend) {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        let mut spec = json!({ "id": "card1" });
        spec.as_object_mut()
            .expect("spec is an object")
            .extend(patch.as_object().expect("patch is an object").clone());
        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "create": { "c1": spec }
        });
        let HandlerOutput { primary: resp, .. } =
            handle_contact_card_copy(&backend, &(), args, "c0")
                .await
                .expect("/copy must not return a top-level error");
        (resp, backend)
    }

    /// Assert that a rejected copy left everything as it was: `c1` is
    /// `invalidPatch` in notCopied, nothing was copied, the backend copy
    /// was never called, and the source card is byte-identical to the
    /// fixture.
    async fn assert_copy_rejected_as_invalid_patch(resp: &Value, backend: &MockBackend) {
        use jmap_server::JmapBackend;

        assert_eq!(
            resp["notCopied"]["c1"]["type"], "invalidPatch",
            "patch must be rejected as invalidPatch: {resp}"
        );
        assert!(resp["copied"].is_null(), "nothing may be copied: {resp}");
        assert!(
            !*backend.copy_called.lock().unwrap(),
            "backend copy must not be called for a rejected patch"
        );
        let (cards, not_found) = backend
            .get_objects::<ContactCard>(&(), &Id::from("acc1"), Some(&[Id::from("card1")]), None)
            .await
            .expect("get source card");
        assert!(not_found.is_empty(), "source card must still exist");
        assert_eq!(
            serde_json::to_value(&cards[0]).expect("serialize card"),
            serde_json::to_value(
                serde_json::from_value::<ContactCard>(json!({
                    "id": "card1",
                    "addressBookIds": { "ab1": true }
                }))
                .expect("fixture deserializes")
            )
            .expect("serialize fixture"),
            "source card must be unchanged"
        );
    }

    /// Oracle: RFC 8620 §5.3 — "All parts prior to the last (i.e., the
    /// value after the final slash) MUST already exist on the object
    /// being patched", else "the update MUST be rejected with an
    /// "invalidPatch" error". The fixture card has no `name`, so
    /// `name/full` must be rejected rather than creating `name`.
    #[tokio::test]
    async fn copy_patch_through_absent_parent_is_invalid_patch() {
        let (resp, backend) = copy_card1_with_patch(json!({ "name/full": "Jane Doe" })).await;
        assert_copy_rejected_as_invalid_patch(&resp, &backend).await;
    }

    /// Oracle: RFC 8620 §5.3 — "All parts prior to the last ... MUST
    /// already exist on the object being patched". `addressBookIds/ab1`
    /// is the boolean `true`, which has no member `x`, so
    /// `addressBookIds/ab1/x` must be rejected as invalidPatch and the
    /// existing boolean preserved (never clobbered into an object).
    #[tokio::test]
    async fn copy_patch_through_non_object_parent_is_invalid_patch() {
        let (resp, backend) =
            copy_card1_with_patch(json!({ "addressBookIds/ab1/x": "would-clobber" })).await;
        assert_copy_rejected_as_invalid_patch(&resp, &backend).await;
    }

    /// Oracle: RFC 8620 §5.3 — "If null, ... remove the property from the
    /// patched object" and "All parts prior to the last ... MUST already
    /// exist". A null value does not relax the parent rule: `name/full`
    /// with an absent `name` is invalidPatch, not a silent no-op.
    #[tokio::test]
    async fn copy_null_patch_through_absent_parent_is_invalid_patch() {
        let (resp, backend) = copy_card1_with_patch(json!({ "name/full": null })).await;
        assert_copy_rejected_as_invalid_patch(&resp, &backend).await;
    }

    /// Oracle: RFC 8620 §5.3 — a pointer whose parents exist is applied:
    /// "Anything else: The value to set for this property (this may be a
    /// replacement or addition to the object being patched)". `ab1`'s
    /// parent `addressBookIds` exists, so `addressBookIds/ab2` is added
    /// alongside it in the copy.
    #[tokio::test]
    async fn copy_patch_through_existing_parent_adds_member() {
        let (resp, _backend) = copy_card1_with_patch(json!({ "addressBookIds/ab2": true })).await;
        assert_eq!(
            resp["copied"]["c1"]["addressBookIds"],
            json!({ "ab1": true, "ab2": true }),
            "pointer patch must add the member to the existing map: {resp}"
        );
    }

    /// Oracle (RFC 8620 §5.4): `destroyFromIfInState` matching the
    /// source account state lets the implicit destroy proceed.
    #[tokio::test]
    async fn copy_destroy_from_if_in_state_match_proceeds() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "onSuccessDestroyOriginal": true,
            "destroyFromIfInState": "0", // MockBackend.get_state always returns "0"
            "create": { "c1": { "id": "card1" } }
        });
        let HandlerOutput {
            extra_invocations: extra,
            ..
        } = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect("/copy must succeed");

        // Destroy was attempted (MockBackend always returns NotFound for
        // destroys, so the result is notDestroyed[card1] = notFound, NOT
        // stateMismatch — the state check passed).
        let Invocation {
            arguments: set_resp,
            ..
        } = &extra[0];
        assert_eq!(
            set_resp["notDestroyed"]["card1"]["type"], "notFound",
            "destroyFromIfInState match must allow destroy attempt: {set_resp}"
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
        let err =
            handle_contact_card_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
                .await
                .expect_err("non-string destroy element must be a method error");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
        let desc = err.description.as_deref().unwrap_or_default();
        assert!(desc.contains("got object"), "{desc}");
        assert!(!desc.contains("CANARY"), "{desc}");
    }
}
