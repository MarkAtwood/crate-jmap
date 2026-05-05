//! AddressBook/* method handlers (draft-ietf-jmap-contacts-10 §2).
//!
//! Provides handlers for:
//! - `AddressBook/get`
//! - `AddressBook/changes`
//! - `AddressBook/set` (with `onDestroyRemoveContents` and `onSuccessSetIsDefault`)
//!
//! **No `AddressBook/query` or `AddressBook/queryChanges`** — the spec does
//! not define these methods for AddressBook.

use jmap_contacts_types::AddressBook;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ContactsBackend, SetError, SetErrorType};
use crate::helpers::{extract_account_id, set_error_value};

// ---------------------------------------------------------------------------
// AddressBook/get
// ---------------------------------------------------------------------------

/// Handle an `AddressBook/get` method call (contacts-10 §2.1).
pub async fn handle_address_book_get<B: ContactsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<AddressBook, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// AddressBook/changes
// ---------------------------------------------------------------------------

/// Handle an `AddressBook/changes` method call (contacts-10 §2.2).
pub async fn handle_address_book_changes<B: ContactsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<AddressBook, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// AddressBook/set
// ---------------------------------------------------------------------------

/// Handle an `AddressBook/set` method call (contacts-10 §2.3).
///
/// Contacts-specific extensions:
///
/// - **`onDestroyRemoveContents`** (bool, default `false`): if `false` and
///   the address book still contains ContactCards, the destroy is rejected with
///   `SetError { type: "addressBookHasContents" }`. If `true`, the backend
///   must remove the contents itself.
///
/// - **`onSuccessSetIsDefault`** (object or null): a patch object applied
///   *after* all other set operations succeed, mapping address book ids
///   (or creation-id references) to `isDefault` values.  Implemented as
///   a best-effort `update_object` call per entry; individual patch failures
///   are collected into `notUpdated` but do not roll back the overall set.
pub async fn handle_address_book_set<B: ContactsBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    // Parse contacts-specific arguments before consuming args.
    let on_destroy_remove_contents = args
        .get("onDestroyRemoveContents")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let on_success_set_is_default = args.remove("onSuccessSetIsDefault");

    let old_state = backend
        .get_state::<AddressBook>(&account_id)
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
                    // AddressBook requires myRights — inject a default if absent.
                    m.entry("myRights").or_insert_with(|| {
                        json!({
                            "mayRead": true,
                            "mayWrite": true,
                            "mayShare": false,
                            "mayDelete": false
                        })
                    });
                    Value::Object(m)
                }
                other => other,
            };

            let ab: AddressBook = match serde_json::from_value(obj_with_id) {
                Ok(a) => a,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            match backend
                .create_object::<AddressBook>(&account_id, &create_id, ab)
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
                .update_object::<AddressBook>(&account_id, &id, patch_val)
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

            // contacts-10 §2.3: if onDestroyRemoveContents is false and the
            // address book has contents, reject with addressBookHasContents.
            if !on_destroy_remove_contents
                && backend.address_book_has_contents(&account_id, &id).await
            {
                not_destroyed.insert(
                    id_str,
                    set_error_value(&SetError::new(SetErrorType::Custom(
                        "addressBookHasContents".to_owned(),
                    ))),
                );
                continue;
            }

            match backend
                .destroy_object::<AddressBook>(&account_id, &id)
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
            .get_state::<AddressBook>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    } else {
        old_state.clone()
    };

    // -----------------------------------------------------------------------
    // onSuccessSetIsDefault — post-set isDefault patch
    // contacts-10 §2.3: after the main set operations, if onSuccessSetIsDefault
    // is a non-null object, apply each { id: isDefault } entry as an update.
    // -----------------------------------------------------------------------
    if let Some(Value::Object(is_default_map)) = on_success_set_is_default {
        for (id_str, is_default_val) in is_default_map {
            // Resolve creation references (#<create_id>).
            let resolved_id = if let Some(stripped) = id_str.strip_prefix('#') {
                // Look up the server-assigned id in the created map.
                if let Some(created_obj) = created.get(stripped) {
                    created_obj
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&id_str)
                        .to_owned()
                } else {
                    // Creation failed — skip.
                    continue;
                }
            } else {
                id_str.clone()
            };

            let id = Id::from(resolved_id.as_str());
            let patch = json!({ "isDefault": is_default_val });

            match backend
                .update_object::<AddressBook>(&account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
                    );
                }
                Ok(None) => {
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: contacts-10 §2.3 — destroy with non-empty address book and
    /// onDestroyRemoveContents=false (default) returns addressBookHasContents.
    ///
    /// The mock backend's `address_book_has_contents` returns true for
    /// `"ab-nonempty"`.
    #[tokio::test]
    async fn set_destroy_non_empty_returns_address_book_has_contents() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": ["ab-nonempty"]
        });
        let (resp, _) = handle_address_book_set(&backend, args)
            .await
            .expect("must not return top-level error");

        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );
        assert_eq!(
            not_destroyed["ab-nonempty"]["type"], "addressBookHasContents",
            "non-empty book with onDestroyRemoveContents=false must yield addressBookHasContents: {resp}"
        );
        assert!(
            resp["destroyed"].is_null(),
            "destroyed must be null when blocked: {resp}"
        );
    }

    /// Oracle: contacts-10 §2.3 — destroy with onDestroyRemoveContents=true
    /// bypasses the contents check and delegates to the backend.
    #[tokio::test]
    async fn set_destroy_with_on_destroy_remove_contents_true_proceeds() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "onDestroyRemoveContents": true,
            "destroy": ["ab-nonempty"]
        });
        let (resp, _) = handle_address_book_set(&backend, args)
            .await
            .expect("must not return top-level error");

        // The mock backend returns notFound for destroys; verify we got past
        // the contents check (notDestroyed will have notFound, not addressBookHasContents).
        let not_destroyed = &resp["notDestroyed"];
        if not_destroyed.is_object() {
            assert_ne!(
                not_destroyed["ab-nonempty"]["type"], "addressBookHasContents",
                "contents check must be bypassed: {resp}"
            );
        }
    }

    /// Oracle: AddressBook/get with unknown account → accountNotFound.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let err = handle_address_book_get(&backend, args)
            .await
            .expect_err("must return error for unknown account");
        assert_eq!(err.error_type.as_str(), "accountNotFound");
    }

    /// Oracle: AddressBook/changes with known account returns valid response.
    #[tokio::test]
    async fn changes_known_account_returns_response() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sinceState": "0" });
        let (resp, _) = handle_address_book_changes(&backend, args)
            .await
            .expect("must not error for known account");
        assert_eq!(resp["accountId"], "acc1");
    }
}
