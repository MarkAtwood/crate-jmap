//! `ContactCard/copy` runs the ContactCard create checks on the merged copy
//! (bd:JMAP-bfdh).
//!
//! Oracle: RFC 8620 §5.4 (rfc8620.txt:2312-2317): notCreated holds "a
//! SetError object for each record that failed to be copied" and "The
//! SetError may be any of the standard set errors returned for a create
//! or update"; RFC 9610 §3 (rfc9610.txt:307-313): "there MUST NOT be more
//! than one ContactCard with the same uid in an Account" and "A card MUST
//! belong to at least one AddressBook at all times".

mod common;

use std::collections::BTreeMap;

use common::MemoryBackend;
use jmap_contacts_server::{
    handle_address_book_set, handle_contact_card_copy, handle_contact_card_set,
};
use jmap_types::Id;
use serde_json::{Value, json};

const FROM: &str = "a1";
const TO: &str = "a2";
const UID: &str = "urn:uuid:8f3c1a52-7d6e-4b0a-9c2f-1e5d4a3b2c10";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:contacts".to_owned(),
    ]
}

async fn create_book(b: &MemoryBackend, account: &str) -> String {
    let args = json!({ "accountId": account, "create": { "c": { "name": "b" } } });
    let resp = handle_address_book_set(b, &(), args, &using())
        .await
        .expect("AddressBook/set must not fail at method level")
        .primary;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("AddressBook/set create failed: {resp}"))
        .to_owned()
}

async fn create_card(b: &MemoryBackend, account: &str, book: &str) -> String {
    let create = json!({ "addressBookIds": { book: true }, "uid": UID });
    let args = json!({ "accountId": account, "create": { "c": create } });
    let resp = handle_contact_card_set(b, &(), &BTreeMap::new(), args, &using())
        .await
        .expect("ContactCard/set must not fail at method level")
        .primary;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("ContactCard/set create failed: {resp}"))
        .to_owned()
}

async fn copy(b: &MemoryBackend, entry: Value) -> Value {
    let args = json!({ "fromAccountId": FROM, "accountId": TO, "create": { "k": entry } });
    handle_contact_card_copy(b, &(), args, "c0", &using())
        .await
        .expect("/copy must not fail at method level")
        .primary
}

fn backend() -> MemoryBackend {
    MemoryBackend::new()
        .with_account(&Id::from(FROM))
        .with_account(&Id::from(TO))
}

fn assert_not_created(resp: &Value, property: &str) {
    assert_eq!(resp["created"]["k"], Value::Null, "{resp}");
    assert_eq!(
        resp["notCreated"]["k"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );
    assert_eq!(
        resp["notCreated"]["k"]["properties"],
        json!([property]),
        "{resp}"
    );
}

/// A copy whose override empties `addressBookIds` would leave the new card
/// in no AddressBook, which a create rejects.
#[tokio::test]
async fn copy_rejects_empty_address_book_ids() {
    let b = backend();
    let book = create_book(&b, FROM).await;
    let src = create_card(&b, FROM, &book).await;
    let resp = copy(&b, json!({ "id": src, "addressBookIds": {} })).await;
    assert_not_created(&resp, "addressBookIds");
}

/// A copy whose uid is already used by a card in the destination account
/// is rejected, as the same create would be.
#[tokio::test]
async fn copy_rejects_duplicate_uid_in_destination() {
    let b = backend();
    let from_book = create_book(&b, FROM).await;
    let src = create_card(&b, FROM, &from_book).await;
    let to_book = create_book(&b, TO).await;
    create_card(&b, TO, &to_book).await;
    let resp = copy(
        &b,
        json!({ "id": src, "addressBookIds": { to_book: true } }),
    )
    .await;
    assert_not_created(&resp, "uid");
}

/// The same copy succeeds when the destination has no card with that uid.
#[tokio::test]
async fn copy_with_unique_uid_and_book_succeeds() {
    let b = backend();
    let from_book = create_book(&b, FROM).await;
    let src = create_card(&b, FROM, &from_book).await;
    let to_book = create_book(&b, TO).await;
    let resp = copy(
        &b,
        json!({ "id": src, "addressBookIds": { to_book: true } }),
    )
    .await;
    assert!(resp["created"]["k"]["id"].is_string(), "{resp}");
}
