//! The owner's client-facing id is the accountId; the literal `"self"`
//! no longer names the owner (bd:JMAP-405z.2).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00.md at jmap-chat-spec
//! cefc217, §Terminology "Owner": "The owner has no ChatContact: a
//! client-facing reference to the owner carries the accountId of the
//! owner's account ({{RFC8620}} Section 1.6.2), including where this
//! document describes the reference as a ChatContact.id, and no
//! ChatContact has that accountId as its `id`."

mod common;

use common::{ACCOUNT_ID, MemoryBackend, seed_group_chat};
use jmap_chat_server::message::{handle_message_get, handle_message_query, handle_message_set};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

fn insert_message(backend: &MemoryBackend, id: &str, sender_id: &str, extra: Value) {
    let mut msg = json!({
        "id": id,
        "senderMsgId": format!("smsg-{id}"),
        "senderId": sender_id,
        "chatId": "c1",
        "body": "original",
        "bodyType": "text/plain",
        "attachments": [],
        "mentions": [],
        "broadcastMentions": [],
        "actions": [],
        "reactions": {},
        "sentAt": "2024-01-01T00:00:00Z",
        "receivedAt": "2024-01-01T00:00:01Z",
        "deliveryState": "delivered"
    });
    if let (Some(m), Some(e)) = (msg.as_object_mut(), extra.as_object()) {
        m.extend(e.clone());
    }
    backend.insert_object_for_test("Message", ACCOUNT_ID, id, msg);
}

async fn update(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { id: patch } }),
        &[],
    )
    .await
    .expect("Message/set update");
    resp
}

async fn get_message(backend: &MemoryBackend, id: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
    )
    .await
    .expect("Message/get");
    resp["list"][0].clone()
}

fn assert_updated(resp: &Value, id: &str) {
    assert!(
        resp["updated"]
            .as_object()
            .is_some_and(|u| u.contains_key(id)),
        "{resp}"
    );
}

/// §Message `senderId`: "The accountId for owner-composed messages; for
/// inbound messages, the `id` of the ChatContact whose `identity` is the
/// sender identity verified by the authentication layer."
#[tokio::test]
async fn created_message_sender_id_is_account_id() {
    let backend = MemoryBackend::new();
    seed_group_chat(&backend, "c1");
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m": { "chatId": "c1", "body": "hi", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
        &[],
    )
    .await
    .expect("Message/set create");
    let id = resp["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("not created: {resp}"))
        .to_owned();
    assert_eq!(get_message(&backend, &id).await["senderId"], ACCOUNT_ID);
}

/// §Adding a Reaction: "The server MUST set `senderId` to the accountId
/// and MUST propagate the reaction to all recipients via `Peer/deliver`
/// `reactionUpdate` payload, carrying the `senderReactionId` as the map
/// key."
#[tokio::test]
async fn added_reaction_sender_id_is_account_id() {
    let backend = MemoryBackend::new();
    insert_message(&backend, "m1", ACCOUNT_ID, json!({}));
    let resp = update(
        &backend,
        "m1",
        json!({ "reactions/r1": { "emoji": "x", "senderId": "self" } }),
    )
    .await;
    assert_updated(&resp, "m1");
    assert_eq!(
        get_message(&backend, "m1").await["reactions"]["r1"]["senderId"],
        ACCOUNT_ID
    );
}

/// §Removing a Reaction: "Clients SHOULD only add or remove reactions
/// where `senderId` is the accountId. Servers MUST reject attempts to add
/// or remove reactions for other senders with `forbidden`."
#[tokio::test]
async fn removing_reaction_whose_sender_id_is_account_id_is_permitted() {
    let backend = MemoryBackend::new();
    insert_message(
        &backend,
        "m1",
        "contact-alice",
        json!({ "reactions": {
            "r1": { "emoji": "x", "senderId": ACCOUNT_ID, "sentAt": "2024-01-01T00:00:00Z" }
        } }),
    );
    let resp = update(&backend, "m1", json!({ "reactions/r1": null })).await;
    assert_updated(&resp, "m1");
}

