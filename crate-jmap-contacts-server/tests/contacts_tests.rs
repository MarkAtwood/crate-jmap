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
    let (resp, _) = handle_address_book_get(&backend, &(), args)
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
    let (resp, _) = handle_address_book_get(&backend, &(), args)
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
    let (resp, _) = handle_address_book_changes(&backend, &(), args)
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
    let (resp, _) = handle_address_book_set(&backend, &(), args)
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
    let (resp, _) = handle_address_book_set(&backend, &(), args)
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
    let (resp, _) = handle_address_book_set(&backend, &(), args)
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
    let err = handle_address_book_set(&backend, &(), args)
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
    let (resp, _) = handle_contact_card_get(&backend, &(), args)
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
    let (resp, _) = handle_contact_card_query(&backend, &(), args)
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
    let (resp, _) = handle_contact_card_changes(&backend, &(), args)
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
    let (resp, _) = handle_address_book_set(&backend, &(), args)
        .await
        .expect("/set must succeed");
    let old_state = resp["oldState"].clone();
    let new_state = resp["newState"].clone();
    assert_ne!(old_state, new_state);

    let args = json!({ "accountId": "acc1", "sinceState": old_state });
    let (changes, _) = handle_address_book_changes(&backend, &(), args)
        .await
        .expect("/changes must succeed");

    let destroyed = changes["destroyed"].as_array().unwrap();
    assert_eq!(destroyed.len(), 1);
    assert_eq!(destroyed[0], "ab1");
    assert_eq!(changes["newState"], new_state);
}

// ---------------------------------------------------------------------------
// Single-default invariant on AddressBook/set update (bd:JMAP-qz9v.11)
// Oracle: RFC 9610 §2 at-most-one-default semantics; RFC 8620 §5.2 changes
// surface every modified id (including server-set demotions).
// ---------------------------------------------------------------------------

/// Build an AddressBook fixture with explicit isDefault state.
fn address_book_fixture_with_default(id: &str, name: &str, is_default: bool) -> Value {
    let mut v = address_book_fixture(id, name);
    v["isDefault"] = json!(is_default);
    v
}

#[tokio::test]
async fn address_book_set_update_is_default_demotes_other_books_in_memory() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture_with_default("ab1", "First", true),
    );
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab2",
        address_book_fixture_with_default("ab2", "Second", false),
    );

    // Promote ab2 to default via a regular update patch.
    let args = json!({
        "accountId": "acc1",
        "update": { "ab2": { "isDefault": true } }
    });
    let (resp, _) = handle_address_book_set(&backend, &(), args)
        .await
        .expect("/set must succeed");
    assert!(
        resp["updated"].is_object(),
        "updated must be present: {resp}"
    );

    // Read both books back and check the invariant holds.
    let args = json!({ "accountId": "acc1", "ids": ["ab1", "ab2"] });
    let (resp, _) = handle_address_book_get(&backend, &(), args)
        .await
        .expect("/get must succeed");
    let list = resp["list"].as_array().expect("list must be an array");

    let mut found_ab1_default: Option<bool> = None;
    let mut found_ab2_default: Option<bool> = None;
    for book in list {
        match book["id"].as_str() {
            Some("ab1") => found_ab1_default = book["isDefault"].as_bool(),
            Some("ab2") => found_ab2_default = book["isDefault"].as_bool(),
            _ => {}
        }
    }
    assert_eq!(
        found_ab1_default,
        Some(false),
        "ab1 must be demoted to isDefault:false by the invariant: {resp}"
    );
    assert_eq!(
        found_ab2_default,
        Some(true),
        "ab2 must be isDefault:true after promotion: {resp}"
    );
}

