//! Space/set `updateMembers`: the self-nick exemption, per
//! draft-atwood-jmap-chat-00 §Space/set.

mod common;

use common::{seed_space, IdentityBackend, ACCOUNT_ID, SPACE_ID};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_types::Id;
use serde_json::{json, Value};

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
            { "id": "u-regular", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
        ]),
    );
}

/// Space/set update of `SPACE_ID` by `caller` with the given `updateMembers`.
async fn update_members(backend: &IdentityBackend, caller: &str, entries: Value) -> Value {
    let (resp, _) = handle_space_set(
        backend,
        &Id::from(caller),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "updateMembers": entries }}
        }),
    )
    .await
    .expect("handle_space_set");
    resp
}

/// `member`'s nick as seen by `admin-user`.
async fn nick_of(backend: &IdentityBackend, member: &str) -> Value {
    let (resp, _) = handle_space_get(
        backend,
        &Id::from("admin-user"),
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
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
/// their own and which contains only `nick`; the server MUST permit this
/// regardless of the caller's permission set."
#[tokio::test]
async fn own_nick_without_manage_members_is_permitted() {
    let backend = IdentityBackend::new();
    admin_and_regular(&backend);

    let resp = update_members(
        &backend,
        "u-regular",
        json!([{ "id": "u-regular", "nick": "Captain" }]),
    )
    .await;
    assert!(resp["notUpdated"][SPACE_ID].is_null(), "{resp:?}");
    assert_eq!(nick_of(&backend, "u-regular").await, "Captain");
}

/// Oracle: draft §Space/set `updateMembers` — "Changing another member's
/// `nick` requires `"manage_members"`."
#[tokio::test]
async fn other_members_nick_without_manage_members_is_forbidden() {
    let backend = IdentityBackend::new();
    admin_and_regular(&backend);

    let resp = update_members(
        &backend,
        "u-regular",
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
/// single entry whose `id` is their own and which contains only `nick`";
/// "Role changes require `"manage_roles"`."
#[tokio::test]
async fn own_entry_with_role_ids_is_not_exempt() {
    let backend = IdentityBackend::new();
    admin_and_regular(&backend);

    let resp = update_members(
        &backend,
        "u-regular",
        json!([{ "id": "u-regular", "nick": "Captain", "roleIds": [] }]),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
    assert_eq!(nick_of(&backend, "u-regular").await, Value::Null);
}
