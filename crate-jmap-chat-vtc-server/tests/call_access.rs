//! The Chat rules of per-call access.
//!
//! Oracle: vtc L926 at jmap-chat-spec 6c38e2f (§Joining a Room or
//! Scheduled Call): "When `urn:ietf:params:jmap:chat` is present, callers
//! without access include a caller who lacks `"view"` permission on the
//! channel of a Space-bound call, a caller who is not a member of the Chat
//! bound by `chatId`, and a caller whom the call initiator has blocked
//! (`ChatContact.blocked` is `true` on the initiator's contact record for
//! the caller)." vtc L1660 (§Chat-Bound Calls): "A call is visible to
//! members of the bound Chat or Space channel." Member references: chat
//! L551, "The participant's ChatContact.id, or the accountId for the owner";
//! chat L587, `contactId` of a direct Chat is
//! "The ChatContact.id of the other participant".

mod common;

use common::*;
use jmap_chat_vtc_server::may_access_call;
use jmap_types::Id;
use jmap_vtc_types::VtcCall;
use serde_json::{Value, json};
use std::collections::HashSet;

const SPACE: &str = "S1";
const CHANNEL: &str = "CH1";

/// An active room call initiated by the owner, with `binding` (`chatId`,
/// `spaceId`, `channelId`) merged in.
fn call(binding: Value) -> VtcCall {
    let mut v = json!({
        "id": "C1",
        "callType": "room",
        "state": "active",
        "createdAt": "2026-01-01T00:00:00Z",
        "endedAt": null,
        "endReason": null,
        "initiatorId": OWNER,
        "subject": null,
        "joinUri": "https://vtc.example.com/join/C1",
        "joinPassword": null,
        "mediaTypes": ["audio"],
        "activeParticipantCount": 1,
        "parentCallId": null,
        "gatewayPin": null
    });
    v.as_object_mut()
        .expect("object")
        .extend(binding.as_object().expect("object").clone());
    serde_json::from_value(v).expect("VTCCall fixture")
}

async fn access(backend: &Bound, viewer: &str, call: &VtcCall) -> bool {
    may_access_call(
        &backend.chat,
        &(),
        &Id::from(ACCOUNT),
        &Id::from(viewer),
        call,
    )
    .await
    .expect("may_access_call")
}

fn direct_chat(id: &str, contact_id: &str) -> Value {
    json!({
        "id": id,
        "kind": "direct",
        "createdAt": "2026-01-01T00:00:00Z",
        "unreadCount": 0,
        "contactId": contact_id,
        "activeCallId": null
    })
}

#[tokio::test]
async fn group_chat_members_only() {
    let backend = Bound::new();
    backend.seed_chat_object("Chat", group_chat("chat1", &[ACCOUNT, "c-bob"]));
    let c = call(json!({ "chatId": "chat1" }));
    assert!(access(&backend, ACCOUNT, &c).await, "owner listed");
    assert!(access(&backend, "c-bob", &c).await, "contact listed");
    assert!(!access(&backend, "c-alice", &c).await, "not listed");
}

#[tokio::test]
async fn direct_chat_admits_the_owner_and_the_other_participant() {
    let backend = Bound::new();
    backend.seed_chat_object("Chat", direct_chat("dm1", "c-bob"));
    let c = call(json!({ "chatId": "dm1" }));
    assert!(access(&backend, ACCOUNT, &c).await, "owner");
    assert!(access(&backend, "c-bob", &c).await, "contactId");
    assert!(!access(&backend, "c-alice", &c).await, "third party");
}

#[tokio::test]
async fn unresolved_chat_id_denies() {
    let backend = Bound::new();
    let c = call(json!({ "chatId": "no-such-chat" }));
    assert!(!access(&backend, ACCOUNT, &c).await);
}

/// chat L798: `"view"` — "see the channel".
#[tokio::test]
async fn channel_call_needs_view() {
    let backend = Bound::new();
    backend.seed_chat_object(
        "Space",
        space(
            SPACE,
            &["view"],
            &[("c-bob", &["R-CALLER"]), ("c-alice", &[])],
        ),
    );
    backend.seed_chat_object("Chat", channel_chat(CHANNEL, SPACE, json!([])));
    let c = call(json!({ "spaceId": SPACE, "channelId": CHANNEL }));
    assert!(access(&backend, "c-bob", &c).await, "role grants view");
    assert!(
        !access(&backend, "c-alice", &c).await,
        "member without view"
    );
    assert!(!access(&backend, "c-carol", &c).await, "not a Space member");
}

/// chat L1707 includes "the implicit `@everyone` role" in the union, and a
/// `targetType: "member"` deny removes the permission (chat L1710).
#[tokio::test]
async fn channel_view_from_everyone_and_member_deny() {
    let backend = Bound::new();
    backend
        .chat
        .set_everyone_permissions(HashSet::from([jmap_chat_types::ChatPermission::View]));
    backend.seed_chat_object(
        "Space",
        space(SPACE, &[], &[("c-bob", &[]), ("c-alice", &[])]),
    );
    backend.seed_chat_object(
        "Chat",
        channel_chat(
            CHANNEL,
            SPACE,
            json!([{ "targetId": "c-alice", "targetType": "member", "allow": [], "deny": ["view"] }]),
        ),
    );
    let c = call(json!({ "channelId": CHANNEL }));
    assert!(access(&backend, "c-bob", &c).await, "@everyone grants view");
    assert!(!access(&backend, "c-alice", &c).await, "member deny");
}

#[tokio::test]
async fn space_call_admits_space_members() {
    let backend = Bound::new();
    backend.seed_chat_object("Space", space(SPACE, &[], &[("c-bob", &[])]));
    let c = call(json!({ "spaceId": SPACE }));
    assert!(access(&backend, "c-bob", &c).await, "member");
    assert!(!access(&backend, "c-alice", &c).await, "non-member");
}

#[tokio::test]
async fn blocked_caller_is_denied_even_as_a_member() {
    let backend = Bound::new();
    backend.seed_chat_object("ChatContact", contact("c-bob", BOB, true));
    backend.seed_chat_object("ChatContact", contact("c-alice", ALICE, false));
    backend.seed_chat_object("Chat", group_chat("chat1", &[ACCOUNT, "c-bob", "c-alice"]));
    let bound = call(json!({ "chatId": "chat1" }));
    assert!(!access(&backend, "c-bob", &bound).await, "blocked member");
    assert!(
        access(&backend, "c-alice", &bound).await,
        "unblocked member"
    );
    let standalone = call(json!({}));
    assert!(
        !access(&backend, "c-bob", &standalone).await,
        "blocked, no binding"
    );
    assert!(
        access(&backend, "c-alice", &standalone).await,
        "no Chat rule denies"
    );
}