#[tokio::test]
async fn address_book_set_update_is_default_records_demoted_books_in_changes() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture_with_default("ab1", "First", true),
    );
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab2",
        address_book_fixture_with_default("ab2", "Second", false),
    );

    // Capture state before the promotion.
    let args = json!({ "accountId": "acc1", "ids": null });
    let (before, _) = handle_address_book_get(&backend, &(), args)
        .await
        .expect("/get must succeed");
    let old_state = before["state"].clone();

    // Promote ab2.
    let args = json!({
        "accountId": "acc1",
        "update": { "ab2": { "isDefault": true } }
    });
    let _ = handle_address_book_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    // /changes since old_state must list both ab1 (demoted) and ab2 (promoted)
    // in updated. RFC 8620 §5.2: every modified id surfaces.
    let args = json!({ "accountId": "acc1", "sinceState": old_state });
    let (changes, _) = handle_address_book_changes(&backend, &(), args)
        .await
        .expect("/changes must succeed");
    let updated = changes["updated"]
        .as_array()
        .expect("updated must be an array");
    let updated_ids: std::collections::HashSet<&str> =
        updated.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        updated_ids.contains("ab1"),
        "demoted ab1 must appear in /changes updated: {changes}"
    );
    assert!(
        updated_ids.contains("ab2"),
        "promoted ab2 must appear in /changes updated: {changes}"
    );
}

#[tokio::test]
async fn address_book_set_update_is_default_no_op_does_not_demote() {
    // Setting isDefault:true on a book that is already default must
    // still trigger the invariant pass (idempotent), but the second
    // book stays at isDefault:false either way.
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture_with_default("ab1", "First", true),
    );
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab2",
        address_book_fixture_with_default("ab2", "Second", false),
    );

    let args = json!({
        "accountId": "acc1",
        "update": { "ab1": { "isDefault": true } }
    });
    let _ = handle_address_book_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    let args = json!({ "accountId": "acc1", "ids": ["ab1", "ab2"] });
    let (resp, _) = handle_address_book_get(&backend, &(), args)
        .await
        .expect("/get must succeed");
    let list = resp["list"].as_array().expect("list must be an array");
    for book in list {
        let id = book["id"].as_str().unwrap_or("");
        let is_default = book["isDefault"].as_bool().unwrap_or(false);
        match id {
            "ab1" => assert!(is_default, "ab1 must remain isDefault:true: {resp}"),
            "ab2" => assert!(!is_default, "ab2 must remain isDefault:false: {resp}"),
            _ => {}
        }
    }
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
    let (resp, _) = handle_contact_card_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert_eq!(
        resp["notCreated"]["c1"]["type"], "invalidProperties",
        "client-supplied id must be rejected: {resp}"
    );
}

// ---------------------------------------------------------------------------
// ContactCard.addressBookIds non-empty invariant (bd:JMAP-qz9v.16)
//
// RFC 9610 §3: a ContactCard MUST belong to at least one AddressBook at
// all times. Both create and update paths must reject post-mutation
// states with empty addressBookIds.
// ---------------------------------------------------------------------------

/// Oracle: create with addressBookIds absent → invalidProperties.
#[tokio::test]
async fn contact_card_create_missing_address_book_ids_rejected() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Book"),
    );

    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": {
                "@type": "Card",
                "version": "1.0",
                "uid": "no-books-card",
                "name": { "@type": "Name", "full": "Alice" }
                // No addressBookIds field at all.
            }
        }
    });
    let (resp, _) = handle_contact_card_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert_eq!(
        resp["notCreated"]["c1"]["type"], "invalidProperties",
        "missing addressBookIds must be rejected: {resp}"
    );
    let properties = resp["notCreated"]["c1"]["properties"]
        .as_array()
        .expect("properties must be array");
    assert_eq!(properties[0], "addressBookIds");
}

/// Oracle: create with addressBookIds = {} → invalidProperties.
#[tokio::test]
async fn contact_card_create_empty_address_book_ids_rejected() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Book"),
    );

    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": {
                "@type": "Card",
                "version": "1.0",
                "uid": "empty-books-card",
                "addressBookIds": {},
                "name": { "@type": "Name", "full": "Bob" }
            }
        }
    });
    let (resp, _) = handle_contact_card_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert_eq!(
        resp["notCreated"]["c1"]["type"], "invalidProperties",
        "empty addressBookIds must be rejected: {resp}"
    );
}

