//! ReadPosition records are managed by the server (bd:JMAP-yt1w.1).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 §ReadPosition: "For
//! direct and group Chats, a ReadPosition is created when the first
//! message is delivered." and §ReadPosition/set: "`create` and `destroy`
//! are not supported; both MUST return a SetError of type `forbidden`.
//! ReadPosition records are managed by the server."

mod common;

use common::{InjectableBackend, MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{
    handle_chat_set, handle_message_set, handle_position_get, handle_position_set, ChatBackend,
};
use serde_json::{json, Value};

async fn create_group<B: ChatBackend<CallerCtx = ()>>(backend: &B) -> String {
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

async fn send<B: ChatBackend<CallerCtx = ()>>(backend: &B, chat_id: &str, key: &str) {
    let (resp, _) = handle_message_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { key: { "chatId": chat_id, "body": "hi", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("Message/set");
    assert!(resp["created"][key].is_object(), "{resp}");
}

async fn positions<B: ChatBackend<CallerCtx = ()>>(backend: &B) -> Vec<Value> {
    let (resp, _) = handle_position_get(backend, &(), json!({ "accountId": ACCOUNT_ID }))
        .await
        .expect("ReadPosition/get");
    resp["list"].as_array().expect("list").clone()
}

/// "For direct and group Chats, a ReadPosition is created when the first
/// message is delivered." Later deliveries reuse that one record.
#[tokio::test]
async fn first_message_in_group_creates_one_read_position() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    assert!(positions(&backend).await.is_empty());

    send(&backend, &chat_id, "m1").await;
    let list = positions(&backend).await;
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["chatId"], chat_id.as_str());
    assert!(list[0].get("lastReadMessageId").is_none(), "{list:?}");

    send(&backend, &chat_id, "m2").await;
    assert_eq!(positions(&backend).await.len(), 1);
}

/// The ReadPosition is a side effect of delivering a Message that is
/// already stored, so a ReadPosition failure must not fail the method.
/// RFC 8620 §3.6.2: "the externally visible state of the server MUST NOT
/// have changed if an error is returned at the method level". The next
/// delivery creates the missing record ("a ReadPosition is created when
/// the first message is delivered").
#[tokio::test]
async fn read_position_create_failure_does_not_fail_message_set() {
    let backend = InjectableBackend::new();
    let chat_id = create_group(&backend).await;

    backend.inject("ReadPosition", "create");
    send(&backend, &chat_id, "m1").await;
    assert!(positions(&backend).await.is_empty());

    send(&backend, &chat_id, "m2").await;
    let list = positions(&backend).await;
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["chatId"], chat_id.as_str());
}

/// "`create` and `destroy` are not supported; both MUST return a SetError
/// of type `forbidden`."
#[tokio::test]
async fn read_position_set_create_and_destroy_forbidden() {
    let backend = MemoryBackend::new();
    let rp_id = "rp1".to_owned();
    backend.insert_object_for_test(
        "ReadPosition",
        ACCOUNT_ID,
        &rp_id,
        json!({ "id": rp_id, "chatId": "c1" }),
    );

    let (resp, _) = handle_position_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "rp0": { "chatId": "c-other" } },
            "destroy": [rp_id]
        }),
    )
    .await
    .expect("ReadPosition/set");

    assert_eq!(resp["notCreated"]["rp0"]["type"], "forbidden", "{resp}");
    assert_eq!(resp["notDestroyed"][&rp_id]["type"], "forbidden", "{resp}");
    let list = positions(&backend).await;
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["id"], rp_id.as_str());
}
