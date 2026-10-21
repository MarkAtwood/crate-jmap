//! `Chat/set` updates of `pinnedMessageIds` are permission-gated per chat
//! kind (bd:JMAP-hd2y).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 §Chat
//! `pinnedMessageIds`: "Ordered list of pinned Message ids,
//! most-recently-pinned first. For group chats, only admins may modify
//! this list. For direct chats, the owner may modify it freely. For
//! channel chats, only members with the `"pin"` Space permission may
//! modify this list. Empty by default."

mod common;

use common::{seed_space, IdentityBackend, ACCOUNT_ID, SPACE_ID};
use jmap_chat_server::{handle_chat_get, handle_chat_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

const CHAT_ID: &str = "chat1";

/// Store a Chat of `kind` with an empty `pinnedMessageIds`, merging
/// `extra` kind-specific properties.
fn seed_chat(backend: &IdentityBackend, kind: &str, extra: Value) {
    let mut chat = json!({
        "id": CHAT_ID,
        "kind": kind,
        "createdAt": "2026-01-01T00:00:00Z",
        "unreadCount": 0,
        "pinnedMessageIds": [],
    });
    let (Value::Object(chat_map), Value::Object(extra_map)) = (&mut chat, extra) else {
        unreachable!("both are object literals");
    };
    chat_map.extend(extra_map);
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    backend
        .inner()
        .insert_object_for_test("Chat", ACCOUNT_ID, CHAT_ID, chat);
}

/// Seed a group Chat where the owner, whose member entry is the
/// accountId (draft §Terminology), has `owner_role` and `c-peer` has the
/// other role.
fn seed_group(backend: &IdentityBackend, owner_role: &str) {
    let peer_role = if owner_role == "admin" {
        "member"
    } else {
        "admin"
    };
    seed_chat(
        backend,
        "group",
        json!({
            "name": "g",
            "members": [
                { "id": ACCOUNT_ID, "role": owner_role, "joinedAt": "2026-01-01T00:00:00Z" },
                { "id": "c-peer", "role": peer_role, "joinedAt": "2026-01-01T00:00:00Z" },
            ],
        }),
    );
}

/// Seed a Space where exactly one of the owner (member entry =
/// accountId) and `c-peer` holds a role granting `pin`, plus a channel
/// Chat in that Space.
fn seed_channel(backend: &IdentityBackend, owner_may_pin: bool) {
    let (owner_roles, peer_roles) = if owner_may_pin {
        (json!(["r-pin"]), json!([]))
    } else {
        (json!([]), json!(["r-pin"]))
    };
    seed_space(
        backend,
        json!([{ "id": "r-pin", "name": "Pinner", "permissions": ["pin"], "position": 10 }]),
        json!([
            { "id": ACCOUNT_ID, "roleIds": owner_roles, "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "c-peer", "roleIds": peer_roles, "joinedAt": "2026-01-01T00:00:00Z" },
        ]),
    );
    seed_chat(
        backend,
        "channel",
        json!({ "name": "c", "spaceId": SPACE_ID }),
    );
}

/// Send `patch` as the owner and return the `Chat/set` response.
async fn update(backend: &IdentityBackend, patch: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        backend,
        &Id::from("owner"),
        json!({ "accountId": ACCOUNT_ID, "update": { CHAT_ID: patch } }),
    )
    .await
    .expect("Chat/set");
    resp
}

/// Return the stored `pinnedMessageIds` via `Chat/get`.
async fn pinned(backend: &IdentityBackend) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_chat_get(
        backend,
        &Id::from("admin"),
        json!({ "accountId": ACCOUNT_ID, "ids": [CHAT_ID] }),
    )
    .await
    .expect("Chat/get");
    resp["list"][0]["pinnedMessageIds"].clone()
}

fn assert_updated(resp: &Value) {
    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");
    assert!(resp["updated"].get(CHAT_ID).is_some(), "{resp:?}");
}

fn assert_forbidden(resp: &Value) {
    assert_eq!(resp["updated"], Value::Null, "{resp:?}");
    assert_eq!(resp["notUpdated"][CHAT_ID]["type"], "forbidden", "{resp:?}");
}

/// "For direct chats, the owner may modify it freely."
#[tokio::test]
async fn direct_chat_owner_may_update_pins() {
    let backend = IdentityBackend::new();
    seed_chat(&backend, "direct", json!({ "contactId": "peer" }));

    let resp = update(&backend, json!({ "pinnedMessageIds": ["m2", "m1"] })).await;

    assert_updated(&resp);
    assert_eq!(pinned(&backend).await, json!(["m2", "m1"]));
}

