//! Message/set applies the `"mention_broadcast"` gate to edits that add
//! or retain a broadcast mention (bd:JMAP-yt1w.8). Rich-body spans are
//! display-only and never gated (bd:JMAP-yt1w.17; see
//! `message_mention_arrays.rs`).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 §Message/set. The
//! `TrackingBackend` denies `may_broadcast_mention`; messages are seeded
//! through its inner `MemoryBackend`, which shares storage and permits
//! them because Space/set create gives the owner every permission.

mod common;

use common::{ACCOUNT_ID, MemoryBackend, TrackingBackend};
use jmap_chat_server::message::handle_message_set;
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

const RICH: &str = "application/jmap-chat-rich";
const RICH_BROADCAST: &str = r#"{"spans":[{"type":"text","text":"hi "},{"type":"broadcast","text":"@here","scope":"here"}]}"#;
const RICH_PLAIN: &str = r#"{"spans":[{"type":"text","text":"hi"}]}"#;

async fn create_channel(backend: &MemoryBackend) -> String {
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": { "s": { "name": "S" } } }),
    )
    .await
    .expect("Space/set create");
    let space_id = resp["created"]["s"]["id"].as_str().expect("space id");
    handle_space_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { space_id: { "addChannels": [{ "name": "general" }] } }
        }),
    )
    .await
    .expect("Space/set addChannels");
    let HandlerOutput { primary: resp, .. } = handle_space_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [space_id] }),
    )
    .await
    .expect("Space/get");
    resp["list"][0]["uncategorizedChannelIds"][0]
        .as_str()
        .expect("channel id")
        .to_owned()
}

async fn create_on<B: jmap_chat_server::ChatBackend<CallerCtx = ()>>(
    backend: &B,
    message: Value,
) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": { "m": message } }),
    )
    .await
    .expect("Message/set create");
    resp
}

async fn update(backend: &TrackingBackend, msg_id: &str, patch: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { msg_id: patch } }),
    )
    .await
    .expect("Message/set update");
    resp
}

/// Seed a message through the permissive inner backend and return its id.
async fn seed(backend: &TrackingBackend, message: Value) -> String {
    let resp = create_on(backend.inner(), message).await;
    resp["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("seed message: {resp}"))
        .to_owned()
}

/// "A create request with a non-empty `broadcastMentions` array MUST be
/// rejected with `forbidden` when the target is a channel Chat and the
/// sender lacks the `"mention_broadcast"` permission in its Space." Only
/// such a create is gated, so a rich body without one is accepted.
#[tokio::test]
async fn create_rich_body_without_broadcast_span_is_accepted() {
    let backend = TrackingBackend::with_broadcast_mention_denied();
    let chat_id = create_channel(backend.inner()).await;
    let resp = create_on(
        &backend,
        json!({
            "chatId": chat_id,
            "body": RICH_PLAIN,
            "bodyType": RICH,
            "sentAt": "2024-01-01T00:00:00Z"
        }),
    )
    .await;
    assert!(resp["created"]["m"].is_object(), "{resp}");
}

/// "The same `"mention_broadcast"` permission check applied at create time
/// applies to updates that add or retain a broadcast mention."
#[tokio::test]
async fn edit_retaining_broadcast_mentions_is_forbidden() {
    let backend = TrackingBackend::with_broadcast_mention_denied();
    let chat_id = create_channel(backend.inner()).await;
    let msg_id = seed(
        &backend,
        json!({ "chatId": chat_id, "body": "hi @everyone", "sentAt": "2024-01-01T00:00:00Z" }),
    )
    .await;
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend.inner(),
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { &msg_id: {
                "broadcastMentions": [{ "scope": "everyone", "offset": 3, "length": 9 }]
            } }
        }),
    )
    .await
    .expect("seed broadcastMentions");
    assert!(resp["notUpdated"].is_null(), "{resp}");
    let resp = update(&backend, &msg_id, json!({ "body": "hi @everyone!" })).await;
    assert_eq!(resp["notUpdated"][&msg_id]["type"], "forbidden", "{resp}");
}

/// "The same `"mention_broadcast"` permission check applied at create time
/// applies to updates that add or retain a broadcast mention." An edit
/// that removes the only broadcast mention does neither. §BroadcastMention:
/// "The `broadcastMentions` array is authoritative for every `bodyType`",
/// so the mention is seeded and removed through the array.
#[tokio::test]
async fn edit_removing_broadcast_mention_is_accepted() {
    let backend = TrackingBackend::with_broadcast_mention_denied();
    let chat_id = create_channel(backend.inner()).await;
    let msg_id = seed(
        &backend,
        json!({
            "chatId": chat_id,
            "body": RICH_BROADCAST,
            "bodyType": RICH,
            "broadcastMentions": [{ "scope": "here" }],
            "sentAt": "2024-01-01T00:00:00Z"
        }),
    )
    .await;
    let resp = update(
        &backend,
        &msg_id,
        json!({ "body": RICH_PLAIN, "broadcastMentions": [] }),
    )
    .await;
    assert!(
        resp["updated"]
            .as_object()
            .is_some_and(|u| u.contains_key(&msg_id)),
        "{resp}"
    );
}