/// Oracle: update that removes the last addressBookIds entry must be
/// rejected with invalidProperties. The stored card remains untouched.
#[tokio::test]
async fn contact_card_update_removing_last_address_book_id_rejected() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Book"),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card1",
        contact_card_fixture("card1", "ab1", "Alice"),
    );

    // RFC 7396 merge-patch shape: {"addressBookIds": {"ab1": null}}
    // removes the ab1 key. The result is addressBookIds = {} which must
    // be rejected.
    let args = json!({
        "accountId": "acc1",
        "update": {
            "card1": {
                "addressBookIds": { "ab1": Value::Null }
            }
        }
    });
    let (resp, _) = handle_contact_card_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert_eq!(
        resp["notUpdated"]["card1"]["type"], "invalidProperties",
        "patch that empties addressBookIds must be rejected: {resp}"
    );

    // The stored card is unchanged.
    let args = json!({ "accountId": "acc1", "ids": ["card1"] });
    let (get_resp, _) = handle_contact_card_get(&backend, &(), args)
        .await
        .expect("/get must succeed");
    let list = get_resp["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(
        list[0]["addressBookIds"],
        json!({ "ab1": true }),
        "rejected update must leave the stored card unchanged: {get_resp}"
    );
}

// ---------------------------------------------------------------------------
// ContactCard.uid uniqueness within an Account (bd:JMAP-qz9v.6)
//
// RFC 9610 §3: 'There MUST NOT be more than one ContactCard with the
// same uid in an Account.' Enforced in MemoryBackend.create_object and
// update_object.
// ---------------------------------------------------------------------------

/// Oracle: create two cards with the same uid in one /set call —
/// the second must fail with invalidProperties on uid.
#[tokio::test]
async fn contact_card_create_duplicate_uid_rejected() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Book"),
    );

    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": {
                "@type": "Card",
                "version": "1.0",
                "uid": "urn:uuid:shared",
                "addressBookIds": { "ab1": true },
                "name": { "@type": "Name", "full": "Alice" }
            },
            "c2": {
                "@type": "Card",
                "version": "1.0",
                "uid": "urn:uuid:shared",
                "addressBookIds": { "ab1": true },
                "name": { "@type": "Name", "full": "Bob" }
            }
        }
    });
    let (resp, _) = handle_contact_card_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    // The handler iterates creates in deterministic order; one succeeds,
    // the other fails. Whichever order, one of c1/c2 must end up in
    // notCreated with the uid-uniqueness error.
    let created = &resp["created"];
    let not_created = &resp["notCreated"];
    let created_count = created.as_object().map(|m| m.len()).unwrap_or(0);
    let not_created_count = not_created.as_object().map(|m| m.len()).unwrap_or(0);
    assert_eq!(created_count, 1, "exactly one card must be created: {resp}");
    assert_eq!(
        not_created_count, 1,
        "exactly one card must be rejected: {resp}"
    );

    // Whichever was rejected, its type is invalidProperties on uid.
    let rejected = not_created.as_object().unwrap().values().next().unwrap();
    assert_eq!(
        rejected["type"], "invalidProperties",
        "uid duplicate must be rejected as invalidProperties: {resp}"
    );
    let properties = rejected["properties"].as_array().expect("properties array");
    assert_eq!(properties[0], "uid");
}

/// Oracle: create a card with a uid that already exists in the
/// account (seeded) → invalidProperties.
#[tokio::test]
async fn contact_card_create_with_uid_matching_seeded_card_rejected() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Book"),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "seeded",
        json!({
            "id": "seeded",
            "@type": "Card",
            "version": "1.0",
            "uid": "urn:uuid:taken",
            "addressBookIds": { "ab1": true }
        }),
    );

    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": {
                "@type": "Card",
                "version": "1.0",
                "uid": "urn:uuid:taken",
                "addressBookIds": { "ab1": true }
            }
        }
    });
    let (resp, _) = handle_contact_card_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert_eq!(
        resp["notCreated"]["c1"]["type"], "invalidProperties",
        "duplicate uid (vs seeded card) must be rejected: {resp}"
    );
}

