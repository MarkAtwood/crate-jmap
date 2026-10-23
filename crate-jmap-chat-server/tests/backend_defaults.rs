//! The `ChatBackend` default method bodies, called directly on a backend
//! that overrides none of them (bd:JMAP-w8fu.5).
//!
//! [`FaultyBackend`] fails every `JmapBackend` storage call, so a default
//! that answers without error is shown not to consult storage, and a
//! default that reads storage is shown to propagate its error.
//!
//! Oracles: the "Default implementation" contract stated in each method's
//! rustdoc, and workspace decision JMAP-bghk ("permission/policy hooks
//! default fail-open"); the expected values below are written from that
//! text, not read from the code.

mod common;

use common::{ACCOUNT_ID, FaultyBackend, SPACE_ID};
use jmap_chat_server::{ChatBackend, EmojiSetOp, OutboundEvent};
use jmap_chat_types::{Chat, Message, SpaceBan, SpaceInvite};
use jmap_types::Id;
use serde_json::json;

const CHAT_ID: &str = "chat1";
const MESSAGE_ID: &str = "msg1";

fn account() -> Id {
    Id::from(ACCOUNT_ID)
}

fn chat() -> Chat {
    serde_json::from_value(json!({
        "id": CHAT_ID,
        "kind": "group",
        "name": "",
        "members": [],
        "createdAt": "2026-01-01T00:00:00Z",
        "unreadCount": 0,
        "pinnedMessageIds": [],
    }))
    .expect("Chat fixture")
}

fn message() -> Message {
    serde_json::from_value(json!({
        "id": MESSAGE_ID,
        "senderMsgId": MESSAGE_ID,
        "senderId": "self",
        "chatId": CHAT_ID,
        "body": "hello",
        "bodyType": "text/plain",
        "reactions": {},
        "attachments": [],
        "sentAt": "2026-01-01T00:00:00Z",
        "receivedAt": "2026-01-01T00:00:00Z",
        "deliveryState": "delivered",
    }))
    .expect("Message fixture")
}

fn invite() -> SpaceInvite {
    serde_json::from_value(json!({
        "id": "inv1",
        "code": "code-inv1",
        "spaceId": SPACE_ID,
        "createdBy": "other",
        "uses": 0,
        "createdAt": "2026-01-01T00:00:00Z",
    }))
    .expect("SpaceInvite fixture")
}

fn ban() -> SpaceBan {
    serde_json::from_value(json!({
        "id": "ban1",
        "spaceId": SPACE_ID,
        "userId": "target",
        "bannedBy": "banner",
        "createdAt": "2026-01-01T00:00:00Z",
    }))
    .expect("SpaceBan fixture")
}

/// JMAP-bghk: every `may_*` gate that returns `Result<(), SetError>`
/// defaults to permit (`Ok(Ok(()))`).
#[tokio::test]
async fn may_set_gates_default_to_permit() {
    let b = FaultyBackend;
    let a = account();
    let space = Id::from(SPACE_ID);
    let chat_id = Id::from(CHAT_ID);
    let (chat, message) = (chat(), message());

    for op in [EmojiSetOp::Create, EmojiSetOp::Update, EmojiSetOp::Destroy] {
        assert!(matches!(
            b.may_set_custom_emoji(&(), &a, None, op).await,
            Ok(Ok(()))
        ));
        assert!(matches!(
            b.may_set_custom_emoji(&(), &a, Some(&space), op).await,
            Ok(Ok(()))
        ));
    }
    assert!(matches!(
        b.may_broadcast_mention(&(), &a, &space, &chat_id).await,
        Ok(Ok(()))
    ));
    assert!(matches!(
        b.may_set_pinned_messages(&(), &a, &chat_id).await,
        Ok(Ok(()))
    ));
    assert!(matches!(
        b.may_set_space_ban(&(), &a, &space).await,
        Ok(Ok(()))
    ));
    assert!(matches!(
        b.may_set_space_invite(&(), &a, &space, None).await,
        Ok(Ok(()))
    ));
    assert!(matches!(
        b.may_set_space_invite(&(), &a, &space, Some(&Id::from("other")))
            .await,
        Ok(Ok(()))
    ));
    assert!(matches!(
        b.may_send_message(&(), &a, &chat_id).await,
        Ok(Ok(()))
    ));
    assert!(matches!(
        b.may_create_message(&(), &a, &chat).await,
        Ok(Ok(()))
    ));
    assert!(matches!(
        b.may_update_message(&(), &a, &chat, &message, &["body"])
            .await,
        Ok(Ok(()))
    ));
    assert!(matches!(
        b.may_update_chat(&(), &a, &chat, &["name"]).await,
        Ok(Ok(()))
    ));
}

