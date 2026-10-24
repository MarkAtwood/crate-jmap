//! Integration tests for the Role + Member variants of
//! [`MemoryBackend::apply_space_patch`] with caller identity wired
//! through [`common::IdentityBackend`] (bd:JMAP-g7wu.2.4.3).
//!
//! Covers acceptance criteria 1, 2, 3-replacement (last-admin
//! protection), and 6 (whole-patch reject on permission failure) from
//! the bead's re-scoped acceptance block, plus the owner's gates when
//! `principal_id` returns `None` (bd:JMAP-yt1w.33).
//!
//! Tests that exercise the parse-time path or that don't need a
//! resolved caller identity live in `tests/integration.rs`. Tests
//! here always seed a `Space` directly via
//! `MemoryBackend::insert_object_for_test` and drive `handle_space_set`
//! through the `IdentityBackend`, which forwards `apply_space_patch`
//! to the wrapped `MemoryBackend`.

mod common;

use common::{ACCOUNT_ID, IdentityBackend, MemoryBackend, SPACE_ID, seed_space, seed_with_admin};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::json;

// ---------------------------------------------------------------------------
// No caller identity: the gates still apply to the owner's member entry
// ---------------------------------------------------------------------------

/// Oracle: draft-atwood-jmap-chat-00 §Space/set `addRoles`: "Requires
/// `"manage_roles"`." When the backend's `principal_id` returns `None`
/// (the reference `MemoryBackend`'s `CallerCtx = ()` default), the
/// owner, whose member entry is the accountId (§SpaceMember `id`), is
/// still gated (bd:JMAP-yt1w.33).
#[tokio::test]
async fn owner_without_member_entry_cannot_add_roles() {
    let backend = MemoryBackend::new();

    // No members, no roles — the owner has neither manage_roles nor
    // any role at a useful position.
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "Test",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 0,
            "categories": [],
            "uncategorizedChannelIds": [],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [],
            "members": []
        }),
    );

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "Mod", "permissions": ["manage_members"], "position": 50
            }]}}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "an owner without manage_roles must be refused AddRole: {resp:?}"
    );
}

// ---------------------------------------------------------------------------
// Criterion 1: permission gating against caller's effective permissions
// ---------------------------------------------------------------------------

/// Oracle: draft-atwood-jmap-chat-00 §Space/set `addRoles`: "Each
/// entry: `name` (String), `permissions` (String[]), `position`
/// (UnsignedInt) [...] Server assigns ULIDs." An entry without `id` is
/// accepted, and an `id` a client sends anyway is not stored: the role
/// gets a server-assigned id.
#[tokio::test]
async fn add_role_stores_server_assigned_id() {
    let backend = IdentityBackend::new();
    let caller = Id::from(ACCOUNT_ID);
    seed_with_admin(&backend, ACCOUNT_ID);

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [
                { "name": "NoId", "permissions": [], "position": 10 },
                { "id": "client-chosen", "name": "WithId", "permissions": [], "position": 20 }
            ]}}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");
    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "addRoles must succeed: {resp:?}"
    );

    let HandlerOutput {
        primary: get_resp, ..
    } = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");
    let roles = get_resp["list"][0]["roles"]
        .as_array()
        .expect("roles array");
    for name in ["NoId", "WithId"] {
        let role = roles
            .iter()
            .find(|r| r["name"] == name)
            .unwrap_or_else(|| panic!("role {name} stored: {roles:?}"));
        let id = role["id"].as_str().expect("role id is a string");
        assert!(
            !id.is_empty() && id != "client-chosen" && id != "r-admin",
            "role {name} must carry a fresh server-assigned id, got {id:?}"
        );
    }
}

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 1. A caller who is not a
/// member of the Space has no effective permissions; their Role/Member
/// ops are rejected with `forbidden`.
#[tokio::test]
async fn non_member_caller_lacks_all_permissions() {
    let backend = IdentityBackend::new();

    // Seed a Space with one admin who is NOT our caller.
    seed_with_admin(&backend, "admin-user");

    let outsider = Id::from("outsider");
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &outsider,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "Mod", "permissions": [], "position": 50
            }]}}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "non-member caller should fail with forbidden: {resp:?}"
    );
    let desc = resp["notUpdated"][SPACE_ID]["description"]
        .as_str()
        .expect("description must be a string");
    assert!(
        desc.contains("manage_roles"),
        "error should name the missing permission: {desc:?}"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 1. A caller who is a member
/// but holds no explicit roles (only the implicit `@everyone`) has
/// no Role/Member permissions and is rejected for any such op.
#[tokio::test]
async fn member_with_only_everyone_role_lacks_permissions() {
    let backend = IdentityBackend::new();

    // Seed the caller as a member with no role ids → `@everyone`-only.
    let caller = Id::from("caller");
    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_roles"],
            "position": 100
        }]),
        json!([{
            "id": ACCOUNT_ID,
            "roleIds": [],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "Mod", "permissions": [], "position": 50
            }]}}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "@everyone-only caller should fail: {resp:?}"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 1. A caller with the