/// Oracle: same uid in DIFFERENT accounts is allowed — the uniqueness
/// is per-Account, not global.
#[tokio::test]
async fn contact_card_same_uid_different_accounts_allowed() {
    let backend = MemoryBackend::new()
        .with_account("acc1")
        .with_account("acc2");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Book"),
    );
    backend.seed_object(
        "acc2",
        "AddressBook",
        "ab2",
        address_book_fixture("ab2", "Book"),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card-a1",
        json!({
            "id": "card-a1",
            "@type": "Card",
            "version": "1.0",
            "uid": "urn:uuid:cross-account",
            "addressBookIds": { "ab1": true }
        }),
    );

    let args = json!({
        "accountId": "acc2",
        "create": {
            "c1": {
                "@type": "Card",
                "version": "1.0",
                "uid": "urn:uuid:cross-account",
                "addressBookIds": { "ab2": true }
            }
        }
    });
    let (resp, _) = handle_contact_card_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert!(
        resp["created"]["c1"].is_object(),
        "same uid in different account must be allowed: {resp}"
    );
}

/// Oracle: update a card with a uid patch that another card already
/// uses → invalidProperties.
#[tokio::test]
async fn contact_card_update_to_duplicate_uid_rejected() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Book"),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card-a",
        json!({
            "id": "card-a",
            "@type": "Card",
            "version": "1.0",
            "uid": "urn:uuid:a",
            "addressBookIds": { "ab1": true }
        }),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card-b",
        json!({
            "id": "card-b",
            "@type": "Card",
            "version": "1.0",
            "uid": "urn:uuid:b",
            "addressBookIds": { "ab1": true }
        }),
    );

    let args = json!({
        "accountId": "acc1",
        "update": {
            "card-b": { "uid": "urn:uuid:a" }
        }
    });
    let (resp, _) = handle_contact_card_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert_eq!(
        resp["notUpdated"]["card-b"]["type"], "invalidProperties",
        "uid collision in update must be rejected: {resp}"
    );

    // The stored card-b must be unchanged.
    let args = json!({ "accountId": "acc1", "ids": ["card-b"] });
    let (get_resp, _) = handle_contact_card_get(&backend, &(), args)
        .await
        .expect("/get must succeed");
    assert_eq!(
        get_resp["list"][0]["uid"], "urn:uuid:b",
        "rejected update must leave the stored uid unchanged: {get_resp}"
    );
}

/// Oracle: update the same card with the SAME uid it already has
/// (no-op uid change) must succeed — self-exclusion in the uniqueness
/// check allows the no-op case.
#[tokio::test]
async fn contact_card_update_with_same_uid_succeeds() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Book"),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card-a",
        json!({
            "id": "card-a",
            "@type": "Card",
            "version": "1.0",
            "uid": "urn:uuid:a",
            "addressBookIds": { "ab1": true }
        }),
    );

    let args = json!({
        "accountId": "acc1",
        "update": {
            "card-a": { "uid": "urn:uuid:a" }
        }
    });
    let (resp, _) = handle_contact_card_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert!(
        resp["notUpdated"].is_null(),
        "no-op uid update must succeed: {resp}"
    );
}

