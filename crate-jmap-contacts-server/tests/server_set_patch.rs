//! Server-set values and empty patches in `AddressBook/set` and
//! `ContactCard/set` updates (bd:JMAP-t8ej.25.3).
//!
//! Oracle, RFC 8620 §5.3 (update): "Any server-set properties MAY be
//! included in the patch if their value is identical to the current server
//! value (before applying the patches to the object).  Otherwise, the
//! update MUST be rejected with an "invalidProperties" SetError."
//!
//! RFC 9610 §2: AddressBook `id` is "(immutable; server-set)", `isDefault`
//! and `myRights` "(server-set)"; §3: ContactCard `id` is "(immutable;
//! server-set)".
//!
//! An empty patch `{}`, literal or left empty once the identical
//! server-set values are dropped, succeeds as a no-op: `updated` maps the
//! id to null, nothing is written, and the state is unchanged (decision
//! recorded on JMAP-t8ej.25).

mod common;

use common::MemoryBackend;
use jmap_contacts_server::{
    handle_address_book_get, handle_address_book_set, handle_contact_card_get,
    handle_contact_card_set,
};
use jmap_types::Id;
use serde_json::{Value, json};

/// A backend with AddressBook `ab1` and ContactCard `card1` in it.
fn seeded_backend() -> MemoryBackend {
    let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
    backend.seed_object(
        &Id::from("acc1"),
        "AddressBook",
        &Id::from("ab1"),
        json!({
            "id": "ab1",
            "name": "Personal",
            "sortOrder": 0,
            "isDefault": true,
            "isSubscribed": true,
            "myRights": {
                "mayRead": true,
                "mayWrite": true,
                "mayShare": true,
                "mayDelete": true
            }
        }),
    );
    backend.seed_object(
        &Id::from("acc1"),
        "ContactCard",
        &Id::from("card1"),
        json!({
            "id": "card1",
            "@type": "Card",
            "version": "1.0",
            "uid": "card1-uid",
            "addressBookIds": { "ab1": true },
            "name": { "@type": "Name", "full": "Alice" }
        }),
    );
    backend
}

/// Assert `resp` reports `id` as a no-op update: `updated[id]` is null,
/// nothing failed, and the state did not move.
fn assert_noop(resp: &Value, id: &str) {
    assert!(resp["notUpdated"].is_null(), "{resp}");
    let updated = resp["updated"]
        .as_object()
        .unwrap_or_else(|| panic!("{resp}"));
    assert_eq!(updated.get(id), Some(&Value::Null), "{resp}");
    assert_eq!(resp["oldState"], resp["newState"], "{resp}");
}

async fn book_set(backend: &MemoryBackend, patch: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": { "ab1": patch } });
    handle_address_book_set(backend, &(), args, &[])
        .await
        .expect("AddressBook/set")
        .primary
}

async fn book_get(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["ab1"] });
    handle_address_book_get(backend, &(), args, &[])
        .await
        .expect("AddressBook/get")
        .primary["list"][0]
        .clone()
}

async fn card_set(backend: &MemoryBackend, patch: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": { "card1": patch } });
    handle_contact_card_set(backend, &(), &std::collections::BTreeMap::new(), args, &[])
        .await
        .expect("ContactCard/set")
        .primary
}

async fn card_get(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["card1"] });
    handle_contact_card_get(backend, &(), args, &[])
        .await
        .expect("ContactCard/get")
        .primary["list"][0]
        .clone()
}

/// §5.3: the AddressBook server-set values sent as seeded are allowed;
/// with nothing else, the update is a no-op.
#[tokio::test]
async fn address_book_identical_server_set_values_are_a_noop() {
    let backend = seeded_backend();
    let before = book_get(&backend).await;
    let patch = json!({
        "id": "ab1",
        "isDefault": true,
        "myRights": {
            "mayRead": true,
            "mayWrite": true,
            "mayShare": true,
            "mayDelete": true
        }
    });
    let resp = book_set(&backend, patch).await;
    assert_noop(&resp, "ab1");
    assert_eq!(book_get(&backend).await, before);
}

/// §5.3: an identical server-set value next to a client change is dropped
/// and the change applies.
#[tokio::test]
async fn address_book_identical_server_set_value_with_change_applies() {
    let backend = seeded_backend();
    let resp = book_set(&backend, json!({ "isDefault": true, "name": "Home" })).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert!(resp["updated"].get("ab1").is_some(), "{resp}");
    assert_eq!(book_get(&backend).await["name"], "Home");
}

/// Decision on JMAP-t8ej.25: a literal `{}` is a no-op success.
#[tokio::test]
async fn address_book_empty_patch_is_a_noop() {
    let backend = seeded_backend();
    let before = book_get(&backend).await;
    let resp = book_set(&backend, json!({})).await;
    assert_noop(&resp, "ab1");
    assert_eq!(book_get(&backend).await, before);
}

/// §5.3 "Otherwise, the update MUST be rejected with an
/// "invalidProperties" SetError."
#[tokio::test]
async fn address_book_changed_server_set_value_is_invalid_properties() {
    let backend = seeded_backend();
    let before = book_get(&backend).await;
    let resp = book_set(&backend, json!({ "isDefault": false, "name": "Home" })).await;
    assert_eq!(
        resp["notUpdated"]["ab1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"]["ab1"]["properties"],
        json!(["isDefault"]),
        "{resp}"
    );
    assert_eq!(book_get(&backend).await, before);
}

/// §5.3: ContactCard `id` sent with its stored value is allowed; with
/// nothing else, the update is a no-op.
#[tokio::test]
async fn card_identical_server_set_value_is_a_noop() {
    let backend = seeded_backend();
    let before = card_get(&backend).await;
    let resp = card_set(&backend, json!({ "id": "card1" })).await;
    assert_noop(&resp, "card1");
    assert_eq!(card_get(&backend).await, before);
}

/// Decision on JMAP-t8ej.25: a literal `{}` is a no-op success.
#[tokio::test]
async fn card_empty_patch_is_a_noop() {
    let backend = seeded_backend();
    let before = card_get(&backend).await;
    let resp = card_set(&backend, json!({})).await;
    assert_noop(&resp, "card1");
    assert_eq!(card_get(&backend).await, before);
}

/// §5.3 "Otherwise, the update MUST be rejected with an
/// "invalidProperties" SetError."
#[tokio::test]
async fn card_changed_server_set_value_is_invalid_properties() {
    let backend = seeded_backend();
    let before = card_get(&backend).await;
    let resp = card_set(&backend, json!({ "id": "other", "name/full": "Bob" })).await;
    assert_eq!(
        resp["notUpdated"]["card1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"]["card1"]["properties"],
        json!(["id"]),
        "{resp}"
    );
    assert_eq!(card_get(&backend).await, before);
}

/// Decision on JMAP-t8ej.25: an empty patch of a missing id is
/// `notFound`.
#[tokio::test]
async fn card_empty_patch_of_missing_id_is_not_found() {
    let backend = seeded_backend();
    let args = json!({ "accountId": "acc1", "update": { "nope": {} } });
    let resp =
        handle_contact_card_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
            .await
            .expect("ContactCard/set")
            .primary;
    assert_eq!(resp["notUpdated"]["nope"]["type"], "notFound", "{resp}");
}
