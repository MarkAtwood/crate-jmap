//! `Chat/set` per-kind create, update and destroy rules (bd:JMAP-y3f2.2).
//!
//! Oracle: draft-atwood-jmap-chat-00.md at jmap-chat-spec 91672c9, quoted
//! per test with its line number. The owner is referenced by the accountId
//! (§Terminology), so `ACCOUNT_ID` is the caller's own member id.

mod common;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::{ChatLimits, handle_chat_get, handle_chat_set};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

async fn chat_set(backend: &MemoryBackend, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary: resp, .. } =
        handle_chat_set(backend, &(), &std::collections::BTreeMap::new(), args)
            .await
            .expect("Chat/set");
    resp
}

async fn chat_get(backend: &MemoryBackend, id: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_chat_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
    )
    .await
    .expect("Chat/get");
    resp["list"][0].clone()
}

/// Store a Chat of `kind` with id `id`; `members` is written as given.
fn store_chat(backend: &MemoryBackend, id: &str, kind: &str, members: Value) {
    backend.insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "kind": kind,
            "name": "Original",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "members": members,
        }),
    );
}

/// L1049: "For channel Chats, destroy via `Chat/set` is not supported; the
/// server MUST return a SetError of type `forbidden`."
#[tokio::test]
async fn channel_destroy_is_forbidden() {
    let backend = MemoryBackend::new();
    store_chat(&backend, "ch1", "channel", json!(null));

    let resp = chat_set(&backend, json!({ "destroy": ["ch1"] })).await;

    assert_eq!(resp["notDestroyed"]["ch1"]["type"], "forbidden", "{resp}");
    assert!(
        !resp["destroyed"]
            .as_array()
            .is_some_and(|d| d.contains(&json!("ch1"))),
        "{resp}"
    );
    assert_eq!(chat_get(&backend, "ch1").await["kind"], "channel");
}

/// L1049: "For direct and group Chats, `Chat/set` destroy removes the Chat
/// from the caller's view."
#[tokio::test]
async fn group_destroy_still_succeeds() {
    let backend = MemoryBackend::new();
    store_chat(&backend, "g1", "group", json!([]));

    let resp = chat_set(&backend, json!({ "destroy": ["g1"] })).await;

    assert_eq!(resp["destroyed"], json!(["g1"]), "{resp}");
}

/// RFC 8620 §5.3 types `destroy` as `Id[]|null`, and §3.6.2 says
/// `invalidArguments` is returned when an argument "is of the wrong type".
/// A non-string element after a valid id must fail the whole call rather
/// than being skipped while the valid id is destroyed.
#[tokio::test]
async fn non_string_destroy_element_is_invalid_arguments() {
    let backend = MemoryBackend::new();
    store_chat(&backend, "g1", "group", json!([]));

    let err = handle_chat_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "destroy": ["g1", 7] }),
    )
    .await
    .expect_err("non-string destroy element must be a method error");

    assert_eq!(err.error_type.to_string(), "invalidArguments");
    assert_eq!(chat_get(&backend, "g1").await["kind"], "group");
}

/// L1030: "For group chats, admins additionally may update: `name`,
/// `description`, `avatarBlobId`." A group member who is not an admin may
/// not.
#[tokio::test]
async fn non_admin_group_metadata_update_is_forbidden() {
    let backend = MemoryBackend::new();
    store_chat(
        &backend,
        "g1",
        "group",
        json!([
            { "id": "alice", "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": ACCOUNT_ID, "role": "member", "joinedAt": "2026-01-01T00:00:00Z" }
        ]),
    );

    for patch in [
        json!({ "name": "Renamed" }),
        json!({ "description": "new" }),
        json!({ "avatarBlobId": "blob-1" }),
    ] {
        let resp = chat_set(&backend, json!({ "update": { "g1": patch } })).await;
        assert_eq!(resp["notUpdated"]["g1"]["type"], "forbidden", "{resp}");
    }
    assert_eq!(chat_get(&backend, "g1").await["name"], "Original");
}

/// L1030: "For group chats, admins additionally may update: `name`,
/// `description`, `avatarBlobId`."
#[tokio::test]
async fn admin_group_metadata_update_applies() {
    let backend = MemoryBackend::new();
    store_chat(
        &backend,
        "g1",
        "group",
        json!([{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }]),
    );

    let resp = chat_set(
        &backend,
        json!({ "update": { "g1": {
            "name": "Renamed", "description": "new", "avatarBlobId": "blob-1"
        } } }),
    )
    .await;

    assert!(resp["updated"].get("g1").is_some(), "{resp}");
    let chat = chat_get(&backend, "g1").await;
    assert_eq!(chat["name"], "Renamed");
    assert_eq!(chat["description"], "new");
    assert_eq!(chat["avatarBlobId"], "blob-1");
}