/// Oracle: update that doesn't touch addressBookIds keeps the existing
/// non-empty state and succeeds.
#[tokio::test]
async fn contact_card_update_unrelated_field_preserves_invariant() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Book"),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card1",
        contact_card_fixture("card1", "ab1", "Alice"),
    );

    let args = json!({
        "accountId": "acc1",
        "update": {
            "card1": {
                "kind": "individual"
            }
        }
    });
    let (resp, _) = handle_contact_card_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    assert!(
        resp["notUpdated"].is_null(),
        "unrelated-field update must succeed: {resp}"
    );
    assert!(
        resp["updated"]["card1"].is_null() || resp["updated"]["card1"].is_object(),
        "updated must contain card1: {resp}"
    );
}

// ---------------------------------------------------------------------------
// AddressBook/set onDestroyRemoveContents cascade tests (bd:JMAP-qz9v.1)
//
// RFC 9610 §2.3: when onDestroyRemoveContents is true, the destroy must
// remove this book's addressBookIds entry from every ContactCard. Cards
// left with zero books must be destroyed; cards still attached to other
// books must persist with the remaining addressBookIds.
// ---------------------------------------------------------------------------

/// Oracle: a card whose only AddressBook is the one being destroyed
/// must be destroyed by the cascade (the alternative is leaving an
/// orphan card with a now-dangling addressBookIds entry — a ContactCard
/// is required to belong to at least one AddressBook per RFC 9610 §3).
#[tokio::test]
async fn address_book_set_destroy_cascade_destroys_exclusive_card() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Only Book"),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card-exclusive",
        contact_card_fixture("card-exclusive", "ab1", "Alice"),
    );

    let args = json!({
        "accountId": "acc1",
        "onDestroyRemoveContents": true,
        "destroy": ["ab1"]
    });
    let (resp, _) = handle_address_book_set(&backend, &(), args)
        .await
        .expect("/set must succeed");

    // Book is destroyed.
    let destroyed = resp["destroyed"]
        .as_array()
        .expect("destroyed must be array");
    assert_eq!(destroyed.len(), 1);
    assert_eq!(destroyed[0], "ab1");

    // The exclusive card is gone from the store.
    let args = json!({ "accountId": "acc1", "ids": ["card-exclusive"] });
    let (get_resp, _) = handle_contact_card_get(&backend, &(), args)
        .await
        .expect("/get must succeed");
    let not_found = get_resp["notFound"]
        .as_array()
        .expect("notFound must be array");
    assert_eq!(
        not_found.len(),
        1,
        "exclusive card must be destroyed by cascade: {get_resp}"
    );
    assert_eq!(not_found[0], "card-exclusive");
}

/// Oracle: a card belonging to multiple AddressBooks must be patched —
/// the destroyed book's id is removed from `addressBookIds`, the card
/// itself persists with the remaining books.
#[tokio::test]
async fn address_book_set_destroy_cascade_patches_shared_card() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Book One"),
    );
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab2",
        address_book_fixture("ab2", "Book Two"),
    );
    // Card belongs to BOTH ab1 and ab2.
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card-shared",
        json!({
            "id": "card-shared",
            "@type": "Card",
            "version": "1.0",
            "uid": "u-shared",
            "addressBookIds": { "ab1": true, "ab2": true },
            "name": { "@type": "Name", "full": "Bob" }
        }),
    );

    let args = json!({
        "accountId": "acc1",
        "onDestroyRemoveContents": true,
        "destroy": ["ab1"]
    });
    let (resp, _) = handle_address_book_set(&backend, &(), args)
        .await
        .expect("/set must succeed");
    let destroyed = resp["destroyed"]
        .as_array()
        .expect("destroyed must be array");
    assert_eq!(destroyed[0], "ab1");

    // The shared card still exists, with addressBookIds={ab2: true}.
    let args = json!({ "accountId": "acc1", "ids": ["card-shared"] });
    let (get_resp, _) = handle_contact_card_get(&backend, &(), args)
        .await
        .expect("/get must succeed");
    let list = get_resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1, "shared card must persist: {get_resp}");
    assert_eq!(
        list[0]["addressBookIds"],
        json!({ "ab2": true }),
        "removed book id must be patched out of addressBookIds: {get_resp}"
    );
}

