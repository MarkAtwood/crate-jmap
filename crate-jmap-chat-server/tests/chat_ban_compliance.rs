//! jmap-chat-compliance-tests 9.2a and 9.2b: ChatBan expiry and the
//! `addMembers` block (bd:JMAP-it7g.1).
//!
//! Oracles, quoted on each test: jmap-chat-compliance-tests.md §9.2a and
//! §9.2b, and draft-atwood-jmap-chat-00 §ChatBan and §ChatBan/set.

mod common;

use std::collections::BTreeMap;
use std::time::SystemTime;

use common::{ACCOUNT_ID, MemoryBackend, account_backend};
use jmap_chat_server::{handle_chat_ban_get, handle_chat_ban_set, handle_chat_set, sweep_expired};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

/// A group Chat where the owner is the admin and `bob` a member.
async fn group(backend: &MemoryBackend) -> String {
    let HandlerOutput { primary, .. } = handle_chat_set(
        backend,
        &(),
        &BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "g": { "kind": "group", "name": "Team", "memberIds": ["bob"] } },
        }),
        &[],
    )
    .await
    .expect("Chat/set create");
    primary["created"]["g"]["id"]
        .as_str()
        .expect("created")
        .to_owned()
}

async fn ban_set(backend: &MemoryBackend, body: Value) -> Value {
    let mut args = body;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } = handle_chat_ban_set(backend, &(), &BTreeMap::new(), args)
        .await
        .expect("ChatBan/set");
    primary
}

async fn chat_update(backend: &MemoryBackend, chat_id: &str, patch: Value) -> Value {
    let HandlerOutput { primary, .. } = handle_chat_set(
        backend,
        &(),
        &BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "update": { chat_id: patch } }),
        &[],
    )
    .await
    .expect("Chat/set update");
    primary
}

async fn member_ids(backend: &MemoryBackend, chat_id: &str) -> Vec<String> {
    let HandlerOutput { primary, .. } = jmap_chat_server::handle_chat_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [chat_id] }),
        &[],
    )
    .await
    .expect("Chat/get");
    let mut ids: Vec<String> = primary["list"][0]["members"]
        .as_array()
        .expect("members")
        .iter()
        .map(|m| m["id"].as_str().expect("id").to_owned())
        .collect();
    ids.sort_unstable();
    ids
}

/// 9.2a ChatBan expiry auto-restore: "Create a ChatBan with `expiresAt` in
/// the past. Run the expiry sweep. Verify the ban is removed and the user
/// is not added back to the Chat." Draft §ChatBan `expiresAt`: "If
/// present, the ban expires at this time and the server MUST lift it."
#[tokio::test]
async fn compliance_9_2a_expired_ban_is_lifted_without_re_adding() {
    let backend = account_backend();
    let chat = group(&backend).await;
    let resp = ban_set(
        &backend,
        json!({ "create": { "b": {
            "chatId": chat, "userId": "bob", "expiresAt": "2020-01-01T00:00:00Z",
        } } }),
    )
    .await;
    let ban_id = resp["created"]["b"]["id"]
        .as_str()
        .expect("created")
        .to_owned();
    assert_eq!(member_ids(&backend, &chat).await, vec![ACCOUNT_ID]);

    sweep_expired(&backend, &(), &Id::from(ACCOUNT_ID), SystemTime::now())
        .await
        .expect("sweep");

    let HandlerOutput { primary, .. } = handle_chat_ban_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [ban_id] }),
    )
    .await
    .expect("ChatBan/get");
    assert_eq!(primary["list"], json!([]), "{primary}");
    assert_eq!(primary["notFound"], json!([ban_id]), "{primary}");
    assert_eq!(member_ids(&backend, &chat).await, vec![ACCOUNT_ID]);
}

/// The sweep lifts only bans whose `expiresAt` has passed; a permanent
/// ban ("If absent, the ban lasts until it is destroyed") and a future one
/// stay.
#[tokio::test]
async fn sweep_keeps_bans_in_force() {
    let backend = account_backend();
    let chat = group(&backend).await;
    let resp = ban_set(
        &backend,
        json!({ "create": {
            "permanent": { "chatId": chat, "userId": "bob" },
            "future": { "chatId": chat, "userId": "carol", "expiresAt": "2999-01-01T00:00:00Z" },
        } }),
    )
    .await;
    assert_eq!(resp["notCreated"], Value::Null, "{resp}");
    sweep_expired(&backend, &(), &Id::from(ACCOUNT_ID), SystemTime::now())
        .await
        .expect("sweep");
    let HandlerOutput { primary, .. } =
        handle_chat_ban_get(&backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("ChatBan/get");
    assert_eq!(
        primary["list"].as_array().map(Vec::len),
        Some(2),
        "{primary}"
    );
}

/// 9.2b ChatBan blocks addMembers: "As a group Chat admin, create a
/// ChatBan for a member. Verify the member is no longer in `members`. Call
/// `Chat/set` with `addMembers` naming the banned user. Verify a
/// `forbidden` SetError. Destroy the ban, repeat `addMembers`, verify
/// success." Draft §ChatBan/set: "while the ban lasts the server MUST
/// reject adding the user with an `addMembers` entry, returning a
/// `forbidden` SetError."
#[tokio::test]
async fn compliance_9_2b_ban_blocks_add_members_until_destroyed() {
    let backend = account_backend();
    let chat = group(&backend).await;
    let resp = ban_set(
        &backend,
        json!({ "create": { "b": { "chatId": chat, "userId": "bob" } } }),
    )
    .await;
    let ban_id = resp["created"]["b"]["id"]
        .as_str()
        .expect("created")
        .to_owned();
    assert_eq!(member_ids(&backend, &chat).await, vec![ACCOUNT_ID]);

    let add = json!({ "addMembers": [{ "id": "bob" }] });
    let resp = chat_update(&backend, &chat, add.clone()).await;
    assert_eq!(resp["notUpdated"][&chat]["type"], "forbidden", "{resp}");
    assert_eq!(member_ids(&backend, &chat).await, vec![ACCOUNT_ID]);

    let resp = ban_set(&backend, json!({ "destroy": [ban_id] })).await;
    assert_eq!(resp["destroyed"], json!([ban_id]), "{resp}");

    let resp = chat_update(&backend, &chat, add).await;
    assert_eq!(resp["notUpdated"], Value::Null, "{resp}");
    assert!(resp["updated"].get(&chat).is_some(), "{resp}");
    assert_eq!(member_ids(&backend, &chat).await, vec![ACCOUNT_ID, "bob"]);
}

/// "while the ban lasts": a ban whose `expiresAt` has passed but that no
/// sweep has removed yet no longer blocks `addMembers`, and a ban in
/// another Chat never does.
#[tokio::test]
async fn expired_or_other_chat_ban_does_not_block_add_members() {
    let backend = account_backend();
    let chat = group(&backend).await;
    let other = group(&backend).await;
    let resp = ban_set(
        &backend,
        json!({ "create": {
            "old": { "chatId": chat, "userId": "carol", "expiresAt": "2020-01-01T00:00:00Z" },
            "elsewhere": { "chatId": other, "userId": "dave" },
        } }),
    )
    .await;
    assert_eq!(resp["notCreated"], Value::Null, "{resp}");
    let resp = chat_update(
        &backend,
        &chat,
        json!({ "addMembers": [{ "id": "carol" }, { "id": "dave" }] }),
    )
    .await;
    assert_eq!(resp["notUpdated"], Value::Null, "{resp}");
}
