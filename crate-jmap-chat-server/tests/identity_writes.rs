//! Integration tests for caller-identity vs account-id on
//! identity-bearing writes (bd:JMAP-x2gd.1).
//!
//! Per draft-atwood-jmap-chat-00 the following fields MUST carry
//! `ChatContact.id` (i.e. the caller's authenticated userId) rather
//! than the JMAP `accountId`:
//!
//! - `SpaceMember.id`     (§SpaceMember.id, lines 645-646)
//! - `SpaceInvite.createdBy` (§SpaceInvite.createdBy, lines 773-774)
//! - `SpaceBan.bannedBy`  (§SpaceBan.bannedBy, lines 801-802)
//! - `CustomEmoji.createdBy` (semantic parallel; the field carries
//!   the actor's identity, not the JMAP account)
//!
//! Each test exercises both postures via two distinct backends:
//!
//! - `MemoryBackend` with `CallerCtx = ()`: inherits the default
//!   `principal_id` impl returning `None`. Workspace AGENTS.md
//!   "Caller identity (foundation seam)" specifies this as
//!   single-user posture — the kit falls back to `account_id`. The
//!   tests pin the fallback as the documented behavior.
//!
//! - `IdentityBackend` with `CallerCtx = Id`: overrides
//!   `principal_id` to return `Some(caller)`. The tests assert the
//!   resolved principal id lands in the identity-bearing field, NOT
//!   the JMAP `accountId`.
//!
//! Oracle independence: each assertion compares against literal
//! string constants chosen so account_id and principal_id are
//! distinct values. A handler that writes `account_id` into a
//! caller-identity field fails the assertion immediately.

mod common;

use common::{IdentityBackend, MemoryBackend};
use jmap_chat_server::{
    handle_ban_set, handle_emoji_set, handle_invite_get, handle_invite_set, handle_space_get,
    handle_space_join,
};
use jmap_types::Id;
use serde_json::json;

const ACCOUNT_ID: &str = "acct-1";
const PRINCIPAL_ID: &str = "user-alice";
const SPACE_ID: &str = "s1";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Seed a public Space owned by `ACCOUNT_ID` with a single admin
/// member whose id deliberately differs from both `ACCOUNT_ID` and
/// `PRINCIPAL_ID` — the join paths must add a NEW member, not match
/// an existing one.
fn seed_public_space_on_memory(backend: &MemoryBackend) {
    let space_val = json!({
        "id": SPACE_ID,
        "name": "Atlas",
        "description": "Public space for join tests",
        "createdAt": "2026-01-01T00:00:00Z",
        "memberCount": 1,
        "isPublic": true,
        "isPubliclyPreviewable": true,
        "members": [
            { "id": "preseeded-admin", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
        ],
        "categories": [],
        "uncategorizedChannelIds": [],
        "roles": []
    });
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test("Space", ACCOUNT_ID, SPACE_ID, space_val);
}

/// Same seed on an `IdentityBackend` (which wraps a `MemoryBackend`).
fn seed_public_space_on_identity(backend: &IdentityBackend) {
    seed_public_space_on_memory(backend.inner());
}

// ---------------------------------------------------------------------------
// Space/join writes the caller's identity, not the account id
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-x2gd.1 acceptance. Single-user posture
/// (`principal_id == None`) preserves the historical fallback —
/// the new `SpaceMember.id` equals the JMAP `accountId`.
#[tokio::test]
async fn space_join_single_user_writes_account_id_into_member_id() {
    let backend = MemoryBackend::new();
    seed_public_space_on_memory(&backend);

    let (resp, _) = handle_space_join(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "spaceId": SPACE_ID }),
    )
    .await
    .expect("handle_space_join");

    assert_eq!(resp["spaceId"], SPACE_ID);

    let (space_resp, _) = handle_space_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("handle_space_get");

    let members = space_resp["list"][0]["members"]
        .as_array()
        .expect("members array");
    // Two members now: the preseeded admin + the joining caller.
    let joiner = members
        .iter()
        .find(|m| m["id"] != "preseeded-admin")
        .expect("new member");
    assert_eq!(
        joiner["id"], ACCOUNT_ID,
        "single-user mode falls back to account_id per the foundation \
         seam contract (workspace AGENTS.md): {joiner:?}"
    );
}

