//! Integration tests for `jmap-contacts-server` using `MemoryBackend`.
//!
//! All expected values are derived from the spec (RFC 9610) and RFC 8620,
//! not from the code under test. Wire-shape literals are hand-written.
//!
//! Bead: JMAP-hwdv.6.

mod common;

use common::MemoryBackend;
use jmap_contacts_server::{
    handle_address_book_changes, handle_address_book_get, handle_address_book_set,
    handle_contact_card_changes, handle_contact_card_get, handle_contact_card_query,
    handle_contact_card_set,
};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a minimal valid AddressBook JSON value.
fn address_book_fixture(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "description": null,
        "sortOrder": 0,
        "isDefault": false,
        "isSubscribed": true,
        "shareWith": null,
        "myRights": {
            "mayRead": true,
            "mayWrite": true,
            "mayShare": true,
            "mayDelete": true
        }
    })
}

/// Build a minimal valid ContactCard JSON value referencing the given
/// address book.
fn contact_card_fixture(id: &str, address_book_id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "@type": "Card",
        "version": "1.0",
        "uid": format!("{id}-uid"),
        "addressBookIds": { address_book_id: true },
        "name": { "@type": "Name", "full": name }
    })
}

// ---------------------------------------------------------------------------
// Test 1: AddressBook/get against empty account → empty list
// Oracle: RFC 8620 §5.1.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn address_book_get_empty_account_returns_empty_list() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({ "accountId": "acc1", "ids": null });
    let (resp, _) = handle_address_book_get(&backend, args)
        .await
        .expect("/get must succeed");

    assert!(resp["list"].as_array().unwrap().is_empty());
    assert!(
        resp["notFound"].is_array(),
        "notFound MUST be array per RFC 8620 §5.1: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: AddressBook/get seeded + unknown id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn address_book_get_seeded_and_unknown_id() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Contacts"),
    );

    let args = json!({ "accountId": "acc1", "ids": ["ab1", "missing"] });
    let (resp, _) = handle_address_book_get(&backend, args)
        .await
        .expect("/get must succeed");

    let list = resp["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], "ab1");
    assert_eq!(list[0]["name"], "Contacts");

    let not_found = resp["notFound"].as_array().unwrap();
    assert_eq!(not_found.len(), 1);
    assert_eq!(not_found[0], "missing");
}

// ---------------------------------------------------------------------------
// Test 3: AddressBook/changes on empty account
// ---------------------------------------------------------------------------

#[tokio::test]
async fn address_book_changes_empty_store() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({ "accountId": "acc1", "sinceState": "0" });
    let (resp, _) = handle_address_book_changes(&backend, args)
        .await
        .expect("/changes must succeed");

    assert_eq!(resp["oldState"], "0");
    assert_eq!(resp["newState"], "0");
    assert!(resp["created"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Test 4: AddressBook/set destroy empty book succeeds
// Oracle: RFC 9610 §3 — destroy proceeds when book has no contacts.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn address_book_set_destroy_empty_book_succeeds() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Contacts"),
    );

    let args = json!({ "accountId": "acc1", "destroy": ["ab1"] });
    let (resp, _) = handle_address_book_set(&backend, args)
        .await
        .expect("/set must succeed");

    let destroyed = resp["destroyed"].as_array().unwrap();
    assert_eq!(destroyed.len(), 1);
    assert_eq!(destroyed[0], "ab1");
    assert_ne!(resp["oldState"], resp["newState"]);
}

// ---------------------------------------------------------------------------
// Test 5: AddressBook/set destroy non-empty book → addressBookHasContents
// Oracle: RFC 9610 §3 — when onDestroyRemoveContents is false and the
// book has contacts, destroy MUST fail with `addressBookHasContents`.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn address_book_set_destroy_with_contents_returns_error() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Contacts"),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card1",
        contact_card_fixture("card1", "ab1", "Alice"),
    );

    let args = json!({ "accountId": "acc1", "destroy": ["ab1"] });
    let (resp, _) = handle_address_book_set(&backend, args)
        .await
        .expect("/set must succeed");

    assert!(
        resp["notDestroyed"].is_object(),
        "notDestroyed must be present: {resp}"
    );
    assert_eq!(
        resp["notDestroyed"]["ab1"]["type"], "addressBookHasContents",
        "RFC 9610 §3 requires addressBookHasContents: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 6: AddressBook/set create with client-supplied id rejected
// Oracle: RFC 8620 §5.3.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn address_book_set_create_with_client_id_rejected() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": { "id": "client-id", "name": "Inbox" }
        }
    });
    let (resp, _) = handle_address_book_set(&backend, args)
        .await
        .expect("/set must succeed");

    assert_eq!(
        resp["notCreated"]["c1"]["type"], "invalidProperties",
        "client id must be rejected: {resp}"
    );
    assert_eq!(resp["notCreated"]["c1"]["properties"][0], "id");
}

