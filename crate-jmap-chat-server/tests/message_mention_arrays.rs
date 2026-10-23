//! `mentions` and `broadcastMentions` are authoritative for every
//! `bodyType`, carry no position in `body`, and rich-body spans are
//! display-only (bd:JMAP-yt1w.17).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 at jmap-chat-spec
//! 0983787. The `TrackingBackend` denies `may_broadcast_mention`;
//! channels are created through its permissive inner `MemoryBackend`,
//! which shares storage.

mod common;

use common::{ACCOUNT_ID, MemoryBackend, TrackingBackend, make_chat_group};
use jmap_chat_server::message::{handle_message_get, handle_message_set};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

const RICH: &str = "application/jmap-chat-rich";
const RICH_BROADCAST: &str = r#"{"spans":[{"type":"text","text":"hi "},{"type":"broadcast","text":"@here","scope":"here"}]}"#;
const RICH_MENTION: &str =
    r#"{"spans":[{"type":"text","text":"hi "},{"type":"mention","text":"@u1","userId":"u1"}]}"#;

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
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "create": { "m": message } }),
    )
    .await
    .expect("Message/set create");
    resp
}

fn created_id(resp: &Value) -> String {
    resp["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("message not created: {resp}"))
        .to_owned()
}

async fn get_message(backend: &MemoryBackend, msg_id: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [msg_id] }),
    )
    .await
    .expect("Message/get");
    resp["list"][0].clone()
}

/// §Message/set, Creating a Message: "A create request with a non-empty
/// `broadcastMentions` array MUST be rejected with `forbidden` when the
/// target is a channel Chat and the sender lacks the `"mention_broadcast"`
/// permission in its Space."
///
/// §BroadcastMention: "The `broadcastMentions` array is authoritative for
/// every `bodyType`". A rich body is therefore gated on the array, not
/// rejected for carrying it.
#[tokio::test]
async fn create_rich_body_with_broadcast_mentions_in_channel_is_forbidden() {
    let backend = TrackingBackend::with_broadcast_mention_denied();
    let chat_id = create_channel(backend.inner()).await;
    let resp = create_on(
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
    assert_eq!(resp["notCreated"]["m"]["type"], "forbidden", "{resp}");
}

/// §Rich Body Format: "A client composing a rich body MUST list ... each
/// scope it marks with a `"broadcast"` span in `broadcastMentions`
/// ({{broadcast-mention}}); only those arrays take effect."
///
/// §BroadcastMention: "Like Mention, a BroadcastMention carries no
/// position in `body`." A permitted sender's rich body with a positionless
/// `broadcastMentions` entry is accepted and stored as sent.
#[tokio::test]
async fn create_rich_body_with_broadcast_mentions_when_permitted_is_stored() {
    let backend = common::account_backend();
    let chat_id = create_channel(&backend).await;
    let resp = create_on(
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
    let msg_id = created_id(&resp);
    let msg = get_message(&backend, &msg_id).await;
    assert_eq!(
        msg["broadcastMentions"],
        json!([{ "scope": "here" }]),
        "{msg}"
    );
}

/// §Rich Body Format: "Spans are presentation only: servers MUST NOT make
/// any authorization, notification, push, query, or delivery decision from
/// span content." A `"broadcast"` span with an empty `broadcastMentions`
/// array is not a broadcast mention, so the permission gate does not apply.
#[tokio::test]
async fn create_rich_broadcast_span_without_array_is_not_gated() {
    let backend = TrackingBackend::with_broadcast_mention_denied();
    let chat_id = create_channel(backend.inner()).await;
    let resp = create_on(
        &backend,
        json!({
            "chatId": chat_id,
            "body": RICH_BROADCAST,
            "bodyType": RICH,
            "sentAt": "2024-01-01T00:00:00Z"
        }),
    )
    .await;
    created_id(&resp);
}

/// §Rich Body Format: "Spans are presentation only: servers MUST NOT make
/// any authorization, notification, push, query, or delivery decision from
/// span content." An edit that only adds a `"broadcast"` span neither adds
/// nor retains a broadcast mention (§Editing a Message).
#[tokio::test]
async fn edit_adding_rich_broadcast_span_without_array_is_not_gated() {
    let backend = TrackingBackend::with_broadcast_mention_denied();
    let chat_id = create_channel(backend.inner()).await;
    let msg_id = created_id(
        &create_on(
            backend.inner(),
            json!({ "chatId": chat_id, "body": "hi", "sentAt": "2024-01-01T00:00:00Z" }),
        )
        .await,
    );
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { &msg_id: { "body": RICH_BROADCAST, "bodyType": RICH } }
        }),
    )
    .await
    .expect("Message/set update");
    assert!(
        resp["updated"]
            .as_object()
            .is_some_and(|u| u.contains_key(&msg_id)),
        "{resp}"
    );
}

/// §Mention: "A Mention carries no position in `body`. ... The `mentions`
/// array is authoritative for every `bodyType`". Positionless `mentions`
/// and `broadcastMentions` on a plain body are accepted and stored as
/// sent, with no `offset` or `length` added.
#[tokio::test]
async fn create_plain_body_with_positionless_mentions_is_stored() {
    let backend = common::account_backend();
    let chat_id = make_chat_group(&backend, "G").await;
    let resp = create_on(
        &backend,
        json!({
            "chatId": chat_id,
            "body": "hi @u1 @everyone",
            "mentions": [{ "id": "u1" }],
            "broadcastMentions": [{ "scope": "everyone" }],
            "sentAt": "2024-01-01T00:00:00Z"
        }),
    )
    .await;
    let msg_id = created_id(&resp);
    let msg = get_message(&backend, &msg_id).await;
    assert_eq!(msg["mentions"], json!([{ "id": "u1" }]), "{msg}");
    assert_eq!(
        msg["broadcastMentions"],
        json!([{ "scope": "everyone" }]),
        "{msg}"
    );
}

/// §Mention: "The `mentions` array is authoritative for every `bodyType`:
/// servers MUST take mentions only from it, for mention-based push,
/// notification, and query behavior". A rich body listing its `"mention"`
/// span user in `mentions` is accepted and the array is stored as sent.
#[tokio::test]
async fn create_rich_body_with_mentions_is_stored() {
    let backend = common::account_backend();
    let chat_id = make_chat_group(&backend, "G").await;
    let resp = create_on(
        &backend,
        json!({
            "chatId": chat_id,
            "body": RICH_MENTION,
            "bodyType": RICH,
            "mentions": [{ "id": "u1" }],
            "sentAt": "2024-01-01T00:00:00Z"
        }),
    )
    .await;
    let msg_id = created_id(&resp);
    let msg = get_message(&backend, &msg_id).await;
    assert_eq!(msg["mentions"], json!([{ "id": "u1" }]), "{msg}");
}
