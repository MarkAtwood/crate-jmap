//! ContactCard/set `update` applies RFC 8620 §5.3 PatchObjects
//! (bd:JMAP-cr0z), not RFC 7396 JSON Merge Patch.
//!
//! Every expected value is derived from the quoted RFC 8620 §5.3 text,
//! not from the code under test.

mod common;

use common::MemoryBackend;
use jmap_contacts_server::{
    handle_address_book_get, handle_address_book_set, handle_contact_card_get,
    handle_contact_card_set,
};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

/// A minimal valid AddressBook.
fn address_book(id: &str) -> Value {
    json!({
        "id": id,
        "name": id,
        "sortOrder": 0,
        "isDefault": false,
        "isSubscribed": true,
        "myRights": {
            "mayRead": true,
            "mayWrite": true,
            "mayShare": true,
            "mayDelete": true
        }
    })
}

/// A backend with AddressBooks `ab1` and `ab2` and ContactCard `card1`,
/// which belongs to `ab1` and has `name.full` "Alice".
fn seeded_backend() -> MemoryBackend {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "AddressBook", "ab1", address_book("ab1"));
    backend.seed_object("acc1", "AddressBook", "ab2", address_book("ab2"));
    backend.seed_object(
        "acc1",
        "ContactCard",
        "card1",
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

/// Send `ContactCard/set` with `update: { "card1": patch }`.
async fn update_card1(backend: &MemoryBackend, patch: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": { "card1": patch } });
    handle_contact_card_set(backend, &(), args)
        .await
        .expect("/set must succeed at the method level")
        .primary
}

/// Fetch `card1` with `ContactCard/get`.
async fn get_card1(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["card1"] });
    let HandlerOutput { primary: resp, .. } = handle_contact_card_get(backend, &(), args)
        .await
        .expect("/get must succeed");
    resp["list"][0].clone()
}

/// RFC 8620 §5.3: "The keys are a path in JSON Pointer format [RFC6901],
/// with an implicit leading "/" (i.e., prefix each key with "/" before
/// applying the JSON Pointer evaluation algorithm)."
///
/// So `"name/full"` sets the `full` member of `name`. It is not a literal
/// top-level `"name/full"` property.
#[tokio::test]
async fn update_path_key_sets_nested_member() {
    let backend = seeded_backend();
    let resp = update_card1(&backend, json!({ "name/full": "Bob" })).await;
    assert!(
        resp["updated"].get("card1").is_some(),
        "card1 must be updated: {resp}"
    );

    let card = get_card1(&backend).await;
    assert_eq!(card["name"], json!({ "@type": "Name", "full": "Bob" }));
    assert!(
        card.get("name/full").is_none(),
        "no literal \"name/full\" property: {card}"
    );
}

/// RFC 8620 §5.3: "Anything else: The value to set for this property
/// (this may be a replacement or addition to the object being patched)."
///
/// The key `"addressBookIds"` points at the whole property, so the given
/// map replaces it wholesale; `ab1` is not merged back in.
#[tokio::test]
async fn update_object_value_replaces_property_wholesale() {
    let backend = seeded_backend();
    let resp = update_card1(&backend, json!({ "addressBookIds": { "ab2": true } })).await;
    assert!(
        resp["updated"].get("card1").is_some(),
        "card1 must be updated: {resp}"
    );

    let card = get_card1(&backend).await;
    assert_eq!(card["addressBookIds"], json!({ "ab2": true }));
}

