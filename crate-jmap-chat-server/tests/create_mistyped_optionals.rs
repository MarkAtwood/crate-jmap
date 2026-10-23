//! `/set` create rejects a wrongly-typed optional property instead of
//! silently treating it as absent (bd:JMAP-q2wa.7.1).
//!
//! Oracle: RFC 8620 §5.3 defines the `invalidProperties` SetError as "The
//! record given is invalid in some way", with "a property called
//! `properties` with the list of invalid properties". The property types
//! come from draft-atwood-jmap-chat-00, e.g. §SpaceBan "`reason` (String,
//! optional)", §SpaceInvite "`maxUses` (UnsignedInt, optional)", §Space
//! "`isPublic` (Boolean)", and §Chat "`muted` (Boolean)". RFC 8620 §1.3:
//! "UnsignedInt ... where the value MUST be in the range 0 <= value <=
//! 2^53-1."

mod common;

use common::{ACCOUNT_ID, make_space};
use jmap_chat_server::message::handle_message_set;
use jmap_chat_server::{
    handle_ban_set, handle_chat_set, handle_emoji_set, handle_invite_set, handle_space_set,
};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

/// The SetError RFC 8620 §5.3 requires, naming exactly `props`.
fn invalid(props: &[&str]) -> Value {
    json!({ "type": "invalidProperties", "properties": props })
}

fn args(obj: Value) -> Value {
    json!({ "accountId": ACCOUNT_ID, "create": { "c": obj } })
}

#[tokio::test]
async fn ban_rejects_non_string_reason_and_expires_at() {
    let backend = common::account_backend();
    let HandlerOutput { primary: resp, .. } = handle_ban_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        args(json!({ "spaceId": "s", "userId": "u", "reason": 42, "expiresAt": 5 })),
    )
    .await
    .expect("SpaceBan/set");
    assert_eq!(resp["notCreated"]["c"], invalid(&["reason", "expiresAt"]));
}

#[tokio::test]
async fn invite_rejects_negative_max_uses() {
    let backend = common::account_backend();
    let HandlerOutput { primary: resp, .. } = handle_invite_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        args(json!({ "spaceId": "s", "maxUses": -1 })),
    )
    .await
    .expect("SpaceInvite/set");
    assert_eq!(resp["notCreated"]["c"], invalid(&["maxUses"]));
}

#[tokio::test]
async fn invite_rejects_max_uses_above_unsigned_int_range() {
    let backend = common::account_backend();
    let HandlerOutput { primary: resp, .. } = handle_invite_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        args(json!({ "spaceId": "s", "maxUses": 9_007_199_254_740_992_u64 })),
    )
    .await
    .expect("SpaceInvite/set");
    assert_eq!(resp["notCreated"]["c"], invalid(&["maxUses"]));
}

#[tokio::test]
async fn invite_rejects_non_string_default_channel_and_expires_at() {
    let backend = common::account_backend();
    let HandlerOutput { primary: resp, .. } = handle_invite_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        args(json!({ "spaceId": "s", "defaultChannelId": 7, "expiresAt": true })),
    )
    .await
    .expect("SpaceInvite/set");
    assert_eq!(
        resp["notCreated"]["c"],
        invalid(&["defaultChannelId", "expiresAt"])
    );
}

/// 2^53-1 is the top of the UnsignedInt range, so it is accepted.
#[tokio::test]
async fn invite_accepts_max_unsigned_int_max_uses() {
    let backend = common::account_backend();
    let space_id = make_space(&backend, "S").await;
    let HandlerOutput { primary: resp, .. } = handle_invite_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        args(json!({ "spaceId": space_id, "maxUses": 9_007_199_254_740_991_u64 })),
    )
    .await
    .expect("SpaceInvite/set");
    assert_eq!(
        resp["created"]["c"]["maxUses"],
        json!(9_007_199_254_740_991_u64),
        "response: {resp}"
    );
}

#[tokio::test]
async fn space_rejects_mistyped_optionals() {
    let backend = common::account_backend();
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        args(json!({
            "name": "S",
            "description": 42,
            "isPublic": "yes",
            "isPubliclyPreviewable": 1,
        })),
    )
    .await
    .expect("Space/set");
    assert_eq!(
        resp["notCreated"]["c"],
        invalid(&["description", "isPublic", "isPubliclyPreviewable"])
    );
}

/// `null` for an optional property is read as absent, so the create
/// succeeds with the defaults (§Space `isPublic`: "Default is `false`").
#[tokio::test]
async fn space_accepts_null_optionals() {
    let backend = common::account_backend();
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        args(json!({ "name": "S", "description": null, "isPublic": null })),
    )
    .await
    .expect("Space/set");
    assert_eq!(resp["created"]["c"]["isPublic"], json!(false), "{resp}");
    assert!(resp["notCreated"]["c"].is_null(), "{resp}");
}

#[tokio::test]
async fn chat_rejects_mistyped_optionals() {
    let backend = common::account_backend();
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        args(json!({
            "kind": "group",
            "name": ["G"],
            "description": 1,
            "muted": "no",
            "receiveTypingIndicators": 0,
        })),
    )
    .await
    .expect("Chat/set");
    assert_eq!(
        resp["notCreated"]["c"],
        invalid(&["name", "description", "muted", "receiveTypingIndicators"])
    );
}

#[tokio::test]
async fn chat_rejects_non_string_contact_and_space_ids() {
    let backend = common::account_backend();
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        args(json!({ "kind": "direct", "contactId": 3, "spaceId": false })),
    )
    .await
    .expect("Chat/set");
    assert_eq!(resp["notCreated"]["c"], invalid(&["contactId", "spaceId"]));
}

#[tokio::test]
async fn message_rejects_mistyped_optionals() {
    let backend = common::account_backend();
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        args(json!({
            "chatId": "x",
            "body": "hi",
            "sentAt": "2024-01-01T00:00:00Z",
            "broadcastMentions": {},
            "replyTo": 1,
            "threadRootId": 2,
            "senderExpiresAt": 3,
            "burnOnRead": "true",
        })),
    )
    .await
    .expect("Message/set");
    assert_eq!(
        resp["notCreated"]["c"],
        invalid(&[
            "broadcastMentions",
            "replyTo",
            "threadRootId",
            "senderExpiresAt",
            "burnOnRead",
        ])
    );
}

#[tokio::test]
async fn emoji_rejects_non_string_space_id() {
    let backend = common::account_backend();
    let HandlerOutput { primary: resp, .. } = handle_emoji_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        args(json!({ "name": "party", "blobId": "b", "spaceId": 9 })),
    )
    .await
    .expect("CustomEmoji/set");
    assert_eq!(resp["notCreated"]["c"], invalid(&["spaceId"]));
}
