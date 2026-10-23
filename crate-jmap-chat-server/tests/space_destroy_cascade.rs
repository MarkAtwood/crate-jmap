//! `Space/set` destroy cascades to the Space's channels (bd:JMAP-x2gd.59)
//! and requires `manage_space` (bd:JMAP-wwwx).
//!
//! Oracle: draft-atwood-jmap-chat-00 §Destroying a Space: "Cascades to
//! all channel Chats and their Messages. Hard-deletes all records; no
//! tombstones are retained." RFC 8620 §5.1: "If the data changes, this
//! string MUST change", so `Chat/changes` and `Message/changes` report
//! the cascaded destroys.

mod common;

use common::{ACCOUNT_ID, MemoryBackend, SPACE_ID, TrackingBackend};
use jmap_chat_server::message::{handle_message_changes, handle_message_get};
use jmap_chat_server::{handle_chat_changes, handle_chat_get, handle_space_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const OTHER_SPACE_ID: &str = "space2";

/// Seed a Space whose only member is the owner (member entry id =
/// accountId, draft §Terminology), holding a role with `"view"`, so its
/// channel messages are visible: §Message/query "For channel Chats, the
/// server MUST verify the caller holds the `"view"` permission before
/// returning messages in `Message/get`". The role also grants
/// `"manage_space"`, which §Destroying a Space requires.
fn seed_space(backend: &MemoryBackend, id: &str) {
    seed_space_with(backend, id, json!(["view", "manage_space"]));
}

fn seed_space_with(backend: &MemoryBackend, id: &str, permissions: Value) {
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "name": "Test",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 1,
            "categories": [],
            "uncategorizedChannelIds": [],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [{ "id": "r-view", "name": "Viewer", "permissions": permissions, "position": 1 }],
            "members": [{ "id": ACCOUNT_ID, "roleIds": ["r-view"], "joinedAt": "2026-01-01T00:00:00Z" }]
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

/// §Destroying a Space "Cascades to all channel Chats": when the channel
/// Chats cannot be read, the cascade cannot run, so the Space is not
/// destroyed. RFC 8620 §5.3: `notDestroyed` is "A map of the id to a
/// SetError for each record that failed to be destroyed", and
/// `serverFail` is a SetError type. Nothing is removed.
#[tokio::test]
async fn channel_read_failure_leaves_space_and_channels() {
    let backend = TrackingBackend::with_get_objects_failing("Chat");
    seed(backend.inner());

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "destroy": [SPACE_ID] }),
    )
    .await
    .expect("Space/set");
    assert_eq!(
        resp["notDestroyed"][SPACE_ID]["type"], "serverFail",
        "{resp}"
    );
    assert!(
        !resp["destroyed"]
            .as_array()
            .is_some_and(|d| d.contains(&json!(SPACE_ID))),
        "{resp}"
    );

    let inner = backend.inner();
    let HandlerOutput { primary: chats, .. } = handle_chat_get(
        inner,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": ["ch1", "ch2"] }),
    )
    .await
    .expect("Chat/get");
    assert_eq!(chats["notFound"], json!([]), "{chats:?}");
    let space = jmap_chat_server::JmapBackend::get_objects::<jmap_chat_types::Space>(
        inner,
        &(),
        &Id::from(ACCOUNT_ID),
        Some(&[Id::from(SPACE_ID)]),
        None,
    )
    .await
    .expect("read");
    assert_eq!(space.0.len(), 1, "the Space must survive");
}

/// §Destroying a Space (jmap-chat-spec 33702a5 L1383): "The caller MUST
/// hold `"manage_space"` ... Servers MUST return a `forbidden` SetError
/// when the caller does not satisfy the deployment's destroy-authorization
/// rules." A caller whose only role grants `"view"` gets `forbidden`, and
/// the Space and its channels survive.
#[tokio::test]
async fn destroy_without_manage_space_is_forbidden() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    seed_space_with(&backend, SPACE_ID, json!(["view"]));
    seed_chat(
        &backend,
        "ch1",
        json!({ "kind": "channel", "name": "a", "spaceId": SPACE_ID }),
    );

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "destroy": [SPACE_ID] }),
    )
    .await
    .expect("Space/set");
    assert_eq!(
        resp["notDestroyed"][SPACE_ID]["type"], "forbidden",
        "{resp}"
    );
    assert!(
        !resp["destroyed"]
            .as_array()
            .is_some_and(|d| d.contains(&json!(SPACE_ID))),
        "{resp}"
    );

    let HandlerOutput { primary: chats, .. } = handle_chat_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": ["ch1"] }),
    )
    .await
    .expect("Chat/get");
    assert_eq!(chats["notFound"], json!([]), "{chats:?}");
    let space = jmap_chat_server::JmapBackend::get_objects::<jmap_chat_types::Space>(
        &backend,
        &(),
        &Id::from(ACCOUNT_ID),
        Some(&[Id::from(SPACE_ID)]),
        None,
    )
    .await
    .expect("read");
    assert_eq!(space.0.len(), 1, "the Space must survive");
}
