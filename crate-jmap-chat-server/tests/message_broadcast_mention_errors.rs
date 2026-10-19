//! Message/set rejects a malformed `broadcastMentions` entry with
//! `invalidProperties` naming `broadcastMentions` (bd:JMAP-yt1w.4(e)).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 §Input Validation:
//! "`broadcastMentions`: reject any entry whose `scope` is not one of
//! `\"everyone\"`, `\"here\"`, `\"admins\"`, and any entry where
//! `offset + length` exceeds body byte length." and "When a `Message/set`
//! create or update fails one of these checks and this document names no
//! other error for that check, the server MUST reject the entry with an
//! `invalidProperties` SetError ([RFC8620] §5.3) whose `properties` names
//! the offending property." The draft names `invalidArguments` only for an
//! unrecognized `scope` value (§BroadcastMention), so the bounds check and
//! a missing `scope` take the default.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_chat_set, handle_message_set};
use serde_json::{json, Value};

async fn create_group(backend: &MemoryBackend) -> String {
    let (resp, _) = handle_chat_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "g": { "kind": "group", "name": "G", "memberIds": ["contact-bob"] } }
        }),
    )
    .await
    .expect("Chat/set");
    resp["created"]["g"]["id"]
        .as_str()
        .expect("created group id")
        .to_owned()
}

async fn create(backend: &MemoryBackend, chat_id: &str, broadcast_mentions: Value) -> Value {
    let (resp, _) = handle_message_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m": {
                "chatId": chat_id,
                "body": "hi @everyone",
                "sentAt": "2024-01-01T00:00:00Z",
                "broadcastMentions": broadcast_mentions,
            } }
        }),
    )
    .await
    .expect("Message/set");
    resp
}

fn assert_invalid_broadcast_mentions(err: &Value) {
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["broadcastMentions"]), "{err}");
}

#[tokio::test]
async fn create_rejects_out_of_bounds_broadcast_mention() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let resp = create(
        &backend,
        &chat_id,
        json!([{ "scope": "everyone", "offset": 3, "length": 999 }]),
    )
    .await;
    assert_invalid_broadcast_mentions(&resp["notCreated"]["m"]);
}

#[tokio::test]
async fn create_rejects_overflowing_broadcast_mention() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let resp = create(
        &backend,
        &chat_id,
        json!([{ "scope": "everyone", "offset": u64::MAX, "length": 1 }]),
    )
    .await;
    assert_invalid_broadcast_mentions(&resp["notCreated"]["m"]);
}

#[tokio::test]
async fn create_rejects_broadcast_mention_without_scope() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let resp = create(&backend, &chat_id, json!([{ "offset": 3, "length": 9 }])).await;
    assert_invalid_broadcast_mentions(&resp["notCreated"]["m"]);
}

#[tokio::test]
async fn update_rejects_out_of_bounds_broadcast_mention() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let resp = create(&backend, &chat_id, json!([])).await;
    let msg_id = resp["created"]["m"]["id"]
        .as_str()
        .expect("created message id");
    let (resp, _) = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { msg_id: {
                "broadcastMentions": [{ "scope": "everyone", "offset": 3, "length": 999 }]
            } }
        }),
    )
    .await
    .expect("Message/set");
    assert_invalid_broadcast_mentions(&resp["notUpdated"][msg_id]);
}