/// required permissions and an adequate role-position succeeds.
#[tokio::test]
async fn admin_with_required_permissions_succeeds() {
    let backend = IdentityBackend::new();
    let caller = Id::from("admin-user");
    seed_with_admin(&backend, ACCOUNT_ID);

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "Mod", "permissions": [], "position": 50
            }]}}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "admin should succeed: {resp:?}"
    );
}

// ---------------------------------------------------------------------------
// Criterion 2: role-position hierarchy
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 2 + draft §Space/set
/// L1260, L1266. A caller may only add or modify roles whose `position`
/// is STRICTLY less than their own highest-position role.
#[tokio::test]
async fn add_role_at_caller_position_rejected() {
    let backend = IdentityBackend::new();
    let caller = Id::from("mod-user");

    // Caller has a "Moderator" role at position 50, with manage_roles
    // permission.
    seed_space(
        &backend,
        json!([{
            "id": "r-mod",
            "name": "Moderator",
            "permissions": ["manage_roles"],
            "position": 50
        }]),
        json!([{
            "id": ACCOUNT_ID,
            "roleIds": ["r-mod"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    // Try to add a role AT the same position (50) — must fail.
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "PeerMod", "permissions": [], "position": 50
            }]}}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "AddRole at caller's own position must be rejected (strictly-less rule): {resp:?}"
    );
    let desc = resp["notUpdated"][SPACE_ID]["description"]
        .as_str()
        .expect("description must be a string");
    assert!(
        desc.contains("hierarchy") || desc.contains("position"),
        "error should mention hierarchy: {desc:?}"
    );

    // A role BELOW the caller's position succeeds.
    let HandlerOutput {
        primary: resp_ok, ..
    } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "JuniorMod", "permissions": [], "position": 49
            }]}}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");
    assert!(
        resp_ok["notUpdated"][SPACE_ID].is_null(),
        "AddRole below caller's position should succeed: {resp_ok:?}"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 2. UpdateRole respects
/// hierarchy on BOTH the existing role's position (caller must
/// outrank it) AND the new position if `patch.position` is set
/// (caller must outrank the new position too).
#[tokio::test]
async fn update_role_hierarchy_checks_both_old_and_new_position() {
    let backend = IdentityBackend::new();
    let caller = Id::from("mod-user");

    seed_space(
        &backend,
        json!([
            {
                "id": "r-mod",
                "name": "Moderator",
                "permissions": ["manage_roles"],
                "position": 50
            },
            {
                "id": "r-low",
                "name": "Low",
                "permissions": [],
                "position": 10
            }
        ]),
        json!([{
            "id": ACCOUNT_ID,
            "roleIds": ["r-mod"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    // Update r-low (position 10, below caller) to position 100 (above
    // caller). Even though caller can modify r-low (existing position
    // 10 < 50), promoting it to 100 (>= 50) is forbidden.
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "updateRoles": [{
                "id": "r-low", "position": 100
            }]}}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "promoting a role above caller's ceiling must be rejected: {resp:?}"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 2. AddMember whose
/// `role_ids` include any role at or above the caller's highest
/// position is rejected.
#[tokio::test]
async fn add_member_with_role_above_caller_rejected() {
    let backend = IdentityBackend::new();
    let caller = Id::from("mod-user");

    seed_space(
        &backend,
        json!([
            {
                "id": "r-mod",
                "name": "Moderator",
                "permissions": ["manage_members"],
                "position": 50
            },
            {
                "id": "r-admin",
                "name": "Admin",
                "permissions": [],
                "position": 100
            }
        ]),
        json!([{
            "id": ACCOUNT_ID,
            "roleIds": ["r-mod"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addMembers": [{
                "id": "u-new",
                "roleIds": ["r-admin"]
            }]}}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "granting a role above caller's ceiling must be rejected: {resp:?}"
    );
}

// ---------------------------------------------------------------------------
// Criterion 4: RemoveRole cascade demotes members to @everyone-only
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 4 + draft §Space/set
/// L1263. RemoveRole strips the role id from every member's
/// `roleIds`. Members whose only role was the removed one end up
/// with empty `roleIds` (the @everyone-only state).
#[tokio::test]
async fn remove_role_cascades_demotion_to_everyone() {
    let backend = IdentityBackend::new();
    let caller = Id::from("admin-user");

    seed_space(
        &backend,
        json!([
            {
                "id": "r-admin",
                "name": "Admin",
                "permissions": ["manage_roles"],
                "position": 100
            },
            {
                "id": "r-mod",
                "name": "Mod",
                "permissions": [],
                "position": 50
            }
        ]),
        json!([
            {
                "id": ACCOUNT_ID,
                "roleIds": ["r-admin"],
                "joinedAt": "2026-01-01T00:00:00Z"
            },
            {
                "id": "u-multi",
                "roleIds": ["r-mod", "r-admin"],
                "joinedAt": "2026-01-01T00:00:00Z"
            },
            {
                "id": "u-mod-only",
                "roleIds": ["r-mod"],
                "joinedAt": "2026-01-01T00:00:00Z"
            }
        ]),
    );

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeRoles": ["r-mod"] }}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");
    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "RemoveRole should succeed: {resp:?}"
    );

    // Re-fetch and confirm cascade.
    let HandlerOutput {
        primary: get_resp, ..
    } = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");

    let members = get_resp["list"][0]["members"]
        .as_array()
        .expect("members array");

    // u-multi: still has r-admin.
    let multi = members
        .iter()
        .find(|m| m["id"] == "u-multi")
        .expect("u-multi");
    let multi_roles = multi["roleIds"].as_array().expect("roleIds");
    assert_eq!(multi_roles.len(), 1);
    assert_eq!(multi_roles[0], "r-admin");

    // u-mod-only: now @everyone-only (empty roleIds).
    let only = members
        .iter()
        .find(|m| m["id"] == "u-mod-only")
        .expect("u-mod-only");
    let only_roles = only["roleIds"].as_array().expect("roleIds");
    assert!(
        only_roles.is_empty(),
        "u-mod-only should be demoted to @everyone (empty roleIds), got {only_roles:?}"
    );
}

/// Oracle: draft-atwood-jmap-chat-00 §Space/set `removeRoles`
/// (jmap-chat-spec 2ac2603): "Servers SHOULD also remove each
/// channel's `permissionOverrides` entries whose `targetId` is a
/// removed role". The fixture is hand-written: `ch-a` carries an
/// override for the removed role plus two that must survive, `ch-b`
/// carries none for the removed role. Only `ch-a` changes, and
/// `Chat/changes` reports it as updated (bd:JMAP-yt1w.18).
#[tokio::test]
async fn remove_role_cascades_channel_permission_overrides() {
    let backend = IdentityBackend::new();
    let caller = Id::from("admin-user");

    seed_space(
        &backend,
        json!([
            { "id": "r-admin", "name": "Admin", "permissions": ["manage_roles"], "position": 100 },
            { "id": "r-mod", "name": "Mod", "permissions": [], "position": 50 }
        ]),
        json!([
            { "id": ACCOUNT_ID, "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" }
        ]),
    );
    let mod_override = json!({
        "targetId": "r-mod", "targetType": "role", "allow": ["send_message"], "deny": []
    });
    let admin_override = json!({
        "targetId": "r-admin", "targetType": "role", "allow": [], "deny": ["send_message"]
    });
    let member_override = json!({
        "targetId": "u-x", "targetType": "member", "allow": [], "deny": ["send_message"]
    });
    for (chat_id, overrides) in [
        (
            "ch-a",
            json!([mod_override, admin_override, member_override]),
        ),
        ("ch-b", json!([admin_override])),
    ] {
        backend.inner().insert_object_for_test(
            "Chat",
            ACCOUNT_ID,
            chat_id,
            json!({
                "id": chat_id,
                "kind": "channel",
                "spaceId": SPACE_ID,
                "name": chat_id,
                "createdAt": "2026-01-01T00:00:00Z",
                "unreadCount": 0,
                "pinnedMessageIds": [],
                "muted": false,
                "receiveTypingIndicators": true,
                "permissionOverrides": overrides
            }),
        );
    }

    let HandlerOutput {
        primary: before, ..
    } = jmap_chat_server::handle_chat_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [] }),
        &[],
    )
    .await
    .expect("Chat/get before");
    let since_state = before["state"].clone();

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeRoles": ["r-mod"] }}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");
    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "RemoveRole should succeed: {resp:?}"
    );

    let HandlerOutput { primary: got, .. } = jmap_chat_server::handle_chat_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": ["ch-a", "ch-b"] }),
        &[],
    )
    .await
    .expect("Chat/get after");
    let list = got["list"].as_array().expect("list");
    let overrides_of = |id: &str| {
        list.iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("{id} in {got:?}"))["permissionOverrides"]
            .clone()
    };
    assert_eq!(
        overrides_of("ch-a"),
        json!([admin_override, member_override])
    );
    assert_eq!(overrides_of("ch-b"), json!([admin_override]));

    let HandlerOutput {
        primary: changes, ..
    } = jmap_chat_server::handle_chat_changes(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "sinceState": since_state }),
        &[],
    )
    .await
    .expect("Chat/changes");
    assert_eq!(changes["updated"], json!(["ch-a"]), "{changes:?}");
    assert_eq!(changes["created"], json!([]), "{changes:?}");
    assert_eq!(changes["destroyed"], json!([]), "{changes:?}");
}

