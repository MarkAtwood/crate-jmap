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

/// Apply a JMAP patch path (RFC 8620 §5.3) to a JSON object.
///
/// Paths use `/`-separated segments; `~1` decodes to `/` and `~0` to `~`
/// per RFC 6901.  A `null` value removes the key at the path; any other value
/// sets it.  Intermediate objects are created as needed.
fn apply_jmap_patch(obj: &mut serde_json::Map<String, Value>, path: &str, val: Value) {
    fn decode_segment(s: &str) -> String {
        s.replace("~1", "/").replace("~0", "~")
    }

    let parts: Vec<String> = path.split('/').map(decode_segment).collect();
    if parts.is_empty() {
        return;
    }

    if parts.len() == 1 {
        let key = &parts[0];
        if val.is_null() {
            obj.remove(key);
        } else {
            obj.insert(key.clone(), val);
        }
        return;
    }

    // Navigate/create intermediate objects.
    let Some(leaf_key) = parts.last().cloned() else {
        return;
    };
    let mut current = obj;
    for seg in &parts[..parts.len() - 1] {
        let next = current
            .entry(seg.clone())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Value::Object(ref mut map) = next {
            current = map;
        } else {
            // Target exists but is not an object — replace with object.
            *next = Value::Object(serde_json::Map::new());
            if let Value::Object(ref mut map) = next {
                current = map;
            } else {
                return;
            }
        }
    }

    if val.is_null() {
        current.remove(&leaf_key);
    } else {
        current.insert(leaf_key, val);
    }
}

/// Handle a `ContactCard/copy` method call (RFC 9610 §3.4 / RFC 8620 §6.3).
///
/// Fetches cards from `fromAccountId`, delegates copy to the backend, and
/// returns `copied`/`notCopied` maps.
pub async fn handle_contact_card_copy<B: ContactsBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    _call_id: &str,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (to_account_id, mut args) = extract_account_id(args)?;

    let from_account_id = args
        .get("fromAccountId")
        .and_then(|v| v.as_str())
        .map(Id::from)
        .ok_or_else(|| JmapError::invalid_arguments("fromAccountId is required"))?;

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
                        create_id,
                        serde_json::to_value(&copied_obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
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

    Ok((
        json!({
            "fromAccountId": from_account_id.as_ref(),
            "accountId": to_account_id.as_ref(),
            "oldState": old_state.as_ref(),
            "newState": new_state.as_ref(),
            "copied":    if copied.is_empty()     { Value::Null } else { Value::Object(copied) },
            "notCopied": if not_copied.is_empty() { Value::Null } else { Value::Object(not_copied) },
        }),
        vec![],
    ))
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
}