/// Oracle: bd:JMAP-x2gd.1 acceptance. With identity wired
/// (`principal_id == Some(caller)`), the new `SpaceMember.id`
/// equals the resolved principal, NOT the JMAP `accountId`.
#[tokio::test]
async fn space_join_with_identity_writes_principal_id_into_member_id() {
    let backend = IdentityBackend::new();
    seed_public_space_on_identity(&backend);
    let caller = Id::from(PRINCIPAL_ID);

    let (resp, _) = handle_space_join(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "spaceId": SPACE_ID }),
    )
    .await
    .expect("handle_space_join");
    assert_eq!(resp["spaceId"], SPACE_ID);

    let (space_resp, _) = handle_space_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("handle_space_get");

    let members = space_resp["list"][0]["members"]
        .as_array()
        .expect("members array");
    let joiner = members
        .iter()
        .find(|m| m["id"] != "preseeded-admin")
        .expect("new member");
    assert_eq!(
        joiner["id"], PRINCIPAL_ID,
        "identity-wired mode must write principal_id into SpaceMember.id per \
         draft-atwood-jmap-chat-00 §SpaceMember.id, not the account_id ({ACCOUNT_ID}): \
         {joiner:?}"
    );
    assert_ne!(
        joiner["id"], ACCOUNT_ID,
        "regression guard: SpaceMember.id MUST NOT be the JMAP account_id when \
         caller identity is resolvable: {joiner:?}"
    );
}

/// Oracle: bd:JMAP-x2gd.1 — the writer-side identity must match the
/// reader-side membership check, or `already_member` will misfire.
/// With identity wired, a caller who joins twice via the same
/// principal must hit the `already a member` rejection on the
/// second call.
#[tokio::test]
async fn space_join_identity_already_member_check_uses_principal() {
    let backend = IdentityBackend::new();
    seed_public_space_on_identity(&backend);
    let caller = Id::from(PRINCIPAL_ID);

    let _ = handle_space_join(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "spaceId": SPACE_ID }),
    )
    .await
    .expect("first join");

    let err = handle_space_join(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "spaceId": SPACE_ID }),
    )
    .await
    .expect_err("second join must fail");
    assert_eq!(
        err.error_type.as_str(),
        "invalidArguments",
        "the writer-side identity must agree with the reader-side identity, \
         or the already_member check fails to fire: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// SpaceInvite/set createdBy carries the caller's identity
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-x2gd.1 acceptance. With identity wired, a newly
/// created SpaceInvite's `createdBy` field MUST be the caller's
/// resolved principal id, NOT the JMAP `accountId`.
#[tokio::test]
async fn invite_set_with_identity_writes_principal_id_into_created_by() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    let caller = Id::from(PRINCIPAL_ID);

    let (resp, _) = handle_invite_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "create": {
                "i0": { "spaceId": SPACE_ID }
            }
        }),
    )
    .await
    .expect("handle_invite_set");

    let invite_id = resp["created"]["i0"]["id"]
        .as_str()
        .expect("invite id created")
        .to_owned();

    let (inv_get, _) = handle_invite_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [invite_id] }),
    )
    .await
    .expect("handle_invite_get");

    let created_by = inv_get["list"][0]["createdBy"].as_str().expect("createdBy");
    assert_eq!(
        created_by, PRINCIPAL_ID,
        "SpaceInvite.createdBy MUST be the caller's ChatContact.id per \
         draft-atwood-jmap-chat-00 §SpaceInvite.createdBy, not account_id \
         ({ACCOUNT_ID})"
    );
}

