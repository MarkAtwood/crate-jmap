//! Space/set `removeMembers`: self-removal and last-administrator
//! protection, per draft-atwood-jmap-chat-00 §Space/set. The owner's
//! member entry is the accountId (§Terminology).

mod common;

use common::{seed_space, IdentityBackend, MemoryBackend, ACCOUNT_ID, SPACE_ID};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_types::Id;
use serde_json::json;

/// Seed the Space with `admin` (holding `r-admin`) and `regular` (no roles).
fn admin_and_regular(backend: &IdentityBackend, admin: &str, regular: &str) {
    seed_space(
        backend,
        json!([{
            "id": "r-admin",
            "name": "Admin",
            "permissions": ["manage_space", "manage_members"],
            "position": 100
        }]),
        json!([
            { "id": admin, "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": regular, "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
        ]),
    );
}

/// Space member ids as seen by `viewer`, who must still be a member.
async fn member_ids(backend: &IdentityBackend, viewer: &str) -> Vec<String> {
    let (resp, _) = handle_space_get(
        backend,
        &Id::from(viewer),
        json!({ "accountId": ACCOUNT_ID, "ids": [SPACE_ID] }),
    )
    .await
    .expect("Space/get");
    resp["list"][0]["members"]
        .as_array()
        .expect("members")
        .iter()
        .map(|m| m["id"].as_str().expect("member id").to_owned())
        .collect()
}

/// Oracle: draft §Space/set `removeMembers` — "A member who does not hold
/// `"manage_members"` permission MAY remove themselves from the Space by
/// including only the accountId in `removeMembers`. The server MUST
/// permit self-removal regardless of the caller's permission set."
#[tokio::test]
async fn self_removal_without_manage_members_is_permitted() {
    let backend = IdentityBackend::new();
    admin_and_regular(&backend, "admin-user", ACCOUNT_ID);
    let caller = Id::from(ACCOUNT_ID);

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": [ACCOUNT_ID] }}
        }),
    )
    .await
    .expect("handle_space_set");
    assert!(resp["notUpdated"][SPACE_ID].is_null(), "{resp:?}");
    assert_eq!(member_ids(&backend, "admin-user").await, ["admin-user"]);
}

/// Oracle: draft §Space/set `removeMembers` — "Last-administrator
/// protection does not apply to self-removal."
#[tokio::test]
async fn last_admin_self_removal_bypasses_last_admin_protection() {
    let backend = IdentityBackend::new();
    backend.inner().set_protect_last_admin_for_test(true);
    admin_and_regular(&backend, ACCOUNT_ID, "u-regular");
    let caller = Id::from(ACCOUNT_ID);

    let (resp, _) = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": [ACCOUNT_ID] }}
        }),
    )
    .await
    .expect("handle_space_set");
    assert!(resp["notUpdated"][SPACE_ID].is_null(), "{resp:?}");
    assert_eq!(member_ids(&backend, "u-regular").await, ["u-regular"]);
}

/// Oracle: draft §Space/set `removeMembers` — "Servers MUST return an
/// `invalidProperties` SetError ({{RFC8620}} §5.3) naming `removeMembers`
/// when refusing on this basis."
///
/// Single-user mode (no caller identity) cannot self-remove, so removing
/// the only admin is removing another member.
#[tokio::test]
async fn last_admin_refusal_is_invalid_properties_naming_remove_members() {
    let backend = MemoryBackend::new();
    backend.set_protect_last_admin_for_test(true);
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "Test",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 2,
            "categories": [],
            "uncategorizedChannelIds": [],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [{
                "id": "r-admin",
                "name": "Admin",
                "permissions": ["manage_members"],
                "position": 100
            }],
            "members": [
                { "id": "admin-user", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
                { "id": "u-regular", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
            ]
        }),
    );

    let (resp, _) = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": ["admin-user"] }}
        }),
    )
    .await
    .expect("handle_space_set");
    let err = &resp["notUpdated"][SPACE_ID];
    assert_eq!(err["type"], "invalidProperties", "{resp:?}");
    assert_eq!(err["properties"], json!(["removeMembers"]), "{resp:?}");
}