/// RFC 8620 §5.3: "All parts prior to the last (i.e., the value after the
/// final slash) MUST already exist on the object being patched." and "if
/// there is any violation, the update MUST be rejected with an
/// "invalidPatch" error".
///
/// card1 has no `nicknames`, so `"nicknames/n1"` is rejected and the
/// other patch in the same PatchObject is not applied.
#[tokio::test]
async fn update_path_through_absent_parent_is_invalid_patch() {
    let backend = seeded_backend();
    let before = get_card1(&backend).await;
    let resp = update_card1(
        &backend,
        json!({
            "name/full": "Bob",
            "nicknames/n1": { "@type": "Nickname", "name": "Al" }
        }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["card1"]["type"], "invalidPatch",
        "absent parent must be invalidPatch: {resp}"
    );
    assert_eq!(get_card1(&backend).await, before, "card1 must be unchanged");
}

/// RFC 8620 §5.3: "There MUST NOT be two patches in the PatchObject where
/// the pointer of one is the prefix of the pointer of the other, e.g.,
/// "alerts/1/offset" and "alerts"." and "if there is any violation, the
/// update MUST be rejected with an "invalidPatch" error".
#[tokio::test]
async fn update_prefix_conflict_is_invalid_patch() {
    let backend = seeded_backend();
    let before = get_card1(&backend).await;
    let resp = update_card1(
        &backend,
        json!({
            "name": { "@type": "Name", "full": "Carol" },
            "name/full": "Bob"
        }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["card1"]["type"], "invalidPatch",
        "prefix conflict must be invalidPatch: {resp}"
    );
    assert_eq!(get_card1(&backend).await, before, "card1 must be unchanged");
}

/// RFC 8620 §5.3: "null: Set to the default value if specified for this
/// property; otherwise, the property is removed." RFC 9610
/// (rfc9610.txt:180): "sortOrder: UnsignedInt (default: 0)".
///
/// So patching `sortOrder` to null on an AddressBook whose sortOrder is 7
/// must leave it readable with sortOrder 0.
#[tokio::test]
async fn update_address_book_sort_order_null_resets_to_default() {
    let backend = seeded_backend();
    let mut ab1 = address_book("ab1");
    ab1["sortOrder"] = json!(7);
    backend.seed_object("acc1", "AddressBook", "ab1", ab1);

    let args = json!({ "accountId": "acc1", "update": { "ab1": { "sortOrder": null } } });
    let HandlerOutput { primary: resp, .. } = handle_address_book_set(&backend, &(), args)
        .await
        .expect("/set must succeed at the method level");
    assert!(
        resp["updated"].get("ab1").is_some(),
        "ab1 must be updated: {resp}"
    );

    let args = json!({ "accountId": "acc1", "ids": ["ab1"] });
    let HandlerOutput { primary: resp, .. } = handle_address_book_get(&backend, &(), args)
        .await
        .expect("AddressBook/get must succeed after sortOrder: null");
    assert_eq!(resp["list"][0]["sortOrder"], json!(0), "{resp}");
}

/// RFC 8620 §5.3: "If null, set to the default value if specified for
/// this property; otherwise, remove the property from the patched
/// object." RFC 9610 §2 gives `*name*: String` with no default, so the
/// patched AddressBook lacks a required property and, per RFC 8620 §5.3,
/// "invalidProperties": "The record given is invalid in some way." The
/// rejected update must leave the stored book readable and unchanged.
#[tokio::test]
async fn address_book_required_name_null_is_invalid_properties() {
    let backend = seeded_backend();
    let args = json!({ "accountId": "acc1", "update": { "ab1": { "name": null } } });
    let HandlerOutput { primary: resp, .. } = handle_address_book_set(&backend, &(), args)
        .await
        .expect("AddressBook/set must succeed at the method level");
    assert_eq!(
        resp["notUpdated"]["ab1"]["type"], "invalidProperties",
        "{resp}"
    );

    let args = json!({ "accountId": "acc1", "ids": ["ab1"] });
    let HandlerOutput { primary: resp, .. } = handle_address_book_get(&backend, &(), args)
        .await
        .expect("AddressBook/get must succeed after a rejected update");
    assert_eq!(resp["list"][0]["name"], "ab1", "{resp}");
}

/// RFC 8620 §5.3 "invalidProperties": "It contains properties that are
/// invalid according to the type specification of this record type."
/// RFC 9553 §2.1.9: "uid: String (mandatory)", so a number is rejected
/// and the stored card stays readable and unchanged.
#[tokio::test]
async fn contact_card_wrong_typed_uid_is_invalid_properties() {
    let backend = seeded_backend();
    let before = get_card1(&backend).await;
    let resp = update_card1(&backend, json!({ "uid": 5 })).await;
    assert_eq!(
        resp["notUpdated"]["card1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(get_card1(&backend).await, before, "card1 must be unchanged");
}
