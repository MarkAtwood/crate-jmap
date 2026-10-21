//! `Chat/set` and `Message/set` handler edge cases (bd:JMAP-y3f2.16).
//!
//! Oracles:
//!
//! - RFC 8620 §3.6.2 `serverFail`: "The method call made no changes to
//!   the server's state." §5.3: "It is permissible for the server to
//!   commit changes to some objects but not others".
//! - draft-atwood-jmap-chat-00.md at jmap-chat-spec 91672c9, §Chat Object
//!   Fields: L506 `contactId` "**Direct chats only.**"; L509 `name`
//!   "**Group and channel Chats.**"; L512 `description` and L515
//!   `avatarBlobId` "**Group chats only.**"; L524 `spaceId` "**Channel
//!   Chats only.**". L1003 a direct create "accepts" `contactId`; L1018
//!   group create: "Optional at creation: `description` (String),
//!   `avatarBlobId` (String), `messageExpirySeconds` (UnsignedInt)."
//! - `ChatBackend::create_object` rustdoc: "The backend mints the new
//!   object's id, calls `build(&id)` exactly once to obtain the complete
//!   object, and stores that object under `id`".
//! - The bead: the Chat/set and Message/set update checks reuse the one
//!   stored-object read instead of re-reading it per check.

mod common;

use common::{MemoryBackend, TrackingBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_chat_set, handle_message_set, store_server_message, StoreOutcome};
use jmap_chat_types::{DeliveryState, Message};
use jmap_server::HandlerOutput;
use jmap_types::{Id, UTCDate};
use serde_json::{json, Value};

const GROUP: &str = "g1";
const OWN_MSG: &str = "m1";

/// A group Chat whose only member is the owner, as admin, holding one
/// Message the owner sent.
fn seed(backend: &MemoryBackend) {
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        GROUP,
        json!({
            "id": GROUP,
            "kind": "group",
            "name": "G",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "members": [{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }],
        }),
    );
    backend.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        OWN_MSG,
        json!({
            "id": OWN_MSG,
            "senderMsgId": "01J00000000000000000000000",
            "senderId": ACCOUNT_ID,
            "chatId": GROUP,
            "body": "hello",
            "bodyType": "text/plain",
            "reactions": {},
            "attachments": [],
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:00Z",
            "deliveryState": "delivered",
        }),
    );
}

async fn chat_set<B: jmap_chat_server::ChatBackend<CallerCtx = ()>>(
    backend: &B,
    args: Value,
) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } =
        handle_chat_set(backend, &(), args).await.expect("Chat/set");
    primary
}

/// The sorted `properties` of a SetError.
fn properties(err: &Value) -> Vec<String> {
    let mut props: Vec<String> = err["properties"]
        .as_array()
        .unwrap_or_else(|| panic!("no properties: {err}"))
        .iter()
        .map(|p| p.as_str().expect("string").to_owned())
        .collect();
    props.sort();
    props
}

/// §3.6.2: a method `serverFail` claims no changes, so a failed destroy
/// read after a committed create is reported per id, and the read's
/// failure destroys nothing.
#[tokio::test]
async fn destroy_read_failure_is_per_id_server_fail() {
    let backend = TrackingBackend::with_get_objects_failing("Chat");
    seed(backend.inner());

    let resp = chat_set(
        &backend,
        json!({
            "create": { "c1": { "kind": "group", "name": "New", "memberIds": [] } },
            "destroy": [GROUP],
        }),
    )
    .await;

    assert!(resp["created"]["c1"]["id"].is_string(), "{resp}");
    assert_eq!(resp["notDestroyed"][GROUP]["type"], "serverFail", "{resp}");
    assert!(
        !resp["destroyed"]
            .as_array()
            .is_some_and(|d| d.contains(&json!(GROUP))),
        "{resp}"
    );
    let (chats, _) = jmap_chat_server::JmapBackend::get_objects::<jmap_chat_types::Chat>(
        backend.inner(),
        &(),
        &Id::from(ACCOUNT_ID),
        Some(&[Id::from(GROUP)]),
        None,
    )
    .await
    .expect("read");
    assert_eq!(chats.len(), 1, "the Chat must survive");
}

