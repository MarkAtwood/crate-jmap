//! Owner self-operations and owner references use the accountId, not the
//! caller's principal id (bd:JMAP-yt1w.29).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 §Terminology ("identity
//! / id"), jmap-chat-spec commit cefc217: "The owner has no
//! ChatContact: a client-facing reference to the owner carries the
//! accountId of the owner's account ({{RFC8620}} Section 1.6.2),
//! including where this document describes the reference as a
//! ChatContact.id, and no ChatContact has that accountId as its `id`."
//!
//! Every test runs on `IdentityBackend`, whose `principal_id` returns the
//! caller verbatim, with a caller principal (`PRINCIPAL`) that differs
//! from the accountId (`ACCOUNT_ID`). A handler that keys an owner
//! reference on `principal_id` instead of the accountId fails here; the
//! single-user posture (`principal_id == None`) cannot tell them apart.

mod common;

use common::{seed_space, IdentityBackend, ACCOUNT_ID, SPACE_ID};
use jmap_chat_server::{
    handle_ban_set, handle_chat_get, handle_chat_set, handle_emoji_set, handle_invite_set,
    handle_space_join, handle_space_set, JmapBackend,
};
use jmap_chat_types::Space;
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

/// The owner's authenticated principal, deliberately not the accountId.
const PRINCIPAL: &str = "user-alice";
const CHAT_ID: &str = "g1";

fn owner() -> Id {
    Id::from(PRINCIPAL)
}

/// Store a group Chat whose `members` are `members`.
fn seed_group(backend: &IdentityBackend, members: Value) {
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    backend.inner().insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        CHAT_ID,
        json!({
            "id": CHAT_ID,
            "kind": "group",
            "name": "Team",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "members": members,
        }),
    );
}

async fn chat_set(backend: &IdentityBackend, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_chat_set(backend, &owner(), args)
        .await
        .expect("Chat/set")
        .primary
}

async fn stored_chat_members(backend: &IdentityBackend) -> Vec<Value> {
    let HandlerOutput { primary: resp, .. } = handle_chat_get(
        backend,
        &owner(),
        json!({ "accountId": ACCOUNT_ID, "ids": [CHAT_ID] }),
    )
    .await
    .expect("Chat/get");
    resp["list"][0]["members"]
        .as_array()
        .expect("group Chat must carry members")
        .clone()
}

/// Seed the Space with an admin `c-admin` (`manage_members`) and the
/// owner as a member without roles.
fn seed_space_with_owner_member(backend: &IdentityBackend) {
    seed_space(
        backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_space", "manage_members"],
            "position": 100
        }]),
        json!([
            { "id": "c-admin", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": ACCOUNT_ID, "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
        ]),
    );
}

async fn space_set_update(backend: &IdentityBackend, patch: Value) -> Value {
    handle_space_set(
        backend,
        &owner(),
        json!({ "accountId": ACCOUNT_ID, "update": { SPACE_ID: patch } }),
    )
    .await
    .expect("Space/set")
    .primary
}

/// Stored Space members, read from the backend directly: the owner may
/// have left, and draft §Space/get hides a non-previewable Space from a
/// non-member.
async fn space_members(backend: &IdentityBackend) -> Vec<Value> {
    let (list, _) = backend
        .inner()
        .get_objects::<Space>(
            &(),
            &Id::from(ACCOUNT_ID),
            Some(&[Id::from(SPACE_ID)]),
            None,
        )
        .await
        .expect("get_objects");
    serde_json::to_value(&list[0].members)
        .expect("serialize members")
        .as_array()
        .expect("members")
        .clone()
}

/// Store a public Space with one member, `c-admin`, that the owner may join.
fn seed_public_space(backend: &IdentityBackend) {
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    backend.inner().insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "Atlas",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 1,
            "isPublic": true,
            "isPubliclyPreviewable": true,
            "members": [
                { "id": "c-admin", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
            ],
            "categories": [],
            "uncategorizedChannelIds": [],
            "roles": []
        }),
    );
}

