//! `#creationId` references in chat `/set` Id-typed properties, through
//! the full dispatcher (bd:JMAP-1p88).
//!
//! Oracle: RFC 8620 §5.3 (rfc8620.txt:1983-1996): "Some records may hold
//! references to other records (foreign keys). That reference may be set
//! (via create or update) in the same request as the referenced record is
//! created. To do this, the client refers to the new record using its
//! creation id prefixed with a "#". [...] the server MUST keep a simple map
//! for the duration of the request of creation id to record id for each
//! newly created record, so it can substitute in the correct value if
//! necessary in later method calls. In the case of records with
//! references to the same type, the server MUST order the creates and
//! updates within a single method call so that creates happen before their
//! creation ids are referenced by another create/update/destroy in the same
//! call." draft-atwood-jmap-chat-00 §Chat/set: `memberIds` are
//! "ChatContact.ids"; §Message: `replyTo` is a Message id; §Chat:
//! `pinnedMessageIds` "Each id MUST be the id of a Message in this Chat".
//!
//! Each assertion compares the referencing property with the id the server
//! reported for the referenced record, or with a hand-written SetError
//! shape.

mod common;

use std::sync::Arc;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::register_chat_handlers;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

fn dispatcher() -> Dispatcher<()> {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    let mut dispatcher = Dispatcher::new();
    register_chat_handlers(&mut dispatcher, Arc::new(backend));
    dispatcher
}

async fn run(dispatcher: &Dispatcher<()>, method_calls: Value) -> JmapResponse {
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher.dispatch(request, (), State::from("s0")).await
}

fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

fn created(resp: &JmapResponse, i: usize, key: &str) -> Value {
    let entry = args(resp, i)["created"][key].clone();
    assert!(entry["id"].is_string(), "created {key}: {}", args(resp, i));
    entry
}

fn message(chat: &str, extra: Value) -> Value {
    let mut obj = json!({
        "chatId": chat,
        "body": "x",
        "bodyType": "text/plain",
        "sentAt": "2024-01-01T00:00:00Z"
    });
    for (k, v) in extra.as_object().expect("object") {
        obj[k] = v.clone();
    }
    obj
}

/// ChatContact/set, Chat/set naming the contact in `memberIds`, and
/// Message/set whose reply sorts before the message it replies to; then a
/// second request that creates a Message and pins it by creation id.
#[tokio::test]
async fn chat_and_message_references_resolve() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["ChatContact/set", { "accountId": ACCOUNT_ID, "create": {
                "bob": { "identity": "bob@example.net" }
            } }, "0"],
            ["Chat/set", { "accountId": ACCOUNT_ID, "create": {
                "g": { "kind": "group", "name": "G", "memberIds": ["#bob"] }
            } }, "1"],
            ["Message/set", { "accountId": ACCOUNT_ID, "create": {
                "a-reply": message("#g", json!({ "replyTo": "#z-first" })),
                "z-first": message("#g", json!({}))
            } }, "2"]
        ]),
    )
    .await;
    let bob = created(&resp, 0, "bob");
    let chat = created(&resp, 1, "g");
    let first = created(&resp, 2, "z-first");
    let reply = created(&resp, 2, "a-reply");
    assert_eq!(reply["chatId"], chat["id"], "{reply}");
    assert_eq!(first["chatId"], chat["id"], "{first}");
    assert_eq!(reply["replyTo"], first["id"], "{reply}");
    let chat_id = chat["id"].as_str().expect("chat id").to_owned();

    let resp = run(
        &d,
        json!([
            ["Message/set", { "accountId": ACCOUNT_ID, "create": {
                "p": message(&chat_id, json!({}))
            } }, "0"],
            ["Chat/set", { "accountId": ACCOUNT_ID, "update": {
                (chat_id.clone()): { "pinnedMessageIds": ["#p"] }
            } }, "1"],
            ["Chat/get", { "accountId": ACCOUNT_ID, "ids": [chat_id],
                "properties": ["members", "pinnedMessageIds"] }, "2"]
        ]),
    )
    .await;
    let pinned = created(&resp, 0, "p");
    let stored = &args(&resp, 2)["list"][0];
    assert_eq!(
        stored["pinnedMessageIds"],
        json!([pinned["id"]]),
        "{stored}"
    );
    let members = stored["members"].as_array().cloned().unwrap_or_default();
    assert!(
        members.iter().any(|m| m["id"] == bob["id"]),
        "members must include the resolved ChatContact id: {stored}"
    );
}

#[tokio::test]
async fn unknown_reference_is_invalid_properties() {
    let resp = run(
        &dispatcher(),
        json!([["Message/set", { "accountId": ACCOUNT_ID, "create": {
            "m": message("#nope", json!({}))
        } }, "0"]]),
    )
    .await;
    let err = &args(&resp, 0)["notCreated"]["m"];
    assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, 0));
    assert_eq!(err["properties"], json!(["chatId"]), "{err}");
}
