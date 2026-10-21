//! The reference `MemoryBackend` resolves the `"admins"` broadcast scope
//! of the `Message/query` `hasMention` filter against the owner's member
//! entry (accountId), not unconditionally (bd:JMAP-yt1w.20).
//!
//! Oracle: draft-atwood-jmap-chat-00 (jmap-chat-spec 33702a5):
//! - L446 `"admins"`: "members holding administrative authority in the
//!   Space or Chat";
//! - L554 ChatMember `role`: "Either `\"admin\"` or `\"member\"`. Admins
//!   may add and remove members and update group chat metadata.";
//! - L1356 last-administrator protection: "any member holding
//!   `\"manage_members\"` or `\"manage_space\"`";
//! - L1291 `hasMention`: "the accountId appears in `mentions`, OR
//!   `broadcastMentions` contains a scope whose resolved recipient set
//!   includes the owner."
//!
//! Fixtures are hand-written JSON.

mod common;

use common::{MemoryBackend, ACCOUNT_ID, SPACE_ID};
use jmap_chat_server::handle_message_query;
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

const CHAT_ID: &str = "c1";

fn seed_admins_message(backend: &MemoryBackend) {
    backend.insert_object_for_test(
        "Message",
        ACCOUNT_ID,
        "m1",
        json!({
            "id": "m1",
            "senderMsgId": "smsg-m1",
            "senderId": "someone-else",
            "chatId": CHAT_ID,
            "body": "hi admins",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "broadcastMentions": [{ "scope": "admins" }],
            "actions": [],
            "reactions": {},
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:01Z",
            "deliveryState": "delivered"
        }),
    );
}

fn seed_group(backend: &MemoryBackend, owner_role: &str) {
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        CHAT_ID,
        json!({
            "id": CHAT_ID,
            "kind": "group",
            "name": "g",
            "createdAt": "2026-01-01T00:00:00Z",
            "members": [
                { "id": ACCOUNT_ID, "role": owner_role, "joinedAt": "2026-01-01T00:00:00Z" },
                { "id": "someone-else", "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }
            ]
        }),
    );
}

/// A channel in `SPACE_ID` where the owner holds role `r1` with
/// `owner_perms`, and another member holds `manage_space`.
fn seed_channel(backend: &MemoryBackend, owner_perms: Value) {
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "S",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 2,
            "categories": [],
            "uncategorizedChannelIds": [CHAT_ID],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [
                { "id": "r1", "name": "Owner role", "permissions": owner_perms, "position": 1 },
                { "id": "r2", "name": "Admin", "permissions": ["view", "manage_space"], "position": 2 }
            ],
            "members": [
                { "id": ACCOUNT_ID, "roleIds": ["r1"], "joinedAt": "2026-01-01T00:00:00Z" },
                { "id": "someone-else", "roleIds": ["r2"], "joinedAt": "2026-01-01T00:00:00Z" }
            ]
        }),
    );
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        CHAT_ID,
        json!({
            "id": CHAT_ID,
            "kind": "channel",
            "name": "general",
            "spaceId": SPACE_ID,
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "pinnedMessageIds": [],
            "slowModeSeconds": 0,
            "permissionOverrides": []
        }),
    );
}

async fn has_mention_ids(backend: &MemoryBackend) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_query(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "filter": { "hasMention": true } }),
    )
    .await
    .expect("Message/query");
    resp["ids"].clone()
}

#[tokio::test]
async fn group_admin_owner_matches_admins() {
    let backend = MemoryBackend::new();
    seed_group(&backend, "admin");
    seed_admins_message(&backend);
    assert_eq!(has_mention_ids(&backend).await, json!(["m1"]));
}

#[tokio::test]
async fn group_member_owner_does_not_match_admins() {
    let backend = MemoryBackend::new();
    seed_group(&backend, "member");
    seed_admins_message(&backend);
    assert_eq!(has_mention_ids(&backend).await, json!([]));
}

#[tokio::test]
async fn channel_manage_space_owner_matches_admins() {
    let backend = MemoryBackend::new();
    seed_channel(&backend, json!(["view", "manage_space"]));
    seed_admins_message(&backend);
    assert_eq!(has_mention_ids(&backend).await, json!(["m1"]));
}

#[tokio::test]
async fn channel_manage_members_owner_matches_admins() {
    let backend = MemoryBackend::new();
    seed_channel(&backend, json!(["view", "manage_members"]));
    seed_admins_message(&backend);
    assert_eq!(has_mention_ids(&backend).await, json!(["m1"]));
}

#[tokio::test]
async fn channel_non_admin_owner_does_not_match_admins() {
    let backend = MemoryBackend::new();
    seed_channel(&backend, json!(["view", "send"]));
    seed_admins_message(&backend);
    assert_eq!(has_mention_ids(&backend).await, json!([]));
}

/// A direct Chat has no `"admin"` role (L554 applies to group chats), so
/// `"admins"` never targets the owner there.
#[tokio::test]
async fn direct_chat_does_not_match_admins() {
    let backend = MemoryBackend::new();
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        CHAT_ID,
        json!({
            "id": CHAT_ID,
            "kind": "direct",
            "createdAt": "2026-01-01T00:00:00Z",
            "contactId": "someone-else"
        }),
    );
    seed_admins_message(&backend);
    assert_eq!(has_mention_ids(&backend).await, json!([]));
}
