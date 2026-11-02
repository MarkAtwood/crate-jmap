//! Server-set Chat fields are not `Chat/set` patch keys (bd:JMAP-yt1w.4).
//!
//! Oracles: draft-atwood-jmap-chat-00 §Chat `closedAt`: "`closedAt`
//! (UTCDate, optional, server-set) ... Once set, `closedAt` MUST NOT be
//! cleared." and §Chat `activeCallId`: "Clients cannot clear the field
//! themselves: `activeCallId` is not a `Chat/set` patch key." and
//! §Chat `activeCallAccountId`: "Like `activeCallId`, it is not a
//! `Chat/set` patch key."
//!
//! jmap-chat-spec 77c3474 draft-atwood-jmap-chat-00 L613-625:
//! "`categoryId` (String, optional, server-set)", "`position`
//! (UnsignedInt, optional, server-set)", "`topic` (String, optional,
//! server-set)" (L619), "`slowModeSeconds` (UnsignedInt, server-set)" and "`permissionOverrides` (ChannelPermission\[],
//! server-set)", each changed "through `Space/set` ..., not through
//! `Chat/set`" (bd:JMAP-t8ej.13). RFC 8620 §5.3 `invalidProperties`: "It
//! contains a property that may only be set by the server ... and is
//! different to the current value."

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::{handle_chat_get, handle_chat_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

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
            "activeCallAccountId": "shared1",
        }),
    );
}

async fn update(backend: &MemoryBackend, patch: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { CHAT: patch } }),
        &[],
    )
    .await
    .expect("Chat/set");
    resp
}

async fn stored(backend: &MemoryBackend) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_chat_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [CHAT] }),
        &[],
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
        ("activeCallAccountId", Value::Null),
        ("activeCallAccountId", json!("shared2")),
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
        assert_eq!(
            chat["activeCallAccountId"], "shared1",
            "{key}: {value}: {chat}"
        );
    }
}

#[tokio::test]
async fn chat_set_rejects_server_set_channel_fields() {
    let backend = MemoryBackend::new();
    seed_closed_group(&backend);
    let before = stored(&backend).await;

    for (key, value) in [
        ("categoryId", json!("cat1")),
        ("position", json!(3)),
        ("topic", json!("New topic")),
        ("slowModeSeconds", json!(30)),
        (
            "permissionOverrides",
            json!([{ "targetId": "r1", "targetType": "role", "allow": [], "deny": [] }]),
        ),
    ] {
        let resp = update(&backend, json!({ key: value })).await;
        assert_eq!(
            resp["notUpdated"][CHAT],
            json!({ "type": "invalidProperties", "properties": [key] }),
            "{key}: {resp}"
        );
        assert_eq!(stored(&backend).await, before, "{key}: Chat unchanged");
    }
}

/// Oracle: RFC 8620 §5.3: "Any server-set properties MAY be included in
/// the patch if their value is identical to the current server value
/// (before applying the patches to the object).  Otherwise, the update
/// MUST be rejected with an "invalidProperties" SetError." The repeated
/// values are the hand-written seed, so a patch of only those succeeds
/// (`updated` maps the id to null) with no write and no state change, and
/// the same values beside a client change let that change apply.
/// draft-atwood-jmap-chat-00 §Chat `closedAt`: "After it is set, the
/// server MUST reject with a `forbidden` SetError ... every `Chat/set`
/// update that changes ... shared group settings", identical values or
/// not.
#[tokio::test]
async fn chat_set_admits_identical_server_set_values() {
    let backend = MemoryBackend::new();
    seed_closed_group(&backend);
    let before = stored(&backend).await;
    let same = json!({
        "id": CHAT,
        "kind": "group",
        "createdAt": "2026-01-01T00:00:00Z",
        "closedAt": CLOSED_AT,
        "activeCallId": "call1",
        "activeCallAccountId": "shared1",
    });

    let resp = update(&backend, same.clone()).await;
    assert_eq!(resp["updated"], json!({ CHAT: null }), "{resp}");
    assert_eq!(resp["notUpdated"], Value::Null, "{resp}");
    assert_eq!(resp["newState"], resp["oldState"], "{resp}");
    assert_eq!(stored(&backend).await, before);

    let mut with_change = same.clone();
    with_change["muted"] = json!(true);
    let resp = update(&backend, with_change).await;
    assert!(resp["updated"].get(CHAT).is_some(), "{resp}");
    assert_eq!(stored(&backend).await["muted"], json!(true));

    let mut changed = same.clone();
    changed["activeCallId"] = json!("call2");
    let resp = update(&backend, changed).await;
    assert_eq!(
        resp["notUpdated"][CHAT],
        json!({ "type": "invalidProperties", "properties": ["activeCallId"] }),
        "{resp}"
    );

    let mut shared = same;
    shared["name"] = json!("Renamed");
    let resp = update(&backend, shared).await;
    assert_eq!(resp["notUpdated"][CHAT]["type"], "forbidden", "{resp}");
}
