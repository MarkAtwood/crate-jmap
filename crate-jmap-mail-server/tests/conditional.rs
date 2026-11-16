//! Conditional mail Foo/set (draft-ietf-jmap-conditional-02) through
//! `register_mail_conditional` and the memory backend's `ScopedState`
//! scopes (bd JMAP-y1yu.7; docs/design/conditional.md §5.1, §6, §7).
//!
//! Oracles: conditional-02 §3.1, §4.2 and example 5.2; RFC 8621 §2 (one
//! role per account, one name per parent, no parent loop); RFC 9661 (one
//! name per script) and the memory backend's per-account script cap.

use std::sync::Arc;

use jmap_mail_server::MailBackend;
use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{
    register_mail_conditional, register_mail_handlers, register_sieve_handlers,
};
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const MAIL: &str = "urn:ietf:params:jmap:mail";
const SIEVE: &str = "urn:ietf:params:jmap:sieve";
const CONDITIONAL: &str = "urn:ietf:params:jmap:conditional";
const USING: &[&str] = &[CORE, MAIL, SIEVE, CONDITIONAL];

fn u1() -> Id {
    Id::from("u1")
}

fn mailbox(id: &str, name: &str, parent: Option<&str>, role: Option<&str>) -> Value {
    json!({
        "id": id, "name": name, "parentId": parent, "role": role,
        "sortOrder": 0, "totalEmails": 0, "unreadEmails": 0,
        "totalThreads": 0, "unreadThreads": 0,
        "myRights": {
            "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
            "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
            "mayRename": true, "mayDelete": true, "maySubmit": true
        },
        "isSubscribed": true
    })
}

/// Account u1 with mailboxes m1 "Inbox" (role inbox) and m2 "Work".
fn backend() -> Arc<MemoryBackend> {
    let b = MemoryBackend::new().with_account(&u1());
    b.seed_object(
        &u1(),
        "Mailbox",
        &Id::from("m1"),
        mailbox("m1", "Inbox", None, Some("inbox")),
    );
    b.seed_object(
        &u1(),
        "Mailbox",
        &Id::from("m2"),
        mailbox("m2", "Work", None, None),
    );
    Arc::new(b)
}

fn dispatcher(b: &Arc<MemoryBackend>) -> Dispatcher<()> {
    let mut d = Dispatcher::new();
    register_mail_handlers(&mut d, Arc::clone(b));
    register_sieve_handlers(&mut d, Arc::clone(b));
    register_mail_conditional(&mut d, Arc::clone(b));
    d
}

/// Run one call; its response name and arguments.
async fn call(d: &Dispatcher<()>, call: Value) -> (String, Value) {
    let req: JmapRequest =
        serde_json::from_value(json!({"using": USING, "methodCalls": [call]})).unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

/// `type`'s objects in u1 (`ids: null`), and the state string.
async fn get(d: &Dispatcher<()>, type_name: &str) -> (Vec<Value>, Value) {
    let (name, r) = call(
        d,
        json!([format!("{type_name}/get"), {"accountId": "u1"}, "g"]),
    )
    .await;
    assert_eq!(name, format!("{type_name}/get"), "{r:?}");
    (r["list"].as_array().unwrap().clone(), r["state"].clone())
}

fn ids(list: &[Value]) -> Vec<&str> {
    let mut ids: Vec<&str> = list.iter().map(|o| o["id"].as_str().unwrap()).collect();
    ids.sort_unstable();
    ids
}

fn by_id<'a>(list: &'a [Value], id: &str) -> &'a Value {
    list.iter().find(|o| o["id"] == id).unwrap()
}

/// Import a message into m1; its id.
async fn import(b: &MemoryBackend, n: u32) -> String {
    let blob = Id::from(format!("blob{n}"));
    b.store_blob(
        &blob,
        format!("Message-ID: <m{n}@example.com>\r\nSubject: {n}\r\n\r\nbody\r\n").into_bytes(),
    );
    let (id, _) = b
        .import_email(&(), &u1(), &blob, &[Id::from("m1")], &[], None)
        .await
        .unwrap();
    id.as_ref().to_owned()
}

/// Example 5.2: destroy a message only while it is unread. The ids are
/// the backend's, not the example's "M7".
#[tokio::test]
async fn example_5_2_conditional_destroy() {
    let b = backend();
    let unread = import(&b, 7).await;
    let read = import(&b, 8).await;
    let d = dispatcher(&b);
    let (_, r) = call(
        &d,
        json!(["Email/set", {"accountId": "u1", "update": {&read: {"keywords/$seen": true}}}, "s"]),
    )
    .await;
    assert!(r["updated"].get(&read).is_some(), "{r:?}");
    let destroy_if_unread = |id: &str| {
        json!(["Email/set", {
            "accountId": "u1",
            "ifUnchangedBy": { id: { "keywords/$seen": null } },
            "destroy": [id]
        }, "0"])
    };

    let (name, r) = call(&d, destroy_if_unread(&unread)).await;
    assert_eq!(name, "Email/set", "{r:?}");
    assert_eq!(r["destroyed"], json!([unread]));

    // "If the message has since been marked "$seen", the precondition
    // fails, the whole method is rejected with a "stateMismatch" error,
    // and "M7" is not deleted."
    let (name, r) = call(&d, destroy_if_unread(&read)).await;
    assert_eq!(
        (name.as_str(), r),
        ("error", json!({"type": "stateMismatch", "failed": [read]}))
    );
    assert_eq!(ids(&get(&d, "Email").await.0), [read.as_str()]);
}