/// "For group chats, only admins may modify this list." (admin case)
#[tokio::test]
async fn group_chat_admin_may_update_pins() {
    let backend = IdentityBackend::new();
    seed_group(&backend, "admin");

    let resp = update(&backend, json!({ "pinnedMessageIds": ["m1"] })).await;

    assert_updated(&resp);
    assert_eq!(pinned(&backend).await, json!(["m1"]));
}

/// "For group chats, only admins may modify this list." (non-admin case)
#[tokio::test]
async fn group_chat_non_admin_forbidden() {
    let backend = IdentityBackend::new();
    seed_group(&backend, "member");

    let resp = update(&backend, json!({ "pinnedMessageIds": ["m1"] })).await;

    assert_forbidden(&resp);
    assert_eq!(pinned(&backend).await, json!([]));
}

/// "For group chats, only admins may modify this list." A JSON Pointer
/// path into the list (RFC 8620 §5.3 PatchObject) is a modification too.
#[tokio::test]
async fn group_chat_non_admin_pointer_path_forbidden() {
    let backend = IdentityBackend::new();
    seed_group(&backend, "member");

    let resp = update(&backend, json!({ "pinnedMessageIds/0": "m1" })).await;

    assert_forbidden(&resp);
    assert_eq!(pinned(&backend).await, json!([]));
}

/// "For channel chats, only members with the `"pin"` Space permission
/// may modify this list." (without the permission)
#[tokio::test]
async fn channel_without_pin_permission_forbidden() {
    let backend = IdentityBackend::new();
    seed_channel(&backend, false);

    let resp = update(&backend, json!({ "pinnedMessageIds": ["m1"] })).await;

    assert_forbidden(&resp);
    assert_eq!(pinned(&backend).await, json!([]));
}

/// "For channel chats, only members with the `"pin"` Space permission
/// may modify this list." (with the permission)
#[tokio::test]
async fn channel_with_pin_permission_may_update_pins() {
    let backend = IdentityBackend::new();
    seed_channel(&backend, true);

    let resp = update(&backend, json!({ "pinnedMessageIds": ["m1"] })).await;

    assert_updated(&resp);
    assert_eq!(pinned(&backend).await, json!(["m1"]));
}

/// Replace the channel seeded by [`seed_channel`] with one whose
/// `permissionOverrides` is `overrides`.
fn set_channel_overrides(backend: &IdentityBackend, overrides: Value) {
    seed_chat(
        backend,
        "channel",
        json!({ "name": "c", "spaceId": SPACE_ID, "permissionOverrides": overrides }),
    );
}

/// draft-atwood-jmap-chat-00 §Space Permission Resolution: "When
/// determining whether a member may perform an action in a channel Chat,
/// servers MUST evaluate permissions in this order", step 2: "Apply
/// `deny` entries from any ChannelPermission records matching the
/// member's roles (`targetType: \"role\"`)". The Space role grants
/// `pin`; the channel denies it for that role (bd:JMAP-559u).
#[tokio::test]
async fn channel_role_deny_removes_pin() {
    let backend = IdentityBackend::new();
    seed_channel(&backend, true);
    set_channel_overrides(
        &backend,
        json!([{ "targetId": "r-pin", "targetType": "role", "allow": [], "deny": ["pin"] }]),
    );

    let resp = update(&backend, json!({ "pinnedMessageIds": ["m1"] })).await;

    assert_forbidden(&resp);
    assert_eq!(pinned(&backend).await, json!([]));
}

/// §Space Permission Resolution step 5: "Apply `allow` entries from the
/// same member-targeted record". The owner holds no role granting `pin`;
/// a member-targeted allow grants it in this channel (bd:JMAP-559u).
#[tokio::test]
async fn channel_member_allow_grants_pin() {
    let backend = IdentityBackend::new();
    seed_channel(&backend, false);
    set_channel_overrides(
        &backend,
        json!([{ "targetId": ACCOUNT_ID, "targetType": "member", "allow": ["pin"], "deny": [] }]),
    );

    let resp = update(&backend, json!({ "pinnedMessageIds": ["m1"] })).await;

    assert_updated(&resp);
    assert_eq!(pinned(&backend).await, json!(["m1"]));
}