// ---------------------------------------------------------------------------
// Criterion 3 replacement: last-admin protection knob
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 3 replacement. With
/// `protect_last_admin = true`, RemoveMember that would leave the
/// Space with zero `manage_members`/`manage_space` holders is
/// rejected with `invalidProperties`.
///
/// Updated to draft §Space/set `removeMembers`: "Last-administrator
/// protection does not apply to self-removal." and "Servers MUST return
/// an `invalidProperties` SetError ({{RFC8620}} §5.3) naming
/// `removeMembers` when refusing on this basis." The patch therefore
/// also removes another member, so it is not self-removal.
#[tokio::test]
async fn last_admin_protection_blocks_removal_when_enabled() {
    let backend = IdentityBackend::new();
    backend.inner().set_protect_last_admin_for_test(true);
    let caller = Id::from("admin-user");

    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_members"],
            "position": 100
        }]),
        json!([
            {
                "id": ACCOUNT_ID,
                "roleIds": ["r-admin"],
                "joinedAt": "2026-01-01T00:00:00Z"
            },
            {
                "id": "u-regular",
                "roleIds": [],
                "joinedAt": "2026-01-01T00:00:00Z"
            }
        ]),
    );

    // Caller (the only admin) tries to remove themselves and another
    // member → blocked.
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": [ACCOUNT_ID, "u-regular"] }}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "invalidProperties",
        "removing the last admin must be rejected when knob is on: {resp:?}"
    );
    let desc = resp["notUpdated"][SPACE_ID]["description"]
        .as_str()
        .expect("description must be a string");
    assert!(
        desc.contains("last-admin") || desc.contains("manage_members"),
        "error should mention last-admin protection: {desc:?}"
    );

    // Confirm the admin is still a member (the patch was rejected
    // wholesale; no partial mutation).
    let HandlerOutput {
        primary: get_resp, ..
    } = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");
    let members = get_resp["list"][0]["members"].as_array().expect("members");
    assert!(
        members.iter().any(|m| m["id"] == ACCOUNT_ID),
        "the owner must remain a member after the rejected removal"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 3 replacement. With
/// `protect_last_admin = false` (the reference backend default),
/// removing the last admin succeeds. The protection is a deployment-
/// policy stand-in only. The patch also removes another member, so it is
/// not self-removal (draft §Space/set `removeMembers`), which is exempt
/// regardless of the knob.
#[tokio::test]
async fn last_admin_protection_disabled_allows_removal() {
    let backend = IdentityBackend::new();
    // The default is `false`; this is the explicit assertion.
    let caller = Id::from("admin-user");

    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_members"],
            "position": 100
        }]),
        json!([
            {
                "id": ACCOUNT_ID,
                "roleIds": ["r-admin"],
                "joinedAt": "2026-01-01T00:00:00Z"
            },
            {
                "id": "u-regular",
                "roleIds": [],
                "joinedAt": "2026-01-01T00:00:00Z"
            }
        ]),
    );

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": [ACCOUNT_ID, "u-regular"] }}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");
    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "removal must succeed when protection is off: {resp:?}"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 3 replacement. With the
