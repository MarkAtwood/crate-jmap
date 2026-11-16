//! Conditional contacts Foo/set (draft-ietf-jmap-conditional-02) through
//! `register_contacts_conditional` and the memory backend's `ScopedState`
//! scopes (bd JMAP-y1yu.12; docs/design/conditional.md §5.1, §7).
//!
//! Oracles: conditional-02 §3.1 and §4.2 ("the server MUST NOT make any
//! of the changes"), RFC 9610 §2.3 (`onDestroyRemoveContents` destroys the
//! AddressBook's ContactCards with it).
//!
//! The crate has no cross-object rule for an atomic commit to check: one
//! default AddressBook holds by construction (setting one clears the
//! others), and there are no caps. So rollback is what is tested.

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_contacts_server::{register_contacts_conditional, register_contacts_handlers};
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:contacts",
    "urn:ietf:params:jmap:conditional",
];

/// Account acc1 with AddressBook ab1 "Friends" holding ContactCard card1.
fn backend() -> Arc<MemoryBackend> {
    let acc = Id::from("acc1");
    let b = MemoryBackend::new().with_account(&acc);
    b.seed_object(
        &acc,
        "AddressBook",
        &Id::from("ab1"),
        json!({"id": "ab1", "name": "Friends", "description": null, "sortOrder": 0,
               "isDefault": false, "isSubscribed": true, "shareWith": null,
               "myRights": {"mayRead": true, "mayWrite": true, "mayShare": true, "mayDelete": true}}),
    );
    b.seed_object(
        &acc,
        "ContactCard",
        &Id::from("card1"),
        json!({"id": "card1", "@type": "Card", "version": "1.0", "uid": "card1-uid",
               "addressBookIds": {"ab1": true}, "name": {"@type": "Name", "full": "Alice"}}),
    );
    Arc::new(b)
}

fn dispatcher(b: &Arc<MemoryBackend>) -> Dispatcher<()> {
    let mut d = Dispatcher::new();
    register_contacts_handlers(&mut d, Arc::clone(b));
    register_contacts_conditional(&mut d, Arc::clone(b));
    d
}

async fn call(d: &Dispatcher<()>, method: &str, mut args: Value) -> (String, Value) {
    args["accountId"] = json!("acc1");
    let req: JmapRequest =
        serde_json::from_value(json!({"using": USING, "methodCalls": [[method, args, "0"]]}))
            .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

/// The ids `method` (a `/get`) returns, sorted, and the state string.
async fn ids(d: &Dispatcher<()>, method: &str) -> (Vec<String>, Value) {
    let (name, r) = call(d, method, json!({"properties": ["id"]})).await;
    assert_eq!(name, method, "{r}");
    let mut ids: Vec<String> = r["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    (ids, r["state"].clone())
}

#[tokio::test]
async fn precondition_on_an_address_book_name() {
    let b = backend();
    let d = dispatcher(&b);
    let rename_if = |expected: &str| json!({"ifUnchangedBy": {"ab1": {"name": expected}}, "update": {"ab1": {"name": "Family"}}});
    let (name, r) = call(&d, "AddressBook/set", rename_if("Work")).await;
    assert_eq!(
        (name.as_str(), r),
        ("error", json!({"type": "stateMismatch", "failed": ["ab1"]}))
    );
    let (name, r) = call(&d, "AddressBook/set", rename_if("Friends")).await;
    assert_eq!(name, "AddressBook/set", "{r}");
    assert!(r["updated"].get("ab1").is_some(), "{r}");
}

/// §4.2 with a cascade: the `onDestroyRemoveContents` destroy of ab1 and
/// its card roll back with the failing update, state strings included.
#[tokio::test]
async fn atomic_failure_rolls_back_a_destroy_cascade() {
    let b = backend();
    let d = dispatcher(&b);
    let books_before = ids(&d, "AddressBook/get").await;
    let cards_before = ids(&d, "ContactCard/get").await;
    assert_eq!(cards_before.0, ["card1"]);

    let (name, r) = call(
        &d,
        "AddressBook/set",
        json!({
            "atomic": true,
            "onDestroyRemoveContents": true,
            "destroy": ["ab1"],
            "update": {"nope": {"name": "x"}}
        }),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert_eq!(r["notUpdated"]["nope"]["type"], "notFound", "{r}");
    assert_eq!(ids(&d, "AddressBook/get").await, books_before);
    assert_eq!(ids(&d, "ContactCard/get").await, cards_before);

    let (name, r) = call(
        &d,
        "AddressBook/set",
        json!({"atomic": true, "onDestroyRemoveContents": true, "destroy": ["ab1"]}),
    )
    .await;
    assert_eq!(name, "AddressBook/set", "{r}");
    assert!(ids(&d, "ContactCard/get").await.0.is_empty());
}

#[test]
fn ready_after_handlers_and_conditional() {
    let b = backend();
    let mut d = Dispatcher::new();
    register_contacts_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps().len(), 2, "{:?}", d.conditional_gaps());
    register_contacts_conditional(&mut d, b);
    assert!(
        d.conditional_gaps().is_empty(),
        "{:?}",
        d.conditional_gaps()
    );
    assert!(d.conditional_ready());
}
