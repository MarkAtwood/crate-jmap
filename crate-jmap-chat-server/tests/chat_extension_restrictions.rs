//! Per-Chat restriction hooks for extension-bound Chats (bd:JMAP-dfo4):
//! `ChatBackend::may_create_message`, `may_update_message` and
//! `may_update_chat`.
//!
//! Oracles, quoted from jmap-chat-spec draft-atwood-jmap-chat-mimi-00.md
//! (91672c9), which the restricting `TrackingBackend` mode encodes:
//!
//! - L251 (§Chat.roomId): "The server MUST reject, with `forbidden`, a
//!   `Chat/set` update on such a Chat that sets `name`, `description`, or
//!   `avatarBlobId` or that uses the `addMembers`, `removeMembers`, or
//!   `updateMemberRoles` patch key".
//! - L424 (§message/mls): "The server MUST reject a `Message/set` create
//!   in a Chat with a `roomId` with a `forbidden` SetError."
//! - L426 (§message/mls): "The server MUST reject a `Message/set` update
//!   that changes `body`, `bodyType`, or `reactions` of a Message in a
//!   Chat with a `roomId` with an `invalidProperties` SetError naming the
//!   changed properties."
//!
//! L439: every Message in such a Chat has as `senderId` the ChatContact
//! of the room's hub, so the seeded Message is not the owner's.
//!
//! The default-permit path is JMAP-bghk's fail-open decision: a backend
//! that does not override the hooks restricts nothing.

mod common;

use common::{ACCOUNT_ID, TrackingBackend};
use jmap_chat_server::handle_chat_set;
use jmap_chat_server::message::handle_message_set;
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const ROOM_CHAT: &str = "room1";
const ROOM_ID: &str = "mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a";
const HUB_MSG: &str = "m1";

/// A Chat bound to a MIMI room (L251: kind `"group"`; L253: `members` is
/// the empty list) holding one Message from the hub contact.
fn seed(backend: &TrackingBackend) {
    let inner = backend.inner();
    inner.register_account(&Id::from(ACCOUNT_ID));
    inner.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        ROOM_CHAT,
        json!({
            "id": ROOM_CHAT,
            "kind": "group",
            "name": "",
            "members": [],
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "roomId": ROOM_ID,
        }),
    );
    inner.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        HUB_MSG,
        json!({
            "id": HUB_MSG,
            "senderMsgId": "01J00000000000000000000000",
            "senderId": "hub",
            "chatId": ROOM_CHAT,
            "body": "AAEC",
            "bodyType": "application/mimi-fanout",
            "reactions": {},
            "attachments": [],
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:00Z",
            "deliveryState": "delivered",
        }),
    );
}

fn restricting() -> TrackingBackend {
    let backend = TrackingBackend::with_room_restrictions();
    seed(&backend);
    backend
}

fn default_hooks() -> TrackingBackend {
    let backend = TrackingBackend::new();
    seed(&backend);
    backend
}

async fn message_set(backend: &TrackingBackend, args: Value) -> Value {
    let HandlerOutput { primary, .. } =
        handle_message_set(backend, &(), &std::collections::BTreeMap::new(), args)
            .await
            .expect("Message/set");
    primary
}

async fn chat_set(backend: &TrackingBackend, args: Value) -> Value {
    let HandlerOutput { primary, .. } =
        handle_chat_set(backend, &(), &std::collections::BTreeMap::new(), args)
            .await
            .expect("Chat/set");
    primary
}

async fn create_text(backend: &TrackingBackend) -> Value {
    message_set(
        backend,
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "c1": {
                "chatId": ROOM_CHAT,
                "body": "hello",
                "sentAt": "2026-01-02T00:00:00Z",
            } },
        }),
    )
    .await
}

async fn update_message(backend: &TrackingBackend, patch: Value) -> Value {
    message_set(
        backend,
        json!({ "accountId": ACCOUNT_ID, "update": { HUB_MSG: patch } }),
    )
    .await
}

async fn update_chat(backend: &TrackingBackend, patch: Value) -> Value {
    chat_set(
        backend,
        json!({ "accountId": ACCOUNT_ID, "update": { ROOM_CHAT: patch } }),
    )
    .await
}

/// L424: a create in a Chat with a `roomId` is `forbidden`.
#[tokio::test]
async fn create_in_room_chat_is_forbidden() {
    let backend = restricting();
    let resp = create_text(&backend).await;
    assert_eq!(resp["notCreated"]["c1"]["type"], "forbidden", "{resp}");
    assert!(resp["created"].get("c1").is_none(), "{resp}");
}

/// L426: changing `body` is `invalidProperties` naming `body`, even for
/// a Message the owner did not send (L439), which chat-00 alone would
/// reject with `forbidden`.
#[tokio::test]
async fn body_update_names_body() {
    let backend = restricting();
    let resp = update_message(&backend, json!({ "body": "edited" })).await;
    let err = &resp["notUpdated"][HUB_MSG];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(err["properties"], json!(["body"]), "{resp}");
}

