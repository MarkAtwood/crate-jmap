//! `Chat.unreadCount` and the ReadPosition lifecycle (bd:JMAP-y3f2.4).
//!
//! Oracles, from jmap-chat-spec 91672c9:
//!
//! - draft-atwood-jmap-chat-00.md L547-548, §Chat `unreadCount`: "Count
//!   of Messages in this Chat whose `id` ULID is lexicographically greater
//!   than `ReadPosition.lastReadMessageId`. Derived server-side from the
//!   owner's ReadPosition record. If `lastReadMessageId` is absent, all
//!   messages are counted as unread."
//! - jmap-chat-compliance-tests.md §11.1: "Send 5 messages. Set
//!   ReadPosition to message 3. Call `Chat/get`. Verify `unreadCount` is
//!   2 (messages 4 and 5)."
//! - L1350-1352, §ReadPosition/set: "`update` supports:
//!   `lastReadMessageId`. The server sets `lastReadAt` to the current
//!   time and recomputes `Chat.unreadCount` for the affected Chat."
//! - L907-908, §ReadPosition: "The server creates a ReadPosition record
//!   automatically when a Chat first becomes visible to the owner, and
//!   destroys it when the Chat is destroyed. [...] For channel Chats, a
//!   ReadPosition is created when the owner joins the containing Space."
//! - L921, `lastReadAt` "(UTCDate, server-set)"; RFC 8620 §5.3 rejects a
//!   client write of a server-set property with `invalidProperties`.
//! - RFC 8620 §5.1: "If the data changes, this string MUST change", so
//!   `ReadPosition/changes` reports the cascaded destroy, and a change
//!   to the derived `unreadCount` advances the `Chat` state.
//! - RFC 8620 §5.2: `updated` lists "the ids for records that have been
//!   updated since the old state", so `Chat/changes` names the Chat
//!   whose `unreadCount` moved.

mod common;

use common::{MemoryBackend, ACCOUNT_ID, SPACE_ID};
use jmap_chat_server::{
    handle_chat_changes, handle_chat_get, handle_chat_set, handle_message_set,
    handle_position_changes, handle_position_get, handle_position_set, handle_space_join,
    handle_space_set,
};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

// Hand-picked ULIDs (Crockford base32, 26 chars) in ascending order:
// "ULIDs are lexicographically ordered by time" (draft L591).
const M1: &str = "01J0000000000000000000000A";
const M2: &str = "01J0000000000000000000000B";
const M3: &str = "01J0000000000000000000000C";
const M4: &str = "01J0000000000000000000000D";
const M5: &str = "01J0000000000000000000000E";
const OTHER_MSG: &str = "01J0000000000000000000000F";

const SEEDED_READ_AT: &str = "2000-01-01T00:00:00Z";
const OTHER_SPACE_ID: &str = "space2";

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

fn seed_group(backend: &MemoryBackend, id: &str) {
    seed_chat(
        backend,
        id,
        json!({
            "kind": "group",
            "name": "G",
            "members": [{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }],
        }),
    );
}

fn seed_message(backend: &MemoryBackend, id: &str, chat_id: &str) {
    backend.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "senderMsgId": id,
            "senderId": ACCOUNT_ID,
            "chatId": chat_id,
            "body": "x",
            "bodyType": "text/plain",
            "attachments": [],
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:00Z",
            "deliveryState": "delivered",
        }),
    );
}

fn seed_position(backend: &MemoryBackend, id: &str, chat_id: &str) {
    backend.insert_object_for_test(
        "ReadPosition",
        ACCOUNT_ID,
        id,
        json!({ "id": id, "chatId": chat_id, "lastReadAt": SEEDED_READ_AT }),
    );
}

/// Group Chat `g` with M1..M5 and its ReadPosition `rp-g` (no
/// `lastReadMessageId`), plus group Chat `h` with OTHER_MSG.
fn seed_five(backend: &MemoryBackend) {
    backend.register_account(&Id::from(ACCOUNT_ID));
    seed_group(backend, "g");
    seed_group(backend, "h");
    for m in [M1, M2, M3, M4, M5] {
        seed_message(backend, m, "g");
    }
    seed_message(backend, OTHER_MSG, "h");
    seed_position(backend, "rp-g", "g");
}