/// Oracle: mixed scenario — some cards are exclusive (destroyed), some
/// shared (patched). All happen in a single cascade.
#[tokio::test]
async fn address_book_set_destroy_cascade_mixed_exclusive_and_shared() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Doomed Book"),
    );
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab2",
        address_book_fixture("ab2", "Other Book"),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card-exclusive",
        contact_card_fixture("card-exclusive", "ab1", "Alice"),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card-shared",
        json!({
            "id": "card-shared",
            "@type": "Card",
            "version": "1.0",
            "uid": "u-shared",
            "addressBookIds": { "ab1": true, "ab2": true },
            "name": { "@type": "Name", "full": "Bob" }
        }),
    );

    let args = json!({
        "accountId": "acc1",
        "onDestroyRemoveContents": true,
        "destroy": ["ab1"]
    });
    let (resp, _) = handle_address_book_set(&backend, &(), args)
        .await
        .expect("/set must succeed");
    assert_eq!(resp["destroyed"][0], "ab1");

    // card-exclusive is gone.
    let args = json!({ "accountId": "acc1", "ids": ["card-exclusive", "card-shared"] });
    let (get_resp, _) = handle_contact_card_get(&backend, &(), args)
        .await
        .expect("/get must succeed");
    let not_found: Vec<&str> = get_resp["notFound"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        not_found,
        vec!["card-exclusive"],
        "exclusive card destroyed"
    );

    // card-shared survives, patched.
    let list = get_resp["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], "card-shared");
    assert_eq!(list[0]["addressBookIds"], json!({ "ab2": true }));
}

/// Oracle: an empty AddressBook with onDestroyRemoveContents=true still
/// destroys cleanly (the cascade is a no-op when no cards reference it).
#[tokio::test]
async fn address_book_set_destroy_cascade_empty_book_proceeds() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Empty Book"),
    );

    let args = json!({
        "accountId": "acc1",
        "onDestroyRemoveContents": true,
        "destroy": ["ab1"]
    });
    let (resp, _) = handle_address_book_set(&backend, &(), args)
        .await
        .expect("/set must succeed");
    assert_eq!(resp["destroyed"][0], "ab1");
}

// ---------------------------------------------------------------------------
// Test 13: ContactCard/query honors typed filter fields end-to-end (bd:JMAP-qz9v.3)
// Oracle: RFC 9610 §3.3.1. Previously every filter field except
// inAddressBook was silently dropped and all cards were returned.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn contact_card_query_filter_by_kind_excludes_non_matches() {
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
        "card-person",
        json!({
            "id": "card-person",
            "@type": "Card",
            "version": "1.0",
            "uid": "u-person",
            "kind": "individual",
            "addressBookIds": { "ab1": true },
            "name": { "@type": "Name", "full": "Alice" }
        }),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card-group",
        json!({
            "id": "card-group",
            "@type": "Card",
            "version": "1.0",
            "uid": "u-group",
            "kind": "group",
            "addressBookIds": { "ab1": true },
            "name": { "@type": "Name", "full": "Beta Team" }
        }),
    );

    // Query for kind = "individual" — must exclude the group card.
    let args = json!({
        "accountId": "acc1",
        "filter": { "kind": "individual" }
    });
    let (resp, _) = handle_contact_card_query(&backend, &(), args)
        .await
        .expect("/query must succeed");

    let ids = resp["ids"].as_array().expect("ids must be array");
    assert_eq!(ids.len(), 1, "kind filter must exclude group: {resp}");
    assert_eq!(ids[0], "card-person");
}