/// JMAP-bghk; `may_get_space_invite` and `may_get_space_ban` rustdoc:
/// "Returns `Ok(true)`, so every invite [ban] is visible";
/// `may_view_chat` likewise defaults to visible.
#[tokio::test]
async fn visibility_gates_default_to_visible() {
    let b = FaultyBackend;
    let a = account();
    assert!(matches!(
        b.may_get_space_invite(&(), &a, &invite()).await,
        Ok(true)
    ));
    assert!(matches!(
        b.may_get_space_ban(&(), &a, &ban()).await,
        Ok(true)
    ));
    assert!(matches!(
        b.may_view_chat(&(), &a, &Id::from(CHAT_ID)).await,
        Ok(true)
    ));
}

/// `slow_mode_check` defaults to no slow mode and `expire_message` to a
/// no-op, both succeeding.
#[tokio::test]
async fn slow_mode_and_expiry_default_to_ok() {
    let b = FaultyBackend;
    let a = account();
    assert!(b.slow_mode_check(&(), &a, &Id::from(CHAT_ID)).await.is_ok());
    assert!(
        b.expire_message(&(), &a, &Id::from(MESSAGE_ID))
            .await
            .is_ok()
    );
}

/// The lifecycle and outbound callbacks default to no-ops that complete.
#[tokio::test]
async fn lifecycle_callbacks_default_to_no_op() {
    let b = FaultyBackend;
    let a = account();
    b.on_chat_destroyed(&(), &a, &Id::from(CHAT_ID)).await;
    b.on_space_destroyed(&(), &a, &Id::from(SPACE_ID)).await;
    b.on_space_ban_created(&(), &a, &ban()).await;
    let event = OutboundEvent::NewMessage {
        chat_id: Id::from(CHAT_ID),
        message_id: Id::from(MESSAGE_ID),
        sender_msg_id: Id::from(MESSAGE_ID),
    };
    b.on_outbound_event(&(), &a, &event).await;
}

/// Policy flags: `protect_last_admin` "Returns `true` — production-safe
/// by default"; `retains_edit_history` "Returns `false` — no retention";
/// `maintains_unread_count` "Returns `false`"; `owner_user_id` "Returns
/// `Ok(None)`: the owner's identity is unknown".
#[tokio::test]
async fn policy_flags_have_documented_defaults() {
    let b = FaultyBackend;
    let a = account();
    assert!(b.protect_last_admin(&(), &a));
    assert!(!b.retains_edit_history());
    assert!(!b.maintains_unread_count());
    assert!(matches!(b.owner_user_id(&(), &a).await, Ok(None)));
}

/// `is_contact_blocked` rustdoc: the default "Fetches `contact_id` from
/// `account_id` with [`JmapBackend::get_objects`]", so a storage failure
/// there surfaces as the hook's error rather than as "not blocked".
#[tokio::test]
async fn is_contact_blocked_default_propagates_storage_error() {
    let b = FaultyBackend;
    let result = b
        .is_contact_blocked(&(), &account(), &Id::from("contact1"))
        .await;
    assert!(result.is_err(), "{result:?}");
}
