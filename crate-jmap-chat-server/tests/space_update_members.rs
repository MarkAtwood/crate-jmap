//! Space/set `updateMembers`: the self-nick exemption, the permission gate
//! for `roleIds` and the role-position hierarchy, per
//! draft-atwood-jmap-chat-00 §Space/set. The owner, who acts here, is the
//! caller; its member entry is the accountId (§Terminology).

mod common;

use common::{ACCOUNT_ID, IdentityBackend, SPACE_ID, seed_space};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

fn admin_and_regular(backend: &IdentityBackend) {
    seed_space(
        backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_space", "manage_members", "manage_roles"],
            "position": 100
        }]),
        json!([
            { "id": "admin-user", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": ACCOUNT_ID, "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
        ]),
    );
}

/// Space/set update of `SPACE_ID` by `caller` with the given `updateMembers`.
async fn update_members(backend: &IdentityBackend, caller: &str, entries: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        backend,
        &Id::from(caller),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "updateMembers": entries }}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");
    resp
}

/// `member`'s nick as seen by `admin-user`.
async fn nick_of(backend: &IdentityBackend, member: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_space_get(
        backend,
        &Id::from("admin-user"),
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");
    resp["list"][0]["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["id"] == member)
        .expect("member present")
        .get("nick")
        .cloned()
        .unwrap_or(Value::Null)
}

/// Oracle: draft §Space/set `updateMembers` — "A member MAY set their own
/// `nick` by submitting `updateMembers` with a single entry whose `id` is
/// the accountId and which contains only `nick`; the server MUST permit
/// this regardless of the caller's permission set."
#[tokio::test]
async fn own_nick_without_manage_members_is_permitted() {
    let backend = IdentityBackend::new();
    admin_and_regular(&backend);

    let resp = update_members(
        &backend,
        ACCOUNT_ID,
        json!([{ "id": ACCOUNT_ID, "nick": "Captain" }]),
    )
    .await;
    assert!(resp["notUpdated"][SPACE_ID].is_null(), "{resp:?}");
    assert_eq!(nick_of(&backend, ACCOUNT_ID).await, "Captain");
}

/// Oracle: draft §Space/set `updateMembers` — "Changing another member's
/// `nick` requires `"manage_members"`."
#[tokio::test]
async fn other_members_nick_without_manage_members_is_forbidden() {
    let backend = IdentityBackend::new();
    admin_and_regular(&backend);

    let resp = update_members(
        &backend,
        ACCOUNT_ID,
        json!([{ "id": "admin-user", "nick": "Boss" }]),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
    assert_eq!(nick_of(&backend, "admin-user").await, Value::Null);
}

/// Oracle: draft §Space/set `updateMembers` — the exemption covers only "a
/// single entry whose `id` is the accountId and which contains only
/// `nick`";
/// "Role changes require `"manage_roles"`."
#[tokio::test]
async fn own_entry_with_role_ids_is_not_exempt() {
    let backend = IdentityBackend::new();
    admin_and_regular(&backend);

    let resp = update_members(
        &backend,
        ACCOUNT_ID,
        json!([{ "id": ACCOUNT_ID, "nick": "Captain", "roleIds": [] }]),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
    assert_eq!(nick_of(&backend, ACCOUNT_ID).await, Value::Null);
}

/// Roles at positions 100 (`r-high`), 50 (`r-mod`, `manage_roles` only)
/// and 10 (`r-low`). The owner (accountId) holds `r-mod`; `admin-user`
/// holds `r-high`; `plain-user` holds nothing.
fn role_manager_only(backend: &IdentityBackend) {
    seed_space(
        backend,
        json!([
            {
                "id": "r-high",
                "name": "Admin",
                "permissions": ["manage_space", "manage_members", "manage_roles"],
                "position": 100
            },
            { "id": "r-mod", "name": "Mod", "permissions": ["manage_roles"], "position": 50 },
            { "id": "r-low", "name": "Helper", "permissions": [], "position": 10 }
        ]),
        json!([
            { "id": "admin-user", "roleIds": ["r-high"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": ACCOUNT_ID, "roleIds": ["r-mod"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "plain-user", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
        ]),
    );
}

/// `member`'s `roleIds` as seen by `admin-user`.
async fn role_ids_of(backend: &IdentityBackend, member: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_space_get(
        backend,
        &Id::from("admin-user"),
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");
    resp["list"][0]["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["id"] == member)
        .expect("member present")["roleIds"]
        .clone()
}

/// Oracle: draft-atwood-jmap-chat-00 L1277 — "Role changes require
/// `"manage_roles"`." No other permission is named, so a caller holding
/// only `manage_roles` may grant a role ranked below its own.
#[tokio::test]
async fn role_change_needs_only_manage_roles() {
    let backend = IdentityBackend::new();
    role_manager_only(&backend);

    let resp = update_members(
        &backend,
        ACCOUNT_ID,
        json!([{ "id": "plain-user", "roleIds": ["r-low"] }]),
    )
    .await;
    assert!(resp["notUpdated"][SPACE_ID].is_null(), "{resp:?}");
    assert_eq!(role_ids_of(&backend, "plain-user").await, json!(["r-low"]));
}

/// Oracle: draft-atwood-jmap-chat-00 L1277 — "Changing another member's
/// `nick` requires `"manage_members"`." `manage_roles` does not cover the
/// `nick` half of the entry.
#[tokio::test]
async fn role_change_with_other_nick_needs_manage_members() {
    let backend = IdentityBackend::new();
    role_manager_only(&backend);

    let resp = update_members(
        &backend,
        ACCOUNT_ID,
        json!([{ "id": "plain-user", "roleIds": ["r-low"], "nick": "Helper" }]),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
    assert_eq!(role_ids_of(&backend, "plain-user").await, json!([]));
}

/// Oracle: draft-atwood-jmap-chat-00 L1240 — "Adding a member checks
/// role-hierarchy constraints"; L1631 — "members may only create or modify
/// SpaceRoles whose `position` is strictly less than their own
/// highest-position role." Stripping `r-high` (100) from another member is
/// a role change above the caller's `r-mod` (50).
#[tokio::test]
async fn removing_role_above_caller_is_forbidden() {
    let backend = IdentityBackend::new();
    role_manager_only(&backend);

    let resp = update_members(
        &backend,
        ACCOUNT_ID,
        json!([{ "id": "admin-user", "roleIds": [] }]),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
    assert_eq!(role_ids_of(&backend, "admin-user").await, json!(["r-high"]));
}

/// Oracle: draft-atwood-jmap-chat-00 L1240 / L1631, as for
/// `removing_role_above_caller_is_forbidden`: granting `r-high` (100)
/// is above the caller's `r-mod` (50).
#[tokio::test]
async fn granting_role_above_caller_is_forbidden() {
    let backend = IdentityBackend::new();
    role_manager_only(&backend);

    let resp = update_members(
        &backend,
        ACCOUNT_ID,
        json!([{ "id": "plain-user", "roleIds": ["r-high"] }]),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
    assert_eq!(role_ids_of(&backend, "plain-user").await, json!([]));
}

/// Oracle: draft-atwood-jmap-chat-00 L1277 — "Role changes require
/// `"manage_roles"`"; the hierarchy (L1631) bounds the roles changed.
/// Keeping `r-high` on `admin-user` while granting `r-low` changes only
/// `r-low`, which ranks below the caller's `r-mod`.
#[tokio::test]
async fn unchanged_role_above_caller_is_not_a_change() {
    let backend = IdentityBackend::new();
    role_manager_only(&backend);

    let resp = update_members(
        &backend,
        ACCOUNT_ID,
        json!([{ "id": "admin-user", "roleIds": ["r-high", "r-low"] }]),
    )
    .await;
    assert!(resp["notUpdated"][SPACE_ID].is_null(), "{resp:?}");
    assert_eq!(
        role_ids_of(&backend, "admin-user").await,
        json!(["r-high", "r-low"])
    );
}