/// draft-atwood-jmap-chat-00 §Chat/set `removeMembers`: "The owner MAY
/// leave the group by including only the accountId in `removeMembers`;
/// the server MUST permit this whether or not the owner holds the admin
/// role".
#[tokio::test]
async fn group_self_leave_by_account_id_is_permitted() {
    let backend = IdentityBackend::new();
    seed_group(
        &backend,
        json!([
            { "id": "c-bob", "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": ACCOUNT_ID, "role": "member", "joinedAt": "2026-01-01T00:00:00Z" }
        ]),
    );

    let resp = chat_set(
        &backend,
        json!({ "update": { CHAT_ID: { "removeMembers": [ACCOUNT_ID] } } }),
    )
    .await;
    assert!(resp["updated"].get(CHAT_ID).is_some(), "{resp}");
    let ids: Vec<Value> = stored_chat_members(&backend)
        .await
        .iter()
        .map(|m| m["id"].clone())
        .collect();
    assert_eq!(ids, [json!("c-bob")]);
}

/// draft-atwood-jmap-chat-00 §ChatMember `invitedBy`: "The ChatContact.id
/// of the member who added this participant, or the accountId when the
/// owner added them."
#[tokio::test]
async fn owner_admin_add_members_records_account_id_as_invited_by() {
    let backend = IdentityBackend::new();
    seed_group(
        &backend,
        json!([
            { "id": ACCOUNT_ID, "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "c-bob", "role": "member", "joinedAt": "2026-01-01T00:00:00Z" }
        ]),
    );

    let resp = chat_set(
        &backend,
        json!({ "update": { CHAT_ID: { "addMembers": [{ "id": "c-dave" }] } } }),
    )
    .await;
    assert!(resp["updated"].get(CHAT_ID).is_some(), "{resp}");
    let members = stored_chat_members(&backend).await;
    let dave = members
        .iter()
        .find(|m| m["id"] == "c-dave")
        .expect("c-dave added");
    assert_eq!(dave["invitedBy"], ACCOUNT_ID, "{dave}");
}

/// draft-atwood-jmap-chat-00 §ChatMember `id`: "The participant's
/// ChatContact.id, or the accountId for the owner." The owner creating a
/// group is that participant.
#[tokio::test]
async fn group_create_records_owner_member_as_account_id() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));

    let resp = chat_set(
        &backend,
        json!({ "create": { "g": { "kind": "group", "name": "Team", "memberIds": ["c-bob"] } } }),
    )
    .await;
    let created = &resp["created"]["g"];
    let members = created["members"].as_array().expect("members");
    let admin = members
        .iter()
        .find(|m| m["role"] == "admin")
        .expect("creator is admin");
    assert_eq!(admin["id"], ACCOUNT_ID, "{created}");
    let bob = members
        .iter()
        .find(|m| m["id"] == "c-bob")
        .expect("c-bob is a member");
    assert_eq!(bob["invitedBy"], ACCOUNT_ID, "{created}");
}

/// draft-atwood-jmap-chat-00 §Space/set `removeMembers`: "A member who
/// does not hold `"manage_members"` permission MAY remove themselves from
/// the Space by including only the accountId in `removeMembers`. The
/// server MUST permit self-removal regardless of the caller's permission
/// set."
#[tokio::test]
async fn space_self_removal_by_account_id_is_permitted() {
    let backend = IdentityBackend::new();
    seed_space_with_owner_member(&backend);

    let resp = space_set_update(&backend, json!({ "removeMembers": [ACCOUNT_ID] })).await;
    assert!(resp["notUpdated"][SPACE_ID].is_null(), "{resp}");
    let ids: Vec<Value> = space_members(&backend)
        .await
        .iter()
        .map(|m| m["id"].clone())
        .collect();
    assert_eq!(ids, [json!("c-admin")]);
}

/// draft-atwood-jmap-chat-00 §Space/set `updateMembers`: "A member MAY set
/// their own `nick` by submitting `updateMembers` with a single entry whose
/// `id` is the accountId and which contains only `nick`; the server MUST
/// permit this regardless of the caller's permission set."
#[tokio::test]
async fn space_self_nick_by_account_id_is_permitted() {
    let backend = IdentityBackend::new();
    seed_space_with_owner_member(&backend);

    let resp = space_set_update(
        &backend,
        json!({ "updateMembers": [{ "id": ACCOUNT_ID, "nick": "Al" }] }),
    )
    .await;
    assert!(resp["notUpdated"][SPACE_ID].is_null(), "{resp}");
    let members = space_members(&backend).await;
    let me = members
        .iter()
        .find(|m| m["id"] == ACCOUNT_ID)
        .expect("owner member");
    assert_eq!(me["nick"], "Al", "{me}");
}

