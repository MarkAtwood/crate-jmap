//! ContactCard/* method handlers (draft-ietf-jmap-contacts-10 §3).
//!
//! Provides handlers for:
//! - `ContactCard/get`
//! - `ContactCard/changes`
//! - `ContactCard/set`
//! - `ContactCard/copy`
//! - `ContactCard/query`
//! - `ContactCard/queryChanges`

use jmap_contacts_types::ContactCard;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ContactsBackend};
use crate::helpers::{extract_account_id, set_error_value};

// ---------------------------------------------------------------------------
// ContactCard/get
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/get` method call (contacts-10 §3.1).
pub async fn handle_contact_card_get<B: ContactsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<ContactCard, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// ContactCard/changes
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/changes` method call (contacts-10 §3.2).
pub async fn handle_contact_card_changes<B: ContactsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<ContactCard, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// ContactCard/set
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/set` method call (contacts-10 §3.3).
pub async fn handle_contact_card_set<B: ContactsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    let old_state = backend
        .get_state::<ContactCard>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

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
                .create_object::<ContactCard>(&account_id, &create_id, card)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    created.insert(
                        create_id,
                        serde_json::to_value(&created_obj).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
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
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

            match backend
                .update_object::<ContactCard>(&account_id, &id, patch_val)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
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
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s.to_owned(),
                None => continue,
            };
            let id = Id::from(id_str.as_str());

            match backend
                .destroy_object::<ContactCard>(&account_id, &id)
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
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<ContactCard>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    } else {
        old_state.clone()
    };

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "oldState": old_state.as_ref(),
            "newState": new_state.as_ref(),
            "created":      if created.is_empty()        { Value::Null } else { Value::Object(created) },
            "updated":      if updated.is_empty()        { Value::Null } else { Value::Object(updated) },
            "destroyed":    if destroyed_list.is_empty() { Value::Null } else { Value::Array(destroyed_list) },
            "notCreated":   if not_created.is_empty()    { Value::Null } else { Value::Object(not_created) },
            "notUpdated":   if not_updated.is_empty()    { Value::Null } else { Value::Object(not_updated) },
            "notDestroyed": if not_destroyed.is_empty()  { Value::Null } else { Value::Object(not_destroyed) },
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// ContactCard/copy
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/copy` method call (contacts-10 §3.4 / RFC 8620 §6.3).
///
/// Fetches cards from `fromAccountId`, delegates copy to the backend, and
/// returns `copied`/`notCopied` maps.
pub async fn handle_contact_card_copy<B: ContactsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let to_account_id = extract_account_id(&args)?;

    let from_account_id = args
        .get("fromAccountId")
        .and_then(|v| v.as_str())
        .map(Id::from)
        .ok_or_else(|| JmapError::invalid_arguments("fromAccountId is required"))?;

    // Verify both accounts exist.
    let to_exists = backend
        .account_exists(&to_account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;
    if !to_exists {
        return Err(JmapError::account_not_found());
    }

    let from_exists = backend
        .account_exists(&from_account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;
    if !from_exists {
        return Err(JmapError::from_account_not_found());
    }

    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    let old_state = backend
        .get_state::<ContactCard>(&to_account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

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
                    &from_account_id,
                    Some(std::slice::from_ref(&source_id)),
                    None,
                )
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;

            if !not_found.is_empty() || cards.is_empty() {
                not_copied.insert(create_id, json!({ "type": "notFound" }));
                continue;
            }

            let mut card = cards.remove(0);

            // Apply any patch fields from the copy spec (RFC 8620 §6.3).
            if let Value::Object(spec_obj) = &spec_val {
                for (k, v) in spec_obj {
                    if k == "id" {
                        continue;
                    }
                    // Merge top-level patch fields into the card JSON.
                    let mut card_val = serde_json::to_value(&card).unwrap_or_default();
                    if let Value::Object(ref mut m) = card_val {
                        m.insert(k.clone(), v.clone());
                    }
                    card = serde_json::from_value(card_val).unwrap_or(card);
                }
            }

            match backend
                .copy_contact_card(&from_account_id, &to_account_id, card)
                .await
            {
                Ok((_new_id, copied_obj)) => {
                    mutated = true;
                    copied.insert(
                        create_id,
                        serde_json::to_value(&copied_obj).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
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
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<ContactCard>(&to_account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
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

/// Handle a `ContactCard/query` method call (contacts-10 §3.3).
pub async fn handle_contact_card_query<B: ContactsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<ContactCard, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// ContactCard/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `ContactCard/queryChanges` method call (contacts-10 §3.4).
pub async fn handle_contact_card_query_changes<B: ContactsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<ContactCard, B>(backend, args).await
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
        let err = handle_contact_card_get(&backend, args)
            .await
            .expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: ContactCard/changes with known account returns valid response.
    #[tokio::test]
    async fn changes_known_account_returns_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sinceState": "0" });
        let (resp, _) = handle_contact_card_changes(&backend, args)
            .await
            .expect("must not error");
        assert_eq!(resp["accountId"], "acc1");
    }

    /// Oracle: ContactCard/query with known account returns valid response.
    #[tokio::test]
    async fn query_known_account_returns_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "filter": null, "sort": null });
        let (resp, _) = handle_contact_card_query(&backend, args)
            .await
            .expect("must not error");
        assert_eq!(resp["accountId"], "acc1");
    }

    /// Oracle: ContactCard/queryChanges with known account returns valid response.
    #[tokio::test]
    async fn query_changes_known_account_returns_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sinceQueryState": "0" });
        let (resp, _) = handle_contact_card_query_changes(&backend, args)
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
        let err = handle_contact_card_copy(&backend, args)
            .await
            .expect_err("must return error for unknown fromAccountId");
        assert_eq!(err.error_type.as_str(), "fromAccountNotFound");
    }

    /// Oracle: ContactCard/copy calls copy_contact_card on the backend.
    ///
    /// Source: contacts-10 §3.4 — copy must succeed when both accounts exist
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
        let (resp, _) = handle_contact_card_copy(&backend, args)
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
        let (resp, _) = handle_contact_card_set(&backend, args)
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
}
