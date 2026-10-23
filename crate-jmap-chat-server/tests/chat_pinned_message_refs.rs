//! `pinnedMessageIds` referential integrity (bd:JMAP-4azw).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 §Chat
//! `pinnedMessageIds`: "Each id MUST be the id of a Message in this Chat;
//! the server MUST reject a `Chat/set` update that would add any other id
//! with `invalidProperties`. When a pinned Message is hard-deleted, the
//! server MUST remove its id from this list."
//!
//! RFC 8620 §5.3 forbids patch pointers into arrays, so the only way to
//! add an id is a whole-list replace; no pointer-path case is tested here.

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::message::{handle_message_get, handle_message_set};
use jmap_chat_server::{ChatBackend, handle_chat_changes, handle_chat_get, handle_chat_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const CHAT: &str = "chat1";
const OTHER_CHAT: &str = "chat2";

/// Store two direct Chats with empty `pinnedMessageIds`.
fn seed_chats(backend: &MemoryBackend) {
    backend.register_account(&Id::from(ACCOUNT_ID));
    for (id, contact) in [(CHAT, "peer1"), (OTHER_CHAT, "peer2")] {
        backend.insert_object_for_test(
            "Chat",
            ACCOUNT_ID,
            id,
            json!({
                "id": id,
                "kind": "direct",
                "contactId": contact,
                "createdAt": "2026-01-01T00:00:00Z",
                "unreadCount": 0,
                "pinnedMessageIds": [],
            }),
        );
    }
}

/// Create a Message in `chat_id` via `Message/set`, merging `extra`, and
/// return its server id.
async fn create_message(backend: &MemoryBackend, chat_id: &str, extra: Value) -> String {
    let mut msg = json!({
        "chatId": chat_id,
        "body": "hello",
        "sentAt": "2026-01-01T00:00:00Z",
    });
    let (Value::Object(msg_map), Value::Object(extra_map)) = (&mut msg, extra) else {
        unreachable!("both are object literals");
    };
    msg_map.extend(extra_map);
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": { "m": msg } }),
    )
    .await
    .expect("Message/set create");
    resp["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("create failed: {resp:?}"))
        .to_owned()
}

async fn message_set(backend: &MemoryBackend, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_message_set(backend, &(), args)
        .await
        .expect("Message/set")
        .primary
}

async fn set_pins(backend: &MemoryBackend, pins: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { CHAT: { "pinnedMessageIds": pins } },
        }),
    )
    .await
    .expect("Chat/set");
    resp
}

async fn pinned(backend: &MemoryBackend) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_chat_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [CHAT] }),
    )
    .await
    .expect("Chat/get");
    resp["list"][0]["pinnedMessageIds"].clone()
}

fn assert_updated(resp: &Value) {
    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");
    assert!(resp["updated"].get(CHAT).is_some(), "{resp:?}");
}

fn assert_invalid_pins(resp: &Value) {
    assert_eq!(resp["updated"], Value::Null, "{resp:?}");
    let err = &resp["notUpdated"][CHAT];
    assert_eq!(err["type"], "invalidProperties", "{resp:?}");
    assert_eq!(err["properties"], json!(["pinnedMessageIds"]), "{resp:?}");
}

/// Pin `msg_id` in `CHAT`, asserting success.
async fn pin(backend: &MemoryBackend, msg_id: &str) {
    assert_updated(&set_pins(backend, json!([msg_id])).await);
    assert_eq!(pinned(backend).await, json!([msg_id]));
}

/// "Each id MUST be the id of a Message in this Chat; the server MUST
/// reject a `Chat/set` update that would add any other id with
/// `invalidProperties`." (id of a Message in another Chat)
#[tokio::test]
async fn adding_message_from_other_chat_is_invalid_properties() {
    let backend = MemoryBackend::new();
    seed_chats(&backend);
    let kept = create_message(&backend, CHAT, json!({})).await;
    let foreign = create_message(&backend, OTHER_CHAT, json!({})).await;
    pin(&backend, &kept).await;

    let resp = set_pins(&backend, json!([foreign, kept])).await;

    assert_invalid_pins(&resp);
    assert_eq!(pinned(&backend).await, json!([kept]), "storage unchanged");
}

