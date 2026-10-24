//! `Space/set` create makes the caller an administering member of the
//! new Space (bd:JMAP-yt1w.34).
//!
//! Oracle: draft-atwood-jmap-chat-00 §Space/set "Creating a Space"
//! (jmap-chat-spec commit cefc217): "The server assigns the Space's
//! `id` and adds the caller to `members` with at least one role granting
//! permissions sufficient to administer the Space." §SpaceMember `id`:
//! "The participant's ChatContact.id, or the accountId for the owner."
//! The permission vocabulary is the closed list in §SpaceRole
//! `permissions`.

mod common;

use common::{ACCOUNT_ID, IdentityBackend, MemoryBackend};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Every permission name defined in draft-atwood-jmap-chat-00 §SpaceRole
/// `permissions` (cefc217).
const ALL_PERMISSIONS: [&str; 10] = [
    "view",
    "send",
    "pin",
    "manage_channels",
    "manage_members",
    "manage_roles",
    "manage_space",
    "ban",
    "mention_broadcast",
    "start_call",
];

/// 2^53-1, the largest JMAP UnsignedInt (RFC 8620 §1.3).
const MAX_UNSIGNED_INT: u64 = 9_007_199_254_740_991;

/// Create a name-only Space as the owner of [`ACCOUNT_ID`] and return
/// the full `Space/set` response.
async fn create_space(backend: &MemoryBackend) -> Value {
    backend.register_account(&Id::from(ACCOUNT_ID));
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "create": { "s0": { "name": "New" } } }),
        &[],
    )
    .await
    .expect("Space/set create");
    resp
}

/// Return the owner's member entry (id == accountId) of `space` and the
/// union of the permissions of the roles it holds.
fn owner_permissions(space: &Value) -> (Value, BTreeSet<String>) {
    let members = space["members"]
        .as_array()
        .unwrap_or_else(|| panic!("Space has no members array: {space:?}"));
    let owner = members
        .iter()
        .find(|m| m["id"] == ACCOUNT_ID)
        .unwrap_or_else(|| panic!("creator (accountId) is not a member: {space:?}"))
        .clone();
    let role_ids = owner["roleIds"]
        .as_array()
        .unwrap_or_else(|| panic!("owner member has no roleIds: {space:?}"));
    let roles = space["roles"]
        .as_array()
        .unwrap_or_else(|| panic!("Space has no roles array: {space:?}"));
    let mut perms = BTreeSet::new();
    for rid in role_ids {
        let role = roles
            .iter()
            .find(|r| &r["id"] == rid)
            .unwrap_or_else(|| panic!("owner roleId {rid} is not a Space role: {space:?}"));
        for p in role["permissions"].as_array().expect("role permissions") {
            perms.insert(p.as_str().expect("permission string").to_owned());
        }
    }
    (owner, perms)
}

/// draft-atwood-jmap-chat-00 §Space/set "Creating a Space" (cefc217):
/// "The server assigns the Space's `id` and adds the caller to `members`
/// with at least one role granting permissions sufficient to administer
/// the Space." The caller is the owner, whose member entry is the
/// accountId (§SpaceMember `id`). Checked in both the create response and
/// a subsequent `Space/get`.
#[tokio::test]
async fn space_create_adds_creator_as_member_with_admin_role() {
    let backend = MemoryBackend::new();
    let resp = create_space(&backend).await;
    let created = &resp["created"]["s0"];
    let space_id = created["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no server-assigned id: {resp:?}"))
        .to_owned();

    let HandlerOutput { primary: get, .. } = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [space_id] }),
        &[],
    )
    .await
    .expect("Space/get");

    for space in [created, &get["list"][0]] {
        let (owner, perms) = owner_permissions(space);
        assert!(
            !owner["roleIds"].as_array().expect("roleIds").is_empty(),
            "creator must hold at least one role: {space:?}"
        );
        for p in ["manage_space", "manage_roles", "manage_members"] {
            assert!(perms.contains(p), "creator lacks {p}: {space:?}");
        }
    }
}