async fn unread_counts(backend: &MemoryBackend, args: Value) -> Vec<Value> {
    let HandlerOutput { primary: resp, .. } =
        handle_chat_get(backend, &(), args).await.expect("Chat/get");
    resp["list"].as_array().expect("list").clone()
}

async fn unread_of(backend: &MemoryBackend, chat_id: &str) -> Value {
    let list = unread_counts(
        backend,
        json!({ "accountId": ACCOUNT_ID, "ids": [chat_id] }),
    )
    .await;
    assert_eq!(list.len(), 1, "{list:?}");
    list[0]["unreadCount"].clone()
}

async fn set_last_read(backend: &MemoryBackend, rp: &str, patch: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_position_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { rp: patch } }),
    )
    .await
    .expect("ReadPosition/set");
    resp
}

async fn positions(backend: &MemoryBackend) -> Vec<Value> {
    let HandlerOutput { primary: resp, .. } =
        handle_position_get(backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("ReadPosition/get");
    resp["list"].as_array().expect("list").clone()
}

async fn position_chat_ids(backend: &MemoryBackend) -> Vec<String> {
    let mut ids: Vec<String> = positions(backend)
        .await
        .iter()
        .map(|p| p["chatId"].as_str().expect("chatId").to_owned())
        .collect();
    ids.sort();
    ids
}

async fn position_state(backend: &MemoryBackend) -> String {
    let HandlerOutput { primary: resp, .. } =
        handle_position_get(backend, &(), json!({ "accountId": ACCOUNT_ID, "ids": [] }))
            .await
            .expect("ReadPosition/get");
    resp["state"].as_str().expect("state").to_owned()
}

async fn destroyed_positions_since(backend: &MemoryBackend, since: &str) -> Vec<Value> {
    let HandlerOutput { primary: resp, .. } = handle_position_changes(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
    )
    .await
    .expect("ReadPosition/changes");
    resp["destroyed"].as_array().expect("destroyed").clone()
}

/// L548: "If `lastReadMessageId` is absent, all messages are counted as
/// unread." Only Messages "in this Chat" count, so `h` has 1.
#[tokio::test]
async fn absent_last_read_counts_every_message() {
    let backend = MemoryBackend::new();
    seed_five(&backend);
    assert_eq!(unread_of(&backend, "g").await, 5);
    assert_eq!(unread_of(&backend, "h").await, 1);
}

/// Compliance §11.1: "Send 5 messages. Set ReadPosition to message 3.
/// Call `Chat/get`. Verify `unreadCount` is 2 (messages 4 and 5)."
#[tokio::test]
async fn compliance_11_1_unread_count_from_read_position() {
    let backend = MemoryBackend::new();
    seed_five(&backend);

    let resp = set_last_read(&backend, "rp-g", json!({ "lastReadMessageId": M3 })).await;
    assert!(resp["updated"].get("rp-g").is_some(), "{resp}");

    assert_eq!(unread_of(&backend, "g").await, 2);
    assert_eq!(unread_of(&backend, "h").await, 1, "other Chat unaffected");

    // L1351: the count is recomputed on each ReadPosition/set.
    set_last_read(&backend, "rp-g", json!({ "lastReadMessageId": M5 })).await;
    assert_eq!(unread_of(&backend, "g").await, 0);
}

/// `unreadCount` is computed for a full `Chat/get` (no `ids`) as well.
#[tokio::test]
async fn unread_count_in_full_get() {
    let backend = MemoryBackend::new();
    seed_five(&backend);
    set_last_read(&backend, "rp-g", json!({ "lastReadMessageId": M4 })).await;

    let list = unread_counts(&backend, json!({ "accountId": ACCOUNT_ID })).await;
    let count = |id: &str| {
        list.iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("{id} missing: {list:?}"))["unreadCount"]
            .clone()
    };
    assert_eq!(count("g"), 1);
    assert_eq!(count("h"), 1);
}

/// L1351: "The server sets `lastReadAt` to the current time".
#[tokio::test]
async fn read_position_set_updates_last_read_at() {
    let backend = MemoryBackend::new();
    seed_five(&backend);

    set_last_read(&backend, "rp-g", json!({ "lastReadMessageId": M2 })).await;
    let list = positions(&backend).await;
    let rp = list.iter().find(|p| p["id"] == "rp-g").expect("rp-g");
    assert_eq!(rp["lastReadMessageId"], M2, "{rp}");
    let read_at = rp["lastReadAt"].as_str().expect("lastReadAt is a UTCDate");
    assert_ne!(read_at, SEEDED_READ_AT, "{rp}");
    assert!(
        read_at > "2026-01-01T00:00:00Z",
        "{read_at} is not the current time"
    );
}

