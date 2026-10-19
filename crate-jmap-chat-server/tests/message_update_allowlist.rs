//! `Message/set` update admits only the properties draft-atwood-jmap-chat-00
//! §Message/set names (bd:JMAP-x2gd.14).
//!
//! §Message/set defines four client updates: Editing ("`update` with
//! changed `body`, `bodyType`, `mentions`, and/or `broadcastMentions`"),
//! Adding and Removing Reactions ("standard JSON Pointer patch keys on the
//! `reactions` map", `"reactions/<senderReactionId>"`), Deleting ("`update`
//! with `deletedAt: <timestamp>`", optionally `deletedForAll: true`) and
//! Marking as Read ("`update` with `readAt: <UTCDate>` and optionally
//! `readDisposition: <String>`"). RFC 8620 §5.3 `invalidProperties`: "The
//! record given is invalid in some way."

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_message_get, handle_message_set, ChatBackend};
use jmap_chat_types::Message;
use jmap_types::{Id, PatchObject};
use serde_json::{json, Value};

async fn create_message(backend: &MemoryBackend) -> String {
    let (resp, _) = handle_message_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
    )
    .await
    .expect("create");
    resp["created"]["m0"]["id"]
        .as_str()
        .expect("created id")
        .to_owned()
}

async fn update(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let (resp, _) = handle_message_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { id: patch } }),
    )
    .await
    .expect("update");
    resp
}

async fn get(backend: &MemoryBackend, id: &str) -> Value {
    let (resp, _) = handle_message_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
    )
    .await
    .expect("get");
    resp["list"][0].clone()
}

/// Oracle: draft-atwood-jmap-chat-00 §Message Object Fields marks these
/// properties "server-set" or "immutable" (`id`, `senderMsgId`, `chatId`,
/// `senderId`, `replyCount`, `unreadReplyCount`, `topicId`, `subject`,
/// `receivedAt`, `senderExpiresAt`, `burnOnRead`,
/// `expiresAfterReadSeconds`, `deliveryState`, `deliveryReceipts`,
/// `deliveredAt`, `editedAt`, `editHistory`), and §Message/set lists
/// `sentAt`, `attachments`, `actions`, `replyTo` and `threadRootId` only
/// under "Creating a Message". Top-level `reactions` is not a §Message/set
/// update: reactions are "mutated via standard JSON Pointer patch keys on
/// the `reactions` map". Each patch is rejected with `invalidProperties`
/// naming that property, and the stored Message is unchanged.
#[tokio::test]
async fn non_updatable_properties_are_invalid_properties() {
    let cases = [
        ("id", json!("m9")),
        ("senderMsgId", json!("m9")),
        ("chatId", json!("c2")),
        ("senderId", json!("u9")),
        ("replyCount", json!(5)),
        ("unreadReplyCount", json!(5)),
        ("topicId", json!("t1")),
        ("subject", json!("s")),
        ("sentAt", json!("2024-02-01T00:00:00Z")),
        ("receivedAt", json!("2024-02-01T00:00:00Z")),
        ("senderExpiresAt", json!("2099-01-01T00:00:00Z")),
        ("burnOnRead", json!(true)),
        ("expiresAfterReadSeconds", json!(60)),
        ("deliveryState", json!("failed")),
        (
            "deliveryReceipts",
            json!({ "u1": { "deliveredAt": "2024-01-01T00:00:01Z" } }),
        ),
        ("deliveredAt", json!("2024-01-01T00:00:01Z")),
        ("editedAt", json!("2024-01-01T00:00:01Z")),
        (
            "editHistory",
            json!([{ "body": "x", "bodyType": "text/plain", "editedAt": "2024-01-01T00:00:01Z" }]),
        ),
        ("attachments", json!([])),
        ("actions", json!([])),
        ("replyTo", json!("m2")),
        ("threadRootId", json!("m2")),
        ("reactions", json!({})),
    ];
    for (prop, value) in cases {
        let backend = MemoryBackend::new();
        let id = create_message(&backend).await;
        let before = get(&backend, &id).await;

        let resp = update(&backend, &id, json!({ prop: value })).await;

        assert_eq!(
            resp["notUpdated"][&id],
            json!({ "type": "invalidProperties", "properties": [prop] }),
            "{prop}: {resp:?}"
        );
        assert_eq!(get(&backend, &id).await, before, "{prop}");
    }
}

/// Oracle: §Message/set Editing permits `body`; §Message Object Fields
/// marks "`editedAt` (UTCDate, optional, server-set)". A patch combining
/// the two is rejected as a whole, naming only `editedAt`, and `body` is
/// not applied.
#[tokio::test]
async fn allowed_key_does_not_carry_server_set_key() {
    let backend = MemoryBackend::new();
    let id = create_message(&backend).await;
    let before = get(&backend, &id).await;

    let resp = update(
        &backend,
        &id,
        json!({ "body": "edited", "editedAt": "2000-01-01T00:00:00Z" }),
    )
    .await;

    assert_eq!(
        resp["notUpdated"][&id],
        json!({ "type": "invalidProperties", "properties": ["editedAt"] }),
        "{resp:?}"
    );
    assert_eq!(get(&backend, &id).await, before);
}

/// Oracle: §Adding and Removing Reactions: "Servers MUST reject attempts
/// to add or remove reactions for other senders". A top-level `reactions`
/// replacement would remove another sender's reaction, so it is rejected
/// and that reaction survives.
#[tokio::test]
async fn top_level_reactions_cannot_wipe_other_senders() {
    let backend = MemoryBackend::new();
    let id = create_message(&backend).await;
    let other = json!({ "emoji": "👍", "senderId": "contact-1", "sentAt": "2024-01-01T00:00:01Z" });
    let seed: PatchObject =
        serde_json::from_value(json!({ "reactions/r1": other.clone() })).expect("patch");
    backend
        .update_object::<Message>(&(), &Id::from(ACCOUNT_ID), &Id::from(id.as_str()), seed)
        .await
        .expect("seed reaction");

    let resp = update(&backend, &id, json!({ "reactions": {} })).await;

    assert_eq!(
        resp["notUpdated"][&id],
        json!({ "type": "invalidProperties", "properties": ["reactions"] }),
        "{resp:?}"
    );
    assert_eq!(
        get(&backend, &id).await["reactions"],
        json!({ "r1": other })
    );
}
