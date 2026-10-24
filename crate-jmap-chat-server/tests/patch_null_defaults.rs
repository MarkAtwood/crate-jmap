//! `/set update` with a `null` PatchObject value resets a property to
//! its spec default (RFC 8620 §5.3, bd:JMAP-sofk).
//!
//! RFC 8620 §5.3: "null: The value to set for this property is the
//! default value if specified for this property". The in-memory backend
//! removes the member on `null`, so each property below must deserialize
//! to its draft-atwood-jmap-chat-00 default when absent; otherwise the
//! following `/get` fails as a whole.

mod common;

use common::{ACCOUNT_ID, MemoryBackend, make_chat_group, seed_group_chat};
use jmap_chat_server::message::{handle_message_get, handle_message_set};
use jmap_chat_server::{
    ChatBackend, handle_chat_get, handle_chat_set, handle_contact_get, handle_contact_set,
    handle_presence_get, handle_presence_set,
};
use jmap_chat_types::Message;
use jmap_server::HandlerOutput;
use jmap_types::{Id, PatchObject};
use serde_json::{Map, Value, json};

/// Assert a `/set` response updated `id` without error.
fn assert_updated(resp: &Value, id: &str) {
    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");
    assert!(
        resp["updated"]
            .as_object()
            .is_some_and(|m| m.contains_key(id)),
        "{resp:?}"
    );
}

async fn chat_update(backend: &MemoryBackend, id: &str, patch: Value) {
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { id: patch } }),
        &[],
    )
    .await
    .expect("Chat/set");
    assert_updated(&resp, id);
}

async fn chat_get(backend: &MemoryBackend, id: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_chat_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
        &[],
    )
    .await
    .expect("Chat/get");
    resp["list"][0].clone()
}

/// Oracle: draft-atwood-jmap-chat-00 Chat `muted`: "When `true`, push
/// notifications for this chat are suppressed. [...] Default: `false`."
#[tokio::test]
async fn chat_muted_null_resets_to_false() {
    let backend = common::account_backend();
    let id = make_chat_group(&backend, "g").await;
    chat_update(&backend, &id, json!({ "muted": true })).await;

    chat_update(&backend, &id, json!({ "muted": null })).await;

    assert_eq!(chat_get(&backend, &id).await["muted"], json!(false));
}

/// Oracle: draft-atwood-jmap-chat-00 Chat `pinnedMessageIds`: "Ordered
/// list of pinned Message ids, most-recently-pinned first. [...] Empty by
/// default."
#[tokio::test]
async fn chat_pinned_message_ids_null_resets_to_empty() {
    let backend = common::account_backend();
    let id = make_chat_group(&backend, "g").await;

    chat_update(&backend, &id, json!({ "pinnedMessageIds": null })).await;

    assert_eq!(chat_get(&backend, &id).await["pinnedMessageIds"], json!([]));
}

/// Oracle: draft-atwood-jmap-chat-00 Chat `receiveTypingIndicators`:
/// "Default: `true`."
#[tokio::test]
async fn chat_receive_typing_indicators_null_resets_to_true() {
    let backend = common::account_backend();
    let id = make_chat_group(&backend, "g").await;
    chat_update(&backend, &id, json!({ "receiveTypingIndicators": false })).await;

    chat_update(&backend, &id, json!({ "receiveTypingIndicators": null })).await;

    assert_eq!(
        chat_get(&backend, &id).await["receiveTypingIndicators"],
        json!(true)
    );
}

/// Oracle: draft-atwood-jmap-chat-00 Chat `receiptSharing` (line 645-646
/// at spec 29e9a41): "(Boolean, optional) Per-Chat override of the
/// account-level `PresenceStatus.receiptSharing` preference ... When
/// absent, the account-level preference applies." The property has no
/// default, so `null` removes the override and `/get` omits it
/// (bd:JMAP-w9ox.2).
#[tokio::test]
async fn chat_receipt_sharing_null_removes_override() {
    let backend = common::account_backend();
    let id = make_chat_group(&backend, "g").await;
    chat_update(&backend, &id, json!({ "receiptSharing": false })).await;
    assert_eq!(
        chat_get(&backend, &id).await["receiptSharing"],
        json!(false)
    );

    chat_update(&backend, &id, json!({ "receiptSharing": null })).await;

    let chat = chat_get(&backend, &id).await;
    assert!(
        chat.as_object()
            .is_some_and(|m| !m.contains_key("receiptSharing")),
        "{chat:?}"
    );
}

/// Oracle: draft-atwood-jmap-chat-00 ChatContact `blocked`: "When
/// `true`, messages from this ChatContact are silently dropped by this
/// mailbox [...] Default: `false`."
#[tokio::test]
async fn contact_blocked_null_resets_to_false() {
    let backend = common::account_backend();
    // ChatContact objects are server-managed (create is forbidden).
    backend.insert_object_for_test(
        "ChatContact",
        ACCOUNT_ID,
        "ct1",
        json!({
            "id": "ct1",
            "identity": "alice@example.com",
            "login": "alice@example.com",
            "firstSeenAt": "2024-01-01T00:00:00Z",
            "lastSeenAt": "2024-01-01T00:00:00Z",
            "blocked": true,
        }),
    );

    let HandlerOutput { primary: resp, .. } = handle_contact_set(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { "ct1": { "blocked": null } } }),
    )
    .await
    .expect("ChatContact/set");
    assert_updated(&resp, "ct1");

    let HandlerOutput { primary: resp, .. } = handle_contact_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": ["ct1"] }),
    )
    .await
    .expect("ChatContact/get");
    assert_eq!(resp["list"][0]["blocked"], json!(false), "{resp:?}");
}

