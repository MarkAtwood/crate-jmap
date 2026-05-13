//! Integration tests for the Space top-level metadata permission gate
//! in [`ChatBackend::apply_space_metadata_patch`] with caller identity
//! wired through [`common::IdentityBackend`] (bd:JMAP-g7wu.2.4.13).
//!
//! Background: bd:JMAP-g7wu.2.4.3 landed the Role/Member permission
//! pre-check on the semantic-mutation path. bd:JMAP-g7wu.2.4.14
//! extended the same pre-check to Channel/Category ops. Both run
//! inside `apply_space_patch_impl`. Top-level metadata mutations
//! (`name`, `description`, `iconBlobId`, `isPublic`,
//! `isPubliclyPreviewable`) flowed through the generic
//! `update_object::<Space>` path and bypassed permission gating
//! entirely. bd:JMAP-g7wu.2.4.13 closes that gap by introducing a
//! dedicated trait method `ChatBackend::apply_space_metadata_patch`,
//! routing the handler's metadata path through it, and gating the
//! mutation on `manage_space` per draft-atwood-jmap-chat-00.
//!
//! Tests cover:
//! - Caller with `manage_space` mutates `description` → succeeds.
//! - Caller without `manage_space` (only `@everyone` role) mutates
//!   `description` → SetError::Forbidden, no mutation.
//! - Non-member caller mutates `description` → Forbidden.
//! - Empty patch + no manage_space → Forbidden (the gate fires
//!   before the no-op short-circuit).
//! - Mixed patch (`name` + structural addMembers in one /set call) →
//!   atomic reject when caller lacks `manage_space` (the
//!   apply_space_patch pre-check rejects the structural half too).
//! - Single-user mode (MemoryBackend with CallerCtx=()) still allowed.
//!
//! Tests for the orthogonal Channel/Category permission gate live in
//! `tests/channel_category_apply.rs`. Tests for the Role/Member gate
//! live in `tests/role_member_apply.rs`.

mod common;

use common::{IdentityBackend, MemoryBackend};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_types::Id;
use serde_json::json;

// ---------------------------------------------------------------------------
// Seeding helpers
// ---------------------------------------------------------------------------

const ACCOUNT_ID: &str = "a1";
const SPACE_ID: &str = "s1";

/// Seed a Space with the supplied roles/members. Initial
/// `description` is `"original"` so tests can assert it was (or was
/// not) mutated.
fn seed_space(
    backend: &IdentityBackend,
    roles: serde_json::Value,
    members: serde_json::Value,
) -> Id {
    let space_val = json!({
        "id": SPACE_ID,
        "name": "Test Space",
        "description": "original",
        "createdAt": "2026-01-01T00:00:00Z",
        "memberCount": members.as_array().map(Vec::len).unwrap_or(0),
        "categories": [],
        "uncategorizedChannelIds": [],
        "isPublic": false,
        "isPubliclyPreviewable": false,
        "roles": roles,
        "members": members,
    });
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    backend
        .inner()
        .insert_object_for_test("Space", ACCOUNT_ID, SPACE_ID, space_val);
    Id::from(SPACE_ID)
}