/// knob on, removing one admin when another admin remains is fine.
#[tokio::test]
async fn last_admin_protection_allows_when_other_admin_remains() {
    let backend = IdentityBackend::new();
    backend.inner().set_protect_last_admin_for_test(true);
    let caller = Id::from("admin-1");

    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_members"],
            "position": 100
        }]),
        json!([
            {
                "id": ACCOUNT_ID,
                "roleIds": ["r-admin"],
                "joinedAt": "2026-01-01T00:00:00Z"
            },
            {
                "id": "admin-2",
                "roleIds": ["r-admin"],
                "joinedAt": "2026-01-01T00:00:00Z"
            }
        ]),
    );

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": ["admin-2"] }}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");
    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "removal of one admin when another remains must succeed: {resp:?}"
    );
}

// ---------------------------------------------------------------------------
// Criterion 6: whole-patch reject on permission failure
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 6. If any op in the patch
/// fails permission gating, the WHOLE update is rejected — earlier
/// successful ops do NOT mutate state.
#[tokio::test]
async fn whole_patch_reject_on_permission_failure_no_partial_mutation() {
    let backend = IdentityBackend::new();
    let caller = Id::from("mod-user");

    seed_space(
        &backend,
        json!([
            {
                "id": "r-mod",
                "name": "Mod",
                "permissions": ["manage_roles"],
                "position": 50
            },
            {
                "id": "r-target",
                "name": "Target",
                "permissions": [],
                "position": 10
            }
        ]),
        json!([{
            "id": ACCOUNT_ID,
            "roleIds": ["r-mod"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    // Patch with:
    //   updateRoles[0] = legal (renames r-target, position 10 < caller's 50)
    //   addMembers[0] = ILLEGAL (caller lacks manage_members)
    //
    // Pre-validation must catch the addMembers permission failure and
    // reject the whole patch; the updateRoles rename must NOT land.
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: {
                "updateRoles": [{ "id": "r-target", "name": "Renamed" }],
                "addMembers": [{ "id": "u-new", "roleIds": [] }]
            }}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "patch must be rejected: {resp:?}"
    );

    // Confirm the legal updateRoles op did NOT mutate state.
    let HandlerOutput {
        primary: get_resp, ..
    } = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");
    let target_name = get_resp["list"][0]["roles"]
        .as_array()
        .expect("roles")
        .iter()
        .find(|r| r["id"] == "r-target")
        .expect("r-target")
        .get("name")
        .and_then(|v| v.as_str())
        .expect("role name must be a string");
    assert_eq!(
        target_name, "Target",
        "the legal updateRoles op must NOT have applied — criterion 6 atomicity"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 6. A hierarchy failure on
/// any op rejects the whole patch with no partial mutation.
#[tokio::test]
async fn whole_patch_reject_on_hierarchy_failure() {
    let backend = IdentityBackend::new();
    let caller = Id::from("mod-user");

    seed_space(
        &backend,
        json!([{
            "id": "r-mod",
            "name": "Mod",
            "permissions": ["manage_roles"],
            "position": 50
        }]),
        json!([{
            "id": ACCOUNT_ID,
            "roleIds": ["r-mod"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    // Patch with:
    //   addRoles[0] = legal (position 30 < caller's 50)
    //   addRoles[1] = ILLEGAL (position 100 >= caller's 50)
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [
                { "id": "p1", "name": "Legal", "permissions": [], "position": 30 },
                { "id": "p2", "name": "Illegal", "permissions": [], "position": 100 }
            ]}}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "hierarchy failure rejects whole patch: {resp:?}"
    );

    // Confirm neither role landed.
    let HandlerOutput {
        primary: get_resp, ..
    } = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");
    let role_count = get_resp["list"][0]["roles"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    assert_eq!(
        role_count, 1,
        "only the original r-mod should remain; got {role_count} roles"
    );
}

// ---------------------------------------------------------------------------
// Misc: positive Member/Role flows under identity-bearing caller
// ---------------------------------------------------------------------------

/// Oracle: end-to-end positive Member flow with a real caller id.
/// Asserts that AddMember + UpdateMember + RemoveMember each land
/// correctly when the caller has the required permissions.
#[tokio::test]
async fn member_lifecycle_with_authenticated_admin() {
    let backend = IdentityBackend::new();
    let caller = Id::from("admin");

    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_members", "manage_roles"],
            "position": 100
        }, {
            "id": "r-member",
            "name": "Member",
            "permissions": [],
            "position": 10
        }]),
        json!([{
            "id": ACCOUNT_ID,
            "roleIds": ["r-admin"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    // AddMember.
    let HandlerOutput {
        primary: add_resp, ..
    } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addMembers": [{
                "id": "newbie",
                "roleIds": ["r-member"]
            }]}}
        }),
        &[],
    )
    .await
    .expect("addMembers");
    assert!(
        add_resp["notUpdated"][SPACE_ID].is_null(),
        "addMembers should succeed: {add_resp:?}"
    );

    // UpdateMember nick.
    let HandlerOutput {
        primary: upd_resp, ..
    } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "updateMembers": [{
                "id": "newbie",
                "nick": "Captain Newbie"
            }]}}
        }),
        &[],
    )
    .await
    .expect("updateMembers");
    assert!(
        upd_resp["notUpdated"][SPACE_ID].is_null(),
        "updateMembers should succeed: {upd_resp:?}"
    );

    // Verify the nick landed.
    let HandlerOutput {
        primary: get_resp, ..
    } = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");
    let nick = get_resp["list"][0]["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|m| m["id"] == "newbie")
        .expect("newbie")
        .get("nick")
        .and_then(|v| v.as_str())
        .expect("nick must be a string");
    assert_eq!(nick, "Captain Newbie");

    // RemoveMember.
    let HandlerOutput {
        primary: rem_resp, ..
    } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": ["newbie"] }}
        }),
        &[],
    )
    .await
    .expect("removeMembers");
    assert!(
        rem_resp["notUpdated"][SPACE_ID].is_null(),
        "removeMembers should succeed: {rem_resp:?}"
    );

    let HandlerOutput {
        primary: get_resp, ..
    } = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");
    let members = get_resp["list"][0]["members"].as_array().expect("members");
    assert!(!members.iter().any(|m| m["id"] == "newbie"));
    assert_eq!(
        get_resp["list"][0]["memberCount"], 1,
        "memberCount tracks members.len()"
    );
}