/// L921 `lastReadAt` is server-set: a client write is `invalidProperties`
/// (RFC 8620 §5.3) and the record is unchanged.
#[tokio::test]
async fn client_last_read_at_is_rejected() {
    let backend = MemoryBackend::new();
    seed_five(&backend);

    let resp = set_last_read(
        &backend,
        "rp-g",
        json!({ "lastReadMessageId": M2, "lastReadAt": "2030-01-01T00:00:00Z" }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["rp-g"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"]["rp-g"]["properties"],
        json!(["lastReadAt"]),
        "{resp}"
    );
    let list = positions(&backend).await;
    assert_eq!(list[0]["lastReadAt"], SEEDED_READ_AT, "{list:?}");
    assert!(list[0].get("lastReadMessageId").is_none(), "{list:?}");
}

/// L907: the server "destroys it when the Chat is destroyed"; RFC 8620
/// §5.1 makes `ReadPosition/changes` report it.
#[tokio::test]
async fn chat_destroy_destroys_its_read_position() {
    let backend = MemoryBackend::new();
    seed_five(&backend);
    seed_position(&backend, "rp-h", "h");
    let since = position_state(&backend).await;

    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "destroy": ["g"] }),
    )
    .await
    .expect("Chat/set");
    assert_eq!(resp["destroyed"], json!(["g"]), "{resp}");

    assert_eq!(position_chat_ids(&backend).await, vec!["h"]);
    assert_eq!(
        destroyed_positions_since(&backend, &since).await,
        vec![json!("rp-g")]
    );
}

/// A Space whose only member is `preseeded-admin`, holding channels
/// `ch1` and `ch2`, plus channel `other` of `OTHER_SPACE_ID`.
fn seed_space_with_channels(backend: &MemoryBackend, is_public: bool, owner_member: bool) {
    backend.register_account(&Id::from(ACCOUNT_ID));
    let mut members = vec![json!({
        "id": "preseeded-admin", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z"
    })];
    if owner_member {
        members.push(json!({
            "id": ACCOUNT_ID, "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z"
        }));
    }
    for space_id in [SPACE_ID, OTHER_SPACE_ID] {
        backend.insert_object_for_test(
            "Space",
            ACCOUNT_ID,
            space_id,
            json!({
                "id": space_id,
                "name": "S",
                "createdAt": "2026-01-01T00:00:00Z",
                "memberCount": members.len(),
                "categories": [],
                "uncategorizedChannelIds": [],
                "isPublic": is_public,
                "isPubliclyPreviewable": false,
                "roles": [{
                    "id": "r-admin",
                    "name": "Admin",
                    "permissions": ["manage_space", "manage_channels", "view"],
                    "position": 1
                }],
                "members": members,
            }),
        );
    }
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
}

/// L908: "For channel Chats, a ReadPosition is created when the owner
/// joins the containing Space." Only that Space's channels get one.
#[tokio::test]
async fn space_join_creates_read_positions_for_its_channels() {
    let backend = MemoryBackend::new();
    seed_space_with_channels(&backend, true, false);
    assert!(positions(&backend).await.is_empty());

    handle_space_join(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "spaceId": SPACE_ID }),
    )
    .await
    .expect("Space/join");

    assert_eq!(position_chat_ids(&backend).await, vec!["ch1", "ch2"]);
    for p in positions(&backend).await {
        assert!(p.get("lastReadMessageId").is_none(), "{p}");
    }
}

/// L907 "destroys it when the Chat is destroyed" holds for a channel
/// removed by `Space/set` `removeChannels`.
#[tokio::test]
async fn remove_channels_destroys_their_read_positions() {
    let backend = MemoryBackend::new();
    seed_space_with_channels(&backend, false, true);
    seed_position(&backend, "rp1", "ch1");
    seed_position(&backend, "rp2", "ch2");
    seed_position(&backend, "rp3", "other");
    let since = position_state(&backend).await;

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeChannels": ["ch1"] } }
        }),
    )
    .await
    .expect("Space/set");
    assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp}");

    assert_eq!(position_chat_ids(&backend).await, vec!["ch2", "other"]);
    assert_eq!(
        destroyed_positions_since(&backend, &since).await,
        vec![json!("rp1")]
    );
}