/// L509, L512, L515: `name`, `description` and `avatarBlobId` belong to
/// group Chats, so a direct create carrying them is invalid.
#[tokio::test]
async fn direct_create_rejects_group_only_properties() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));

    let resp = chat_set(
        &backend,
        json!({ "create": { "d": {
            "kind": "direct",
            "contactId": "contact-1",
            "name": "N",
            "description": "D",
            "avatarBlobId": "blob-1",
        } } }),
    )
    .await;

    let err = &resp["notCreated"]["d"];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(properties(err), ["avatarBlobId", "description", "name"]);
    assert!(resp["created"].get("d").is_none(), "{resp}");
}

/// L506, L524: `contactId` belongs to direct Chats and `spaceId` to
/// channel Chats, so a group create carrying them is invalid.
#[tokio::test]
async fn group_create_rejects_direct_and_channel_properties() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));

    let resp = chat_set(
        &backend,
        json!({ "create": { "g": {
            "kind": "group",
            "name": "G",
            "memberIds": [],
            "contactId": "contact-1",
            "spaceId": "space-1",
        } } }),
    )
    .await;

    let err = &resp["notCreated"]["g"];
    assert_eq!(err["type"], "invalidProperties", "{resp}");
    assert_eq!(properties(err), ["contactId", "spaceId"]);
}

/// L1018: a group create still accepts `description` and `avatarBlobId`.
#[tokio::test]
async fn group_create_keeps_group_properties() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));

    let resp = chat_set(
        &backend,
        json!({ "create": { "g": {
            "kind": "group",
            "name": "G",
            "memberIds": [],
            "description": "D",
            "avatarBlobId": "blob-1",
        } } }),
    )
    .await;

    let created = &resp["created"]["g"];
    assert_eq!(created["description"], "D", "{resp}");
    assert_eq!(created["avatarBlobId"], "blob-1", "{resp}");
}

/// The `create_object` contract: the backend mints the id and the
/// builder's Message carries it, so the stored id is the one returned.
#[tokio::test]
async fn store_server_message_builds_with_backend_id() {
    let backend = TrackingBackend::new();
    seed(backend.inner());
    let at = UTCDate::from("2026-01-02T00:00:00Z");
    let smid = Id::from("01J00000000000000000000009");
    let build = |id: &Id| {
        Message::new(
            id.clone(),
            smid.clone(),
            Id::from("contact-hub"),
            Id::from(GROUP),
            "AAEC".to_owned(),
            "application/mimi-fanout".to_owned(),
            at.clone(),
            at.clone(),
            DeliveryState::Delivered,
        )
    };

    let outcome = store_server_message(
        &backend,
        &(),
        &Id::from(ACCOUNT_ID),
        &Id::from(GROUP),
        &smid,
        build,
    )
    .await
    .expect("store");

    let StoreOutcome::Stored(id) = outcome else {
        panic!("first store must not be a duplicate: {outcome:?}");
    };
    let ids = backend.create_input_ids();
    assert_eq!(ids.first(), Some(&json!(id.as_ref())), "{ids:?}");
}

/// A Chat/set update touching the admin, member and pin checks reads the
/// stored Chat once.
#[tokio::test]
async fn chat_update_reads_stored_chat_once() {
    let backend = TrackingBackend::new();
    seed(backend.inner());

    let resp = chat_set(
        &backend,
        json!({ "update": { GROUP: {
            "name": "Renamed",
            "addMembers": [{ "id": "contact-2" }],
            "pinnedMessageIds": [OWN_MSG],
        } } }),
    )
    .await;

    assert!(resp["updated"].get(GROUP).is_some(), "{resp}");
    assert_eq!(backend.get_objects_call_count("Chat"), 1);
}

/// A Message/set update touching the edit, reaction, read and
/// broadcast-mention checks reads the stored Message and its Chat once.
#[tokio::test]
async fn message_update_reads_stored_objects_once() {
    let backend = TrackingBackend::new();
    seed(backend.inner());

    let HandlerOutput { primary: resp, .. } = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { OWN_MSG: {
                "body": "edited",
                "readAt": "2026-01-03T00:00:00Z",
                "reactions/r1": { "emoji": "+1" },
            } },
        }),
    )
    .await
    .expect("Message/set");

    assert!(resp["updated"].get(OWN_MSG).is_some(), "{resp}");
    assert_eq!(backend.get_objects_call_count("Message"), 1);
    assert_eq!(backend.get_objects_call_count("Chat"), 1);
}
