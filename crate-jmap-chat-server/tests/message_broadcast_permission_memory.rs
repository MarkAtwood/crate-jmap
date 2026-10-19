//! The reference `MemoryBackend` enforces the `"mention_broadcast"`
//! Space permission on channel `Message/set` create and update
//! (bd:JMAP-yt1w.35), checked against the owner's member entry, whose id
//! is the accountId (draft §Terminology).
//!
//! Oracle: draft-atwood-jmap-chat-00 §BroadcastMention and §Message/set,
//! quoted on each test.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_message_set, handle_space_get, handle_space_set};
use serde_json::{json, Value};

/// Create a Space and one channel in it through the handlers, returning
/// `(space_id, channel_id)`. Space/set create gives the owner a role
/// holding every Space permission, `"mention_broadcast"` included.
async fn create_channel(backend: &MemoryBackend) -> (String, String) {
    let (resp, _) = handle_space_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": { "s": { "name": "S" } } }),
    )
    .await
    .expect("Space/set create");
    let space_id = resp["created"]["s"]["id"]
        .as_str()
        .expect("space id")
        .to_owned();
    handle_space_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { &space_id: { "addChannels": [{ "name": "general" }] } }
        }),
    )
    .await
    .expect("Space/set addChannels");
    let channel_id = space(backend, &space_id).await["uncategorizedChannelIds"][0]
        .as_str()
        .expect("channel id")
        .to_owned();
    (space_id, channel_id)
}

async fn space(backend: &MemoryBackend, space_id: &str) -> Value {
    let (resp, _) = handle_space_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [space_id] }),
    )
    .await
    .expect("Space/get");
    resp["list"][0].clone()
}

/// Remove `"mention_broadcast"` from every role in the Space, so the
/// owner keeps its membership but lacks that one permission.
async fn revoke_mention_broadcast(backend: &MemoryBackend, space_id: &str) {
    let mut space = space(backend, space_id).await;
    for role in space["roles"].as_array_mut().expect("roles") {
        role["permissions"]
            .as_array_mut()
            .expect("permissions")
            .retain(|p| p != "mention_broadcast");
    }
    backend.insert_object_for_test("Space", ACCOUNT_ID, space_id, space);
}

async fn message_set(backend: &MemoryBackend, body: Value) -> Value {
    let mut args = json!({ "accountId": ACCOUNT_ID });
    let (Value::Object(args_map), Value::Object(body_map)) = (&mut args, body) else {
        unreachable!("both are object literals");
    };
    args_map.extend(body_map);
    let (resp, _) = handle_message_set(backend, &(), args)
        .await
        .expect("Message/set");
    resp
}

fn broadcast_message(chat_id: &str) -> Value {
    json!({
        "chatId": chat_id,
        "body": "hello all",
        "sentAt": "2026-01-01T00:00:00Z",
        "broadcastMentions": [{ "scope": "everyone" }]
    })
}

/// draft-atwood-jmap-chat-00 §BroadcastMention: "Sending a Message with a
/// non-empty `broadcastMentions` array requires the `"mention_broadcast"`
/// Space permission. Servers MUST reject `Message/set` create or update
/// requests that would write a broadcast mention from a sender lacking
/// this permission with `forbidden`." §Message/set: "A create request
/// with a non-empty `broadcastMentions` array MUST be rejected with
/// `forbidden` when the target is a channel Chat and the sender lacks the
/// `"mention_broadcast"` permission in its Space."
#[tokio::test]
async fn create_in_channel_without_mention_broadcast_is_forbidden() {
    let backend = MemoryBackend::new();
    let (space_id, channel_id) = create_channel(&backend).await;

    let allowed = message_set(
        &backend,
        json!({ "create": { "m": broadcast_message(&channel_id) } }),
    )
    .await;
    assert!(allowed["created"]["m"].is_object(), "{allowed:?}");

    revoke_mention_broadcast(&backend, &space_id).await;
    let denied = message_set(
        &backend,
        json!({ "create": { "m": broadcast_message(&channel_id) } }),
    )
    .await;
    assert_eq!(denied["created"], Value::Null, "{denied:?}");
    assert_eq!(denied["notCreated"]["m"]["type"], "forbidden", "{denied:?}");
}

/// draft-atwood-jmap-chat-00 §Message/set `update`: "The same
/// `"mention_broadcast"` permission check applied at create time applies
/// to updates that add or retain a broadcast mention."
#[tokio::test]
async fn update_retaining_broadcast_without_mention_broadcast_is_forbidden() {
    let backend = MemoryBackend::new();
    let (space_id, channel_id) = create_channel(&backend).await;
    let created = message_set(
        &backend,
        json!({ "create": { "m": broadcast_message(&channel_id) } }),
    )
    .await;
    let msg_id = created["created"]["m"]["id"]
        .as_str()
        .expect("message id")
        .to_owned();

    revoke_mention_broadcast(&backend, &space_id).await;
    let denied = message_set(
        &backend,
        json!({ "update": { &msg_id: { "body": "hello again" } } }),
    )
    .await;
    assert_eq!(denied["updated"], Value::Null, "{denied:?}");
    assert_eq!(
        denied["notUpdated"][&msg_id]["type"], "forbidden",
        "{denied:?}"
    );
}

/// draft-atwood-jmap-chat-00 §Message/set `update`: "The same
/// `"mention_broadcast"` permission check applied at create time applies
/// to updates that add or retain a broadcast mention."
#[tokio::test]
async fn update_adding_broadcast_without_mention_broadcast_is_forbidden() {
    let backend = MemoryBackend::new();
    let (space_id, channel_id) = create_channel(&backend).await;
    revoke_mention_broadcast(&backend, &space_id).await;
    let created = message_set(
        &backend,
        json!({ "create": { "m": {
            "chatId": channel_id,
            "body": "hello",
            "sentAt": "2026-01-01T00:00:00Z"
        } } }),
    )
    .await;
    let msg_id = created["created"]["m"]["id"]
        .as_str()
        .expect("message id")
        .to_owned();

    let denied = message_set(
        &backend,
        json!({ "update": { &msg_id: { "broadcastMentions": [{ "scope": "here" }] } } }),
    )
    .await;
    assert_eq!(denied["updated"], Value::Null, "{denied:?}");
    assert_eq!(
        denied["notUpdated"][&msg_id]["type"], "forbidden",
        "{denied:?}"
    );
}
