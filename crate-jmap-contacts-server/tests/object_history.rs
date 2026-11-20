//! Object history for AddressBook and ContactCard through the dispatcher
//! and the memory backend (bd JMAP-tna1.3.9).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §3.2.1 (the ContactCard
//! example, verbatim but for `state`), §7.3 (`objectHistory` is reserved
//! in JSContact Card, so a create naming it is `invalidProperties`), §5.2
//! (a reference to a missing container fails as an update would; the
//! container is restored "in an earlier method call in the same request",
//! -04), §3 (an `onDestroyRemoveContents` cascade counts as destroyed),
//! RFC 9610 §2 (one default AddressBook), and the -04 editor's copy §5.1
//! (Foo/changes compares sinceState with now).

use std::sync::Arc;
use std::time::Duration;

use jmap_contacts_server::memory::MemoryBackend;
use jmap_contacts_server::{register_contacts_handlers, register_contacts_handlers_with_history};
use jmap_object_history_server::HistoryRetention;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const USING: [&str; 3] = [
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:contacts",
    "urn:ietf:params:jmap:object-history",
];
const A: &str = "abc";
/// 2026-02-20T14:15:00Z and 2026-03-01T09:00:00Z, the §3.2.1 times.
const T1: i64 = 1_771_596_900_000;
const T2: i64 = 1_772_355_600_000;

fn backend() -> MemoryBackend<()> {
    let b = MemoryBackend::<()>::default()
        .with_account(&Id::from(A))
        .with_object_history(
            serde_json::from_value(json!({
                "maxHistoryDuration": 2_592_000,
                "supportedTypes": ["AddressBook", "ContactCard"]
            }))
            .unwrap(),
            HistoryRetention::new().max_duration(Some(Duration::from_secs(2_592_000))),
        );
    b.set_history_clock_for_test(T1 - 60_000);
    b
}

struct Server {
    b: MemoryBackend<()>,
    d: Dispatcher<()>,
}

impl Server {
    fn new() -> Self {
        let b = backend();
        let mut d = Dispatcher::new();
        register_contacts_handlers_with_history(&mut d, Arc::new(b.clone()));
        Self { b, d }
    }

