//! `Chat/set` patch key `kickMembers` (bd:JMAP-it7g.2).
//!
//! Oracle: draft-atwood-jmap-chat-mimi-00 §Moderation: "The `kickMembers`
//! patch key of `Chat/set` (String\[], ChatContact ids), which this
//! document adds for Chats bound to a room, removes the listed users'
//! clients from the group but keeps them in the participant list, so that
//! they can rejoin (`canKick`). {{JMAP-CHAT}} has no such action, because
//! there a member and its clients are not separate."
//!
//! This crate has no MIMI room model, so the key reaches
//! `ChatBackend::kick_chat_members`. The tests use `TrackingBackend` with
//! room restrictions on, which accepts the kick for a Chat whose extension
//! properties hold `roomId` and records the ids; the plain `MemoryBackend`
//! binds no Chat to a room.

mod common;

use std::collections::BTreeMap;

use common::{ACCOUNT_ID, MemoryBackend, TrackingBackend, account_backend};
use jmap_chat_server::{ChatBackend, handle_chat_get, handle_chat_set};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

/// A group Chat bound to a MIMI room (`roomId`), where the owner is the
/// admin and `bob` and `carol` are members.
const ROOM_CHAT: &str = "g-room";
/// The same group, bound to no room.
const PLAIN_CHAT: &str = "g-plain";

fn group(id: &str, room: bool) -> Value {
    let mut chat = json!({
        "id": id,
        "kind": "group",
        "name": "Team",
        "createdAt": "2026-01-01T00:00:00Z",
        "unreadCount": 0,
        "members": [
            { "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "bob", "role": "member", "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "carol", "role": "member", "joinedAt": "2026-01-01T00:00:00Z" },
        ],
    });
    if room {
        chat["roomId"] = json!("room-1");
    }
    chat
}

fn seed(inner: &MemoryBackend) {
    inner.insert_object_for_test("Chat", ACCOUNT_ID, ROOM_CHAT, group(ROOM_CHAT, true));
    inner.insert_object_for_test("Chat", ACCOUNT_ID, PLAIN_CHAT, group(PLAIN_CHAT, false));
}

fn room_backend() -> TrackingBackend {
    let backend = TrackingBackend::with_room_restrictions();
    seed(backend.inner());
    backend
}

async fn update<B: ChatBackend<CallerCtx = ()>>(backend: &B, chat: &str, patch: Value) -> Value {
    let HandlerOutput { primary, .. } = handle_chat_set(
        backend,
        &(),
        &BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { chat: patch } }),
        &[],
    )
    .await
    .expect("Chat/set");
    primary
}

async fn member_ids<B: ChatBackend<CallerCtx = ()>>(backend: &B, chat: &str) -> Vec<String> {
    let HandlerOutput { primary, .. } = handle_chat_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [chat] }),
        &[],
    )
    .await
    .expect("Chat/get");
    primary["list"][0]["members"]
        .as_array()
        .expect("members")
        .iter()
        .map(|m| m["id"].as_str().expect("id").to_owned())
        .collect()
}

/// "removes the listed users' clients from the group but keeps them in the
/// participant list": the backend hook receives the ids, and `members` is
/// unchanged.
#[tokio::test]
async fn kick_reaches_the_backend_and_keeps_members() {
    let backend = room_backend();
    let resp = update(
        &backend,
        ROOM_CHAT,
        json!({ "kickMembers": ["bob", "carol"] }),
    )
    .await;
    assert_eq!(resp["notUpdated"], Value::Null, "{resp}");
    assert!(resp["updated"].get(ROOM_CHAT).is_some(), "{resp}");
    assert_eq!(
        backend.kick_calls(),
        vec![(
            ROOM_CHAT.to_owned(),
            vec!["bob".to_owned(), "carol".to_owned()]
        )]
    );
    assert_eq!(
        member_ids(&backend, ROOM_CHAT).await,
        vec![ACCOUNT_ID, "bob", "carol"]
    );
}

