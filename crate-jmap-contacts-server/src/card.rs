//! ContactCard/* method handlers (RFC 9610 §3).
//!
//! Provides handlers for:
//! - `ContactCard/get`
//! - `ContactCard/changes`
//! - `ContactCard/set`
//! - `ContactCard/copy`
//! - `ContactCard/query`
//! - `ContactCard/queryChanges`

use jmap_contacts_types::ContactCard;
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ContactsBackend};
use crate::helpers::{extract_account_id, finalize_set_response, set_error_value, SetAccumulators};
use jmap_server::server_fail_from_backend;

// ---------------------------------------------------------------------------
// ContactCard/get
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/get` method call (RFC 9610 §3.1).
pub async fn handle_contact_card_get<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<ContactCard, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// ContactCard/changes
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/changes` method call (RFC 9610 §3.2).
pub async fn handle_contact_card_changes<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<ContactCard, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// ContactCard/set
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/set` method call (RFC 9610 §3.3).
pub async fn handle_contact_card_set<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
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

    let old_state = backend
        .get_state::<ContactCard>(caller, &account_id)
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
            // null) is rejected with invalidProperties:["id"]. ContactCard/copy
            // is a separate handler and intentionally carries the source id.
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

            let card: ContactCard = match serde_json::from_value(obj_with_id) {
                Ok(c) => c,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            match backend
                .create_object::<ContactCard>(caller, &account_id, &create_id, card)
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
                .update_object::<ContactCard>(caller, &account_id, &id, patch)
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
                .destroy_object::<ContactCard>(caller, &account_id, &id)
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

/// Apply a JMAP patch key (RFC 8620 §5.3) to a JSON object.
///
/// Keys may contain `/` separators naming a path into nested objects (e.g.
/// `"name/full"`). `~1` decodes to `/` and `~0` to `~` per RFC 6901. A `null`
/// value removes the target key; any non-null value overwrites or creates it.
///
/// # Non-object intermediate
///
/// If the path traverses through an existing non-object value (e.g. a string
/// at `name` when the path is `name/full`), the patch is silently dropped to
/// preserve the existing value. This matches the canonical
/// `crate-jmap-mail-server` `apply_jmap_patch` (memory.rs:2073). A future
/// workspace canonical-template sweep (bd:JMAP-j6ab) will upgrade both
/// siblings to return `invalidPatch` for this case instead of dropping
/// silently.
///
/// # Array indices not supported
///
/// JSON Pointer numeric segments do NOT index into JSON arrays. A path like
/// `"name/components/0/value"` looks up the literal `"0"` key on the
/// (Object-shaped) sub-value, never the 0th element of an Array. The
/// workspace canonical-template sweep (bd:JMAP-j6ab) tracks adding array
/// index support across all sibling implementations.
fn apply_jmap_patch(base: &mut serde_json::Map<String, Value>, path: &str, val: Value) {
    fn decode_segment(s: &str) -> String {
        s.replace("~1", "/").replace("~0", "~")
    }

    if let Some(slash) = path.find('/') {
        let head = decode_segment(&path[..slash]);
        let tail = &path[slash + 1..];
        if let Some(entry) = base.get_mut(&head) {
            if let Value::Object(inner) = entry {
                apply_jmap_patch(inner, tail, val);
            }
            // Non-object intermediate: silently drop the patch (preserves the
            // existing value). See doc-comment.
        } else if !val.is_null() {
            // Parent absent and value is non-null: create parent then set leaf.
            let mut inner = serde_json::Map::new();
            apply_jmap_patch(&mut inner, tail, val);
            base.insert(head, Value::Object(inner));
        }
        // Parent absent and value is null: nothing to remove — no-op.
    } else {
        let key = decode_segment(path);
        if val.is_null() {
            base.remove(&key);
        } else {
            base.insert(key, val);
        }
    }
}

/// Handle a `ContactCard/copy` method call (RFC 9610 §3.4 / RFC 8620 §6.3).
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
pub async fn handle_contact_card_copy<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    call_id: &str,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (to_account_id, mut args) = extract_account_id(args)?;

    let from_account_id = args
        .get("fromAccountId")
        .and_then(|v| v.as_str())
        .map(Id::from)
        .ok_or_else(|| JmapError::invalid_arguments("fromAccountId is required"))?;

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

    let on_success_destroy_original: bool = args
        .get("onSuccessDestroyOriginal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

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

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
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
            let source_id = match spec_val.get("id").and_then(|v| v.as_str()) {
                Some(s) => Id::from(s),
                None => {
                    not_copied.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": "id is required in copy spec" }),
                    );
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
            // Paths are JSON Pointer segments (RFC 6901): split on '/',
            // decode ~1 → '/' and ~0 → '~'.  null value = delete.
            if let Value::Object(spec_obj) = &spec_val {
                let mut card_val = serde_json::to_value(&card).unwrap_or_default();
                if let Value::Object(ref mut merged_map) = card_val {
                    for (k, v) in spec_obj {
                        if k == "id" {
                            continue;
                        }
                        apply_jmap_patch(merged_map, k, v.clone());
                    }
                }
                card = serde_json::from_value(card_val).unwrap_or(card);
            }

            match backend
                .copy_contact_card(caller, &from_account_id, &to_account_id, card)
                .await
            {
                Ok((_new_id, copied_obj)) => {
                    mutated = true;
                    copied.insert(
                        create_id.clone(),
                        serde_json::to_value(&copied_obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                    copied_source_ids.push((create_id, source_id));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_copied.insert(create_id, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_copied.insert(
                        create_id,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
                Err(_) => {
                    not_copied.insert(
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

        // RFC 8620 §5.4: destroyFromIfInState gates the implicit destroy
        // against the source account state at destroy time. Mismatch
        // skips every destroy with a stateMismatch SetError per source
        // id; the /copy itself is unaffected (already succeeded).
        let state_matches = destroy_from_if_in_state
            .as_deref()
            .is_none_or(|expected| expected == from_old_state.as_ref());

        if !state_matches {
            for (_, source_id) in &copied_source_ids {
                not_destroyed.insert(
                    source_id.as_ref().to_owned(),
                    json!({
                        "type": "stateMismatch",
                        "description":
                            "destroyFromIfInState did not match source account state",
                    }),
                );
            }
        } else {
            for (_, source_id) in &copied_source_ids {
                match backend
                    .destroy_object::<ContactCard>(caller, &from_account_id, source_id)
                    .await
                {
                    Ok(()) => {
                        destroyed.push(Value::String(source_id.as_ref().to_owned()));
                    }
                    Err(BackendSetError::SetError(set_err)) => {
                        not_destroyed
                            .insert(source_id.as_ref().to_owned(), set_error_value(&set_err));
                    }
                    Err(BackendSetError::Other(e)) => {
                        not_destroyed.insert(
                            source_id.as_ref().to_owned(),
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                    }
                    Err(_) => {
                        not_destroyed.insert(
                            source_id.as_ref().to_owned(),
                            json!({
                                "type": "serverFail",
                                "description": "unhandled backend error variant",
                            }),
                        );
                    }
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
        extra.push(("ContactCard/set".to_owned(), set_resp, call_id.to_owned()));
    }

    Ok((resp, extra))
}

// ---------------------------------------------------------------------------
// ContactCard/query
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/query` method call (RFC 9610 §3.3).
pub async fn handle_contact_card_query<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<ContactCard, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// ContactCard/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/queryChanges` method call (RFC 9610 §3.4).
pub async fn handle_contact_card_query_changes<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<ContactCard, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: ContactCard/get with unknown account → accountNotFound.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let err = handle_contact_card_get(&backend, &(), args)
            .await
            .expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: ContactCard/changes with known account returns valid response.
    #[tokio::test]
    async fn changes_known_account_returns_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sinceState": "0" });
        let (resp, _) = handle_contact_card_changes(&backend, &(), args)
            .await
            .expect("must not error");
        assert_eq!(resp["accountId"], "acc1");
    }

    /// Oracle: ContactCard/query with known account returns valid response.
    #[tokio::test]
    async fn query_known_account_returns_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "filter": null, "sort": null });
        let (resp, _) = handle_contact_card_query(&backend, &(), args)
            .await
            .expect("must not error");
        assert_eq!(resp["accountId"], "acc1");
    }

    /// Oracle: ContactCard/queryChanges with known account returns valid response.
    #[tokio::test]
    async fn query_changes_known_account_returns_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sinceQueryState": "0" });
        let (resp, _) = handle_contact_card_query_changes(&backend, &(), args)
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
        assert_eq!(err.error_type.as_str(), "fromAccountNotFound");
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

        use crate::register_contacts_handlers;
        use crate::JMAP_CONTACTS_URI;

        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        // Do NOT seed the source card — get_objects will report not_found.

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_contacts_handlers(&mut dispatcher, Arc::new(backend));

        let req = JmapRequest::new(
            vec![JMAP_CONTACTS_URI.into()],
            vec![(
                "ContactCard/copy".into(),
                json!({
                    "accountId": "acc2",
                    "fromAccountId": "acc1",
                    "create": { "c1": { "id": "nonexistent" } }
                }),
                "c0".into(),
            )],
            None,
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];

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

        use crate::register_contacts_handlers;
        use crate::JMAP_CONTACTS_URI;

        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        register_contacts_handlers(&mut dispatcher, Arc::new(backend));

        let req = JmapRequest::new(
            vec![JMAP_CONTACTS_URI.into()],
            vec![(
                "ContactCard/copy".into(),
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
                "c0".into(),
            )],
            None,
        );
        let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
        let (_, args, _) = &resp.method_responses[0];

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

    /// Oracle: ContactCard/copy calls copy_contact_card on the backend.
    ///
    /// Source: RFC 9610 §3.4 — copy must succeed when both accounts exist
    /// and the source card is found.
    #[tokio::test]
    async fn copy_calls_backend_copy_contact_card() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.add_account("acc2");
        backend.add_contact_card("acc1", "card1");

        let args = json!({
            "accountId": "acc2",
            "fromAccountId": "acc1",
            "create": { "c1": { "id": "card1" } }
        });
        let (resp, _) = handle_contact_card_copy(&backend, &(), args, "c0")
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
        let (resp, _) = handle_contact_card_set(&backend, &(), args)
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
        let (resp, _) = handle_contact_card_copy(&backend, &(), args, "c0")
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
        let (resp, _) = handle_contact_card_copy(&backend, &(), args, "c0")
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
        assert_eq!(err.error_type.as_str(), "invalidArguments");
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
        assert_eq!(err.error_type.as_str(), "stateMismatch");
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
        let (resp, _) = handle_contact_card_copy(&backend, &(), args, "c0")
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
        let (_, extra) = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect("/copy must succeed");

        assert_eq!(
            extra.len(),
            1,
            "onSuccessDestroyOriginal must emit exactly one synthetic invocation"
        );
        let (method, set_resp, returned_call_id) = &extra[0];
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
        let (_, extra) = handle_contact_card_copy(&backend, &(), args, "c0")
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
        let (_, extra) = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect("/copy must succeed");
        assert!(
            extra.is_empty(),
            "no successful copies → no synthetic destroy invocation"
        );
    }

    /// Oracle (RFC 8620 §5.4): `destroyFromIfInState` mismatching the
    /// source account state at destroy time produces a stateMismatch
    /// SetError per source id in the synthetic /set's notDestroyed.
    /// The /copy itself is unaffected (it already succeeded).
    #[tokio::test]
    async fn copy_destroy_from_if_in_state_mismatch_fails_all_destroys() {
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
        let (resp, extra) = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect("/copy must succeed (mismatch only affects implicit destroy)");

        // The /copy itself succeeded.
        assert!(
            resp["copied"]["c1"].is_object(),
            "copy must succeed despite destroyFromIfInState mismatch: {resp}"
        );

        // The synthetic /set's notDestroyed carries stateMismatch.
        let (_, set_resp, _) = &extra[0];
        assert_eq!(
            set_resp["notDestroyed"]["card1"]["type"], "stateMismatch",
            "destroyFromIfInState mismatch must produce stateMismatch: {set_resp}"
        );
        assert!(
            set_resp["destroyed"].is_null(),
            "no destroys must succeed when destroyFromIfInState mismatches: {set_resp}"
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
        let (_, extra) = handle_contact_card_copy(&backend, &(), args, "c0")
            .await
            .expect("/copy must succeed");

        // Destroy was attempted (MockBackend always returns NotFound for
        // destroys, so the result is notDestroyed[card1] = notFound, NOT
        // stateMismatch — the state check passed).
        let (_, set_resp, _) = &extra[0];
        assert_eq!(
            set_resp["notDestroyed"]["card1"]["type"], "notFound",
            "destroyFromIfInState match must allow destroy attempt: {set_resp}"
        );
    }

    // -----------------------------------------------------------------------
    // apply_jmap_patch unit tests (bd:JMAP-qz9v.4)
    //
    // The previous implementation silently CLOBBERED a non-object intermediate
    // value (e.g. patch path `name/full` when `name` was a string would
    // replace the string with `{"full": "..."}`), destroying caller data.
    // The fix aligns the non-object-intermediate behavior with the canonical
    // crate-jmap-mail-server apply_jmap_patch (memory.rs:2073): silently drop
    // the patch, preserving the existing value. The broader RFC 8620 §5.3
    // compliance work (returning invalidPatch, supporting array indices) is
    // tracked by bd:JMAP-j6ab.
    // -----------------------------------------------------------------------

    /// Oracle: flat single-segment key with a non-null value inserts the key.
    #[test]
    fn apply_jmap_patch_flat_key_inserts() {
        let mut obj = serde_json::Map::new();
        apply_jmap_patch(&mut obj, "foo", json!("bar"));
        assert_eq!(obj["foo"], json!("bar"));
    }

    /// Oracle: flat single-segment key with a null value removes the key.
    #[test]
    fn apply_jmap_patch_flat_key_null_removes() {
        let mut obj = serde_json::Map::new();
        obj.insert("foo".to_owned(), json!("bar"));
        apply_jmap_patch(&mut obj, "foo", Value::Null);
        assert!(
            !obj.contains_key("foo"),
            "null patch on flat key must remove"
        );
    }

    /// Oracle: nested path with an existing object intermediate navigates
    /// correctly and sets the leaf without disturbing other sub-keys.
    #[test]
    fn apply_jmap_patch_nested_path_navigates_object_intermediate() {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "name".to_owned(),
            json!({ "full": "Jane", "given": "Jane" }),
        );

        apply_jmap_patch(&mut obj, "name/full", json!("Jane Doe"));

        assert_eq!(
            obj["name"],
            json!({ "full": "Jane Doe", "given": "Jane" }),
            "leaf must be updated, sibling keys preserved"
        );
    }

    /// Oracle: nested path with an absent intermediate and a non-null value
    /// creates the intermediate object and sets the leaf.
    #[test]
    fn apply_jmap_patch_nested_path_absent_intermediate_creates_when_non_null() {
        let mut obj = serde_json::Map::new();

        apply_jmap_patch(&mut obj, "name/full", json!("Jane Doe"));

        assert_eq!(obj["name"], json!({ "full": "Jane Doe" }));
    }

    /// Oracle: nested path with an absent intermediate and a null value is a
    /// no-op (does NOT create an empty intermediate object). This mirrors the
    /// canonical mail-server `apply_jmap_patch` behavior and avoids
    /// introducing a spurious empty object the caller never set.
    #[test]
    fn apply_jmap_patch_nested_path_absent_intermediate_null_value_is_noop() {
        let mut obj = serde_json::Map::new();

        apply_jmap_patch(&mut obj, "name/full", Value::Null);

        assert!(
            !obj.contains_key("name"),
            "null patch with absent parent must not create an intermediate: {obj:?}"
        );
    }

    /// Oracle (bd:JMAP-qz9v.4): a path traversing through a non-object string
    /// intermediate must PRESERVE the original string value, not clobber it
    /// with an empty object. Previously this code path silently destroyed
    /// caller data; the fix aligns with canonical mail-server's silent-no-op
    /// behavior on this case.
    #[test]
    fn apply_jmap_patch_preserves_non_object_string_intermediate() {
        let mut obj = serde_json::Map::new();
        obj.insert("vendorBlob".to_owned(), json!("string-value"));

        apply_jmap_patch(&mut obj, "vendorBlob/sub", json!("would-clobber"));

        assert_eq!(
            obj["vendorBlob"],
            json!("string-value"),
            "non-object string intermediate must be preserved, not clobbered: {obj:?}"
        );
    }

    /// Oracle (bd:JMAP-qz9v.4): a path traversing through a non-object array
    /// intermediate must preserve the original array, not clobber it. Same
    /// fix as the string case; covers the array-shaped variant of the bug.
    #[test]
    fn apply_jmap_patch_preserves_non_object_array_intermediate() {
        let mut obj = serde_json::Map::new();
        obj.insert("vendorList".to_owned(), json!(["a", "b", "c"]));

        apply_jmap_patch(&mut obj, "vendorList/0", json!("would-clobber"));

        assert_eq!(
            obj["vendorList"],
            json!(["a", "b", "c"]),
            "non-object array intermediate must be preserved, not clobbered: {obj:?}"
        );
    }

    /// Oracle (bd:JMAP-qz9v.4): a path traversing through a non-object scalar
    /// (number, boolean) intermediate must preserve the original value.
    #[test]
    fn apply_jmap_patch_preserves_non_object_scalar_intermediate() {
        let mut obj = serde_json::Map::new();
        obj.insert("vendorNumber".to_owned(), json!(42));
        obj.insert("vendorBool".to_owned(), json!(true));

        apply_jmap_patch(&mut obj, "vendorNumber/sub", json!("would-clobber"));
        apply_jmap_patch(&mut obj, "vendorBool/sub", json!("would-clobber"));

        assert_eq!(obj["vendorNumber"], json!(42));
        assert_eq!(obj["vendorBool"], json!(true));
    }

    /// Oracle: RFC 6901 escape decoding — `~1` decodes to `/` and `~0`
    /// decodes to `~`. This lets clients address property names that
    /// themselves contain reserved characters.
    #[test]
    fn apply_jmap_patch_decodes_rfc6901_escapes() {
        let mut obj = serde_json::Map::new();

        // ~1 → / in a single-segment key.
        apply_jmap_patch(&mut obj, "foo~1bar", json!("v"));
        assert_eq!(obj["foo/bar"], json!("v"));

        // ~0 → ~ in a single-segment key.
        apply_jmap_patch(&mut obj, "foo~0bar", json!("w"));
        assert_eq!(obj["foo~bar"], json!("w"));

        // ~1 decodes per-segment, NOT before splitting on '/'.
        // `name~1full` is one segment whose property name is "name/full".
        apply_jmap_patch(&mut obj, "name~1full", json!("Jane Doe"));
        assert_eq!(obj["name/full"], json!("Jane Doe"));
    }
}
