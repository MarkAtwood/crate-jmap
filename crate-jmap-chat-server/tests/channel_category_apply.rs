//! Integration tests for the Channel + Category permission gating in
//! [`MemoryBackend::apply_space_patch`] with caller identity wired
//! through [`common::IdentityBackend`] (bd:JMAP-g7wu.2.4.14).
//!
//! Background: bd:JMAP-g7wu.2.4.3 landed the Role/Member permission
//! pre-check inside `apply_space_patch_impl` but explicitly filtered
//! to Role/Member ops, leaving Channel and Category ops at the
//! backend without a `manage_channels` gate. bd:JMAP-g7wu.2.4.14
//! extends the same pre-validation pass (renamed to
//! `validate_space_patch_ops`) to cover all six Channel and Category
//! semantic-mutation ops:
//!
//!   addChannels / removeChannels / updateChannels
//!   addCategories / removeCategories / updateCategories
//!
//! All require `manage_channels` per draft-atwood-jmap-chat-00
//! §Space/set (the same permission gates both families per the
//! draft).
//!
//! Tests that exercise these ops without a resolved caller identity
//! continue to live in `tests/integration.rs` and pass through
//! `MemoryBackend` directly — its `CallerCtx = ()` and default
//! `principal_id` returns `None`, so identity-dependent gates are
//! skipped (single-user mode).
//!
//! The kitchen-sink mixed-op test (12 semantic-mutation keys in one
//! /set call) lives in bd:JMAP-g7wu.2.4.6.

mod common;

use common::{
    seed_space, seed_with_admin, seed_with_non_admin_caller, IdentityBackend, MemoryBackend,
    ACCOUNT_ID, SPACE_ID,
};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_types::Id;
use serde_json::json;

// ---------------------------------------------------------------------------
// Single-user mode: identity-independent path still allows ops
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.14 + bd:JMAP-g7wu.2.4.3 criterion 7. The
/// reference `MemoryBackend` with `CallerCtx = ()` returns `None` from
/// `principal_id`; identity-dependent gates (now including the
/// Channel/Category `manage_channels` gate) are skipped. Existing
/// integration tests in `tests/integration.rs` rely on this.
#[tokio::test]
async fn single_user_mode_skips_channel_permission_gate() {
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
            "update": { SPACE_ID: {
                "addChannels": [{ "name": "general" }]
            }}
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "single-user mode must allow addChannels unconditionally: {resp:?}"
    );
}

// ---------------------------------------------------------------------------
// Non-member caller (no effective permissions) is rejected
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.14. A caller who is not a member of the
/// Space has no effective permissions; every Channel/Category op is
/// rejected with `forbidden` and the description names
/// `manage_channels`.
#[tokio::test]
async fn non_member_caller_lacks_manage_channels() {
    let backend = IdentityBackend::new();
    seed_with_admin(&backend, "admin-user");
    let outsider = Id::from("outsider");

    let (resp, _) = handle_space_set(
        &backend,
        &outsider,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addChannels": [{ "name": "general" }] } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "non-member should be forbidden: {resp:?}"
    );
    let desc = resp["notUpdated"][SPACE_ID]["description"]
        .as_str()
        .expect("description must be a string");
    assert!(
        desc.contains("manage_channels"),
        "error must name the missing permission: {desc:?}"
    );
}