/// Destroying a Space destroys its channel Chats (§Destroying a Space),
/// so their ReadPositions go too (L907).
#[tokio::test]
async fn space_destroy_destroys_channel_read_positions() {
    let backend = MemoryBackend::new();
    seed_space_with_channels(&backend, false, true);
    seed_position(&backend, "rp1", "ch1");
    seed_position(&backend, "rp2", "ch2");
    seed_position(&backend, "rp3", "other");
    let since = position_state(&backend).await;

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "destroy": [SPACE_ID] }),
    )
    .await
    .expect("Space/set");
    assert_eq!(resp["destroyed"], json!([SPACE_ID]), "{resp}");

    assert_eq!(position_chat_ids(&backend).await, vec!["other"]);
    let mut destroyed = destroyed_positions_since(&backend, &since).await;
    destroyed.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    assert_eq!(destroyed, vec![json!("rp1"), json!("rp2")]);
}

async fn chat_state(backend: &MemoryBackend) -> String {
    let HandlerOutput { primary: resp, .. } =
        handle_chat_get(backend, &(), json!({ "accountId": ACCOUNT_ID, "ids": [] }))
            .await
            .expect("Chat/get");
    resp["state"].as_str().expect("state").to_owned()
}

async fn chats_updated_since(backend: &MemoryBackend, since: &str) -> Vec<Value> {
    let HandlerOutput { primary: resp, .. } = handle_chat_changes(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
    )
    .await
    .expect("Chat/changes");
    resp["updated"].as_array().expect("updated").clone()
}

async fn message_set(backend: &MemoryBackend, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary: resp, .. } = handle_message_set(backend, &(), args)
        .await
        .expect("Message/set");
    resp
}

/// L1351 "recomputes `Chat.unreadCount` for the affected Chat" changes
/// Chat data, so (RFC 8620 §5.1) the Chat state moves and (§5.2)
/// `Chat/changes` lists only the affected Chat.
#[tokio::test]
async fn read_position_set_advances_chat_state() {
    let backend = MemoryBackend::new();
    seed_five(&backend);
    assert_eq!(unread_of(&backend, "g").await, 5);
    let since = chat_state(&backend).await;

    set_last_read(&backend, "rp-g", json!({ "lastReadMessageId": M3 })).await;

    assert_eq!(unread_of(&backend, "g").await, 2);
    assert_ne!(chat_state(&backend).await, since);
    assert_eq!(
        chats_updated_since(&backend, &since).await,
        vec![json!("g")]
    );
}

/// Destroying an unread Message lowers the Chat's `unreadCount`
/// (L547), so `Chat/changes` reports that Chat (RFC 8620 §5.2).
#[tokio::test]
async fn message_destroy_advances_chat_state() {
    let backend = MemoryBackend::new();
    seed_five(&backend);
    let since = chat_state(&backend).await;

    let resp = message_set(&backend, json!({ "destroy": [M5] })).await;
    assert_eq!(resp["destroyed"], json!([M5]), "{resp}");

    assert_eq!(unread_of(&backend, "g").await, 4);
    assert_eq!(
        chats_updated_since(&backend, &since).await,
        vec![json!("g")]
    );
}

/// A new Message in a Chat raises its `unreadCount` (L547), so
/// `Chat/changes` reports that Chat (RFC 8620 §5.2).
#[tokio::test]
async fn message_create_advances_chat_state() {
    let backend = MemoryBackend::new();
    seed_five(&backend);
    let since = chat_state(&backend).await;

    let resp = message_set(
        &backend,
        json!({ "create": { "m": {
            "chatId": "h",
            "body": "hi",
            "bodyType": "text/plain",
            "sentAt": "2026-01-01T00:00:00Z",
        } } }),
    )
    .await;
    assert!(resp["created"].get("m").is_some(), "{resp}");

    assert_eq!(unread_of(&backend, "h").await, 2);
    assert_eq!(
        chats_updated_since(&backend, &since).await,
        vec![json!("h")]
    );
}