/// "... the server MUST reject a `Chat/set` update that would add any
/// other id with `invalidProperties`." (id naming no Message)
#[tokio::test]
async fn adding_nonexistent_id_is_invalid_properties() {
    let backend = MemoryBackend::new();
    seed_chats(&backend);

    let resp = set_pins(&backend, json!(["no-such-message"])).await;

    assert_invalid_pins(&resp);
    assert_eq!(pinned(&backend).await, json!([]), "storage unchanged");
}

/// "Each id MUST be the id of a Message in this Chat" (the permitted
/// case: Messages of this Chat may be pinned).
#[tokio::test]
async fn adding_message_of_this_chat_succeeds() {
    let backend = MemoryBackend::new();
    seed_chats(&backend);
    let first = create_message(&backend, CHAT, json!({})).await;
    let second = create_message(&backend, CHAT, json!({})).await;

    let resp = set_pins(&backend, json!([second, first])).await;

    assert_updated(&resp);
    assert_eq!(pinned(&backend).await, json!([second, first]));
}

/// "When a pinned Message is hard-deleted, the server MUST remove its id
/// from this list." (Message/set destroy). RFC 8620 §5.2: the Chat
/// changed, so `Chat/changes` reports it as updated.
#[tokio::test]
async fn destroying_pinned_message_unpins_it() {
    let backend = MemoryBackend::new();
    seed_chats(&backend);
    let msg = create_message(&backend, CHAT, json!({})).await;
    pin(&backend, &msg).await;
    let HandlerOutput { primary: get, .. } = handle_chat_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [CHAT] }),
    )
    .await
    .expect("Chat/get");
    let since = get["state"].clone();

    let resp = message_set(&backend, json!({ "destroy": [msg] })).await;
    assert_eq!(resp["destroyed"], json!([msg]), "{resp:?}");

    assert_eq!(pinned(&backend).await, json!([]));
    let HandlerOutput {
        primary: changes, ..
    } = handle_chat_changes(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
    )
    .await
    .expect("Chat/changes");
    assert_eq!(changes["updated"], json!([CHAT]), "{changes:?}");
}

/// "When a pinned Message is hard-deleted, the server MUST remove its id
/// from this list." (`burnOnRead`: hard-deleted when `readAt` is set)
#[tokio::test]
async fn burn_on_read_unpins_message() {
    let backend = MemoryBackend::new();
    seed_chats(&backend);
    let msg = create_message(&backend, CHAT, json!({ "burnOnRead": true })).await;
    common::make_received(&backend, &msg).await;
    pin(&backend, &msg).await;

    let resp = message_set(
        &backend,
        json!({ "update": { &msg: { "readAt": "2026-01-02T00:00:00Z" } } }),
    )
    .await;
    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");
    let HandlerOutput { primary: get, .. } = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [&msg] }),
    )
    .await
    .expect("Message/get");
    assert_eq!(get["notFound"], json!([msg]), "message burned: {get:?}");

    assert_eq!(pinned(&backend).await, json!([]));
}

/// "When a pinned Message is hard-deleted, the server MUST remove its id
/// from this list." (scheduler expiry: `senderExpiresAt`,
/// `expiresAfterReadSeconds` and `messageExpirySeconds` deadlines all
/// reach the backend through `ChatBackend::expire_message`)
#[tokio::test]
async fn scheduled_expiry_unpins_message() {
    let backend = MemoryBackend::new();
    seed_chats(&backend);
    let msg = create_message(
        &backend,
        CHAT,
        json!({ "senderExpiresAt": "2099-01-01T00:00:00Z" }),
    )
    .await;
    pin(&backend, &msg).await;

    backend
        .expire_message(&(), &Id::from(ACCOUNT_ID), &Id::from(msg.as_str()))
        .await
        .expect("expire_message");

    assert_eq!(pinned(&backend).await, json!([]));
}

/// Soft deletion (`deletedAt`) retains the Message as a tombstone
/// (§Message `deletedAt`), so it is not a hard delete and the pin stays.
#[tokio::test]
async fn soft_delete_keeps_pin() {
    let backend = MemoryBackend::new();
    seed_chats(&backend);
    let msg = create_message(&backend, CHAT, json!({})).await;
    pin(&backend, &msg).await;

    let resp = message_set(
        &backend,
        json!({ "update": { &msg: { "deletedAt": "2026-01-02T00:00:00Z" } } }),
    )
    .await;
    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");

    assert_eq!(pinned(&backend).await, json!([msg]));
}