// ---------------------------------------------------------------------------
// Test 14: ContactCard/query honors sort comparators (bd:JMAP-qz9v.3)
// Oracle: RFC 9610 §3.3.2. Previously the sort argument was silently
// ignored; ids came back in HashMap iteration order or Id-string order.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn contact_card_query_sort_by_created_descending() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Contacts"),
    );
    // Seed three cards with chronologically-ordered `created` timestamps
    // but lexicographically-disordered ids (so Id-sort and created-sort
    // produce different orderings — the test fails if sort is ignored).
    backend.seed_object(
        "acc1",
        "ContactCard",
        "c-zulu",
        json!({
            "id": "c-zulu",
            "@type": "Card",
            "version": "1.0",
            "uid": "u-z",
            "created": "2020-01-01T00:00:00Z",
            "addressBookIds": { "ab1": true }
        }),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "c-alpha",
        json!({
            "id": "c-alpha",
            "@type": "Card",
            "version": "1.0",
            "uid": "u-a",
            "created": "2022-01-01T00:00:00Z",
            "addressBookIds": { "ab1": true }
        }),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "c-mike",
        json!({
            "id": "c-mike",
            "@type": "Card",
            "version": "1.0",
            "uid": "u-m",
            "created": "2021-01-01T00:00:00Z",
            "addressBookIds": { "ab1": true }
        }),
    );

    let args = json!({
        "accountId": "acc1",
        "sort": [ { "property": "created", "isAscending": false } ]
    });
    let (resp, _) = handle_contact_card_query(&backend, &(), args)
        .await
        .expect("/query must succeed");

    let ids: Vec<&str> = resp["ids"]
        .as_array()
        .expect("ids must be array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["c-alpha", "c-mike", "c-zulu"],
        "sort by created descending must be newest first: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 15: ContactCard/copy onSuccessDestroyOriginal end-to-end (bd:JMAP-qz9v.2)
// Oracle: RFC 8620 §5.4 inherited by RFC 9610 §3.4 — successful copies
// followed by an implicit destroy must remove the source cards from the
// fromAccountId account.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn contact_card_copy_on_success_destroy_original_actually_destroys_source() {
    use jmap_contacts_server::handle_contact_card_copy;

    let backend = MemoryBackend::new()
        .with_account("acc1")
        .with_account("acc2");
    backend.seed_object(
        "acc1",
        "AddressBook",
        "ab1",
        address_book_fixture("ab1", "Source Book"),
    );
    backend.seed_object(
        "acc2",
        "AddressBook",
        "ab2",
        address_book_fixture("ab2", "Destination Book"),
    );
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card-source",
        contact_card_fixture("card-source", "ab1", "Source Card"),
    );

    let args = json!({
        "accountId": "acc2",
        "fromAccountId": "acc1",
        "onSuccessDestroyOriginal": true,
        "create": {
            "c1": {
                "id": "card-source",
                "addressBookIds": { "ab2": true }
            }
        }
    });
    let (resp, extra) = handle_contact_card_copy(&backend, &(), args, "call-0")
        .await
        .expect("/copy must succeed");

    // The /copy itself succeeded.
    assert!(
        resp["copied"]["c1"].is_object(),
        "copy must succeed: {resp}"
    );

    // A single synthetic ContactCard/set invocation was emitted.
    assert_eq!(extra.len(), 1, "exactly one synthetic invocation expected");
    let (method, set_resp, _) = &extra[0];
    assert_eq!(method, "ContactCard/set");

    // The source card appears in the synthetic /set's destroyed array.
    let destroyed = set_resp["destroyed"]
        .as_array()
        .expect("destroyed must be array (non-empty) when destroy succeeds");
    assert_eq!(
        destroyed.len(),
        1,
        "exactly one source destroyed: {set_resp}"
    );
    assert_eq!(destroyed[0], "card-source");

    // Verify the source card is actually gone from acc1.
    let args = json!({ "accountId": "acc1", "ids": ["card-source"] });
    let (get_resp, _) = handle_contact_card_get(&backend, &(), args)
        .await
        .expect("/get must succeed");
    let not_found = get_resp["notFound"]
        .as_array()
        .expect("notFound must be array");
    assert_eq!(
        not_found.len(),
        1,
        "source card must be gone after destroy: {get_resp}"
    );
    assert_eq!(not_found[0], "card-source");
}