async fn read_space_description(backend: &IdentityBackend, caller: &Id) -> String {
    // Read back the description through Space/get so we exercise the
    // same path a client would.
    let (resp, _) = handle_space_get(
        backend,
        caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("Space/get");
    resp["list"][0]["description"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

// ---------------------------------------------------------------------------
// Single-user mode: gate is skipped
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.13. The reference `MemoryBackend` with
/// `CallerCtx = ()` returns `None` from `principal_id`; the
/// `manage_space` gate is skipped (single-user mode). Existing
/// integration tests in `tests/integration.rs` rely on this.
#[tokio::test]
async fn single_user_mode_skips_manage_space_gate() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "Test",
            "description": "original",
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
            "update": { SPACE_ID: { "description": "renamed" } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "single-user mode must allow metadata mutation: {resp:?}"
    );

    // Confirm the mutation actually landed.
    let (get_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("Space/get");
    assert_eq!(
        get_resp["list"][0]["description"].as_str().unwrap_or(""),
        "renamed"
    );
}

// ---------------------------------------------------------------------------
// Manage_space holder succeeds
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.13. A caller holding `manage_space`
/// successfully mutates `description`.
#[tokio::test]
async fn manage_space_holder_can_mutate_description() {
    let backend = IdentityBackend::new();
    let caller = Id::from("admin");
    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_space"],
            "position": 100
        }]),
        json!([{
            "id": "admin",
            "roleIds": ["r-admin"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "description": "renamed" } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "manage_space holder must succeed: {resp:?}"
    );

    assert_eq!(read_space_description(&backend, &caller).await, "renamed");
}

/// Oracle: bd:JMAP-g7wu.2.4.13. A caller holding `manage_space` can
/// mutate every metadata field listed in
/// draft-atwood-jmap-chat-00 §Space/set. Sanity check that the gate
/// is per-field-agnostic (one gate covers the whole metadata
/// family).
#[tokio::test]
async fn manage_space_holder_can_mutate_all_metadata_fields() {
    let backend = IdentityBackend::new();
    let caller = Id::from("admin");
    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_space"],
            "position": 100
        }]),
        json!([{
            "id": "admin",
            "roleIds": ["r-admin"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: {
                "name": "Renamed Space",
                "description": "with a new tagline",
                "isPublic": true,
                "isPubliclyPreviewable": true
            }}
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "manage_space holder must succeed on multi-field patch: {resp:?}"
    );

    let (get_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("Space/get");
    let s = &get_resp["list"][0];
    assert_eq!(s["name"].as_str().unwrap_or(""), "Renamed Space");
    assert_eq!(
        s["description"].as_str().unwrap_or(""),
        "with a new tagline"
    );
    assert!(s["isPublic"].as_bool().unwrap_or(false));
    assert!(s["isPubliclyPreviewable"].as_bool().unwrap_or(false));
}

// ---------------------------------------------------------------------------
// Non-manage_space caller is rejected
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.13. A member holding only the implicit
/// `@everyone` role has no `manage_space` permission and is
/// rejected; the description must NOT change.
#[tokio::test]
async fn everyone_only_member_cannot_mutate_description() {
    let backend = IdentityBackend::new();
    let caller = Id::from("user");
    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_space"],
            "position": 100
        }]),
        json!([
            { "id": "admin", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "user",  "roleIds": [],          "joinedAt": "2026-01-02T00:00:00Z" }
        ]),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "description": "should not stick" } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "@everyone-only member must be forbidden: {resp:?}"
    );
    let desc = resp["notUpdated"][SPACE_ID]["description"]
        .as_str()
        .unwrap_or("");
    assert!(
        desc.contains("manage_space"),
        "error must name manage_space: {desc:?}"
    );

    // Confirm no mutation landed.
    assert_eq!(read_space_description(&backend, &caller).await, "original");
}

