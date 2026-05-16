//! Integration tests for the Role + Member variants of
//! [`MemoryBackend::apply_space_patch`] with caller identity wired
//! through [`common::IdentityBackend`] (bd:JMAP-g7wu.2.4.3).
//!
//! Covers acceptance criteria 1, 2, 3-replacement (last-admin
//! protection), 6 (whole-patch reject on permission failure), and 7
//! (single-user mode when `principal_id` returns `None`) from the
//! bead's re-scoped acceptance block.
//!
//! Tests that exercise the parse-time path or that don't need a
//! resolved caller identity live in `tests/integration.rs`. Tests
//! here always seed a `Space` directly via
//! `MemoryBackend::insert_object_for_test` and drive `handle_space_set`
//! through the `IdentityBackend`, which routes `apply_space_patch`
//! to `MemoryBackend::apply_space_patch_with_caller_id` to supply the
//! caller id.

mod common;

use common::{seed_space, seed_with_admin, IdentityBackend, MemoryBackend, ACCOUNT_ID, SPACE_ID};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_types::Id;
use serde_json::json;

// ---------------------------------------------------------------------------
// Criterion 7: single-user mode allows identity-dependent ops
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 7. When the backend's
/// `principal_id` returns `None` (the reference `MemoryBackend`'s
/// `CallerCtx = ()` default), permission gating and role-hierarchy
/// enforcement are skipped — every Role/Member op succeeds.
#[tokio::test]
async fn single_user_mode_skips_identity_dependent_gates() {
    let backend = MemoryBackend::new();

    // No members, no roles — caller has neither manage_roles nor
    // any role at a useful position. In single-user mode this is
    // still allowed.
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

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "Mod", "permissions": ["manage_members"], "position": 50
            }]}}
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "single-user mode should allow AddRole unconditionally: {resp:?}"
    );
}

