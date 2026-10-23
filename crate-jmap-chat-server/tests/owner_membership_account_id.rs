//! Caller membership and permission resolution match the owner's
//! member entry, which is the accountId (bd:JMAP-yt1w.32).
//!
//! Oracle: draft-atwood-jmap-chat-00 §Terminology (jmap-chat-spec
//! commit cefc217): "The owner has no ChatContact: a client-facing
//! reference to the owner carries the accountId of the owner's account
//! ({{RFC8620}} Section 1.6.2), including where this document describes
//! the reference as a ChatContact.id". §SpaceMember.id: "The
//! participant's ChatContact.id, or the accountId for the owner."
//!
//! The caller of every method is the owner of the account it names, so
//! the owner's SpaceMember entry has `id` == accountId. These tests use
//! [`IdentityBackend`], whose `principal_id` returns the caller verbatim,
//! with a principal (`user-alice`) that differs from the accountId
//! (`a1`). A handler or backend that matches `members[].id` against the
//! principal instead of the accountId treats the owner as a non-member.

mod common;

use common::{ACCOUNT_ID, IdentityBackend, SPACE_ID, seed_space};
use jmap_chat_server::{handle_ban_set, handle_space_get, handle_space_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const PRINCIPAL_ID: &str = "user-alice";

/// Seed the private, non-previewable [`SPACE_ID`] whose only member is
/// the owner (member entry = accountId) holding a role with
/// `permissions`.
fn seed_owner_with(backend: &IdentityBackend, permissions: Value) {
    seed_space(
        backend,
        json!([{ "id": "r-owner", "name": "Owner", "permissions": permissions, "position": 100 }]),
        json!([{ "id": ACCOUNT_ID, "roleIds": ["r-owner"], "joinedAt": "2026-01-01T00:00:00Z" }]),
    );
}

fn caller() -> Id {
    Id::from(PRINCIPAL_ID)
}

/// draft-atwood-jmap-chat-00 §Terminology: "a client-facing reference
/// to the owner carries the accountId of the owner's account ...,
/// including where this document describes the reference as a
/// ChatContact.id". The owner's member entry is the accountId, so the
/// owner is a member of its private Space and Space/get returns the full
/// member view (including `members`), not `notFound` or the non-member
/// preview.
#[tokio::test]
async fn space_get_owner_member_entry_is_account_id_gets_full_view() {
    let backend = IdentityBackend::new();
    seed_owner_with(&backend, json!([]));

    let HandlerOutput { primary: resp, .. } = handle_space_get(
        &backend,
        &caller(),
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("Space/get");

    assert_eq!(resp["notFound"], json!([]), "{resp:?}");
    let members = resp["list"][0]["members"]
        .as_array()
        .unwrap_or_else(|| panic!("owner must get the member view with members: {resp:?}"));
    assert_eq!(members.len(), 1, "{resp:?}");
    assert_eq!(members[0]["id"], ACCOUNT_ID, "{resp:?}");
}

/// draft-atwood-jmap-chat-00 §Terminology: "a client-facing reference
/// to the owner carries the accountId of the owner's account ...". The
/// owner's member entry (accountId) holds `manage_space`, so its
/// metadata update is permitted.
#[tokio::test]
async fn space_set_metadata_uses_owner_member_entry_permissions() {
    let backend = IdentityBackend::new();
    seed_owner_with(&backend, json!(["manage_space"]));

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "description": "renamed" } }
        }),
    )
    .await
    .expect("Space/set");

    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");
    assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp:?}");
}

/// draft-atwood-jmap-chat-00 §Terminology: "a client-facing reference
/// to the owner carries the accountId of the owner's account ...". The
/// owner's member entry (accountId) holds `manage_members`, so its
/// `addMembers` op is permitted.
#[tokio::test]
async fn space_set_add_members_uses_owner_member_entry_permissions() {
    let backend = IdentityBackend::new();
    seed_owner_with(&backend, json!(["manage_members"]));

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "addMembers": [{ "id": "u-new", "roleIds": [] }] } }
        }),
    )
    .await
    .expect("Space/set");

    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");
    assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp:?}");
}

/// draft-atwood-jmap-chat-00 §Terminology: "a client-facing reference
/// to the owner carries the accountId of the owner's account ...";
/// §SpaceBan/set: "Requires `"ban"` permission in the Space." The
/// owner's member entry (accountId) holds `ban`, so its create is
/// permitted.
#[tokio::test]
async fn space_ban_create_uses_owner_member_entry_permissions() {
    let backend = IdentityBackend::new();
    seed_owner_with(&backend, json!(["ban"]));

    let HandlerOutput { primary: resp, .. } = handle_ban_set(
        &backend,
        &caller(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "b0": { "spaceId": SPACE_ID, "userId": "target" } }
        }),
    )
    .await
    .expect("SpaceBan/set");

    assert_eq!(resp["notCreated"], Value::Null, "{resp:?}");
    assert!(resp["created"].get("b0").is_some(), "{resp:?}");
}