/// L1030 grants `name` updates to group admins only ("For group chats");
/// L1026 does not list `name` among the keys "for all chat kinds".
#[tokio::test]
async fn direct_chat_name_update_is_forbidden() {
    let backend = MemoryBackend::new();
    store_chat(&backend, "d1", "direct", json!(null));

    let resp = chat_set(&backend, json!({ "update": { "d1": { "name": "x" } } })).await;

    assert_eq!(resp["notUpdated"]["d1"]["type"], "forbidden", "{resp}");
}

/// L1026: "`update` supports the following patch keys for all chat kinds:
/// `muted`, `muteUntil`, `receiveTypingIndicators`, `pinnedMessageIds`,
/// `messageExpirySeconds`, `receiptSharing`." Keys outside §Chat/set
/// "Updating a Chat" are rejected; RFC 8620 §5.3 `invalidProperties`: "The
/// record given is invalid in some way."
#[tokio::test]
async fn update_key_outside_spec_is_invalid_properties() {
    let backend = MemoryBackend::new();
    store_chat(
        &backend,
        "g1",
        "group",
        json!([{ "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" }]),
    );

    for key in ["contactId", "spaceId", "members", "lastMessageAt", "topic"] {
        let resp = chat_set(&backend, json!({ "update": { "g1": { key: "x" } } })).await;
        assert_eq!(
            resp["notUpdated"]["g1"],
            json!({ "type": "invalidProperties", "properties": [key] }),
            "{resp}"
        );
    }
}

/// L1026: the keys for all chat kinds are accepted on a direct Chat.
#[tokio::test]
async fn all_kind_update_keys_apply_to_direct_chat() {
    let backend = MemoryBackend::new();
    store_chat(&backend, "d1", "direct", json!(null));

    let resp = chat_set(
        &backend,
        json!({ "update": { "d1": {
            "muted": true,
            "muteUntil": "2030-01-01T00:00:00Z",
            "receiveTypingIndicators": false,
            "messageExpirySeconds": 3600,
            "receiptSharing": false
        } } }),
    )
    .await;

    assert!(resp["updated"].get("d1").is_some(), "{resp}");
    let chat = chat_get(&backend, "d1").await;
    assert_eq!(chat["muted"], true);
    assert_eq!(chat["messageExpirySeconds"], 3600);
    assert_eq!(chat["receiptSharing"], false);
}

/// L1018: "Optional at creation: `description` (String), `avatarBlobId`
/// (String), `messageExpirySeconds` (UnsignedInt)."
#[tokio::test]
async fn group_create_optional_fields_round_trip() {
    let backend = MemoryBackend::new();

    let resp = chat_set(
        &backend,
        json!({ "create": { "g": {
            "kind": "group",
            "name": "Team",
            "memberIds": [],
            "description": "about",
            "avatarBlobId": "blob-7",
            "messageExpirySeconds": 86400
        } } }),
    )
    .await;

    let id = resp["created"]["g"]["id"].as_str().expect("created id");
    let chat = chat_get(&backend, id).await;
    assert_eq!(chat["description"], "about");
    assert_eq!(chat["avatarBlobId"], "blob-7");
    assert_eq!(chat["messageExpirySeconds"], 86400);
}

/// L1018 types `messageExpirySeconds` as UnsignedInt; RFC 8620 §1.3: an
/// UnsignedInt is "an Int where the value MUST be in the range
/// 0 <= value <= 2^53-1".
#[tokio::test]
async fn group_create_negative_message_expiry_is_invalid_properties() {
    let backend = MemoryBackend::new();

    let resp = chat_set(
        &backend,
        json!({ "create": { "g": {
            "kind": "group", "name": "Team", "memberIds": [], "messageExpirySeconds": -1
        } } }),
    )
    .await;

    assert_eq!(
        resp["notCreated"]["g"],
        json!({ "type": "invalidProperties", "properties": ["messageExpirySeconds"] }),
        "{resp}"
    );
}

/// L1016: "`memberIds` (String[], required): ChatContact.ids of additional
/// initial members. If the resulting membership would exceed a
/// server-defined limit on the number of members per group Chat, the
/// server MUST return an `overQuota` SetError". With a limit of 2, the
/// creator plus two members is three.
#[tokio::test]
async fn group_create_over_member_limit_is_over_quota() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(ChatLimits::default().with_max_group_chat_members(2)));

    let resp = chat_set(
        &backend,
        json!({ "create": {
            "over": { "kind": "group", "name": "Big", "memberIds": ["bob", "carol"] },
            "fits": { "kind": "group", "name": "Small", "memberIds": ["bob"] }
        } }),
    )
    .await;

    assert_eq!(resp["notCreated"]["over"]["type"], "overQuota", "{resp}");
    assert!(resp["created"].get("fits").is_some(), "{resp}");
}
