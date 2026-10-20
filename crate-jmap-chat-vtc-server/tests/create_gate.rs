//! The Chat rules of `VTCCall/set` create: `chatId` access and
//! membership, and the `start_call` gate.
//!
//! Oracle: vtc L652 (§Creating a Room Call): "When the call carries a
//! `spaceId` or `channelId` binding and {{JMAP-CHAT}} is present, the
//! server MUST verify the caller has the `"start_call"` permission in the
//! target Space or channel; if not, the server MUST return `forbidden`.
//! ... When `chatId` is supplied but does not resolve to a Chat object the
//! caller has access to, the server MUST return `invalidProperties` naming
//! `chatId`. The caller MUST be a member of the Chat referenced by
//! `chatId`. The server MUST return `forbidden` if the caller is not a
//! member." Permission resolution: chat L1621-1625.

mod common;

use common::*;
use serde_json::json;

const SPACE: &str = "S1";
const CHANNEL: &str = "CH1";

fn room_in_channel() -> serde_json::Value {
    json!({ "callType": "room", "mediaTypes": ["audio"], "spaceId": SPACE, "channelId": CHANNEL })
}

#[tokio::test]
async fn unresolved_chat_id_is_invalid_properties() {
    let backend = Bound::new();
    let err = create_call_error(
        &backend,
        json!({ "callType": "room", "mediaTypes": ["audio"], "chatId": "no-such-chat" }),
    )
    .await;
    assert_eq!(err["type"], "invalidProperties");
    assert_eq!(err["properties"], json!(["chatId"]));
}

#[tokio::test]
async fn non_member_of_the_chat_is_forbidden() {
    let backend = Bound::new();
    backend.seed_chat_object("Chat", group_chat("chat1", &["c-bob", "c-alice"]));
    let err = create_call_error(
        &backend,
        json!({ "callType": "room", "mediaTypes": ["audio"], "chatId": "chat1" }),
    )
    .await;
    assert_eq!(err["type"], "forbidden");
}

/// chat L1621: "Compute the union of `permissions` across all SpaceRoles
/// held by the member".
#[tokio::test]
async fn role_granting_start_call_permits_the_create() {
    let backend = Bound::new();
    backend.seed_chat_object(
        "Space",
        space(SPACE, &["start_call"], &[(ACCOUNT, &["R-CALLER"])]),
    );
    backend.seed_chat_object("Chat", channel_chat(CHANNEL, SPACE, json!([])));
    create_call(&backend, room_in_channel()).await;
}

#[tokio::test]
async fn member_without_start_call_is_forbidden() {
    let backend = Bound::new();
    backend.seed_chat_object(
        "Space",
        space(SPACE, &["view"], &[(ACCOUNT, &["R-CALLER"])]),
    );
    backend.seed_chat_object("Chat", channel_chat(CHANNEL, SPACE, json!([])));
    let err = create_call_error(&backend, room_in_channel()).await;
    assert_eq!(err["type"], "forbidden");
}

/// chat L1621 includes "the implicit `@everyone` role" in the union.
#[tokio::test]
async fn everyone_floor_granting_start_call_permits_the_create() {
    let mut backend = Bound::new();
    backend.everyone.insert("start_call".to_owned());
    backend.seed_chat_object("Space", space(SPACE, &[], &[(ACCOUNT, &[])]));
    create_call(
        &backend,
        json!({ "callType": "room", "mediaTypes": ["audio"], "spaceId": SPACE }),
    )
    .await;
}

/// chat L1624: "Apply `deny` entries from any ChannelPermission record
/// matching the member directly (`targetType: "member"`)."
#[tokio::test]
async fn channel_override_denying_start_call_is_forbidden() {
    let backend = Bound::new();
    backend.seed_chat_object(
        "Space",
        space(SPACE, &["start_call"], &[(ACCOUNT, &["R-CALLER"])]),
    );
    backend.seed_chat_object(
        "Chat",
        channel_chat(
            CHANNEL,
            SPACE,
            json!([{ "targetId": ACCOUNT, "targetType": "member", "allow": [], "deny": ["start_call"] }]),
        ),
    );
    let err = create_call_error(&backend, room_in_channel()).await;
    assert_eq!(err["type"], "forbidden");
}

/// The permission is held "in the bound Space or channel", so a caller who
/// is not a member of the Space holds none.
#[tokio::test]
async fn non_member_of_the_space_is_forbidden() {
    let mut backend = Bound::new();
    backend.everyone.insert("start_call".to_owned());
    backend.seed_chat_object(
        "Space",
        space(SPACE, &["start_call"], &[("c-bob", &["R-CALLER"])]),
    );
    let err = create_call_error(
        &backend,
        json!({ "callType": "room", "mediaTypes": ["audio"], "spaceId": SPACE }),
    )
    .await;
    assert_eq!(err["type"], "forbidden");
}