/// Oracle: AddMember rejects a duplicate user_id.
#[tokio::test]
async fn add_member_duplicate_user_id_rejected() {
    let backend = IdentityBackend::new();
    let caller = Id::from("admin");
    seed_with_admin(&backend, ACCOUNT_ID);

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addMembers": [{
                "id": ACCOUNT_ID,
                "roleIds": []
            }]}}
        }),
        &[],
    )
    .await
    .expect("addMembers");
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "invalidProperties",
        "duplicate userId should be invalidProperties: {resp:?}"
    );
}

/// Oracle: AddMember rejects a role_id that doesn't exist.
#[tokio::test]
async fn add_member_unknown_role_id_rejected() {
    let backend = IdentityBackend::new();
    let caller = Id::from("admin");
    seed_with_admin(&backend, ACCOUNT_ID);

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addMembers": [{
                "id": "newbie",
                "roleIds": ["r-does-not-exist"]
            }]}}
        }),
        &[],
    )
    .await
    .expect("addMembers");
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "invalidProperties",
        "unknown roleId should be invalidProperties: {resp:?}"
    );
}

/// Oracle: draft-atwood-jmap-chat-00 §Color Representation — "Servers MUST
/// accept all three representations in color-valued fields and MUST
/// preserve the representation provided by the client." §Space/set
/// updateRoles: "Each entry: `id` (String) and any of `name`, `color`,
/// `permissions`, `position`."
#[tokio::test]
async fn update_role_color_preserves_all_three_representations() {
    let backend = IdentityBackend::new();
    let caller = Id::from("admin");

    seed_space(
        &backend,
        json!([
            {
                "id": "r-admin",
                "name": "Admin",
                "permissions": ["manage_roles"],
                "position": 100
            },
            {
                "id": "r-target",
                "name": "Target",
                "color": "#000000",
                "permissions": [],
                "position": 10
            }
        ]),
        json!([{
            "id": ACCOUNT_ID,
            "roleIds": ["r-admin"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    for color in [
        json!({"$type": "color", "$value": {"colorSpace": "srgb", "components": [0.345, 0.396, 0.949], "hex": "#5865f2"}}),
        json!([62.5, 12.0, -30.25]),
        json!("#5865f2"),
    ] {
        let HandlerOutput { primary: resp, .. } = handle_space_set(
            &backend,
            &caller,
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "update": { SPACE_ID: { "updateRoles": [{
                    "id": "r-target", "color": color
                }]}}
            }),
            &[],
        )
        .await
        .expect("handle_space_set");
        assert!(
            resp["notUpdated"][SPACE_ID].is_null(),
            "updateRoles color {color} rejected: {resp:?}"
        );

        let HandlerOutput {
            primary: get_resp, ..
        } = handle_space_get(
            &backend,
            &caller,
            json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
            &[],
        )
        .await
        .expect("Space/get");
        let target = get_resp["list"][0]["roles"]
            .as_array()
            .expect("roles array")
            .iter()
            .find(|r| r["id"] == "r-target")
            .expect("r-target present");
        assert_eq!(target["color"], color, "color representation not preserved");
    }
}

// ---------------------------------------------------------------------------
// bd:JMAP-yt1w.13 — addRoles entries carry no id; the server assigns one
// ---------------------------------------------------------------------------

/// Oracle: draft-atwood-jmap-chat-00 §Space/set "Updating a Space",
/// `addRoles`: "Each entry: `name` (String), `permissions` (String[]),
/// `position` (UnsignedInt), and optionally `color` (String\|Object\|null,
/// optional) — per the color convention ({{color-convention}}). Server
/// assigns ULIDs." And §SpaceRole: "`id` (String, immutable, server-set):
/// Opaque server-assigned JMAP identifier for this role."
///
/// Entries without `id` must be accepted, and every added role — within
/// one call and across calls — must receive its own distinct id, or
/// `updateRoles` / `removeRoles` cannot address it.
#[tokio::test]
async fn add_roles_without_id_get_distinct_server_assigned_ids() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "Test",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 1,
            "categories": [],
            "uncategorizedChannelIds": [],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [{ "id": "r-owner", "name": "Owner", "permissions": ["manage_roles"], "position": 100 }],
            "members": [{ "id": ACCOUNT_ID, "roleIds": ["r-owner"], "joinedAt": "2026-01-01T00:00:00Z" }]
        }),
    );

    for add_roles in [
        json!([
            { "name": "Mod", "permissions": ["manage_members"], "position": 20 },
            { "name": "Helper", "permissions": [], "position": 10 }
        ]),
        json!([{ "name": "Guest", "permissions": [], "position": 5 }]),
    ] {
        let HandlerOutput { primary: resp, .. } = handle_space_set(
            &backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "update": { SPACE_ID: { "addRoles": add_roles } }
            }),
            &[],
        )
        .await
        .expect("handle_space_set");
        assert!(
            resp["notUpdated"][SPACE_ID].is_null(),
            "addRoles entries without id must be accepted: {resp:?}"
        );
        assert!(
            resp["updated"].get(SPACE_ID).is_some(),
            "addRoles should land in `updated`: {resp:?}"
        );
    }

    let HandlerOutput {
        primary: get_resp, ..
    } = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
        &[],
    )
    .await
    .expect("Space/get");
    let roles = get_resp["list"][0]["roles"]
        .as_array()
        .expect("roles array");
    let mut ids: Vec<&str> = roles
        .iter()
        .map(|r| r["id"].as_str().expect("role id is a string"))
        .filter(|id| *id != "r-owner")
        .collect();
    assert_eq!(ids.len(), 3, "three roles added: {roles:?}");
    assert!(ids.iter().all(|id| !id.is_empty()), "{roles:?}");
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(
        ids.len(),
        3,
        "server-assigned role ids must be distinct: {roles:?}"
    );
}

