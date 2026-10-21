//! Integration tests for caller-identity vs account-id on
//! identity-bearing writes (bd:JMAP-x2gd.1, bd:JMAP-yt1w.29).
//!
//! The following fields carry the ChatContact.id of the actor:
//!
//! - `SpaceMember.id`     (§SpaceMember.id)
//! - `SpaceInvite.createdBy` (§SpaceInvite.createdBy)
//! - `SpaceBan.bannedBy`  (§SpaceBan.bannedBy)
//! - `CustomEmoji.createdBy` (§CustomEmoji.createdBy)
//!
//! The actor here is always the owner, and draft-atwood-jmap-chat-00
//! §Terminology says "a client-facing reference to the owner carries the
//! accountId of the owner's account ..., including where this document
//! describes the reference as a ChatContact.id". Each field therefore
//! carries the JMAP `accountId` in both postures:
//!
//! - `MemoryBackend` with `CallerCtx = ()`: inherits the default
//!   `principal_id` impl returning `None` (single-user posture).
//!
//! - `IdentityBackend` with `CallerCtx = Id`: overrides
//!   `principal_id` to return `Some(caller)`. The resolved principal
//!   id MUST NOT land in the owner-reference field.
//!
//! Oracle independence: each assertion compares against literal
//! string constants chosen so account_id and principal_id are
//! distinct values. A handler that writes the principal id into an
//! owner-reference field fails the assertion immediately.

mod common;

use common::{IdentityBackend, MemoryBackend};
use jmap_chat_server::{
    handle_ban_set, handle_emoji_set, handle_invite_get, handle_invite_set, handle_space_get,
    handle_space_join,
};
use jmap_server::HandlerOutput;
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

    let HandlerOutput { primary: resp, .. } = handle_space_join(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "spaceId": SPACE_ID }),
    )
    .await
    .expect("handle_space_join");

    assert_eq!(resp["spaceId"], SPACE_ID);

    let HandlerOutput {
        primary: space_resp,
        ..
    } = handle_space_get(
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

/// Oracle: §SpaceMember.id "The participant's ChatContact.id, or the
/// accountId for the owner". With identity wired
/// (`principal_id == Some(caller)`), the new `SpaceMember.id` is the
/// JMAP `accountId`, NOT the resolved principal.
#[tokio::test]
async fn space_join_with_identity_writes_account_id_into_member_id() {
    let backend = IdentityBackend::new();
    seed_public_space_on_identity(&backend);
    let caller = Id::from(PRINCIPAL_ID);

    let HandlerOutput { primary: resp, .. } = handle_space_join(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "spaceId": SPACE_ID }),
    )
    .await
    .expect("handle_space_join");
    assert_eq!(resp["spaceId"], SPACE_ID);

    // Read as another principal: every caller of the account is its
    // owner, whose member entry is the accountId (draft §Terminology).
    let HandlerOutput {
        primary: space_resp,
        ..
    } = handle_space_get(
        &backend,
        &Id::from("preseeded-admin"),
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
        joiner["id"], ACCOUNT_ID,
        "the owner's SpaceMember.id is the accountId per \
         draft-atwood-jmap-chat-00 §SpaceMember.id, not the principal \
         ({PRINCIPAL_ID}): {joiner:?}"
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
        err.error_type.to_string(),
        "invalidArguments",
        "the writer-side identity must agree with the reader-side identity, \
         or the already_member check fails to fire: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// SpaceInvite/set createdBy carries the caller's identity
// ---------------------------------------------------------------------------

/// Oracle: §Terminology (owner references carry the accountId). With
/// identity wired, a SpaceInvite the owner creates has `createdBy` equal
/// to the JMAP `accountId`, NOT the resolved principal id.
#[tokio::test]
async fn invite_set_with_identity_writes_account_id_into_created_by() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    let caller = Id::from(PRINCIPAL_ID);

    let HandlerOutput { primary: resp, .. } = handle_invite_set(
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

    let HandlerOutput {
        primary: inv_get, ..
    } = handle_invite_get(
        &backend,
        &caller,
        json!({ "accountId": ACCOUNT_ID, "ids": [invite_id] }),
    )
    .await
    .expect("handle_invite_get");

    let created_by = inv_get["list"][0]["createdBy"].as_str().expect("createdBy");
    assert_eq!(
        created_by, ACCOUNT_ID,
        "SpaceInvite.createdBy references the owner by the accountId per \
         draft-atwood-jmap-chat-00 §Terminology, not the principal \
         ({PRINCIPAL_ID})"
    );
}

/// Oracle: bd:JMAP-x2gd.1 acceptance. Single-user posture preserves
/// the historical fallback for `createdBy`.
#[tokio::test]
async fn invite_set_single_user_writes_account_id_into_created_by() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));

    let HandlerOutput { primary: resp, .. } = handle_invite_set(
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

/// Oracle: §Terminology (owner references carry the accountId). With
/// identity wired, a SpaceBan the owner creates has `bannedBy` equal to
/// the JMAP `accountId`, NOT the resolved principal id.
#[tokio::test]
async fn ban_set_with_identity_writes_account_id_into_banned_by() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    let caller = Id::from(PRINCIPAL_ID);

    let HandlerOutput { primary: resp, .. } = handle_ban_set(
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
        banned_by, ACCOUNT_ID,
        "SpaceBan.bannedBy references the owner by the accountId per \
         draft-atwood-jmap-chat-00 §Terminology, not the principal ({PRINCIPAL_ID})"
    );
}

/// Oracle: bd:JMAP-x2gd.1 acceptance. Single-user posture preserves
/// the historical fallback for `bannedBy`.
#[tokio::test]
async fn ban_set_single_user_writes_account_id_into_banned_by() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));

    let HandlerOutput { primary: resp, .. } = handle_ban_set(
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

/// Oracle: §Terminology (owner references carry the accountId). With
/// identity wired, a CustomEmoji the owner creates has `createdBy` equal
/// to the JMAP `accountId`, NOT the resolved principal id.
#[tokio::test]
async fn emoji_set_with_identity_writes_account_id_into_created_by() {
    let backend = IdentityBackend::new();
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    let caller = Id::from(PRINCIPAL_ID);

    let HandlerOutput { primary: resp, .. } = handle_emoji_set(
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
        created_by, ACCOUNT_ID,
        "CustomEmoji.createdBy references the owner by the accountId, \
         not the principal ({PRINCIPAL_ID})"
    );
}

/// Oracle: bd:JMAP-x2gd.1 acceptance. Single-user posture preserves
/// the historical fallback for emoji `createdBy`.
#[tokio::test]
async fn emoji_set_single_user_writes_account_id_into_created_by() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));

    let HandlerOutput { primary: resp, .. } = handle_emoji_set(
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
