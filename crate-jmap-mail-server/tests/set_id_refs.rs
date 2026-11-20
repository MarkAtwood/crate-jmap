//! `#creationId` references as mail `/set` update keys and destroy ids,
//! through the full dispatcher (bd:JMAP-tna1.14).
//!
//! Oracle: RFC 8620 §5.3 (rfc8620.txt:1983-1996): "the server MUST keep a
//! simple map for the duration of the request of creation id to record id
//! for each newly created record, so it can substitute in the correct value
//! if necessary in later method calls. In the case of records with
//! references to the same type, the server MUST order the creates and
//! updates within a single method call so that creates happen before their
//! creation ids are referenced by another create/update/destroy in the same
//! call." A destroy carries only ids, so a destroy (and likewise an update)
//! references a creation by its creation id as the key or entry.
//!
//! A resolved entry is reported under the real id, which is what
//! `updated`/`destroyed` are typed as (`Id[...]`, `Id[]`). An unknown
//! reference keeps the error any `#` id got before: `invalidProperties`
//! naming `id`.

#![allow(async_fn_in_trait)]

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_mail_server::register_mail_handlers;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "account1";

async fn dispatch(method_calls: Value) -> JmapResponse {
    let backend = MemoryBackend::<()>::default();
    backend.register_account(&Id::from(ACCOUNT));
    backend.store_blob(
        &Id::from("msg"),
        b"Subject: Hi\r\nFrom: alice@example.com\r\n\r\nBody.".to_vec(),
    );
    backend.store_blob(&Id::from("script"), b"keep;".to_vec());
    let mut dispatcher = Dispatcher::new();
    #[cfg(feature = "sieve")]
    jmap_mail_server::register_sieve_handlers(&mut dispatcher, Arc::new(backend.clone()));
    register_mail_handlers(&mut dispatcher, Arc::new(backend));
    let request: JmapRequest = serde_json::from_value(json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:mail",
            "urn:ietf:params:jmap:submission",
            "urn:ietf:params:jmap:sieve"
        ],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher.dispatch(request, (), State::from("s0")).await
}

fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

fn created_id(resp: &JmapResponse, i: usize, key: &str) -> String {
    args(resp, i)["created"][key]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created {key}: {}", args(resp, i)))
        .to_owned()
}

/// `updated` names `id`, `destroyed` lists `gone`, and nothing failed.
fn assert_applied(set: &Value, id: &str, gone: &str) {
    assert!(set["updated"].get(id).is_some(), "updated {id}: {set}");
    assert_eq!(set["destroyed"], json!([gone]), "{set}");
    assert!(set["notUpdated"].is_null(), "{set}");
    assert!(set["notDestroyed"].is_null(), "{set}");
}

#[tokio::test]
async fn mailbox_update_and_destroy_name_earlier_and_same_call_creations() {
    let resp = dispatch(json!([
        ["Mailbox/set", {"accountId": ACCOUNT, "create": {
            "a": {"name": "A"}, "b": {"name": "B"}
        }}, "0"],
        ["Mailbox/set", {"accountId": ACCOUNT,
            "create": {"c": {"name": "C"}},
            "update": {"#a": {"name": "A2"}, "#c": {"name": "C2"}},
            "destroy": ["#b"]
        }, "1"],
        ["Mailbox/get", {"accountId": ACCOUNT, "ids": null, "properties": ["name"]}, "2"]
    ]))
    .await;
    let (a, b, c) = (
        created_id(&resp, 0, "a"),
        created_id(&resp, 0, "b"),
        created_id(&resp, 1, "c"),
    );
    let set = args(&resp, 1);
    assert_applied(set, &a, &b);
    assert!(set["updated"].get(&c).is_some(), "{set}");
    let mut names: Vec<_> = args(&resp, 2)["list"]
        .as_array()
        .expect("list")
        .iter()
        .map(|m| m["name"].as_str().expect("name").to_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["A2", "C2"]);
}

#[tokio::test]
async fn email_update_and_destroy_name_imported_emails() {
    let resp = dispatch(json!([
        ["Mailbox/set", {"accountId": ACCOUNT, "create": {"m": {"name": "In"}}}, "0"],
        ["Email/import", {"accountId": ACCOUNT, "emails": {
            "e": {"blobId": "msg", "mailboxIds": {"#m": true}},
            "f": {"blobId": "msg", "mailboxIds": {"#m": true}}
        }}, "1"],
        ["Email/set", {"accountId": ACCOUNT,
            "update": {"#e": {"keywords/$seen": true}},
            "destroy": ["#f"]
        }, "2"]
    ]))
    .await;
    let (e, f) = (created_id(&resp, 1, "e"), created_id(&resp, 1, "f"));
    assert_applied(args(&resp, 2), &e, &f);
}