// ---------------------------------------------------------------------------
// Criterion 1: permission gating against caller's effective permissions
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 1. A caller who is not a
/// member of the Space has no effective permissions; their Role/Member
/// ops are rejected with `forbidden`.
#[tokio::test]
async fn non_member_caller_lacks_all_permissions() {
    let backend = IdentityBackend::new();

    // Seed a Space with one admin who is NOT our caller.
    seed_with_admin(&backend, "admin-user");

    let outsider = Id::from("outsider");
    let (resp, _) = handle_space_set(
        &backend,
        &outsider,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "Mod", "permissions": [], "position": 50
            }]}}
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "non-member caller should fail with forbidden: {resp:?}"
    );
    let desc = resp["notUpdated"][SPACE_ID]["description"]
        .as_str()
        .unwrap_or("");
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
            "id": "caller",
            "roleIds": [],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "Mod", "permissions": [], "position": 50
            }]}}
        }),
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
    seed_with_admin(&backend, "admin-user");

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "Mod", "permissions": [], "position": 50
            }]}}
        }),
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

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 2 + draft §Space/set lines
/// 1096, 1102. A caller may only add or modify roles whose `position`
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
            "id": "mod-user",
            "roleIds": ["r-mod"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    // Try to add a role AT the same position (50) — must fail.
    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "PeerMod", "permissions": [], "position": 50
            }]}}
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "AddRole at caller's own position must be rejected (strictly-less rule): {resp:?}"
    );
    let desc = resp["notUpdated"][SPACE_ID]["description"]
        .as_str()
        .unwrap_or("");
    assert!(
        desc.contains("hierarchy") || desc.contains("position"),
        "error should mention hierarchy: {desc:?}"
    );

    // A role BELOW the caller's position succeeds.
    let (resp_ok, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [{
                "id": "placeholder", "name": "JuniorMod", "permissions": [], "position": 49
            }]}}
        }),
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
            "id": "mod-user",
            "roleIds": ["r-mod"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    // Update r-low (position 10, below caller) to position 100 (above
    // caller). Even though caller can modify r-low (existing position
    // 10 < 50), promoting it to 100 (>= 50) is forbidden.
    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "updateRoles": [{
                "id": "r-low", "position": 100
            }]}}
        }),
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
            "id": "mod-user",
            "roleIds": ["r-mod"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addMembers": [{
                "id": "u-new",
                "roleIds": ["r-admin"]
            }]}}
        }),
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

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 4 + draft §Space/set line
/// 1099. RemoveRole strips the role id from every member's
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
                "id": "admin-user",
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

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeRoles": ["r-mod"] }}
        }),
    )
    .await
    .expect("handle_space_set");
    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "RemoveRole should succeed: {resp:?}"
    );

    // Re-fetch and confirm cascade.
    let (get_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
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

// ---------------------------------------------------------------------------
// Criterion 3 replacement: last-admin protection knob
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 3 replacement. With
/// `protect_last_admin = true`, RemoveMember that would leave the
/// Space with zero `manage_members`/`manage_space` holders is
/// rejected with `forbidden`.
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
                "id": "admin-user",
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

    // Caller (the only admin) tries to remove themselves → blocked.
    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": ["admin-user"] }}
        }),
    )
    .await
    .expect("handle_space_set");
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "removing the last admin must be rejected when knob is on: {resp:?}"
    );
    let desc = resp["notUpdated"][SPACE_ID]["description"]
        .as_str()
        .unwrap_or("");
    assert!(
        desc.contains("last-admin") || desc.contains("manage_members"),
        "error should mention last-admin protection: {desc:?}"
    );

    // Confirm the admin is still a member (the patch was rejected
    // wholesale; no partial mutation).
    let (get_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("Space/get");
    let members = get_resp["list"][0]["members"].as_array().expect("members");
    assert!(
        members.iter().any(|m| m["id"] == "admin-user"),
        "admin-user must remain a member after the rejected removal"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.3 criterion 3 replacement. With
/// `protect_last_admin = false` (the reference backend default),
/// removing the last admin succeeds. The protection is a deployment-
/// policy stand-in only.
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
                "id": "admin-user",
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

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": ["admin-user"] }}
        }),
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
                "id": "admin-1",
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

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": ["admin-2"] }}
        }),
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
            "id": "mod-user",
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
    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: {
                "updateRoles": [{ "id": "r-target", "name": "Renamed" }],
                "addMembers": [{ "id": "u-new", "roleIds": [] }]
            }}
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "patch must be rejected: {resp:?}"
    );

    // Confirm the legal updateRoles op did NOT mutate state.
    let (get_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
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
        .unwrap_or("");
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
            "id": "mod-user",
            "roleIds": ["r-mod"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    // Patch with:
    //   addRoles[0] = legal (position 30 < caller's 50)
    //   addRoles[1] = ILLEGAL (position 100 >= caller's 50)
    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addRoles": [
                { "id": "p1", "name": "Legal", "permissions": [], "position": 30 },
                { "id": "p2", "name": "Illegal", "permissions": [], "position": 100 }
            ]}}
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "hierarchy failure rejects whole patch: {resp:?}"
    );

    // Confirm neither role landed.
    let (get_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
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
            "id": "admin",
            "roleIds": ["r-admin"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    // AddMember.
    let (add_resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addMembers": [{
                "id": "newbie",
                "roleIds": ["r-member"]
            }]}}
        }),
    )
    .await
    .expect("addMembers");
    assert!(
        add_resp["notUpdated"][SPACE_ID].is_null(),
        "addMembers should succeed: {add_resp:?}"
    );

    // UpdateMember nick.
    let (upd_resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "updateMembers": [{
                "id": "newbie",
                "nick": "Captain Newbie"
            }]}}
        }),
    )
    .await
    .expect("updateMembers");
    assert!(
        upd_resp["notUpdated"][SPACE_ID].is_null(),
        "updateMembers should succeed: {upd_resp:?}"
    );

    // Verify the nick landed.
    let (get_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
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
        .unwrap_or("");
    assert_eq!(nick, "Captain Newbie");

    // RemoveMember.
    let (rem_resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": ["newbie"] }}
        }),
    )
    .await
    .expect("removeMembers");
    assert!(
        rem_resp["notUpdated"][SPACE_ID].is_null(),
        "removeMembers should succeed: {rem_resp:?}"
    );

    let (get_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
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
    seed_with_admin(&backend, "admin");

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addMembers": [{
                "id": "admin",
                "roleIds": []
            }]}}
        }),
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
    seed_with_admin(&backend, "admin");

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addMembers": [{
                "id": "newbie",
                "roleIds": ["r-does-not-exist"]
            }]}}
        }),
    )
    .await
    .expect("addMembers");
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "invalidProperties",
        "unknown roleId should be invalidProperties: {resp:?}"
    );
}