/// Oracle: draft-atwood-jmap-chat-00 §SpaceRole: "`id` (String,
/// immutable, server-set): Opaque server-assigned JMAP identifier for
/// this role." and §Space/set `addRoles`: "Server assigns ULIDs."
/// §ChannelPermission: "`targetId`: A SpaceRole id". A ULID is never
/// reissued, so a role added after `removeRoles` must not receive the
/// removed role's id, or stale references to it would bind to the new
/// role.
#[tokio::test]
async fn add_roles_after_remove_does_not_reuse_freed_id() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "Test",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 1,
            "categories": [],
            "uncategorizedChannelIds": [],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [{ "id": "r-owner", "name": "Owner", "permissions": ["manage_roles"], "position": 100 }],
            "members": [{ "id": ACCOUNT_ID, "roleIds": ["r-owner"], "joinedAt": "2026-01-01T00:00:00Z" }]
        }),
    );

    let update = |patch: serde_json::Value| {
        let backend = &backend;
        async move {
            let HandlerOutput { primary: resp, .. } = handle_space_set(
                backend,
                &(),
                &std::collections::BTreeMap::new(),
                json!({ "accountId": ACCOUNT_ID, "update": { SPACE_ID: patch } }),
                &[],
            )
            .await
            .expect("handle_space_set");
            assert!(
                resp["updated"].get(SPACE_ID).is_some(),
                "update should land in `updated`: {resp:?}"
            );
        }
    };
    let role_id_named = |name: &'static str| {
        let backend = &backend;
        async move {
            let HandlerOutput {
                primary: get_resp, ..
            } = handle_space_get(
                backend,
                &(),
                json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
                &[],
            )
            .await
            .expect("Space/get");
            get_resp["list"][0]["roles"]
                .as_array()
                .expect("roles array")
                .iter()
                .find(|r| r["name"] == name)
                .and_then(|r| r["id"].as_str())
                .unwrap_or_else(|| panic!("role {name} missing: {get_resp:?}"))
                .to_owned()
        }
    };

    update(json!({ "addRoles": [
        { "name": "A", "permissions": [], "position": 20 },
        { "name": "B", "permissions": [], "position": 10 }
    ] }))
    .await;
    let a_id = role_id_named("A").await;
    let b_id = role_id_named("B").await;

    update(json!({ "removeRoles": [b_id.clone()] })).await;
    update(json!({ "addRoles": [{ "name": "C", "permissions": [], "position": 5 }] })).await;
    let c_id = role_id_named("C").await;

    assert_ne!(c_id, b_id, "removed role's id was reissued");
    assert_ne!(c_id, a_id, "live role's id was reissued");
}