// ---------------------------------------------------------------------------
// Test 7: AddressBook/set on unknown account → accountNotFound
// Oracle: RFC 8620 §3.6.2.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn address_book_set_unknown_account_returns_account_not_found() {
    let backend = MemoryBackend::new();

    let args = json!({
        "accountId": "nobody",
        "create": { "c1": { "name": "x" } }
    });
    let err = handle_address_book_set(&backend, args)
        .await
        .expect_err("unknown accountId must produce method-level error");

    let err_str = format!("{err:?}");
    assert!(
        err_str.contains("accountNotFound") || err_str.contains("AccountNotFound"),
        "must be accountNotFound: {err_str}"
    );
}

// ---------------------------------------------------------------------------
// Test 8: ContactCard/get on empty store
// ---------------------------------------------------------------------------

#[tokio::test]
async fn contact_card_get_empty_account_returns_empty_list() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({ "accountId": "acc1", "ids": null });
    let (resp, _) = handle_contact_card_get(&backend, args)
        .await
        .expect("/get must succeed");

    assert!(resp["list"].as_array().unwrap().is_empty());
    assert!(resp["notFound"].is_array());
}

// ---------------------------------------------------------------------------
// Test 9: ContactCard/query empty store
// ---------------------------------------------------------------------------

#[tokio::test]
async fn contact_card_query_empty_store_returns_no_ids() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({ "accountId": "acc1", "calculateTotal": true });
    let (resp, _) = handle_contact_card_query(&backend, args)
        .await
        .expect("/query must succeed");

    assert!(resp["ids"].as_array().unwrap().is_empty());
    assert_eq!(resp["total"], 0);
}

// ---------------------------------------------------------------------------
// Test 10: ContactCard/changes empty store
// ---------------------------------------------------------------------------

#[tokio::test]
async fn contact_card_changes_empty_store() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({ "accountId": "acc1", "sinceState": "0" });
    let (resp, _) = handle_contact_card_changes(&backend, args)
        .await
        .expect("/changes must succeed");

    assert!(resp["created"].as_array().unwrap().is_empty());
    assert!(resp["updated"].as_array().unwrap().is_empty());
    assert!(resp["destroyed"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Test 11: state bumps visible via /changes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn address_book_set_state_bumps_visible_via_changes() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Contacts"),
    );

    let args = json!({ "accountId": "acc1", "destroy": ["ab1"] });
    let (resp, _) = handle_address_book_set(&backend, args)
        .await
        .expect("/set must succeed");
    let old_state = resp["oldState"].clone();
    let new_state = resp["newState"].clone();
    assert_ne!(old_state, new_state);

    let args = json!({ "accountId": "acc1", "sinceState": old_state });
    let (changes, _) = handle_address_book_changes(&backend, args)
        .await
        .expect("/changes must succeed");

    let destroyed = changes["destroyed"].as_array().unwrap();
    assert_eq!(destroyed.len(), 1);
    assert_eq!(destroyed[0], "ab1");
    assert_eq!(changes["newState"], new_state);
}

// ---------------------------------------------------------------------------
// Test 12: ContactCard/set create with client id rejected
// Oracle: RFC 8620 §5.3.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn contact_card_set_create_with_client_id_rejected() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Contacts"),
    );

    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": {
                "id": "client-id",
                "@type": "Card",
                "version": "1.0",
                "uid": "u1",
                "addressBookIds": { "ab1": true },
                "name": { "@type": "Name", "full": "Alice" }
            }
        }
    });
    let (resp, _) = handle_contact_card_set(&backend, args)
        .await
        .expect("/set must succeed");

    assert_eq!(
        resp["notCreated"]["c1"]["type"], "invalidProperties",
        "client-supplied id must be rejected: {resp}"
    );
}
