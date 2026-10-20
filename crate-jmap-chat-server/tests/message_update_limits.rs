//! `Message/set` update enforces `maxBodyBytes` and the encrypted-`bodyType`
//! empty-field rule, as create does (bd:JMAP-y3f2.11, JMAP-4dmj).
//!
//! Oracles: draft-atwood-jmap-chat-00 (jmap-chat-spec 91672c9), quoted per
//! test.

mod common;

use common::{seed_group_chat, MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_message_get, handle_message_set, ChatLimits};
use jmap_server::HandlerOutput;
use serde_json::{json, Value};

const CIPHER: &str = "application/mls-ciphertext";

async fn create_message(backend: &MemoryBackend, extra: Value) -> String {
    seed_group_chat(backend, "c1");
    let mut obj = json!({ "chatId": "c1", "body": "hi", "sentAt": "2024-01-01T00:00:00Z" });
    obj.as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("extra object").clone());
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": { "m0": obj } }),
    )
    .await
    .expect("create");
    resp["created"]["m0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("not created: {resp}"))
        .to_owned()
}

async fn update(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { id: patch } }),
    )
    .await
    .expect("update");
    resp
}

async fn get_body(backend: &MemoryBackend, id: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": ["body"] }),
    )
    .await
    .expect("get");
    resp["list"][0]["body"].clone()
}

fn assert_updated(resp: &Value, id: &str) {
    assert!(resp["updated"].get(id).is_some(), "not updated: {resp}");
}

/// L235-236 `maxBodyBytes`: "Maximum UTF-8 byte length of a Message
/// `body`. Servers MUST reject messages exceeding this limit; in
/// `Message/set` the rejection is a `tooLarge` SetError." An edit that
/// replaces `body` is held to the same byte limit; "é" is two bytes.
#[tokio::test]
async fn edit_body_over_max_body_bytes_is_too_large() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(ChatLimits::default().with_max_body_bytes(4)));
    let id = create_message(&backend, json!({})).await;

    let resp = update(&backend, &id, json!({ "body": "abcde" })).await;
    assert_eq!(resp["notUpdated"][&id]["type"], "tooLarge", "{resp}");
    let resp = update(&backend, &id, json!({ "body": "abc\u{e9}" })).await;
    assert_eq!(resp["notUpdated"][&id]["type"], "tooLarge", "{resp}");
    assert_eq!(get_body(&backend, &id).await, json!("hi"));

    let resp = update(&backend, &id, json!({ "body": "\u{e9}\u{e9}" })).await;
    assert_updated(&resp, &id);
    assert_eq!(get_body(&backend, &id).await, json!("\u{e9}\u{e9}"));
}

/// L1732: "Servers MUST reject a `Message/set` create or update that has
/// an encrypted `bodyType` and any of these fields non-empty with an
/// `invalidProperties` SetError whose `properties` lists each such field".
/// A patch switching `bodyType` to ciphertext over a stored `subject` and
/// `topicId` yields a Message with an encrypted `bodyType` and those fields
/// non-empty.
#[tokio::test]
async fn edit_to_encrypted_body_type_rejects_stored_fields() {
    let backend = MemoryBackend::new();
    let id = create_message(&backend, json!({ "subject": "s", "topicId": "t1" })).await;

    let resp = update(&backend, &id, json!({ "bodyType": CIPHER, "body": "x" })).await;
    assert_eq!(
        resp["notUpdated"][&id],
        json!({
            "type": "invalidProperties",
            "properties": ["topicId", "subject"],
            "description": format!("must be empty or absent when bodyType is {CIPHER}"),
        }),
        "{resp}"
    );
    assert_eq!(get_body(&backend, &id).await, json!("hi"));
}

/// L1732, update half: on a stored encrypted Message, an edit adding
/// `mentions` is rejected naming `mentions`; an edit of `body` alone is
/// accepted.
#[tokio::test]
async fn edit_of_encrypted_message_rejects_non_empty_mentions() {
    let backend = MemoryBackend::new();
    let id = create_message(&backend, json!({ "bodyType": CIPHER })).await;

    let resp = update(
        &backend,
        &id,
        json!({ "body": "x", "mentions": [{ "id": "u1" }] }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&id]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"][&id]["properties"],
        json!(["mentions"]),
        "{resp}"
    );

    let resp = update(
        &backend,
        &id,
        json!({ "body": "ciphertext2", "mentions": [] }),
    )
    .await;
    assert_updated(&resp, &id);
    assert_eq!(get_body(&backend, &id).await, json!("ciphertext2"));
}