#[tokio::test]
async fn mailbox_name_precondition() {
    let b = backend();
    let d = dispatcher(&b);
    let rename_if = |expected: &str| {
        json!(["Mailbox/set", {
            "accountId": "u1",
            "ifUnchangedBy": {"m2": {"name": expected}},
            "update": {"m2": {"name": "Projects"}}
        }, "0"])
    };
    let (name, r) = call(&d, rename_if("Old name")).await;
    assert_eq!(
        (name.as_str(), r),
        ("error", json!({"type": "stateMismatch", "failed": ["m2"]}))
    );
    let (name, r) = call(&d, rename_if("Work")).await;
    assert_eq!(name, "Mailbox/set", "{r:?}");
    assert!(r["updated"].get("m2").is_some(), "{r:?}");
    assert_eq!(by_id(&get(&d, "Mailbox").await.0, "m2")["name"], "Projects");
}

/// §4.2: one failing change rejects the method and nothing it did stays,
/// state string included.
#[tokio::test]
async fn atomic_failure_rolls_back_mailbox_set() {
    let b = backend();
    let d = dispatcher(&b);
    let (before, state) = get(&d, "Mailbox").await;

    let (name, r) = call(
        &d,
        json!(["Mailbox/set", {
            "accountId": "u1",
            "atomic": true,
            "create": {"c1": {"name": "New"}},
            "update": {"m2": {"name": "Renamed"}, "nope": {"name": "X"}}
        }, "0"]),
    )
    .await;
    assert_eq!(name, "error", "{r:?}");
    assert_eq!(r["type"], "atomicFailure", "{r:?}");
    assert_eq!(r["notUpdated"]["nope"]["type"], "notFound", "{r:?}");
    assert!(
        r.get("notCreated").is_none(),
        "only the failing change: {r:?}"
    );

    let (after, after_state) = get(&d, "Mailbox").await;
    assert_eq!(after_state, state, "state string unchanged (§4.2)");
    assert_eq!(ids(&after), ids(&before), "c1 rolled back");
    assert_eq!(by_id(&after, "m2")["name"], "Work", "m2 rolled back");
}

/// Under `atomic` the role check runs on the final state: m2 takes
/// `inbox` while m1 moves to `archive` in the same call. Without
/// `atomic`, m2's claim is checked against the state before the call,
/// where m1 still holds `inbox`.
#[tokio::test]
async fn atomic_role_swap_checked_on_final_state() {
    let swap = |atomic: bool| {
        json!(["Mailbox/set", {
            "accountId": "u1",
            "atomic": atomic,
            "update": {"m1": {"role": "archive"}, "m2": {"role": "inbox"}}
        }, "0"])
    };

    let b = backend();
    let d = dispatcher(&b);
    let (name, r) = call(&d, swap(false)).await;
    assert_eq!(name, "Mailbox/set", "{r:?}");
    assert_eq!(
        r["notUpdated"]["m2"]["properties"],
        json!(["role"]),
        "{r:?}"
    );

    let b = backend();
    let d = dispatcher(&b);
    let (name, r) = call(&d, swap(true)).await;
    assert_eq!(name, "Mailbox/set", "{r:?}");
    let list = get(&d, "Mailbox").await.0;
    assert_eq!(by_id(&list, "m1")["role"], "archive");
    assert_eq!(by_id(&list, "m2")["role"], "inbox");
}

/// The final-state check catches what the skipped pre-checks would have:
/// a second `inbox`, and a parent loop.
#[tokio::test]
async fn atomic_final_state_violations_are_atomic_failures() {
    let b = backend();
    let d = dispatcher(&b);
    let (name, r) = call(
        &d,
        json!(["Mailbox/set", {
            "accountId": "u1",
            "atomic": true,
            "create": {"c1": {"name": "Second Inbox", "role": "inbox"}}
        }, "0"]),
    )
    .await;
    assert_eq!(
        (name.as_str(), r),
        (
            "error",
            json!({"type": "atomicFailure", "notCreated": {
                "c1": {"type": "invalidProperties", "properties": ["role"]}
            }})
        )
    );

    let (name, r) = call(
        &d,
        json!(["Mailbox/set", {
            "accountId": "u1",
            "atomic": true,
            "update": {"m1": {"parentId": "m2"}, "m2": {"parentId": "m1"}}
        }, "0"]),
    )
    .await;
    assert_eq!(name, "error", "{r:?}");
    assert_eq!(r["type"], "atomicFailure", "{r:?}");
    for id in ["m1", "m2"] {
        assert_eq!(
            r["notUpdated"][id]["properties"],
            json!(["parentId"]),
            "{r:?}"
        );
    }
    let list = get(&d, "Mailbox").await.0;
    assert_eq!(ids(&list), ["m1", "m2"]);
    assert!(list.iter().all(|m| m["parentId"].is_null()), "rolled back");
}