// ---------------------------------------------------------------------------
// Member with @everyone only: each of six Channel/Category ops rejected
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.14. A member with only the implicit
/// `@everyone` role has no `manage_channels` permission and is
/// rejected for `addChannels`.
#[tokio::test]
async fn everyone_only_member_cannot_add_channel() {
    let backend = IdentityBackend::new();
    let caller = Id::from("user");
    seed_with_non_admin_caller(&backend, "user");

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addChannels": [{ "name": "general" }] } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
    let desc = resp["notUpdated"][SPACE_ID]["description"]
        .as_str()
        .expect("description must be a string");
    assert!(
        desc.contains("manage_channels"),
        "error must name manage_channels: {desc:?}"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.14. Same gate fires on `removeChannels`.
#[tokio::test]
async fn everyone_only_member_cannot_remove_channel() {
    let backend = IdentityBackend::new();
    let caller = Id::from("user");
    seed_with_non_admin_caller(&backend, "user");

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeChannels": ["any-id"] } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
    let desc = resp["notUpdated"][SPACE_ID]["description"]
        .as_str()
        .expect("description must be a string");
    assert!(
        desc.contains("manage_channels"),
        "error must name manage_channels: {desc:?}"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.14. Same gate fires on `updateChannels`.
#[tokio::test]
async fn everyone_only_member_cannot_update_channel() {
    let backend = IdentityBackend::new();
    let caller = Id::from("user");
    seed_with_non_admin_caller(&backend, "user");

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: {
                "updateChannels": [{ "id": "any-id", "name": "renamed" }]
            }}
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.14. Same gate fires on `addCategories`.
#[tokio::test]
async fn everyone_only_member_cannot_add_category() {
    let backend = IdentityBackend::new();
    let caller = Id::from("user");
    seed_with_non_admin_caller(&backend, "user");

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: {
                "addCategories": [{
                    "id": "placeholder",
                    "name": "Voice",
                    "position": 0,
                    "channelIds": []
                }]
            }}
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
    let desc = resp["notUpdated"][SPACE_ID]["description"]
        .as_str()
        .expect("description must be a string");
    assert!(
        desc.contains("manage_channels"),
        "error must name manage_channels: {desc:?}"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.14. Same gate fires on `removeCategories`.
#[tokio::test]
async fn everyone_only_member_cannot_remove_category() {
    let backend = IdentityBackend::new();
    let caller = Id::from("user");
    seed_with_non_admin_caller(&backend, "user");

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeCategories": ["any-cat"] } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.14. Same gate fires on `updateCategories`.
#[tokio::test]
async fn everyone_only_member_cannot_update_category() {
    let backend = IdentityBackend::new();
    let caller = Id::from("user");
    seed_with_non_admin_caller(&backend, "user");

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: {
                "updateCategories": [{ "id": "any-cat", "name": "renamed" }]
            }}
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
}

// ---------------------------------------------------------------------------
// Member with manage_channels succeeds
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.14. A member with `manage_channels`
/// successfully applies `addChannels`. Sanity check that the gate
/// does not over-reject the affirmative case.
#[tokio::test]
async fn member_with_manage_channels_succeeds() {
    let backend = IdentityBackend::new();
    let caller = Id::from("editor");

    seed_space(
        &backend,
        json!([{
            "id": "r-editor",
            "name": "Editor",
            "permissions": ["manage_channels"],
            "position": 50
        }]),
        json!([{
            "id": "editor",
            "roleIds": ["r-editor"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: {
                "addChannels": [{ "name": "general" }]
            }}
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "manage_channels holder must succeed: {resp:?}"
    );
}

// ---------------------------------------------------------------------------
// Whole-patch reject on permission failure (criterion 6, extended)
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.14 + bd:JMAP-g7wu.2.4.3 criterion 6
/// applied to Channel ops. A patch carrying both `addChannels` AND
/// `addCategories` where the caller lacks `manage_channels` is
/// rejected atomically — no partial mutation. The legal sibling op
/// (a different family the caller IS authorized for) must also be
/// rolled back.
#[tokio::test]
async fn whole_patch_reject_on_channel_permission_failure() {
    let backend = IdentityBackend::new();
    let caller = Id::from("partial-admin");

    // Caller holds `manage_roles` but NOT `manage_channels`. The
    // patch carries one legal updateRoles op and one illegal
    // addChannels op. Pre-validation must catch the addChannels
    // permission failure and reject the whole patch.
    seed_space(
        &backend,
        json!([
            {
                "id": "r-pa",
                "name": "PartialAdmin",
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
            "id": "partial-admin",
            "roleIds": ["r-pa"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: {
                "updateRoles": [{ "id": "r-target", "name": "Renamed" }],
                "addChannels": [{ "name": "should-not-create" }]
            }}
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "whole patch must be rejected: {resp:?}"
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
        .expect("role name must be a string");
    assert_eq!(
        target_name, "Target",
        "the legal updateRoles op must NOT have applied — atomicity"
    );

    // Confirm no channel was created.
    let chan_count = get_resp["list"][0]["uncategorizedChannelIds"]
        .as_array()
        .expect("uncategorizedChannelIds must be an array")
        .len();
    assert_eq!(
        chan_count, 0,
        "the illegal addChannels op must NOT have created a channel"
    );
}

/// Oracle: bd:JMAP-g7wu.2.4.14 atomicity for the Category family.
/// Mixed `addCategories` (illegal) + `addCategories` (would-be-legal)
/// — actually we use one illegal op in a multi-entry array to verify
/// the per-op gate fires inside the single semantic-mutation key.
#[tokio::test]
async fn mixed_channel_category_patch_atomic_reject() {
    let backend = IdentityBackend::new();
    let caller = Id::from("user");
    seed_with_non_admin_caller(&backend, "user");

    // Caller has @everyone only. The patch carries both addChannels
    // and addCategories — both require manage_channels. The first
    // op to fail aborts the whole patch.
    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: {
                "addChannels": [{ "name": "ch1" }],
                "addCategories": [{
                    "id": "p",
                    "name": "Voice",
                    "position": 0,
                    "channelIds": []
                }]
            }}
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "whole patch must be rejected: {resp:?}"
    );

    // Confirm no mutation landed.
    let (get_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("Space/get");
    assert_eq!(
        get_resp["list"][0]["uncategorizedChannelIds"]
            .as_array()
            .expect("uncategorizedChannelIds must be an array")
            .len(),
        0,
        "no channel must have been created"
    );
    assert_eq!(
        get_resp["list"][0]["categories"]
            .as_array()
            .expect("categories must be an array")
            .len(),
        0,
        "no category must have been created"
    );
}
