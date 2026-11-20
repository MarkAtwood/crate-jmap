//! `onSuccessSetIsDefault` changes what clients read (bd JMAP-4sgi).
//!
//! Oracle: draft-ietf-jmap-calendars-34 §4.3 (Calendar) and §3.3
//! (ParticipantIdentity): on success the server makes the given object the
//! default, so its `isDefault` is true and the previous default's is false;
//! RFC 8620 §5.3 / §5.2: a change to stored objects moves the state and is
//! reported by `/changes`.

use std::sync::Arc;

use jmap_calendars_server::memory::MemoryBackend;
use jmap_calendars_server::register_calendars_handlers;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

fn dispatcher() -> Dispatcher<()> {
    let b = Arc::new(MemoryBackend::<()>::default().with_account(&Id::from("acc1")));
    let mut d = Dispatcher::new();
    register_calendars_handlers(&mut d, b);
    d
}

async fn call(d: &Dispatcher<()>, method: &str, mut args: Value) -> Value {
    args["accountId"] = json!("acc1");
    let req: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:calendars"],
        "methodCalls": [[method, args, "0"]]
    }))
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    assert_eq!(inv[0], method, "{inv}");
    inv[1].clone()
}

/// The ids whose `isDefault` is true, and the state.
async fn defaults(d: &Dispatcher<()>, type_name: &str) -> (Vec<String>, Value) {
    let r = call(d, &format!("{type_name}/get"), json!({})).await;
    let ids = r["list"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["isDefault"] == true)
        .map(|o| o["id"].as_str().unwrap().to_owned())
        .collect();
    (ids, r["state"].clone())
}

async fn check_default_moves(type_name: &str, create: Value) {
    let d = dispatcher();
    let set = format!("{type_name}/set");
    let r = call(&d, &set, json!({"create": create})).await;
    let id = |k: &str| r["created"][k]["id"].as_str().expect("created").to_owned();
    let (a, b) = (id("a"), id("b"));

    let (none, before) = defaults(&d, type_name).await;
    assert!(none.is_empty());
    let r = call(&d, &set, json!({"onSuccessSetIsDefault": &a})).await;
    assert_eq!(r["updated"][&a], json!({"isDefault": true}), "{r}");
    assert_ne!(r["newState"], r["oldState"], "the state moves: {r}");
    assert_eq!(defaults(&d, type_name).await.0, std::slice::from_ref(&a));

    let r = call(&d, &set, json!({"onSuccessSetIsDefault": &b})).await;
    assert_eq!(r["updated"][&b], json!({"isDefault": true}), "{r}");
    assert_eq!(r["updated"][&a], json!({"isDefault": false}), "{r}");
    assert_eq!(defaults(&d, type_name).await.0, std::slice::from_ref(&b));

    let changes = call(
        &d,
        &format!("{type_name}/changes"),
        json!({"sinceState": before}),
    )
    .await;
    let mut updated: Vec<String> = serde_json::from_value(changes["updated"].clone()).unwrap();
    updated.sort();
    let mut both = vec![a, b];
    both.sort();
    assert_eq!(updated, both, "{changes}");
}

#[tokio::test]
async fn calendar_default_moves() {
    check_default_moves(
        "Calendar",
        json!({"a": {"name": "Work"}, "b": {"name": "Home"}}),
    )
    .await;
}

#[tokio::test]
async fn participant_identity_default_moves() {
    check_default_moves(
        "ParticipantIdentity",
        json!({
            "a": {"name": "Alice", "calendarAddress": "mailto:alice@example.com"},
            "b": {"name": "Alice at work", "calendarAddress": "mailto:alice@work.example"}
        }),
    )
    .await;
}
