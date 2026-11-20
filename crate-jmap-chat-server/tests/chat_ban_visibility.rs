//! `ChatBan/get` shows the owner only the bans the spec lets it see
//! (bd:JMAP-it7g.1).
//!
//! Oracle: draft-atwood-jmap-chat-00 §ChatBan/get: "Members with the
//! `"admin"` role in a group Chat see all ChatBan records for that Chat.
//! A banned user fetching their own account sees only ChatBan records
//! where `userId` is the accountId."

mod common;

use common::{ACCOUNT_ID, MemoryBackend, account_backend};
use jmap_chat_server::handle_chat_ban_get;
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

/// A ban in a Chat where the owner is an admin.
const IN_ADMIN_CHAT: &str = "ban-admin";
/// A ban on someone else in a Chat where the owner is only a member.
const OTHER_IN_MEMBER_CHAT: &str = "ban-other";
/// A ban on the owner (`userId` is the accountId).
const OF_OWNER: &str = "ban-owner";

fn seeded() -> MemoryBackend<()> {
    let backend = account_backend();
    for (id, owner_role) in [("g-admin", "admin"), ("g-member", "member")] {
        backend.insert_object_for_test(
            "Chat",
            ACCOUNT_ID,
            id,
            json!({
                "id": id,
                "kind": "group",
                "name": "Team",
                "createdAt": "2026-01-01T00:00:00Z",
                "unreadCount": 0,
                "members": [
                    { "id": "alice", "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" },
                    { "id": ACCOUNT_ID, "role": owner_role, "joinedAt": "2026-01-01T00:00:00Z" },
                ],
            }),
        );
    }
    for (id, chat, user) in [
        (IN_ADMIN_CHAT, "g-admin", "mallory"),
        (OTHER_IN_MEMBER_CHAT, "g-member", "mallory"),
        (OF_OWNER, "g-member", ACCOUNT_ID),
    ] {
        backend.insert_object_for_test(
            "ChatBan",
            ACCOUNT_ID,
            id,
            json!({
                "id": id,
                "chatId": chat,
                "userId": user,
                "bannedBy": "alice",
                "createdAt": "2026-01-01T00:00:00Z",
            }),
        );
    }
    backend
}

async fn get(backend: &MemoryBackend<()>, ids: Value) -> Value {
    let HandlerOutput { primary, .. } =
        handle_chat_ban_get(backend, &(), json!({ "accountId": ACCOUNT_ID, "ids": ids }))
            .await
            .expect("ChatBan/get");
    primary
}

fn listed_ids(resp: &Value) -> Vec<&str> {
    let mut ids: Vec<&str> = resp["list"]
        .as_array()
        .expect("list")
        .iter()
        .map(|b| b["id"].as_str().expect("id"))
        .collect();
    ids.sort_unstable();
    ids
}

/// A full fetch lists the admin's Chat's bans and the owner's own ban,
/// and drops the other ban (RFC 8620 §5.1 `notFound` lists only
/// requested ids).
#[tokio::test]
async fn full_fetch_shows_admin_bans_and_own_ban() {
    let resp = get(&seeded(), Value::Null).await;
    assert_eq!(listed_ids(&resp), vec![IN_ADMIN_CHAT, OF_OWNER], "{resp}");
    assert_eq!(resp["notFound"], json!([]), "{resp}");
}

/// A requested hidden ban is reported exactly like a missing one.
#[tokio::test]
async fn requested_hidden_ban_is_not_found() {
    let resp = get(&seeded(), json!([OTHER_IN_MEMBER_CHAT, OF_OWNER])).await;
    assert_eq!(listed_ids(&resp), vec![OF_OWNER], "{resp}");
    assert_eq!(resp["notFound"], json!([OTHER_IN_MEMBER_CHAT]), "{resp}");
}
