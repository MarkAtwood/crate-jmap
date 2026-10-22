//! Message/set rejects a malformed `broadcastMentions` entry with
//! `invalidProperties` naming `broadcastMentions` (bd:JMAP-yt1w.4(e)).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 §Input Validation:
//! "When a `Message/set` create or update fails one of these checks and
//! this document names no other error for that check, the server MUST
//! reject the entry with an `invalidProperties` SetError ([RFC8620] §5.3)
//! whose `properties` names the offending property." A missing `scope`
//! takes that default; an unrecognized one gets the same SetError by
//! §BroadcastMention: "in `Message/set` the rejection is an
//! `invalidProperties` SetError naming `broadcastMentions`." The draft
//! at jmap-chat-spec 0983787 removed the `offset + length` bounds check:
//! "Like Mention, a BroadcastMention carries no position in `body`."

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::handle_chat_set;
use jmap_chat_server::message::handle_message_set;
use jmap_server::HandlerOutput;
use serde_json::{json, Value};

async fn create_group(backend: &MemoryBackend) -> String {
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
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
    let HandlerOutput { primary: resp, .. } = handle_message_set(
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
async fn create_rejects_broadcast_mention_without_scope() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let resp = create(&backend, &chat_id, json!([{ "offset": 3, "length": 9 }])).await;
    assert_invalid_broadcast_mentions(&resp["notCreated"]["m"]);
}

#[tokio::test]
async fn update_rejects_broadcast_mention_without_scope() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let resp = create(&backend, &chat_id, json!([])).await;
    let msg_id = resp["created"]["m"]["id"]
        .as_str()
        .expect("created message id");
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { msg_id: {
                "broadcastMentions": [{}]
            } }
        }),
    )
    .await
    .expect("Message/set");
    assert_invalid_broadcast_mentions(&resp["notUpdated"][msg_id]);
}