/// Seed a PresenceStatus with non-default `presence` and `receiptSharing`.
fn seed_presence(backend: &MemoryBackend) {
    // PresenceStatus objects are server-managed (only update is permitted).
    backend.insert_object_for_test(
        "PresenceStatus",
        ACCOUNT_ID,
        "ps1",
        json!({
            "id": "ps1",
            "presence": "busy",
            "receiptSharing": false,
            "updatedAt": "2024-01-01T00:00:00Z",
        }),
    );
}

async fn presence_null_then_get(backend: &MemoryBackend, prop: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_presence_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { "ps1": { prop: null } } }),
    )
    .await
    .expect("PresenceStatus/set");
    assert_updated(&resp, "ps1");

    let HandlerOutput { primary: resp, .. } = handle_presence_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": ["ps1"] }),
    )
    .await
    .expect("PresenceStatus/get");
    resp["list"][0].clone()
}

/// Oracle: draft-atwood-jmap-chat-00 PresenceStatus `presence`: "One of
/// `"online"`, `"away"`, `"busy"`, `"invisible"`, or `"offline"`.
/// Default is `"online"`."
#[tokio::test]
async fn presence_null_resets_to_online() {
    let backend = common::account_backend();
    seed_presence(&backend);

    let ps = presence_null_then_get(&backend, "presence").await;

    assert_eq!(ps["presence"], json!("online"), "{ps:?}");
}

/// Oracle: draft-atwood-jmap-chat-00 PresenceStatus `receiptSharing`:
/// "When `false`, this account does not send read receipts to peers.
/// [...] Default: `true`."
#[tokio::test]
async fn presence_receipt_sharing_null_resets_to_true() {
    let backend = common::account_backend();
    seed_presence(&backend);

    let ps = presence_null_then_get(&backend, "receiptSharing").await;

    assert_eq!(ps["receiptSharing"], json!(true), "{ps:?}");
}

/// Create a Message, patch `prop` to null in the backend, and return the
/// fetched Message.
///
/// The patch goes to `update_object` directly rather than through
/// `Message/set`: draft-atwood-jmap-chat-00 §Message/set admits only the
/// Editing, Reactions (`reactions/<senderReactionId>`), Deleting and
/// Marking as Read updates, so the handler rejects a whole-property
/// `actions` or `reactions` patch. The default-on-absence contract under
/// test belongs to the stored record, not to the handler.
async fn message_null_then_get(prop: &str) -> Value {
    let backend = common::account_backend();
    seed_group_chat(&backend, "c1");
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m0": { "chatId": "c1", "body": "hi", "sentAt": "2024-01-01T00:00:00Z" } }
        }),
        &[],
    )
    .await
    .expect("Message/set create");
    let id = resp["created"]["m0"]["id"]
        .as_str()
        .expect("created id")
        .to_owned();

    let patch = PatchObject::from(Map::from_iter([(prop.to_owned(), Value::Null)]));
    backend
        .update_object::<Message>(&(), &Id::from(ACCOUNT_ID), &Id::from(id.as_str()), patch)
        .await
        .expect("update_object");

    let HandlerOutput { primary: resp, .. } = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [&id] }),
        &[],
    )
    .await
    .expect("Message/get");
    resp["list"][0].clone()
}

/// Oracle: draft-atwood-jmap-chat-00 Message `mentions`: "Structured
/// per-user @mention annotations. Empty by default."
#[tokio::test]
async fn message_mentions_null_resets_to_empty() {
    let msg = message_null_then_get("mentions").await;
    assert_eq!(msg["mentions"], json!([]), "{msg:?}");
}

/// Oracle: draft-atwood-jmap-chat-00 Message `actions`: "Out-of-band
/// action invitations associated with this message. Empty by default."
#[tokio::test]
async fn message_actions_null_resets_to_empty() {
    let msg = message_null_then_get("actions").await;
    assert_eq!(msg["actions"], json!([]), "{msg:?}");
}

/// Oracle: draft-atwood-jmap-chat-00 Message `reactions`: "Emoji
/// reactions, keyed by `senderReactionId`. [...] Empty object by
/// default."
#[tokio::test]
async fn message_reactions_null_resets_to_empty() {
    let msg = message_null_then_get("reactions").await;
    assert_eq!(msg["reactions"], json!({}), "{msg:?}");
}

/// Oracle: draft-atwood-jmap-chat-00 Message `broadcastMentions`:
/// "Structured broadcast-scope mention annotations [...]. Empty by
/// default."
#[tokio::test]
async fn message_broadcast_mentions_null_resets_to_empty() {
    let msg = message_null_then_get("broadcastMentions").await;
    assert_eq!(msg["broadcastMentions"], json!([]), "{msg:?}");
}