    async fn run(&self, calls: Value) -> Vec<Value> {
        let req: JmapRequest =
            serde_json::from_value(json!({"using": USING, "methodCalls": calls})).unwrap();
        let resp = serde_json::to_value(self.d.dispatch(req, (), State::from("s")).await).unwrap();
        resp["methodResponses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                assert_ne!(r[0], "error", "{r}");
                r[1].clone()
            })
            .collect()
    }

    async fn one(&self, method: &str, mut args: Value) -> Value {
        args["accountId"] = json!(A);
        self.run(json!([[method, args, "0"]])).await.remove(0)
    }

    async fn book(&self, name: &str) -> String {
        let out = self
            .one("AddressBook/set", json!({"create": {"b": {"name": name}}}))
            .await;
        out["created"]["b"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{out}"))
            .to_owned()
    }

    async fn card(&self, book: &str, full: &str) -> String {
        let out = self
            .one(
                "ContactCard/set",
                json!({"create": {"c": {"@type": "Card", "version": "1.0",
                    "addressBookIds": {book: true}, "name": {"full": full}}}}),
            )
            .await;
        out["created"]["c"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{out}"))
            .to_owned()
    }
}

/// §3.2.1, verbatim but for `state`.
#[tokio::test]
async fn example_3_2_1_contact_history() {
    let s = Server::new();
    let acc = Id::from(A);
    s.b.seed_object(
        &acc,
        "AddressBook",
        &Id::from("ab1"),
        json!({"id": "ab1", "name": "Friends", "description": null, "sortOrder": 0,
               "isDefault": false, "isSubscribed": true, "shareWith": null,
               "myRights": {"mayRead": true, "mayWrite": true, "mayShare": true, "mayDelete": true}}),
    );
    s.b.seed_object(
        &acc,
        "ContactCard",
        &Id::from("contact1"),
        json!({"id": "contact1", "@type": "Card", "version": "1.0", "uid": "u1",
               "addressBookIds": {"ab1": true}, "name": {"full": "Robert Smith"},
               "emails": {"e1": {"address": "bob@example.com"}}}),
    );
    s.b.set_history_clock_for_test(T1);
    s.one(
        "ContactCard/set",
        json!({"update": {"contact1": {"name/full": "Bob Smith"}}}),
    )
    .await;
    s.b.set_history_clock_for_test(T2);
    s.one(
        "ContactCard/set",
        json!({"update": {"contact1": {"emails/e2": {"address": "bob@personal.example"}}}}),
    )
    .await;
    let mut got = s
        .one(
            "ContactCard/get",
            json!({"ids": ["contact1"], "properties": ["name", "emails"],
                   "includeReplaced": true, "historyLimit": 10}),
        )
        .await;
    got.as_object_mut().unwrap().remove("state");
    assert_eq!(
        got,
        json!({
          "accountId": "abc",
          "list": [
            {"id": "contact1", "name": {"full": "Robert Smith"},
             "emails": {"e1": {"address": "bob@example.com"}},
             "objectHistory": {"version": 1, "replaced": "2026-02-20T14:15:00Z",
                               "hasOlder": false, "expires": "2026-03-22T14:15:00Z"}},
            {"id": "contact1", "name": {"full": "Bob Smith"},
             "emails": {"e1": {"address": "bob@example.com"}},
             "objectHistory": {"version": 2, "replaced": "2026-03-01T09:00:00Z",
                               "hasOlder": true, "expires": "2026-03-31T09:00:00Z"}},
            {"id": "contact1", "name": {"full": "Bob Smith"},
             "emails": {"e1": {"address": "bob@example.com"},
                        "e2": {"address": "bob@personal.example"}},
             "objectHistory": {"version": 3, "replaced": null,
                               "hasOlder": true, "expires": null}}
          ],
          "notFound": []
        })
    );
}

/// §7.3: `objectHistory` is not a stored ContactCard property.
#[tokio::test]
async fn object_history_in_a_create_is_invalid_properties() {
    let s = Server::new();
    let book = s.book("B").await;
    let out = s
        .one(
            "ContactCard/set",
            json!({"create": {"c": {"@type": "Card", "version": "1.0",
                "addressBookIds": {&book: true}, "objectHistory": {"version": 1}}}}),
        )
        .await;
    assert_eq!(out["notCreated"]["c"]["type"], "invalidProperties");
    assert_eq!(
        out["notCreated"]["c"]["properties"],
        json!(["objectHistory"])
    );
}

/// §3, §5.2, -04 §5.1: a cascade destroy is history; the card's restore
/// fails while its AddressBook is destroyed and succeeds once an earlier
/// call in the same request restores it; /changes then reports both as
/// updated.
#[tokio::test]
async fn cascade_then_restore_book_and_card() {
    let s = Server::new();
    let book = s.book("Family").await;
    let card = s.card(&book, "Ann").await;
    let since_book = s.one("AddressBook/get", json!({"ids": []})).await["state"].clone();
    let since_card = s.one("ContactCard/get", json!({"ids": []})).await["state"].clone();
    s.b.set_history_clock_for_test(T1);
    let out = s
        .one(
            "AddressBook/set",
            json!({"destroy": [&book], "onDestroyRemoveContents": true}),
        )
        .await;
    assert_eq!(out["destroyed"], json!([&book]), "{out}");

    let q = s
        .one("ContactCard/query", json!({"onlyDestroyed": true}))
        .await;
    assert_eq!(q["ids"], json!([&card]));
    let out = s
        .one("ContactCard/set", json!({"restore": {&card: null}}))
        .await;
    assert_eq!(
        out["notRestored"][&card]["properties"],
        json!(["addressBookIds"])
    );

    let r = s
        .run(json!([
            ["AddressBook/set", {"accountId": A, "restore": {&book: null}}, "0"],
            ["ContactCard/set", {"accountId": A, "restore": {&card: null}}, "1"],
            ["ContactCard/get", {"accountId": A, "ids": [&card], "properties": ["name"]}, "2"]
        ]))
        .await;
    assert_eq!(r[0]["restored"], json!({&book: null}), "{}", r[0]);
    assert_eq!(r[1]["restored"], json!({&card: null}), "{}", r[1]);
    assert_eq!(r[2]["list"][0]["name"]["full"], "Ann");

    let ch = s
        .one("AddressBook/changes", json!({"sinceState": since_book}))
        .await;
    assert_eq!(
        (&ch["updated"], &ch["destroyed"]),
        (&json!([&book]), &json!([])),
        "{ch}"
    );
    let ch = s
        .one("ContactCard/changes", json!({"sinceState": since_card}))
        .await;
    assert_eq!(
        (&ch["updated"], &ch["destroyed"]),
        (&json!([&card]), &json!([])),
        "{ch}"
    );
}

/// RFC 9610 §2: a restored default AddressBook gives way to the current
/// default, and `restored` says so.
#[tokio::test]
async fn restored_default_book_is_not_default() {
    let s = Server::new();
    let old = s.book("Old").await;
    s.one("AddressBook/set", json!({"onSuccessSetIsDefault": &old}))
        .await;
    s.one("AddressBook/set", json!({"destroy": [&old]})).await;
    let new = s.book("New").await;
    s.one("AddressBook/set", json!({"onSuccessSetIsDefault": &new}))
        .await;
    let out = s
        .one("AddressBook/set", json!({"restore": {&old: null}}))
        .await;
    assert_eq!(out["restored"][&old], json!({"isDefault": false}), "{out}");
}

#[test]
fn registration_has_no_gaps() {
    let mut d = Dispatcher::new();
    register_contacts_handlers_with_history(&mut d, Arc::new(backend()));
    assert!(
        d.object_history_gaps(&["AddressBook", "ContactCard"])
            .is_empty()
    );
    let mut d = Dispatcher::new();
    register_contacts_handlers(&mut d, Arc::new(backend()));
    assert_eq!(
        d.object_history_gaps(&["AddressBook"]),
        ["AddressBook/get", "AddressBook/set"]
    );
}
