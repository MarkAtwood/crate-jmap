//! Server-originated Message store and `senderMsgId` lookup
//! (bd:JMAP-53vd, bd:JMAP-5mz6.2).
//!
//! Oracles, from jmap-chat-spec 91672c9:
//!
//! - draft-atwood-jmap-chat-mimi-00.md L430, §application/mimi-fanout:
//!   "Only servers create such Messages".
//! - mimi L436: "The server MUST NOT store a FanoutMessage whose
//!   `senderMsgId` it has already stored in the Chat, so a repeated
//!   `/notify` yields one Message."
//! - draft-atwood-jmap-chat-federation-00.md L341: "`senderMsgId` [...]
//!   Functions as an idempotency key; if this value is already known for
//!   the given chat, the receiver MAY silently discard the duplicate."
//! - draft-atwood-jmap-chat-00.md L593: "Servers MUST maintain a durable
//!   index of `senderMsgId` values per chat".
//! - chat-00 L909, §ReadPosition: "For direct and group Chats, a
//!   ReadPosition is created when the first message is delivered."

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::message::{
    find_message_by_sender_msg_id, handle_message_get, store_server_message, StoreOutcome,
};
use jmap_chat_server::{handle_chat_get, handle_position_get};
use jmap_chat_types::{DeliveryState, Message};
use jmap_server::HandlerOutput;
use jmap_types::{Id, UTCDate};
use serde_json::{json, Value};

const FANOUT: &str = "application/mimi-fanout";
const HUB: &str = "contact-hub";
const SMID: &str = "01J00000000000000000000001";

fn seed_group(backend: &MemoryBackend, id: &str) {
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "kind": "group",
            "name": "G",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "members": [{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }],
        }),
    );
}

fn setup() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    seed_group(&backend, "g");
    seed_group(&backend, "h");
    backend
}

fn fanout(chat_id: &str, sender_msg_id: &str, body: &str) -> impl FnOnce(&Id) -> Message + Send {
    let (chat_id, sender_msg_id, body) = (
        chat_id.to_owned(),
        sender_msg_id.to_owned(),
        body.to_owned(),
    );
    move |id: &Id| {
        Message::new(
            id.clone(),
            Id::from(sender_msg_id),
            Id::from(HUB),
            Id::from(chat_id),
            body,
            FANOUT,
            UTCDate::from("2026-01-02T00:00:00Z"),
            UTCDate::from("2026-01-02T00:00:01Z"),
            DeliveryState::Delivered,
        )
    }
}

async fn messages(backend: &MemoryBackend) -> Value {
    let HandlerOutput { primary, .. } = handle_message_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": null }),
    )
    .await
    .expect("Message/get");
    primary
}

async fn positions(backend: &MemoryBackend) -> Vec<Value> {
    let HandlerOutput { primary, .. } = handle_position_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": null }),
    )
    .await
    .expect("ReadPosition/get");
    primary["list"].as_array().expect("list").clone()
}

async fn store(
    backend: &MemoryBackend,
    chat_id: &str,
    sender_msg_id: &str,
    body: &str,
) -> StoreOutcome {
    store_server_message(
        backend,
        &(),
        &Id::from(ACCOUNT_ID),
        &Id::from(chat_id),
        &Id::from(sender_msg_id),
        fanout(chat_id, sender_msg_id, body),
    )
    .await
    .expect("store_server_message")
}

/// The duplicate check keys on the `chat_id`/`sender_msg_id` arguments, so
/// the stored Message carries those keys even when `build` sets others
/// (mimi L443 at jmap-chat-spec 29e9a41):
/// "The server MUST NOT store a FanoutMessage whose `senderMsgId` it has
/// already stored in the Chat" holds only if the stored key is the checked
/// one. Expected values are the literal arguments.
#[tokio::test]
async fn stored_keys_are_the_checked_keys() {
    let backend = setup();
    let StoreOutcome::Stored(_) = store_server_message(
        &backend,
        &(),
        &Id::from(ACCOUNT_ID),
        &Id::from("g"),
        &Id::from(SMID),
        fanout("other-chat", "other-smid", "AAEC"),
    )
    .await
    .expect("store_server_message") else {
        panic!("first store must not be a duplicate");
    };
    let list = messages(&backend).await["list"].clone();
    assert_eq!(list[0]["chatId"], json!("g"), "{list}");
    assert_eq!(list[0]["senderMsgId"], json!(SMID), "{list}");
    assert!(
        matches!(
            store(&backend, "g", SMID, "AAEC").await,
            StoreOutcome::Duplicate(_)
        ),
        "a second store under the checked keys must be a duplicate"
    );
}