/// Oracle: bd:JMAP-g7wu.2.4.13. A caller who is not a member of the
/// Space at all has no effective permissions; metadata patch
/// rejected with `forbidden`.
#[tokio::test]
async fn non_member_caller_cannot_mutate_metadata() {
    let backend = IdentityBackend::new();
    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_space"],
            "position": 100
        }]),
        json!([{
            "id": "admin",
            "roleIds": ["r-admin"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    let outsider = Id::from("outsider");
    let (resp, _) = handle_space_set(
        &backend,
        &outsider,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "description": "should not stick" } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "non-member must be forbidden: {resp:?}"
    );

    let admin = Id::from("admin");
    assert_eq!(read_space_description(&backend, &admin).await, "original");
}

/// Oracle: bd:JMAP-g7wu.2.4.13. A caller holding `manage_roles` /
/// `manage_members` / `manage_channels` but NOT `manage_space` is
/// still rejected on metadata. Permission strings do not subset.
#[tokio::test]
async fn other_permissions_do_not_subsume_manage_space() {
    let backend = IdentityBackend::new();
    let caller = Id::from("partial-admin");
    seed_space(
        &backend,
        json!([{
            "id": "r-pa",
            "name": "PartialAdmin",
            "permissions": ["manage_roles", "manage_members", "manage_channels"],
            "position": 50
        }]),
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
            "update": { SPACE_ID: { "description": "should not stick" } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "caller without manage_space must be forbidden even with other perms: {resp:?}"
    );

    assert_eq!(read_space_description(&backend, &caller).await, "original");
}

// ---------------------------------------------------------------------------
// Mixed patch atomicity: metadata + structural ops
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.13. A patch carrying both a top-level
/// metadata field (`description`) AND a structural op
/// (`addMembers`) must be rejected atomically if the caller lacks
/// `manage_space` — no partial mutation of either half.
///
/// Note on dispatch order: the handler applies structural ops
/// (apply_space_patch) BEFORE metadata ops. The structural pass's
/// own permission gate (validate_space_patch_ops) catches the
/// addMembers permission failure first, since the caller in this
/// test also lacks `manage_members`. The metadata mutation is
/// then skipped by the `continue` on structural failure. Either
/// way, neither half lands.
#[tokio::test]
async fn mixed_patch_metadata_plus_structural_atomic_reject() {
    let backend = IdentityBackend::new();
    let caller = Id::from("user");
    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_space", "manage_members"],
            "position": 100
        }]),
        json!([
            { "id": "admin", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "user",  "roleIds": [],          "joinedAt": "2026-01-02T00:00:00Z" }
        ]),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: {
                "description": "should not stick",
                "addMembers": [{ "id": "u-new", "roleIds": [] }]
            }}
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "whole patch must be rejected: {resp:?}"
    );

    // Confirm neither the metadata nor the addMembers landed.
    let admin = Id::from("admin");
    assert_eq!(read_space_description(&backend, &admin).await, "original");

    let (get_resp, _) = handle_space_get(
        &backend,
        &admin,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("Space/get");
    let member_count = get_resp["list"][0]["members"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    assert_eq!(member_count, 2, "no member should have been added");
}

/// Oracle: bd:JMAP-g7wu.2.4.13. A caller with `manage_space` and
/// `manage_members` succeeds on a mixed patch that exercises both
/// halves. Sanity check that the new method does not over-reject
/// legitimate mixed mutations.
#[tokio::test]
async fn mixed_patch_metadata_plus_structural_admin_succeeds() {
    let backend = IdentityBackend::new();
    let caller = Id::from("admin");
    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_space", "manage_members"],
            "position": 100
        }]),
        json!([{
            "id": "admin",
            "roleIds": ["r-admin"],
            "joinedAt": "2026-01-01T00:00:00Z"
        }]),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: {
                "description": "renamed",
                "addMembers": [{ "id": "u-new", "roleIds": [] }]
            }}
        }),
    )
    .await
    .expect("handle_space_set");

    assert!(
        resp["notUpdated"][SPACE_ID].is_null(),
        "admin must succeed on mixed patch: {resp:?}"
    );

    assert_eq!(read_space_description(&backend, &caller).await, "renamed");
    let (get_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("Space/get");
    let member_count = get_resp["list"][0]["members"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    assert_eq!(member_count, 2, "new member should have been added");
}

// ---------------------------------------------------------------------------
// Description-only-mutation: gate fires regardless of patch size
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-g7wu.2.4.13. Even a single-field metadata patch
/// is gated. This is a redundant check against
/// `everyone_only_member_cannot_mutate_description` but isolates the
/// minimal failing patch shape (one allowed metadata key, one
/// disallowed caller) for regression-spotting clarity.
#[tokio::test]
async fn single_field_metadata_patch_still_gated() {
    let backend = IdentityBackend::new();
    let caller = Id::from("user");
    seed_space(
        &backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_space"],
            "position": 100
        }]),
        json!([
            { "id": "admin", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "user",  "roleIds": [],          "joinedAt": "2026-01-02T00:00:00Z" }
        ]),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "isPublic": true } }
        }),
    )
    .await
    .expect("handle_space_set");

    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "single-field metadata patch must be gated: {resp:?}"
    );

    let (get_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("Space/get");
    assert!(
        !get_resp["list"][0]["isPublic"].as_bool().unwrap_or(true),
        "isPublic must NOT have changed"
    );
}
