//! `Space/set` destroy cascades to the Space's channels (bd:JMAP-x2gd.59).
//!
//! Oracle: draft-atwood-jmap-chat-00 §Destroying a Space: "Cascades to
//! all channel Chats and their Messages. Hard-deletes all records; no
//! tombstones are retained." RFC 8620 §5.1: "If the data changes, this
//! string MUST change", so `Chat/changes` and `Message/changes` report
//! the cascaded destroys.

mod common;

use common::{MemoryBackend, ACCOUNT_ID, SPACE_ID};
use jmap_chat_server::{
    handle_chat_changes, handle_chat_get, handle_message_changes, handle_message_get,
    handle_space_set,
};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

const OTHER_SPACE_ID: &str = "space2";

fn seed_space(backend: &MemoryBackend, id: &str) {
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "name": "Test",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 0,
            "categories": [],
            "uncategorizedChannelIds": [],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [],
            "members": []
        }),
    );
}

fn seed_chat(backend: &MemoryBackend, id: &str, extra: Value) {
    let mut chat = json!({
        "id": id,
        "createdAt": "2026-01-01T00:00:00Z",
        "unreadCount": 0,
        "pinnedMessageIds": [],
    });
    let (Value::Object(chat_map), Value::Object(extra_map)) = (&mut chat, extra) else {
        unreachable!("both are object literals");
    };
    chat_map.extend(extra_map);
    backend.insert_object_for_test("Chat", ACCOUNT_ID, id, chat);
}

fn seed_message(backend: &MemoryBackend, id: &str, chat_id: &str) {
    backend.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "senderMsgId": format!("smsg-{id}"),
            "senderId": ACCOUNT_ID,
            "chatId": chat_id,
            "body": "hi",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "broadcastMentions": [],
            "actions": [],
            "reactions": {},
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:01Z",
            "deliveryState": "delivered"
        }),
    );
}

/// Seed `SPACE_ID` with channels `ch1` (message `m1`) and `ch2` (no
/// messages), plus records the cascade must not touch: direct Chat
/// `dm` with message `m2` and channel `other` of `OTHER_SPACE_ID` with
/// message `m3`.
fn seed(backend: &MemoryBackend) {
    backend.register_account(&Id::from(ACCOUNT_ID));
    seed_space(backend, SPACE_ID);
    seed_space(backend, OTHER_SPACE_ID);
    seed_chat(
        backend,
        "ch1",
        json!({ "kind": "channel", "name": "a", "spaceId": SPACE_ID }),
    );
    seed_chat(
        backend,
        "ch2",
        json!({ "kind": "channel", "name": "b", "spaceId": SPACE_ID }),
    );
    seed_chat(
        backend,
        "other",
        json!({ "kind": "channel", "name": "c", "spaceId": OTHER_SPACE_ID }),
    );
    seed_chat(
        backend,
        "dm",
        json!({ "kind": "direct", "contactId": "c-peer" }),
    );
    seed_message(backend, "m1", "ch1");
    seed_message(backend, "m2", "dm");
    seed_message(backend, "m3", "other");
}

fn sorted_ids(v: &Value) -> Vec<String> {
    let mut ids: Vec<String> = v
        .as_array()
        .unwrap_or_else(|| panic!("expected an id array: {v:?}"))
        .iter()
        .map(|id| id.as_str().expect("id must be a string").to_owned())
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn destroying_space_destroys_its_channels_and_their_messages() {
    let backend = MemoryBackend::new();
    seed(&backend);

    let all_chats = json!({ "accountId": ACCOUNT_ID, "ids": ["ch1", "ch2", "other", "dm"] });
    let all_messages = json!({ "accountId": ACCOUNT_ID, "ids": ["m1", "m2", "m3"] });
    let HandlerOutput {
        primary: before_chats,
        ..
    } = handle_chat_get(&backend, &(), all_chats.clone())
        .await
        .expect("Chat/get");
    let HandlerOutput {
        primary: before_messages,
        ..
    } = handle_message_get(&backend, &(), all_messages.clone())
        .await
        .expect("Message/get");

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "destroy": [SPACE_ID] }),
    )
    .await
    .expect("Space/set");
    assert_eq!(resp["destroyed"], json!([SPACE_ID]), "{resp:?}");

    let HandlerOutput { primary: chats, .. } = handle_chat_get(&backend, &(), all_chats)
        .await
        .expect("Chat/get");
    assert_eq!(sorted_ids(&chats["notFound"]), ["ch1", "ch2"], "{chats:?}");
    let HandlerOutput {
        primary: messages, ..
    } = handle_message_get(&backend, &(), all_messages)
        .await
        .expect("Message/get");
    assert_eq!(sorted_ids(&messages["notFound"]), ["m1"], "{messages:?}");

    let HandlerOutput {
        primary: chat_changes,
        ..
    } = handle_chat_changes(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": before_chats["state"] }),
    )
    .await
    .expect("Chat/changes");
    assert_eq!(
        sorted_ids(&chat_changes["destroyed"]),
        ["ch1", "ch2"],
        "{chat_changes:?}"
    );

    let HandlerOutput {
        primary: message_changes,
        ..
    } = handle_message_changes(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": before_messages["state"] }),
    )
    .await
    .expect("Message/changes");
    assert_eq!(
        sorted_ids(&message_changes["destroyed"]),
        ["m1"],
        "{message_changes:?}"
    );
}