/// mimi L430 "Only servers create such Messages": the helper stores a
/// Message with the server-reserved body type and the server-chosen
/// `senderMsgId` and `senderId`, under a server-assigned `id`.
#[tokio::test]
async fn stores_server_reserved_body_type() {
    let backend = setup();
    let StoreOutcome::Stored(id) = store(&backend, "g", SMID, "AAEC").await else {
        panic!("first store must not be a duplicate");
    };
    let list = messages(&backend).await["list"].clone();
    assert_eq!(list.as_array().map(Vec::len), Some(1), "{list}");
    let m = &list[0];
    assert_eq!(m["id"], json!(id.as_ref()));
    assert_eq!(m["bodyType"], json!(FANOUT));
    assert_eq!(m["senderMsgId"], json!(SMID));
    assert_eq!(m["senderId"], json!(HUB));
    assert_eq!(m["chatId"], json!("g"));
    assert_eq!(m["body"], json!("AAEC"));
}

/// mimi L436: a repeated `senderMsgId` in the same Chat "yields one
/// Message": the second call reports the stored id and writes nothing.
#[tokio::test]
async fn same_sender_msg_id_same_chat_is_duplicate() {
    let backend = setup();
    let StoreOutcome::Stored(first) = store(&backend, "g", SMID, "AAEC").await else {
        panic!("first store must not be a duplicate");
    };
    let before = messages(&backend).await;

    let again = store(&backend, "g", SMID, "different body").await;
    assert_eq!(again, StoreOutcome::Duplicate(first));

    let after = messages(&backend).await;
    assert_eq!(
        after["state"], before["state"],
        "a duplicate writes nothing"
    );
    assert_eq!(after["list"], before["list"]);
}

/// chat-00 L593: the `senderMsgId` index is "per chat", so the same
/// value in another Chat is a new Message.
#[tokio::test]
async fn same_sender_msg_id_other_chat_stores() {
    let backend = setup();
    let StoreOutcome::Stored(in_g) = store(&backend, "g", SMID, "AAEC").await else {
        panic!("first store must not be a duplicate");
    };
    let StoreOutcome::Stored(in_h) = store(&backend, "h", SMID, "AAEC").await else {
        panic!("another Chat is not a duplicate");
    };
    assert_ne!(in_g, in_h);
    assert_eq!(
        messages(&backend).await["list"].as_array().map(Vec::len),
        Some(2)
    );
}

/// chat-00 L909: the first delivered message creates the owner's
/// ReadPosition for a group Chat; a second message does not add another.
/// chat-00 L548, §Chat `unreadCount`: "If `lastReadMessageId` is
/// absent, all messages are counted as unread."
#[tokio::test]
async fn first_message_creates_read_position() {
    let backend = setup();
    assert!(positions(&backend).await.is_empty());

    store(&backend, "g", SMID, "AAEC").await;
    let list = positions(&backend).await;
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["chatId"], json!("g"));

    store(&backend, "g", "01J00000000000000000000002", "AAED").await;
    assert_eq!(positions(&backend).await.len(), 1);

    let HandlerOutput { primary, .. } = handle_chat_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": ["g"] }),
    )
    .await
    .expect("Chat/get");
    assert_eq!(primary["list"][0]["unreadCount"], json!(2));
}

/// federation L341: a `senderMsgId` "already known for the given chat"
/// resolves to the stored Message; the same value in another Chat does
/// not.
#[tokio::test]
async fn lookup_by_chat_and_sender_msg_id() {
    let backend = setup();
    let StoreOutcome::Stored(id) = store(&backend, "g", SMID, "AAEC").await else {
        panic!("first store must not be a duplicate");
    };
    let account = Id::from(ACCOUNT_ID);
    let smid = Id::from(SMID);

    let found = find_message_by_sender_msg_id(&backend, &(), &account, &Id::from("g"), &smid)
        .await
        .expect("lookup");
    assert_eq!(found.map(|m| m.id), Some(id));

    let other = find_message_by_sender_msg_id(&backend, &(), &account, &Id::from("h"), &smid)
        .await
        .expect("lookup");
    assert!(other.is_none());
}

/// draft-atwood-jmap-chat-00 L677 (jmap-chat-spec 29e9a41): "The receiving
/// mailbox stores both its own `id` and the `senderMsgId`." Only an
/// owner-composed Message has `senderMsgId` equal to `id` (L687), so a
/// server-originated Message whose originator chose the `senderMsgId`
/// `"placeholder"` keeps it and gets its own distinct `id` (bd:JMAP-dpbp).
#[tokio::test]
async fn placeholder_sender_msg_id_is_stored_verbatim() {
    let backend = setup();
    let StoreOutcome::Stored(id) = store(&backend, "g", "placeholder", "AAEC").await else {
        panic!("first store must not be a duplicate");
    };
    let list = messages(&backend).await["list"].clone();
    assert_eq!(list.as_array().map(Vec::len), Some(1), "{list}");
    let m = &list[0];
    assert_eq!(m["senderMsgId"], json!("placeholder"), "{m}");
    assert_eq!(m["id"], json!(id.as_ref()), "{m}");
    assert_ne!(m["id"], m["senderMsgId"], "{m}");
}