/// draft-atwood-jmap-chat-00 §Space/join: "The server MUST verify that no
/// active SpaceBan exists for the requesting account's owner in the target
/// Space. If a ban exists, the server MUST reject the request with a
/// `forbidden` method error." The owner's SpaceBan carries the accountId
/// as its `userId` (§SpaceBan/get: "A banned user fetching their own
/// account sees only SpaceBan records where `userId` is the accountId.").
#[tokio::test]
async fn space_join_rejects_owner_banned_by_account_id() {
    let backend = IdentityBackend::new();
    seed_public_space(&backend);
    backend.inner().insert_object_for_test(
        "SpaceBan",
        ACCOUNT_ID,
        "ban1",
        json!({
            "id": "ban1",
            "spaceId": SPACE_ID,
            "userId": ACCOUNT_ID,
            "bannedBy": "c-admin",
            "createdAt": "2026-01-01T00:00:00Z",
        }),
    );

    let err = handle_space_join(
        &backend,
        &owner(),
        json!({ "accountId": ACCOUNT_ID, "spaceId": SPACE_ID }),
    )
    .await
    .expect_err("a banned owner must not join");
    assert_eq!(err.error_type.to_string(), "forbidden");
}

/// draft-atwood-jmap-chat-00 §SpaceMember `id`: "The participant's
/// ChatContact.id, or the accountId for the owner." Space/join adds the
/// owner.
#[tokio::test]
async fn space_join_records_owner_member_as_account_id() {
    let backend = IdentityBackend::new();
    seed_public_space(&backend);

    handle_space_join(
        &backend,
        &owner(),
        json!({ "accountId": ACCOUNT_ID, "spaceId": SPACE_ID }),
    )
    .await
    .expect("Space/join");
    let ids: Vec<Value> = space_members(&backend)
        .await
        .iter()
        .map(|m| m["id"].clone())
        .collect();
    assert_eq!(ids, [json!("c-admin"), json!(ACCOUNT_ID)]);
}

/// draft-atwood-jmap-chat-00 §SpaceBan `bannedBy`: "The ChatContact.id of
/// the Space member who issued this ban." The owner issuing it is
/// referenced by the accountId.
#[tokio::test]
async fn ban_created_by_owner_records_account_id_as_banned_by() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));

    let HandlerOutput { primary: resp, .. } = handle_ban_set(
        &backend,
        &owner(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "b0": { "spaceId": SPACE_ID, "userId": "c-target" } }
        }),
    )
    .await
    .expect("SpaceBan/set");
    assert_eq!(resp["created"]["b0"]["bannedBy"], ACCOUNT_ID, "{resp}");
}

/// draft-atwood-jmap-chat-00 §SpaceInvite `createdBy`: "ChatContact.id of
/// the member who created this invite." The owner creating it is
/// referenced by the accountId.
#[tokio::test]
async fn invite_created_by_owner_records_account_id_as_created_by() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));

    let HandlerOutput { primary: resp, .. } = handle_invite_set(
        &backend,
        &owner(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "i0": { "spaceId": SPACE_ID } }
        }),
    )
    .await
    .expect("SpaceInvite/set");
    assert_eq!(resp["created"]["i0"]["createdBy"], ACCOUNT_ID, "{resp}");
}

/// draft-atwood-jmap-chat-00 §CustomEmoji `createdBy`: "ChatContact.id of
/// the user who created this emoji." The owner creating it is referenced
/// by the accountId.
#[tokio::test]
async fn emoji_created_by_owner_records_account_id_as_created_by() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));

    let HandlerOutput { primary: resp, .. } = handle_emoji_set(
        &backend,
        &owner(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "e0": { "name": "wave", "blobId": "blob-wave-001" } }
        }),
    )
    .await
    .expect("CustomEmoji/set");
    assert_eq!(resp["created"]["e0"]["createdBy"], ACCOUNT_ID, "{resp}");
}