/// A per-account cap met only by the final state: with the cap at one
/// script, replacing the script in one call needs `atomic`.
#[tokio::test]
async fn atomic_sieve_quota_checked_on_final_state() {
    let b = backend();
    b.store_blob(&Id::from("script-blob"), b"keep;".to_vec());
    b.set_max_sieve_scripts_per_account(1);
    let d = dispatcher(&b);
    let (name, r) = call(
        &d,
        json!(["SieveScript/set", {"accountId": "u1", "create": {
            "s": {"name": "old", "blobId": "script-blob"}
        }}, "0"]),
    )
    .await;
    assert_eq!(name, "SieveScript/set", "{r:?}");
    let old = r["created"]["s"]["id"].as_str().unwrap().to_owned();

    let replace = |destroy: &[&str]| {
        json!(["SieveScript/set", {
            "accountId": "u1",
            "atomic": true,
            "create": {"n": {"name": "new", "blobId": "script-blob"}},
            "destroy": destroy
        }, "1"])
    };

    // With the count check skipped under atomic, the commit's check on
    // the final state (one script) lets the replacement through...
    let (name, r) = call(&d, replace(&[&old])).await;
    assert_eq!(name, "SieveScript/set", "{r:?}");
    assert_eq!(r["destroyed"], json!([old]));
    let list = get(&d, "SieveScript").await.0;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "new");

    // ...and rejects a create that leaves two.
    let (name, r) = call(
        &d,
        json!(["SieveScript/set", {
            "accountId": "u1",
            "atomic": true,
            "create": {"n": {"name": "third", "blobId": "script-blob"}}
        }, "3"]),
    )
    .await;
    assert_eq!(
        (name.as_str(), r),
        (
            "error",
            json!({"type": "atomicFailure", "notCreated": {"n": {"type": "overQuota"}}})
        )
    );
    assert_eq!(get(&d, "SieveScript").await.0.len(), 1);
}

#[test]
fn ready_after_handlers_and_conditional() {
    let b = backend();
    let mut d = Dispatcher::new();
    register_mail_handlers(&mut d, Arc::clone(&b));
    assert!(!d.conditional_ready());
    register_mail_conditional(&mut d, Arc::clone(&b));
    assert!(
        d.conditional_gaps().is_empty(),
        "{:?}",
        d.conditional_gaps()
    );
    assert!(d.conditional_ready());

    // Sieve registered later is a gap until the opt-in runs again.
    register_sieve_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps(), ["SieveScript/set"]);
    register_mail_conditional(&mut d, b);
    assert!(d.conditional_ready());
}

/// SieveScript/set runs creates before updates, so a create may take a
/// name an update in the same call gives up only under `atomic`, where
/// uniqueness is checked on the final state.
#[tokio::test]
async fn atomic_sieve_name_handover() {
    let b = backend();
    b.store_blob(&Id::from("script-blob"), b"keep;".to_vec());
    let d = dispatcher(&b);
    let (_, r) = call(
        &d,
        json!(["SieveScript/set", {"accountId": "u1", "create": {
            "s": {"name": "main", "blobId": "script-blob"}
        }}, "0"]),
    )
    .await;
    let old = r["created"]["s"]["id"].as_str().unwrap().to_owned();
    let handover = |atomic: bool| {
        json!(["SieveScript/set", {
            "accountId": "u1",
            "atomic": atomic,
            "create": {"n": {"name": "main", "blobId": "script-blob"}},
            "update": {&old: {"name": "main-old"}}
        }, "1"])
    };

    let (_, r) = call(&d, handover(false)).await;
    assert_eq!(r["notCreated"]["n"]["type"], "alreadyExists", "{r:?}");
    // The update did run: put the name back.
    let (_, r) = call(
        &d,
        json!(["SieveScript/set", {"accountId": "u1", "update": {&old: {"name": "main"}}}, "2"]),
    )
    .await;
    assert!(r["updated"].get(&old).is_some(), "{r:?}");

    let (name, r) = call(&d, handover(true)).await;
    assert_eq!(name, "SieveScript/set", "{r:?}");
    let mut names: Vec<_> = get(&d, "SieveScript")
        .await
        .0
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_owned())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["main", "main-old"]);
}