#[tokio::test]
async fn identity_update_and_destroy_name_earlier_creations() {
    let resp = dispatch(json!([
        ["Identity/set", {"accountId": ACCOUNT, "create": {
            "i": {"name": "Alice", "email": "alice@example.com"},
            "j": {"name": "Alt", "email": "alice@example.com"}
        }}, "0"],
        ["Identity/set", {"accountId": ACCOUNT,
            "update": {"#i": {"name": "Alice A."}},
            "destroy": ["#j"]
        }, "1"]
    ]))
    .await;
    let (i, j) = (created_id(&resp, 0, "i"), created_id(&resp, 0, "j"));
    assert_applied(args(&resp, 1), &i, &j);
}

#[cfg(feature = "sieve")]
#[tokio::test]
async fn sieve_update_and_destroy_name_earlier_creations() {
    let resp = dispatch(json!([
        ["SieveScript/set", {"accountId": ACCOUNT, "create": {
            "s": {"name": "one", "blobId": "script"},
            "t": {"name": "two", "blobId": "script"}
        }}, "0"],
        ["SieveScript/set", {"accountId": ACCOUNT,
            "update": {"#s": {"name": "uno"}},
            "destroy": ["#t"]
        }, "1"]
    ]))
    .await;
    let (s, t) = (created_id(&resp, 0, "s"), created_id(&resp, 0, "t"));
    assert_applied(args(&resp, 1), &s, &t);
}

/// An unknown reference is not an Id: `invalidProperties` naming `id`, as
/// before, keyed as sent.
#[tokio::test]
async fn unknown_reference_keeps_todays_error() {
    let resp = dispatch(json!([
        ["Mailbox/set", {"accountId": ACCOUNT,
            "update": {"#nope": {"name": "X"}},
            "destroy": ["#gone"]
        }, "0"]
    ]))
    .await;
    let set = args(&resp, 0);
    for err in [&set["notUpdated"]["#nope"], &set["notDestroyed"]["#gone"]] {
        assert_eq!(err["type"], "invalidProperties", "{set}");
        assert_eq!(err["properties"], json!(["id"]), "{set}");
    }
}

/// Email/set keeps no creation map of its own: an update and a destroy of
/// drafts created by the same call resolve after its create phase.
#[tokio::test]
async fn email_update_and_destroy_name_same_call_creations() {
    let draft = |subject: &str| {
        json!({"mailboxIds": {"#m": true}, "keywords": {"$draft": true},
               "from": [{"email": "alice@example.com"}], "subject": subject})
    };
    let resp = dispatch(json!([
        ["Mailbox/set", {"accountId": ACCOUNT, "create": {"m": {"name": "Drafts"}}}, "0"],
        ["Email/set", {"accountId": ACCOUNT,
            "create": {"d": draft("one"), "x": draft("two")},
            "update": {"#d": {"keywords/$seen": true}},
            "destroy": ["#x"]
        }, "1"]
    ]))
    .await;
    let (d, x) = (created_id(&resp, 1, "d"), created_id(&resp, 1, "x"));
    assert_applied(args(&resp, 1), &d, &x);
}

#[tokio::test]
async fn submission_update_and_destroy_name_earlier_creations() {
    let submission = json!({
        "identityId": "#i", "emailId": "#e",
        "envelope": {"mailFrom": {"email": "alice@example.com"},
                     "rcptTo": [{"email": "bob@example.com"}]}
    });
    let resp = dispatch(json!([
        ["Identity/set", {"accountId": ACCOUNT, "create": {
            "i": {"name": "Alice", "email": "alice@example.com"}
        }}, "0"],
        ["Mailbox/set", {"accountId": ACCOUNT, "create": {"m": {"name": "Out"}}}, "1"],
        ["Email/import", {"accountId": ACCOUNT, "emails": {
            "e": {"blobId": "msg", "mailboxIds": {"#m": true}}
        }}, "2"],
        ["EmailSubmission/set", {"accountId": ACCOUNT, "create": {
            "s": submission.clone(), "t": submission
        }}, "3"],
        ["EmailSubmission/set", {"accountId": ACCOUNT,
            "update": {"#s": {"undoStatus": "canceled"}},
            "destroy": ["#t"]
        }, "4"]
    ]))
    .await;
    let (s, t) = (created_id(&resp, 3, "s"), created_id(&resp, 3, "t"));
    // The memory backend sends at once, so the cancel reaches the stored
    // submission and is refused (RFC 8621 §7.5 `cannotUnsend`), under its
    // real id.
    let set = args(&resp, 4);
    assert_eq!(set["notUpdated"][&s]["type"], "cannotUnsend", "{set}");
    assert_eq!(set["destroyed"], json!([t]), "{set}");
}
