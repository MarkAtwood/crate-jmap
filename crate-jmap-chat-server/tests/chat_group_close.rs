//! A group Chat closes when its last admin leaves while other members
//! remain, and a closed group is read-only (bd:JMAP-yt1w.4 (b)).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 §Chat `closedAt` and
//! §Chat/set "Updating a Chat" `removeMembers`, quoted per test.

mod common;

use common::{IdentityBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_chat_get, handle_chat_set, handle_message_get, handle_message_set};
use jmap_types::Id;
use serde_json::{json, Value};

/// Create a group Chat as `alice` (admin) with `bob` and `carol` as
/// members and return its id.
async fn group_of_three(backend: &IdentityBackend) -> String {
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    let (resp, _) = handle_chat_set(
        backend,
        &Id::from("alice"),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "g": { "kind": "group", "name": "Team", "memberIds": ["bob", "carol"] } },
        }),
    )
    .await
    .expect("Chat/set create");
    resp["created"]["g"]["id"]
        .as_str()
        .expect("created")
        .to_owned()
}

/// Send one `Chat/set` update as `caller` and return the response.
async fn update_as(backend: &IdentityBackend, caller: &str, chat_id: &str, patch: Value) -> Value {
    let (resp, _) = handle_chat_set(
        backend,
        &Id::from(caller),
        json!({ "accountId": ACCOUNT_ID, "update": { chat_id: patch } }),
    )
    .await
    .expect("Chat/set update");
    resp
}

/// The stored Chat as returned by `Chat/get`.
async fn stored_chat(backend: &IdentityBackend, chat_id: &str) -> Value {
    let (resp, _) = handle_chat_get(
        backend,
        &Id::from("bob"),
        json!({ "accountId": ACCOUNT_ID, "ids": [chat_id] }),
    )
    .await
    .expect("Chat/get");
    resp["list"][0].clone()
}

/// Send one `Message/set` create in `chat_id` as `bob` and return the
/// response.
async fn send(backend: &IdentityBackend, chat_id: &str) -> Value {
    let (resp, _) = handle_message_set(
        backend,
        &Id::from("bob"),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m": {
                "chatId": chat_id,
                "body": "hi",
                "bodyType": "text/plain",
                "sentAt": "2024-01-01T00:00:00Z",
            } },
        }),
    )
    .await
    .expect("Message/set");
    resp
}

/// Close the group by having its only admin, `alice`, leave.
async fn close(backend: &IdentityBackend, chat_id: &str) {
    let resp = update_as(
        backend,
        "alice",
        chat_id,
        json!({ "removeMembers": ["alice"] }),
    )
    .await;
    assert!(resp["updated"].get(chat_id).is_some(), "{resp}");
}

/// "If the owner is the last admin and other members remain, leaving
/// closes the group: the server MUST set `closedAt` ({{chat}}) in the
/// same operation".
#[tokio::test]
async fn last_admin_leaving_with_members_remaining_sets_closed_at() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;
    assert!(stored_chat(&backend, &chat_id)
        .await
        .get("closedAt")
        .is_none());

    close(&backend, &chat_id).await;

    let chat = stored_chat(&backend, &chat_id).await;
    // RFC 8620 §1.4: a UTCDate "MUST have the time-offset component set
    // to `Z`"; the group cannot close before it was created.
    let closed_at = chat["closedAt"].as_str().expect("closedAt set on close");
    let created_at = chat["createdAt"].as_str().expect("createdAt");
    assert!(closed_at.ends_with('Z'), "{closed_at}");
    assert!(closed_at >= created_at, "{closed_at} < {created_at}");
    let mut members: Vec<&str> = chat["members"]
        .as_array()
        .expect("members")
        .iter()
        .map(|m| m["id"].as_str().expect("member id"))
        .collect();
    members.sort();
    assert_eq!(members, ["bob", "carol"]);
}

