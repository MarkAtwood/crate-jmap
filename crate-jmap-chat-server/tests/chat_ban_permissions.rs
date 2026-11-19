//! `ChatBan/set` create, update and destroy are gated on the `"admin"`
//! role in a group Chat, and a create removes the user from `members`
//! (bd:JMAP-it7g.1).
//!
//! Oracle: draft-atwood-jmap-chat-00 §ChatBan/set, quoted on each test.
//! The owner's own member entry is the accountId (§Terminology).

mod common;

use common::{ACCOUNT_ID, MemoryBackend, account_backend};
use jmap_chat_server::{GroupUpdateAction, OutboundEvent, handle_chat_ban_set, handle_chat_get};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

/// A group Chat where the owner is an admin and `bob` a member.
const ADMIN_GROUP: &str = "g-admin";
/// A group Chat where `alice` is the admin and the owner a member.
const MEMBER_GROUP: &str = "g-member";
/// A direct Chat.
const DIRECT: &str = "d1";

fn chat(id: &str, kind: &str, members: Value) -> Value {
    let mut chat = json!({
        "id": id,
        "kind": kind,
        "createdAt": "2026-01-01T00:00:00Z",
        "unreadCount": 0,
    });
    if kind == "group" {
        chat["name"] = json!("Team");
        chat["members"] = members;
    } else {
        chat["contactId"] = json!("bob");
    }
    chat
}

fn member(id: &str, role: &str) -> Value {
    json!({ "id": id, "role": role, "joinedAt": "2026-01-01T00:00:00Z" })
}

fn seeded() -> MemoryBackend {
    let backend = account_backend();
    for (id, value) in [
        (
            ADMIN_GROUP,
            chat(
                ADMIN_GROUP,
                "group",
                json!([member(ACCOUNT_ID, "admin"), member("bob", "member")]),
            ),
        ),
        (
            MEMBER_GROUP,
            chat(
                MEMBER_GROUP,
                "group",
                json!([member("alice", "admin"), member(ACCOUNT_ID, "member")]),
            ),
        ),
        (DIRECT, chat(DIRECT, "direct", Value::Null)),
    ] {
        backend.insert_object_for_test("Chat", ACCOUNT_ID, id, value);
    }
    for (ban, chat_id) in [("ban-admin", ADMIN_GROUP), ("ban-member", MEMBER_GROUP)] {
        backend.insert_object_for_test(
            "ChatBan",
            ACCOUNT_ID,
            ban,
            json!({
                "id": ban,
                "chatId": chat_id,
                "userId": "mallory",
                "bannedBy": "alice",
                "createdAt": "2026-01-01T00:00:00Z",
            }),
        );
    }
    backend
}

async fn set(backend: &MemoryBackend, body: Value) -> Value {
    let mut args = body;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } =
        handle_chat_ban_set(backend, &(), &std::collections::BTreeMap::new(), args)
            .await
            .expect("ChatBan/set");
    primary
}

async fn member_ids(backend: &MemoryBackend, chat_id: &str) -> Vec<String> {
    let HandlerOutput { primary, .. } = handle_chat_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [chat_id] }),
        &[],
    )
    .await
    .expect("Chat/get");
    primary["list"][0]["members"]
        .as_array()
        .expect("members")
        .iter()
        .map(|m| m["id"].as_str().expect("id").to_owned())
        .collect()
}