/// draft-atwood-jmap-chat-00 §Space/set "Creating a Space" (cefc217):
/// "The server assigns the Space's `id` and adds the caller to `members`
/// with at least one role granting permissions sufficient to administer
/// the Space." Administering means the gated updates succeed: §Updating a
/// Space `name`: "Metadata fields. Require `"manage_space"` permission."
/// and `addRoles`: "Requires `"manage_roles"`. Members may only add roles
/// whose `position` is strictly less than their own highest-position
/// role; servers MUST enforce this." The caller is resolved through
/// `IdentityBackend` (principal `user-alice`) so the gates are enforced
/// against the owner's member entry, the accountId (§SpaceMember `id`).
#[tokio::test]
async fn space_create_creator_can_administer_new_space() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    let caller = Id::from("user-alice");
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "create": { "s0": { "name": "New" } } }),
        &[],
    )
    .await
    .expect("Space/set create");
    let space_id = resp["created"]["s0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no server-assigned id: {resp:?}"))
        .to_owned();

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { &space_id: {
                "name": "Renamed",
                "addRoles": [{ "name": "Mod", "permissions": ["pin"], "position": 50 }]
            } }
        }),
        &[],
    )
    .await
    .expect("Space/set update");
    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");
    assert!(
        resp["updated"].get(&space_id).is_some(),
        "creator's admin update must succeed: {resp:?}"
    );
}

/// draft-atwood-jmap-chat-00 §SpaceMember `id` (cefc217): "The
/// participant's ChatContact.id, or the accountId for the owner." With a
/// backend whose principal (`user-alice`) differs from the accountId,
/// the creator's member entry is still the accountId, per §Space/set
/// "Creating a Space": "adds the caller to `members` with at least one
/// role granting permissions sufficient to administer the Space."
#[tokio::test]
async fn space_create_creator_member_entry_is_account_id_not_principal() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &Id::from("user-alice"),
        &std::collections::BTreeMap::new(),
        json!({ "accountId": ACCOUNT_ID, "create": { "s0": { "name": "New" } } }),
        &[],
    )
    .await
    .expect("Space/set create");
    let created = &resp["created"]["s0"];
    let (_, perms) = owner_permissions(created);
    assert!(perms.contains("manage_space"), "{resp:?}");
    assert!(
        created["members"]
            .as_array()
            .expect("members")
            .iter()
            .all(|m| m["id"] != "user-alice"),
        "member entry must not be the principal: {resp:?}"
    );
}

/// Bootstrap configuration chosen for the reference server (Mark decision
/// on bd:JMAP-yt1w.34), not a draft requirement: draft-atwood-jmap-chat-00
/// §Space/set "Creating a Space" (cefc217) says "The specific
/// bootstrap-role configuration is server-defined; servers MAY pre-create
/// a high-position role for the caller, MAY assign every permission from
/// {{space-permissions}}, or MAY use any other mechanism that ensures the
/// newly-created Space is administrable." One role named "Admin" at position 2^53-1
/// holding every §SpaceRole permission, and the creator holds exactly it.
#[tokio::test]
async fn space_create_bootstrap_role_is_max_position_admin_with_every_permission() {
    let backend = MemoryBackend::new();
    let resp = create_space(&backend).await;
    let created = &resp["created"]["s0"];

    let roles = created["roles"].as_array().expect("roles");
    assert_eq!(roles.len(), 1, "{resp:?}");
    let role = &roles[0];
    assert_eq!(role["name"], "Admin", "{resp:?}");
    assert_eq!(role["position"], json!(MAX_UNSIGNED_INT), "{resp:?}");
    let perms: BTreeSet<&str> = role["permissions"]
        .as_array()
        .expect("permissions")
        .iter()
        .map(|p| p.as_str().expect("permission string"))
        .collect();
    assert_eq!(perms, BTreeSet::from(ALL_PERMISSIONS), "{resp:?}");

    let members = created["members"].as_array().expect("members");
    assert_eq!(members.len(), 1, "{resp:?}");
    assert_eq!(members[0]["id"], ACCOUNT_ID, "{resp:?}");
    assert_eq!(members[0]["roleIds"], json!([role["id"]]), "{resp:?}");
}