/// The key combines with other `Chat/set` update keys; both apply.
#[tokio::test]
async fn kick_with_another_key_applies_both() {
    let backend = room_backend();
    let resp = update(
        &backend,
        ROOM_CHAT,
        json!({ "kickMembers": ["bob"], "muted": true }),
    )
    .await;
    assert_eq!(resp["notUpdated"], Value::Null, "{resp}");
    assert_eq!(
        backend.kick_calls(),
        vec![(ROOM_CHAT.to_owned(), vec!["bob".to_owned()])]
    );
}

/// The key is `String[]` of ChatContact ids. Anything else is
/// `invalidProperties` naming `kickMembers` (RFC 8620 §5.3), and the
/// backend is not called. RFC 8620 §1.2: an Id uses only the base64url
/// alphabet, so `"bad id"` is not one.
#[tokio::test]
async fn malformed_kick_is_invalid_properties() {
    for value in [
        json!("bob"),
        json!([1]),
        json!(["bad id"]),
        json!(null),
        json!({ "bob": true }),
    ] {
        let backend = room_backend();
        let resp = update(&backend, ROOM_CHAT, json!({ "kickMembers": value.clone() })).await;
        assert_eq!(
            resp["notUpdated"][ROOM_CHAT]["type"], "invalidProperties",
            "{value}: {resp}"
        );
        assert_eq!(
            resp["notUpdated"][ROOM_CHAT]["properties"],
            json!(["kickMembers"]),
            "{value}: {resp}"
        );
        assert!(backend.kick_calls().is_empty(), "{value}");
    }
}

/// A `#creationId` element resolves as for `removeMembers` (RFC 8620
/// §5.3); an unknown one is `invalidProperties` naming the key.
#[tokio::test]
async fn unknown_creation_reference_is_invalid_properties() {
    let backend = room_backend();
    let resp = update(&backend, ROOM_CHAT, json!({ "kickMembers": ["#nope"] })).await;
    assert_eq!(
        resp["notUpdated"][ROOM_CHAT]["type"], "invalidProperties",
        "{resp}"
    );
    assert!(backend.kick_calls().is_empty());
}

/// An empty list kicks nobody: a no-op success without calling the
/// backend.
#[tokio::test]
async fn empty_kick_is_a_no_op() {
    let backend = room_backend();
    let resp = update(&backend, ROOM_CHAT, json!({ "kickMembers": [] })).await;
    assert!(resp["updated"].get(ROOM_CHAT).is_some(), "{resp}");
    assert!(backend.kick_calls().is_empty());
}

/// The mimi draft adds the key only "for Chats bound to a room". The hook
/// decides; this test backend rejects it for an unbound Chat with the
/// `ChatBackend::kick_chat_members` default.
#[tokio::test]
async fn kick_on_an_unbound_chat_is_rejected() {
    let backend = room_backend();
    let resp = update(&backend, PLAIN_CHAT, json!({ "kickMembers": ["bob"] })).await;
    assert_eq!(
        resp["notUpdated"][PLAIN_CHAT]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"][PLAIN_CHAT]["properties"],
        json!(["kickMembers"]),
        "{resp}"
    );
}

/// The default hook does not pretend to kick: a backend with no MIMI room
/// model (the plain `MemoryBackend`) rejects the key as it would any key
/// outside draft-atwood-jmap-chat-00 §Chat/set "Updating a Chat", even on
/// a Chat that carries a `roomId`.
#[tokio::test]
async fn default_backend_rejects_kick() {
    let backend = account_backend();
    seed(&backend);
    for chat in [ROOM_CHAT, PLAIN_CHAT] {
        let resp = update(&backend, chat, json!({ "kickMembers": ["bob"] })).await;
        assert_eq!(
            resp["notUpdated"][chat]["type"], "invalidProperties",
            "{chat}: {resp}"
        );
        assert_eq!(
            resp["notUpdated"][chat]["properties"],
            json!(["kickMembers"]),
            "{chat}: {resp}"
        );
        assert_eq!(
            member_ids(&backend, chat).await,
            vec![ACCOUNT_ID, "bob", "carol"]
        );
    }
}
