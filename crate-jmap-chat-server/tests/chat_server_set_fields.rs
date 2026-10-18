//! Server-set Chat fields are not `Chat/set` patch keys (bd:JMAP-yt1w.4).
//!
//! Oracles: draft-atwood-jmap-chat-00 §Chat `closedAt`: "`closedAt`
//! (UTCDate, optional, server-set) ... Once set, `closedAt` MUST NOT be
//! cleared." and §Chat `activeCallId`: "Clients cannot clear the field
//! themselves: `activeCallId` is not a `Chat/set` patch key."

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_chat_get, handle_chat_set};
use jmap_types::Id;
use serde_json::{json, Value};

const CHAT: &str = "chat1";
const CLOSED_AT: &str = "2026-02-01T00:00:00Z";

/// Store a group Chat the server has already closed, with an active call.
fn seed_closed_group(backend: &MemoryBackend) {
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        CHAT,
        json!({
            "id": CHAT,
            "kind": "group",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "closedAt": CLOSED_AT,
            "activeCallId": "call1",
        }),
    );
}

async fn update(backend: &MemoryBackend, patch: Value) -> Value {
    let (resp, _) = handle_chat_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { CHAT: patch } }),
    )
    .await
    .expect("Chat/set");
    resp
}

async fn stored(backend: &MemoryBackend) -> Value {
    let (resp, _) = handle_chat_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [CHAT] }),
    )
    .await
    .expect("Chat/get");
    resp["list"][0].clone()
}

#[tokio::test]
async fn chat_set_rejects_server_set_closed_at_and_active_call_id() {
    let backend = MemoryBackend::new();
    seed_closed_group(&backend);

    for (key, value) in [
        ("closedAt", Value::Null),
        ("closedAt", json!("2026-03-01T00:00:00Z")),
        ("activeCallId", Value::Null),
        ("activeCallId", json!("call2")),
    ] {
        let resp = update(&backend, json!({ key: value })).await;
        assert_eq!(
            resp["notUpdated"][CHAT],
            json!({ "type": "invalidProperties", "properties": [key] }),
            "{key}: {value}: {resp}"
        );
        let chat = stored(&backend).await;
        assert_eq!(chat["closedAt"], CLOSED_AT, "{key}: {value}: {chat}");
        assert_eq!(chat["activeCallId"], "call1", "{key}: {value}: {chat}");
    }
}
