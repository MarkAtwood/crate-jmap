//! `#creationId` references in `ContactCard/set` `addressBookIds`, through
//! the full dispatcher (bd:JMAP-1p88).
//!
//! Oracle: RFC 8620 §5.3 (rfc8620.txt:1983-1996): "Some records may hold
//! references to other records (foreign keys). That reference may be set
//! (via create or update) in the same request as the referenced record is
//! created. To do this, the client refers to the new record using its
//! creation id prefixed with a "#". [...] the server MUST keep a simple map
//! for the duration of the request of creation id to record id for each
//! newly created record, so it can substitute in the correct value if
//! necessary in later method calls." RFC 9610 §3: `addressBookIds` is an
//! `Id[Boolean]` set of AddressBook ids.
//!
//! Each assertion compares the `addressBookIds` keys with the id the server
//! reported for the referenced AddressBook, or with a hand-written SetError
//! shape.

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_contacts_server::register_contacts_handlers;
use jmap_server::Dispatcher;
use jmap_types::{JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";

fn dispatcher() -> Dispatcher<()> {
    let mut dispatcher = Dispatcher::new();
    register_contacts_handlers(
        &mut dispatcher,
        Arc::new(MemoryBackend::new().with_account(ACCOUNT)),
    );
    dispatcher
}

async fn run(dispatcher: &Dispatcher<()>, method_calls: Value) -> JmapResponse {
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:contacts"],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher.dispatch(request, (), State::from("s0")).await
}

fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

fn card(address_book_ids: Value) -> Value {
    json!({
        "@type": "Card",
        "version": "1.0",
        "uid": "c-uid",
        "addressBookIds": address_book_ids,
        "name": { "@type": "Name", "full": "Ada" }
    })
}

#[tokio::test]
async fn address_book_ids_key_resolves_on_create() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["AddressBook/set", { "accountId": ACCOUNT, "create": { "b": { "name": "Friends", "isSubscribed": true } } }, "0"],
            ["ContactCard/set", { "accountId": ACCOUNT,
                "create": { "c": card(json!({ "#b": true })) } }, "1"]
        ]),
    )
    .await;
    let book = args(&resp, 0)["created"]["b"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("book: {}", args(&resp, 0)))
        .to_owned();
    let card_id = args(&resp, 1)["created"]["c"]["id"].clone();
    let resp = run(
        &d,
        json!([["ContactCard/get", { "accountId": ACCOUNT, "ids": [card_id],
            "properties": ["addressBookIds"] }, "0"]]),
    )
    .await;
    assert_eq!(
        args(&resp, 0)["list"][0]["addressBookIds"],
        json!({ book: true }),
        "{}",
        args(&resp, 0)
    );
}

#[tokio::test]
async fn address_book_ids_patch_path_resolves_on_update() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["AddressBook/set", { "accountId": ACCOUNT, "create": { "b": { "name": "Friends", "isSubscribed": true } } }, "0"],
            ["ContactCard/set", { "accountId": ACCOUNT,
                "create": { "c": card(json!({ "#b": true })) } }, "1"]
        ]),
    )
    .await;
    let first = args(&resp, 0)["created"]["b"]["id"].clone();
    let card_id = args(&resp, 1)["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("card: {}", args(&resp, 1)))
        .to_owned();

    let resp = run(
        &d,
        json!([
            ["AddressBook/set", { "accountId": ACCOUNT, "create": { "w": { "name": "Work", "isSubscribed": true } } }, "0"],
            ["ContactCard/set", { "accountId": ACCOUNT,
                "update": { (card_id.clone()): { "addressBookIds/#w": true } } }, "1"],
            ["ContactCard/get", { "accountId": ACCOUNT, "ids": [card_id],
                "properties": ["addressBookIds"] }, "2"]
        ]),
    )
    .await;
    let work = args(&resp, 0)["created"]["w"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("work: {}", args(&resp, 0)))
        .to_owned();
    let first = first.as_str().expect("first book id");
    assert_eq!(
        args(&resp, 2)["list"][0]["addressBookIds"],
        json!({ first: true, work: true }),
        "{}",
        args(&resp, 2)
    );
}

#[tokio::test]
async fn unknown_address_book_reference_is_invalid_properties() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["ContactCard/set", { "accountId": ACCOUNT,
            "create": { "c": card(json!({ "#nope": true })) } }, "0"]]),
    )
    .await;
    let err = &args(&resp, 0)["notCreated"]["c"];
    assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, 0));
    assert_eq!(err["properties"], json!(["addressBookIds"]), "{err}");
}
