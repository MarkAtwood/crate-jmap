//! Mailbox/set updates keep the Mailbox tree valid (bd JMAP-h4ht).
//!
//! Oracle: RFC 8621 §2: Mailboxes "form acyclic graphs (forests) directed
//! by the child-to-parent relationship. There MUST NOT be a loop." and a
//! Mailbox must not share its name with a sibling, which the create path
//! answers with `alreadyExists` (RFC 8620 §5.3); a loop is
//! `invalidProperties` naming `parentId`.

use std::sync::Arc;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::register_mail_handlers;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

fn mailbox(id: &str, name: &str, parent: Option<&str>) -> Value {
    json!({
        "id": id, "name": name, "parentId": parent, "role": null,
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

/// Top level: `a` "Work" (child `a1` "Reports"), `b` "Home" (child `b1`
/// "Reports").
fn dispatcher() -> Dispatcher<()> {
    let acc = Id::from("u1");
    let b = MemoryBackend::<()>::default().with_account(&acc);
    for (id, name, parent) in [
        ("a", "Work", None),
        ("a1", "Reports", Some("a")),
        ("b", "Home", None),
        ("b1", "Reports", Some("b")),
    ] {
        b.seed_object(&acc, "Mailbox", &Id::from(id), mailbox(id, name, parent));
    }
    let mut d = Dispatcher::new();
    register_mail_handlers(&mut d, Arc::new(b));
    d
}

async fn update(d: &Dispatcher<()>, update: Value) -> Value {
    let req: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "methodCalls": [["Mailbox/set", {"accountId": "u1", "update": update}, "0"]]
    }))
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    resp["methodResponses"][0][1].clone()
}

fn loop_error() -> Value {
    json!({"type": "invalidProperties", "properties": ["parentId"]})
}

#[tokio::test]
async fn rename_onto_a_sibling_is_already_exists() {
    let d = dispatcher();
    let r = update(&d, json!({"a": {"name": "Home"}})).await;
    assert_eq!(r["notUpdated"]["a"]["type"], "alreadyExists", "{r}");
}

#[tokio::test]
async fn move_next_to_a_namesake_is_already_exists() {
    let d = dispatcher();
    let r = update(&d, json!({"a1": {"parentId": "b"}})).await;
    assert_eq!(r["notUpdated"]["a1"]["type"], "alreadyExists", "{r}");
    // Renaming in the same patch avoids the clash.
    let r = update(&d, json!({"a1": {"parentId": "b", "name": "Work reports"}})).await;
    assert!(r["updated"].get("a1").is_some(), "{r}");
}

#[tokio::test]
async fn a_loop_is_invalid_properties() {
    let d = dispatcher();
    assert_eq!(
        update(&d, json!({"a": {"parentId": "a"}})).await["notUpdated"]["a"],
        loop_error()
    );
    assert_eq!(
        update(&d, json!({"a": {"parentId": "a1"}})).await["notUpdated"]["a"],
        loop_error()
    );
}

/// Earlier changes of the same call count: after `a` moves under `b`,
/// moving `b` under `a` closes a loop.
#[tokio::test]
async fn earlier_changes_in_the_call_count() {
    let d = dispatcher();
    let r = update(&d, json!({"a": {"parentId": "b"}, "b": {"parentId": "a"}})).await;
    let updated = r["updated"].as_object().map_or(0, |m| m.len());
    let refused = r["notUpdated"].as_object().map_or(0, |m| m.len());
    assert_eq!((updated, refused), (1, 1), "exactly one of the two: {r}");
    let loser = r["notUpdated"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    assert_eq!(r["notUpdated"][&loser], loop_error());
}

#[tokio::test]
async fn harmless_renames_and_moves_pass() {
    let d = dispatcher();
    let r = update(
        &d,
        json!({"a": {"name": "Work"}, "b1": {"name": "Archive"}, "a1": {"parentId": null}}),
    )
    .await;
    assert!(r["notUpdated"].is_null(), "{r}");
}
