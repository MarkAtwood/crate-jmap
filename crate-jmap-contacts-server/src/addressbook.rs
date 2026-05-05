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
/// - **`onSuccessSetIsDefault`** (`Id|null`, single string id or null): if
///   non-null, after all other set operations succeed the named address book
///   is patched with `{"isDefault": true}`.  Creation-id references (`#c1`)
///   are resolved to the server-assigned id.  Any error from this patch is
///   silently ignored per §2.3.
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

    // -----------------------------------------------------------------------
    // onSuccessSetIsDefault — post-set isDefault patch
    // contacts-10 §2.3: onSuccessSetIsDefault is Id|null (a single string id,
    // not a map).  After the main set operations, if it is a non-null string,
    // patch the named address book with {"isDefault": true}.  Errors are
    // silently ignored per §2.3.
    //
    // contacts-10 §2.3: this block MUST be skipped if any main set operation
    // (create, update, or destroy) produced an error.
    // -----------------------------------------------------------------------
    let main_ops_all_succeeded =
        not_created.is_empty() && not_updated.is_empty() && not_destroyed.is_empty();
    if main_ops_all_succeeded {
        match on_success_set_is_default {
            Some(Value::String(id_str)) => {
                // Resolve creation reference: if id_str starts with '#', look up in created map.
                let resolved: Option<Id> = if let Some(create_id) = id_str.strip_prefix('#') {
                    created
                        .get(create_id)
                        .and_then(|v| v.get("id"))
                        .and_then(|v| v.as_str())
                        .map(Id::from)
                } else {
                    Some(Id::from(id_str.as_str()))
                };
                if let Some(target_id) = resolved {
                    let patch = json!({"isDefault": true});
                    // §2.3: errors here are silently ignored.
                    match backend
                        .update_object::<AddressBook>(&account_id, &target_id, patch)
                        .await
                    {
                        Ok(Some(obj)) => {
                            mutated = true;
                            updated.insert(
                                target_id.to_string(),
                                serde_json::to_value(&obj).unwrap_or(Value::Null),
                            );
                            // RFC 8620 §5.3: all changed objects must appear in
                            // updated.  When isDefault transfers, the backend clears
                            // it on all other books; re-fetch to pick them up.
                            if let Ok((all_books, _)) = backend
                                .get_objects::<AddressBook>(&account_id, None, None)
                                .await
                            {
                                for book in all_books {
                                    let book_id = book.id.clone();
                                    if book_id == target_id {
                                        continue; // already recorded above
                                    }
                                    // The backend enforces single-default: any book
                                    // returned here whose is_default is now false was
                                    // implicitly demoted.
                                    if !book.is_default {
                                        if let Ok(v) = serde_json::to_value(&book) {
                                            updated.insert(book_id.to_string(), v);
                                        }
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            mutated = true;
                            updated.insert(target_id.to_string(), Value::Null);
                        }
                        Err(_) => {} // silently ignored per §2.3
                    }
                }
            }
            Some(Value::Null) | None => {}
            _ => {} // malformed — silently ignored
        }
    } // end if main_ops_all_succeeded

    // Fetch newState AFTER all mutations including onSuccessSetIsDefault
    // (RFC 8620 §5.3: newState must reflect every mutation in this call).
    let new_state = if mutated {
        backend
            .get_state::<AddressBook>(&account_id)
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

    /// Oracle: contacts-10 §2.3 — onSuccessSetIsDefault with a bare string id
    /// triggers a best-effort isDefault patch.  The MockBackend's update_object
    /// returns NotFound (a SetError), which is silently swallowed per §2.3, so
    /// no top-level error is returned and notUpdated remains null.
    #[tokio::test]
    async fn set_on_success_set_is_default_bare_id() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": [],
            "onSuccessSetIsDefault": "book1"
        });
        let (resp, _) = handle_address_book_set(&backend, args)
            .await
            .expect("must not return top-level error");

        // The update error is silently swallowed — no top-level error and
        // notUpdated must be null (the error is not surfaced per §2.3).
        assert!(
            resp.get("type").is_none(),
            "must not be a top-level error: {resp}"
        );
        assert!(
            resp["notUpdated"].is_null(),
            "§2.3 errors must be silently ignored, notUpdated must be null: {resp}"
        );
    }

    /// Oracle: contacts-10 §2.3 — onSuccessSetIsDefault: null is a no-op.
    /// No error must be returned and the response must be structurally valid.
    #[tokio::test]
    async fn set_on_success_set_is_default_null_ignored() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "onSuccessSetIsDefault": null
        });
        let (resp, _) = handle_address_book_set(&backend, args)
            .await
            .expect("must not return top-level error");

        assert!(
            resp.get("type").is_none(),
            "null onSuccessSetIsDefault must not cause an error: {resp}"
        );
        assert_eq!(resp["accountId"], "acc1");
    }

    /// Oracle: contacts-10 §2.3 — a malformed onSuccessSetIsDefault (object
    /// instead of Id|null) must be silently ignored; no top-level error.
    #[tokio::test]
    async fn set_on_success_set_is_default_bad_type_ignored() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "onSuccessSetIsDefault": {"wrong": "type"}
        });
        let (resp, _) = handle_address_book_set(&backend, args)
            .await
            .expect("must not return top-level error");

        assert!(
            resp.get("type").is_none(),
            "malformed onSuccessSetIsDefault must be silently ignored: {resp}"
        );
        assert_eq!(resp["accountId"], "acc1");
    }

    /// Oracle: RFC 8620 §5.3 — onSuccessSetIsDefault must report the DEMOTED
    /// book (previously isDefault=true) in `updated` in addition to the newly
    /// promoted book.
    ///
    /// Pre-conditions: account has two address books; book1 is the default.
    /// Action: onSuccessSetIsDefault="book2".
    /// Expected: updated contains BOTH book1 (isDefault:false) AND book2 (isDefault:true).
    #[tokio::test]
    async fn set_on_success_reports_demoted_book_in_updated() {
        let mut backend = MockBackend::new_with_account("acc1");
        backend.seed_addressbook("acc1", "book1", true);
        backend.seed_addressbook("acc1", "book2", false);

        let args = json!({
            "accountId": "acc1",
            "onSuccessSetIsDefault": "book2"
        });
        let (resp, _) = handle_address_book_set(&backend, args)
            .await
            .expect("must not return top-level error");

        let updated = resp["updated"]
            .as_object()
            .expect("updated must be an object: {resp}");

        // The newly-promoted book must appear.
        assert!(
            updated.contains_key("book2"),
            "book2 (newly default) must appear in updated: {resp}"
        );
        assert_eq!(
            updated["book2"]["isDefault"],
            serde_json::json!(true),
            "book2 must have isDefault:true: {resp}"
        );

        // The demoted book must also appear (RFC 8620 §5.3).
        assert!(
            updated.contains_key("book1"),
            "book1 (demoted, was isDefault=true) must appear in updated: {resp}"
        );
        assert_eq!(
            updated["book1"]["isDefault"],
            serde_json::json!(false),
            "book1 must have isDefault:false after demotion: {resp}"
        );
    }

    /// Oracle: contacts-10 §2.3 — onSuccessSetIsDefault MUST be skipped when
    /// any main set operation (create, update, or destroy) produced an error.
    /// Here a create with a malformed entry causes notCreated, which means
    /// onSuccessSetIsDefault must NOT be applied.
    #[tokio::test]
    async fn set_on_success_skipped_when_create_fails() {
        let backend = MockBackend::new_with_account("acc1");
        // An empty object {} as the create value has no required "name" field,
        // so deserialization will fail → notCreated entry → main op failed.
        let args = json!({
            "accountId": "acc1",
            "create": { "c1": {} },   // missing required name → invalidProperties
            "onSuccessSetIsDefault": "book1"
        });
        let (resp, _) = handle_address_book_set(&backend, args)
            .await
            .expect("must not return top-level error");

        // The create must have failed.
        assert!(
            resp["notCreated"].is_object(),
            "c1 must be in notCreated: {resp}"
        );
        // onSuccessSetIsDefault must NOT have run (updated must be null/absent).
        // If it did run, "book1" would appear in updated.
        let updated = &resp["updated"];
        let is_empty =
            updated.is_null() || updated.as_object().map(|o| o.is_empty()).unwrap_or(true);
        assert!(
            is_empty,
            "updated must be empty — onSuccessSetIsDefault must not run when create failed: {resp}"
        );
    }
}