/// Oracle: bd:JMAP-x2gd.1 acceptance. Single-user posture preserves
/// the historical fallback for `createdBy`.
#[tokio::test]
async fn invite_set_single_user_writes_account_id_into_created_by() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));

    let (resp, _) = handle_invite_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": {
                "i0": { "spaceId": SPACE_ID }
            }
        }),
    )
    .await
    .expect("handle_invite_set");

    let created_by = resp["created"]["i0"]["createdBy"]
        .as_str()
        .expect("createdBy");
    assert_eq!(
        created_by, ACCOUNT_ID,
        "single-user posture falls back to account_id per the foundation seam"
    );
}

// ---------------------------------------------------------------------------
// SpaceBan/set bannedBy carries the caller's identity
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-x2gd.1 acceptance. With identity wired, a newly
/// created SpaceBan's `bannedBy` field MUST be the caller's
/// resolved principal id.
#[tokio::test]
async fn ban_set_with_identity_writes_principal_id_into_banned_by() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    let caller = Id::from(PRINCIPAL_ID);

    let (resp, _) = handle_ban_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "create": {
                "b0": {
                    "spaceId": SPACE_ID,
                    "userId": "user-target"
                }
            }
        }),
    )
    .await
    .expect("handle_ban_set");

    let banned_by = resp["created"]["b0"]["bannedBy"]
        .as_str()
        .expect("bannedBy");
    assert_eq!(
        banned_by, PRINCIPAL_ID,
        "SpaceBan.bannedBy MUST be the caller's ChatContact.id per \
         draft-atwood-jmap-chat-00 §SpaceBan.bannedBy, not account_id ({ACCOUNT_ID})"
    );
}

/// Oracle: bd:JMAP-x2gd.1 acceptance. Single-user posture preserves
/// the historical fallback for `bannedBy`.
#[tokio::test]
async fn ban_set_single_user_writes_account_id_into_banned_by() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));

    let (resp, _) = handle_ban_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": {
                "b0": {
                    "spaceId": SPACE_ID,
                    "userId": "user-target"
                }
            }
        }),
    )
    .await
    .expect("handle_ban_set");

    let banned_by = resp["created"]["b0"]["bannedBy"]
        .as_str()
        .expect("bannedBy");
    assert_eq!(
        banned_by, ACCOUNT_ID,
        "single-user posture falls back to account_id per the foundation seam"
    );
}

// ---------------------------------------------------------------------------
// CustomEmoji/set createdBy carries the caller's identity
// ---------------------------------------------------------------------------

/// Oracle: bd:JMAP-x2gd.1 acceptance. With identity wired, a newly
/// created CustomEmoji's `createdBy` field MUST be the caller's
/// resolved principal id.
#[tokio::test]
async fn emoji_set_with_identity_writes_principal_id_into_created_by() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    let caller = Id::from(PRINCIPAL_ID);

    let (resp, _) = handle_emoji_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "create": {
                "e0": {
                    "name": "wave",
                    "blobId": "blob-wave-001"
                }
            }
        }),
    )
    .await
    .expect("handle_emoji_set");

    let created_by = resp["created"]["e0"]["createdBy"]
        .as_str()
        .expect("createdBy");
    assert_eq!(
        created_by, PRINCIPAL_ID,
        "CustomEmoji.createdBy MUST be the caller's ChatContact.id, \
         not account_id ({ACCOUNT_ID})"
    );
}

/// Oracle: bd:JMAP-x2gd.1 acceptance. Single-user posture preserves
/// the historical fallback for emoji `createdBy`.
#[tokio::test]
async fn emoji_set_single_user_writes_account_id_into_created_by() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));

    let (resp, _) = handle_emoji_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": {
                "e0": {
                    "name": "wave",
                    "blobId": "blob-wave-001"
                }
            }
        }),
    )
    .await
    .expect("handle_emoji_set");

    let created_by = resp["created"]["e0"]["createdBy"]
        .as_str()
        .expect("createdBy");
    assert_eq!(
        created_by, ACCOUNT_ID,
        "single-user posture falls back to account_id per the foundation seam"
    );
}
