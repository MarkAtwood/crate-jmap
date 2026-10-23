//! `senderMsgId` of an owner-composed Message (bd:JMAP-pnci).
//!
//! Oracles, from jmap-chat-spec 33702a5:
//!
//! - draft-atwood-jmap-chat-00.md L686-687: "`senderMsgId` (String,
//!   immutable, server-set): The sender-assigned ULID carried in
//!   `Peer/deliver`. Equals `id` for messages composed by the owner."
//! - L677: "Servers MUST be able to resolve any stored `senderMsgId` to its
//!   Message within the chat".

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::message::{find_message_by_sender_msg_id, handle_message_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

fn setup() -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        "g",
        json!({
            "id": "g",
            "kind": "group",
            "name": "G",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "members": [{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }],
        }),
    );
    backend
}

async fn create_m1(backend: &MemoryBackend) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m1": {
                "chatId": "g",
                "body": "hi",
                "bodyType": "text/plain",
                "sentAt": "2026-01-02T00:00:00Z",
            } },
        }),
    )
    .await
    .expect("Message/set");
    resp["created"]["m1"].clone()
}

/// L687: the created Message's `senderMsgId` equals its `id`, not the
/// client creation id, so two requests reusing creation id `m1` yield
/// distinct `senderMsgId` values that each resolve to their own Message.
#[tokio::test]
async fn owner_composed_sender_msg_id_equals_id() {
    let backend = setup();
    let first = create_m1(&backend).await;
    let second = create_m1(&backend).await;
    for created in [&first, &second] {
        assert_eq!(created["senderMsgId"], created["id"], "{created}");
        let smid = Id::from(created["senderMsgId"].as_str().expect("senderMsgId"));
        let found = find_message_by_sender_msg_id(
            &backend,
            &(),
            &Id::from(ACCOUNT_ID),
            &Id::from("g"),
            &smid,
        )
        .await
        .expect("lookup")
        .expect("stored");
        assert_eq!(json!(found.id), created["id"]);
    }
    assert_ne!(first["id"], second["id"]);
    let by_creation_id = find_message_by_sender_msg_id(
        &backend,
        &(),
        &Id::from(ACCOUNT_ID),
        &Id::from("g"),
        &Id::from("m1"),
    )
    .await
    .expect("lookup");
    assert!(by_creation_id.is_none());
}