/// "`closedAt` ... Absent while the group is open. A group closes when its
/// last admin leaves while other members remain". A non-admin leaving, an
/// admin leaving while another admin remains, and the last member leaving
/// do not close the group.
#[tokio::test]
async fn leaving_without_closing_conditions_keeps_group_open() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;

    let resp = update_as(
        &backend,
        "bob",
        &chat_id,
        json!({ "removeMembers": ["bob"] }),
    )
    .await;
    assert!(resp["updated"].get(&chat_id).is_some(), "{resp}");
    assert!(stored_chat(&backend, &chat_id)
        .await
        .get("closedAt")
        .is_none());

    let resp = update_as(
        &backend,
        "alice",
        &chat_id,
        json!({ "updateMemberRoles": [{ "id": "carol", "role": "admin" }] }),
    )
    .await;
    assert!(resp["updated"].get(&chat_id).is_some(), "{resp}");
    let resp = update_as(
        &backend,
        "alice",
        &chat_id,
        json!({ "removeMembers": ["alice"] }),
    )
    .await;
    assert!(resp["updated"].get(&chat_id).is_some(), "{resp}");
    assert!(stored_chat(&backend, &chat_id)
        .await
        .get("closedAt")
        .is_none());

    let resp = update_as(
        &backend,
        "carol",
        &chat_id,
        json!({ "removeMembers": ["carol"] }),
    )
    .await;
    assert!(resp["updated"].get(&chat_id).is_some(), "{resp}");
    assert!(stored_chat(&backend, &chat_id)
        .await
        .get("closedAt")
        .is_none());
}

/// "After it is set, the server MUST reject with a `forbidden` SetError
/// every `Message/set` create in this Chat".
#[tokio::test]
async fn closed_group_rejects_message_create() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;
    close(&backend, &chat_id).await;

    let resp = send(&backend, &chat_id).await;
    assert_eq!(resp["notCreated"]["m"]["type"], "forbidden", "{resp}");
}

/// "After it is set, the server MUST reject with a `forbidden` SetError
/// ... every `Chat/set` update that changes membership (`addMembers`,
/// `removeMembers`, `updateMemberRoles`) or shared group settings (`name`,
/// `description`, `avatarBlobId`, `pinnedMessageIds`)."
#[tokio::test]
async fn closed_group_rejects_membership_and_shared_setting_updates() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;
    close(&backend, &chat_id).await;
    let before = stored_chat(&backend, &chat_id).await;

    for patch in [
        json!({ "addMembers": [{ "id": "dave" }] }),
        json!({ "removeMembers": ["bob"] }),
        json!({ "updateMemberRoles": [{ "id": "carol", "role": "admin" }] }),
        json!({ "name": "Renamed" }),
        json!({ "description": "new" }),
        json!({ "avatarBlobId": "blob-1" }),
        json!({ "pinnedMessageIds": [] }),
    ] {
        let resp = update_as(&backend, "bob", &chat_id, patch.clone()).await;
        assert_eq!(
            resp["notUpdated"][&chat_id]["type"], "forbidden",
            "{patch}: {resp}"
        );
    }
    assert_eq!(stored_chat(&backend, &chat_id).await, before);
}

/// "Once set, `closedAt` MUST NOT be cleared. ... Message history remains
/// readable. Owner-side preferences (`muted`, `muteUntil`,
/// `receiveTypingIndicators`, `messageExpirySeconds`, `receiptSharing`)
/// remain updatable, and members MAY still destroy the Chat via `Chat/set`
/// to remove it from their view."
#[tokio::test]
async fn closed_group_keeps_history_prefs_and_destroy() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;
    let sent = send(&backend, &chat_id).await;
    let msg_id = sent["created"]["m"]["id"]
        .as_str()
        .expect("message created")
        .to_owned();
    close(&backend, &chat_id).await;
    let closed_at = stored_chat(&backend, &chat_id).await["closedAt"].clone();

    let (got, _) = handle_message_get(
        &backend,
        &Id::from("bob"),
        json!({ "accountId": ACCOUNT_ID, "ids": [msg_id] }),
    )
    .await
    .expect("Message/get");
    assert_eq!(got["list"][0]["id"], msg_id.as_str(), "{got}");

    let resp = update_as(
        &backend,
        "bob",
        &chat_id,
        json!({
            "muted": true,
            "muteUntil": "2030-01-01T00:00:00Z",
            "receiveTypingIndicators": false,
            "messageExpirySeconds": 60,
            "receiptSharing": false,
        }),
    )
    .await;
    assert!(resp["updated"].get(&chat_id).is_some(), "{resp}");
    let chat = stored_chat(&backend, &chat_id).await;
    assert_eq!(chat["muted"], true, "{chat}");
    assert_eq!(chat["closedAt"], closed_at, "{chat}");

    let resp = update_as(&backend, "bob", &chat_id, json!({ "closedAt": null })).await;
    assert!(resp["notUpdated"].get(&chat_id).is_some(), "{resp}");
    assert_eq!(stored_chat(&backend, &chat_id).await["closedAt"], closed_at);

    let (resp, _) = handle_chat_set(
        &backend,
        &Id::from("bob"),
        json!({ "accountId": ACCOUNT_ID, "destroy": [chat_id] }),
    )
    .await
    .expect("Chat/set destroy");
    assert_eq!(resp["destroyed"], json!([chat_id]), "{resp}");
}