/// "It requires the `"admin"` role in the Chat ... Creating a ban removes
/// the user from `members` if present, as `removeMembers` would". The
/// removal is reported as the `removeMembers` group update.
#[tokio::test]
async fn admin_create_removes_the_user_from_members() {
    let backend = seeded();
    let resp = set(
        &backend,
        json!({ "create": { "b": { "chatId": ADMIN_GROUP, "userId": "bob", "reason": "spam" } } }),
    )
    .await;
    let created = &resp["created"]["b"];
    assert!(created["id"].is_string(), "{resp}");
    assert_eq!(created["bannedBy"], ACCOUNT_ID, "{resp}");
    assert!(created["createdAt"].is_string(), "{resp}");
    assert_eq!(member_ids(&backend, ADMIN_GROUP).await, vec![ACCOUNT_ID]);

    let events = backend.take_outbound_events_for_test();
    assert!(
        events.iter().any(|e| matches!(
            e,
            OutboundEvent::GroupUpdate {
                action: GroupUpdateAction::RemoveMembers { removed_ids },
                recipients,
                ..
            } if removed_ids.iter().map(AsRef::as_ref).eq(["bob"])
                && recipients.iter().any(|r| r.as_ref() == "bob")
        )),
        "{events:?}"
    );
}

/// "the Chat MUST be a group Chat; the server MUST reject any other with a
/// `forbidden` SetError."
#[tokio::test]
async fn create_in_a_direct_chat_is_forbidden() {
    let backend = seeded();
    let resp = set(
        &backend,
        json!({ "create": { "b": { "chatId": DIRECT, "userId": "bob" } } }),
    )
    .await;
    assert_eq!(resp["notCreated"]["b"]["type"], "forbidden", "{resp}");
}

/// "It requires the `"admin"` role in the Chat"; the owner is only a
/// member of this group.
#[tokio::test]
async fn non_admin_create_is_forbidden() {
    let backend = seeded();
    let resp = set(
        &backend,
        json!({ "create": { "b": { "chatId": MEMBER_GROUP, "userId": "alice" } } }),
    )
    .await;
    assert_eq!(resp["notCreated"]["b"]["type"], "forbidden", "{resp}");
    assert_eq!(
        member_ids(&backend, MEMBER_GROUP).await,
        vec!["alice", ACCOUNT_ID]
    );
}

/// The draft leaves an owner banning themselves undefined; this server
/// rejects it with `forbidden` (the owner leaves with `removeMembers`).
#[tokio::test]
async fn owner_banning_themselves_is_forbidden() {
    let backend = seeded();
    let resp = set(
        &backend,
        json!({ "create": { "b": { "chatId": ADMIN_GROUP, "userId": ACCOUNT_ID } } }),
    )
    .await;
    assert_eq!(resp["notCreated"]["b"]["type"], "forbidden", "{resp}");
}

/// "`update` supports: `reason`, `expiresAt`. Requires the `"admin"` role
/// in the Chat."
#[tokio::test]
async fn update_requires_admin() {
    let backend = seeded();
    let resp = set(
        &backend,
        json!({ "update": {
            "ban-admin": { "reason": "new", "expiresAt": "2030-01-01T00:00:00Z" },
            "ban-member": { "reason": "new" },
        } }),
    )
    .await;
    assert!(resp["updated"].get("ban-admin").is_some(), "{resp}");
    assert_eq!(
        resp["notUpdated"]["ban-member"]["type"], "forbidden",
        "{resp}"
    );
}

/// "`destroy` lifts the ban. Requires the `"admin"` role in the Chat.
/// Lifting a ban does not add the user back to the Chat."
#[tokio::test]
async fn destroy_requires_admin_and_does_not_re_add() {
    let backend = seeded();
    let resp = set(
        &backend,
        json!({ "create": { "b": { "chatId": ADMIN_GROUP, "userId": "bob" } } }),
    )
    .await;
    let id = resp["created"]["b"]["id"].as_str().expect("id").to_owned();

    let resp = set(&backend, json!({ "destroy": [id, "ban-member"] })).await;
    assert_eq!(resp["destroyed"], json!([id]), "{resp}");
    assert_eq!(
        resp["notDestroyed"]["ban-member"]["type"], "forbidden",
        "{resp}"
    );
    assert_eq!(member_ids(&backend, ADMIN_GROUP).await, vec![ACCOUNT_ID]);
}