/// L426: the SetError names every changed property among `body` and
/// `bodyType`.
#[tokio::test]
async fn body_and_body_type_update_names_both() {
    let backend = restricting();
    let resp = update_message(
        &backend,
        json!({ "body": "edited", "bodyType": "text/plain" }),
    )
    .await;
    let err = &resp["notUpdated"][HUB_MSG];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    let mut props: Vec<&str> = err["properties"]
        .as_array()
        .expect("properties")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    props.sort_unstable();
    assert_eq!(props, ["body", "bodyType"], "{resp}");
}

/// L426: adding a reaction changes `reactions`, so the SetError names
/// `reactions`.
#[tokio::test]
async fn reaction_update_names_reactions() {
    let backend = restricting();
    let resp = update_message(
        &backend,
        json!({ "reactions/r1": { "emoji": "\u{1F44D}" } }),
    )
    .await;
    let err = &resp["notUpdated"][HUB_MSG];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(err["properties"], json!(["reactions"]), "{resp}");
}

/// L426 restricts only `body`, `bodyType` and `reactions`: marking the
/// Message read is still accepted.
#[tokio::test]
async fn read_update_is_permitted() {
    let backend = restricting();
    let resp = update_message(&backend, json!({ "readAt": "2026-01-03T00:00:00Z" })).await;
    assert!(resp["updated"].get(HUB_MSG).is_some(), "{resp}");
}

/// L251: setting `name`, `description` or `avatarBlobId` is `forbidden`.
#[tokio::test]
async fn chat_metadata_update_is_forbidden() {
    let backend = restricting();
    for patch in [
        json!({ "name": "Renamed" }),
        json!({ "description": "About" }),
        json!({ "avatarBlobId": "blob1" }),
    ] {
        let resp = update_chat(&backend, patch.clone()).await;
        assert_eq!(
            resp["notUpdated"][ROOM_CHAT]["type"], "forbidden",
            "{patch}: {resp}"
        );
    }
}

/// L251: the `addMembers`, `removeMembers` and `updateMemberRoles` patch
/// keys are `forbidden`.
#[tokio::test]
async fn chat_member_patch_keys_are_forbidden() {
    let backend = restricting();
    for patch in [
        json!({ "addMembers": [{ "id": "bob", "role": "member" }] }),
        json!({ "removeMembers": ["bob"] }),
        json!({ "updateMemberRoles": [{ "id": "bob", "role": "admin" }] }),
    ] {
        let resp = update_chat(&backend, patch.clone()).await;
        assert_eq!(
            resp["notUpdated"][ROOM_CHAT]["type"], "forbidden",
            "{patch}: {resp}"
        );
    }
}

/// L251 lists only metadata and member keys: the owner's own settings,
/// such as `muted`, still update.
#[tokio::test]
async fn chat_owner_setting_update_is_permitted() {
    let backend = restricting();
    let resp = update_chat(&backend, json!({ "muted": true })).await;
    assert!(resp["updated"].get(ROOM_CHAT).is_some(), "{resp}");
}

/// JMAP-bghk fail-open: without an override, a create and a reaction in
/// the same Chat succeed.
#[tokio::test]
async fn default_hooks_permit() {
    let backend = default_hooks();
    let resp = create_text(&backend).await;
    assert!(resp["created"].get("c1").is_some(), "{resp}");
    let resp = update_message(
        &backend,
        json!({ "reactions/r1": { "emoji": "\u{1F44D}" } }),
    )
    .await;
    assert!(resp["updated"].get(HUB_MSG).is_some(), "{resp}");
}

/// JMAP-bghk fail-open: without an override, `may_update_chat` restricts
/// nothing. The owner is an admin of this room-bound group, so chat-00
/// allows the `description` update and only the hook could reject it; the
/// restricting backend does (L251), the default one does not.
#[tokio::test]
async fn default_hooks_permit_chat_update() {
    let admin_room = |backend: &TrackingBackend| {
        backend.inner().insert_object_for_test(
            "Chat",
            ACCOUNT_ID,
            "room2",
            json!({
                "id": "room2",
                "kind": "group",
                "name": "",
                "members": [{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }],
                "createdAt": "2026-01-01T00:00:00Z",
                "unreadCount": 0,
                "pinnedMessageIds": [],
                "roomId": ROOM_ID,
            }),
        );
    };
    let update =
        json!({ "accountId": ACCOUNT_ID, "update": { "room2": { "description": "About" } } });

    let backend = default_hooks();
    admin_room(&backend);
    let resp = chat_set(&backend, update.clone()).await;
    assert!(resp["updated"].get("room2").is_some(), "{resp}");

    let backend = restricting();
    admin_room(&backend);
    let resp = chat_set(&backend, update).await;
    assert_eq!(resp["notUpdated"]["room2"]["type"], "forbidden", "{resp}");
}