/// §Removing a Reaction: "Servers MUST reject attempts to add or remove
/// reactions for other senders with `forbidden`." A stored `"self"` is
/// not the accountId, so it names another sender.
#[tokio::test]
async fn removing_reaction_whose_sender_id_is_literal_self_is_forbidden() {
    let backend = MemoryBackend::new();
    insert_message(
        &backend,
        "m1",
        ACCOUNT_ID,
        json!({ "reactions": {
            "r1": { "emoji": "x", "senderId": "self", "sentAt": "2024-01-01T00:00:00Z" }
        } }),
    );
    let resp = update(&backend, "m1", json!({ "reactions/r1": null })).await;
    assert_eq!(resp["notUpdated"]["m1"]["type"], "forbidden", "{resp}");
}

/// §Editing a Message: "`update` with changed `body`, `bodyType`,
/// `mentions`, and/or `broadcastMentions`, on a message where `senderId`
/// is the accountId and `deletedAt` is absent. The server MUST reject an
/// edit of any other message with a `forbidden` SetError."
#[tokio::test]
async fn edit_of_message_whose_sender_id_is_account_id_is_permitted() {
    let backend = MemoryBackend::new();
    insert_message(&backend, "m1", ACCOUNT_ID, json!({}));
    let resp = update(
        &backend,
        "m1",
        json!({ "body": "rewritten", "bodyType": "text/plain" }),
    )
    .await;
    assert_updated(&resp, "m1");
    assert_eq!(get_message(&backend, "m1").await["body"], "rewritten");
}

/// §Editing a Message: "The server MUST reject an edit of any other
/// message with a `forbidden` SetError." A stored `"self"` is not the
/// accountId.
#[tokio::test]
async fn edit_of_message_whose_sender_id_is_literal_self_is_forbidden() {
    let backend = MemoryBackend::new();
    insert_message(&backend, "m1", "self", json!({}));
    let resp = update(
        &backend,
        "m1",
        json!({ "body": "rewritten", "bodyType": "text/plain" }),
    )
    .await;
    assert_eq!(resp["notUpdated"]["m1"]["type"], "forbidden", "{resp}");
}

/// §Deleting a Message: "Servers MUST reject `deletedForAll: true` for
/// messages where `senderId` is not the accountId with a `forbidden`
/// SetError."
#[tokio::test]
async fn delete_for_all_of_message_whose_sender_id_is_account_id_is_permitted() {
    let backend = MemoryBackend::new();
    insert_message(&backend, "m1", ACCOUNT_ID, json!({}));
    let resp = update(&backend, "m1", json!({ "deletedForAll": true })).await;
    assert_updated(&resp, "m1");
}

/// §Deleting a Message: "Servers MUST reject `deletedForAll: true` for
/// messages where `senderId` is not the accountId with a `forbidden`
/// SetError."
#[tokio::test]
async fn delete_for_all_of_message_whose_sender_id_is_literal_self_is_forbidden() {
    let backend = MemoryBackend::new();
    insert_message(&backend, "m1", "self", json!({}));
    let resp = update(&backend, "m1", json!({ "deletedForAll": true })).await;
    assert_eq!(resp["notUpdated"]["m1"]["type"], "forbidden", "{resp}");
}

/// §Message/query `hasMention`: "Filter to messages that mention the
/// owner: the accountId appears in `mentions`, OR `broadcastMentions`
/// contains a scope whose resolved recipient set includes the owner."
#[tokio::test]
async fn has_mention_matches_account_id_and_not_other_mentions() {
    let backend = MemoryBackend::new();
    insert_message(
        &backend,
        "m-owner",
        "contact-alice",
        json!({ "mentions": [{ "id": ACCOUNT_ID }] }),
    );
    insert_message(
        &backend,
        "m-other",
        "contact-alice",
        json!({ "mentions": [{ "id": "contact-bob" }] }),
    );
    let HandlerOutput { primary: resp, .. } = handle_message_query(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": { "hasMention": true } }),
    )
    .await
    .expect("Message/query");
    assert_eq!(resp["ids"], json!(["m-owner"]), "{resp}");
}
