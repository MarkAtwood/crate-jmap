//! Message/set create naming a Chat that does not exist is rejected
//! (bd:JMAP-5mz6.8).
//!
//! Oracle: draft-atwood-jmap-chat-00 §Input Validation: "`chatId`: verify
//! per {{chat-id}} — confirm match with stored id or verify membership;
//! reject mismatches." and "When a `Message/set` create or update fails
//! one of these checks and this document names no other error for that
//! check, the server MUST reject the entry with an `invalidProperties`
//! SetError ([RFC8620] §5.3) whose `properties` names the offending
//! property."

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::handle_chat_set;
use jmap_chat_server::message::{handle_message_get, handle_message_set};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

async fn create_message(backend: &MemoryBackend, chat_id: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m": {
                "chatId": chat_id,
                "body": "hi",
                "bodyType": "text/plain",
                "sentAt": "2024-01-01T00:00:00Z"
            } }
        }),
    )
    .await
    .expect("Message/set");
    resp
}

async fn message_count(backend: &MemoryBackend) -> usize {
    let HandlerOutput { primary: resp, .. } = handle_message_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": null }),
    )
    .await
    .expect("Message/get");
    resp["list"].as_array().expect("list array").len()
}

#[tokio::test]
async fn create_in_unknown_chat_is_invalid_properties_chat_id() {
    let backend = MemoryBackend::new();
    let resp = create_message(&backend, "nope").await;

    assert!(resp.get("created").is_none_or(Value::is_null), "{resp}");
    let err = &resp["notCreated"]["m"];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(err["properties"], json!(["chatId"]), "{resp}");
    assert_eq!(message_count(&backend).await, 0, "nothing stored");
}

#[tokio::test]
async fn create_in_existing_chat_still_succeeds() {
    let backend = MemoryBackend::new();
    let HandlerOutput { primary: chat, .. } = handle_chat_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "g": { "kind": "group", "name": "G", "memberIds": ["contact-bob"] } }
        }),
    )
    .await
    .expect("Chat/set");
    let chat_id = chat["created"]["g"]["id"].as_str().expect("chat id");

    let resp = create_message(&backend, chat_id).await;
    assert!(resp["created"]["m"]["id"].is_string(), "{resp}");
    assert_eq!(message_count(&backend).await, 1);
}
