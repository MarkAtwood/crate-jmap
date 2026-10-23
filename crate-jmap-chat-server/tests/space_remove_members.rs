//! Space/set `removeMembers`: self-removal and last-administrator
//! protection, per draft-atwood-jmap-chat-00 §Space/set. The owner's
//! member entry is the accountId (§Terminology).

mod common;

use common::{ACCOUNT_ID, IdentityBackend, MemoryBackend, SPACE_ID, seed_space};
use jmap_chat_server::{JmapBackend, handle_space_set};
use jmap_chat_types::Space;
use jmap_server::HandlerOutput;
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

/// Stored Space member ids, read from the backend directly: every caller
/// of the account is its owner (draft §Terminology), who has left, and
/// draft §Space/get hides a non-previewable Space from a non-member.
async fn member_ids(backend: &IdentityBackend) -> Vec<String> {
    let (list, _) = backend
        .inner()
        .get_objects::<Space>(
            &(),
            &Id::from(ACCOUNT_ID),
            Some(&[Id::from(SPACE_ID)]),
            None,
        )
        .await
        .expect("get_objects");
    list[0].members.iter().map(|m| m.id.to_string()).collect()
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

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": [ACCOUNT_ID] }}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");
    assert!(resp["notUpdated"][SPACE_ID].is_null(), "{resp:?}");
    assert_eq!(member_ids(&backend).await, ["admin-user"]);
}

/// Oracle: draft §Space/set `removeMembers` — "Last-administrator
/// protection does not apply to self-removal."
#[tokio::test]
async fn last_admin_self_removal_bypasses_last_admin_protection() {
    let backend = IdentityBackend::new();
    backend.inner().set_protect_last_admin_for_test(true);
    admin_and_regular(&backend, ACCOUNT_ID, "u-regular");
    let caller = Id::from(ACCOUNT_ID);

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &caller,
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": [ACCOUNT_ID] }}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");
    assert!(resp["notUpdated"][SPACE_ID].is_null(), "{resp:?}");
    assert_eq!(member_ids(&backend).await, ["u-regular"]);
}

/// Oracle: draft §Space/set `removeMembers` — "Servers MUST return an
/// `invalidProperties` SetError ({{RFC8620}} §5.3) naming `removeMembers`
/// when refusing on this basis." and "Requires `"manage_members"`."
///
/// The owner holds `manage_members` to remove another member, so the
/// patch removes both admins (the other admin and the owner); naming two
/// ids makes it more than self-removal.
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
            "memberCount": 3,
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
                { "id": ACCOUNT_ID, "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" },
                { "id": "u-regular", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" }
            ]
        }),
    );

    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "update": { SPACE_ID: { "removeMembers": ["admin-user", ACCOUNT_ID] }}
        }),
        &[],
    )
    .await
    .expect("handle_space_set");
    let err = &resp["notUpdated"][SPACE_ID];
    assert_eq!(err["type"], "invalidProperties", "{resp:?}");
    assert_eq!(err["properties"], json!(["removeMembers"]), "{resp:?}");
}

/// Seed a Space where the owner holds only `r-top` (`manage_roles`,
/// above every other role, so the role-position hierarchy never refuses)
/// and `admin-user` is the only member holding `manage_members`, via
/// `r-admin`. `r-admin2` grants `manage_space` and is held by nobody.
fn sole_admin_space(backend: &MemoryBackend) {
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
            "roles": [
                { "id": "r-top", "name": "Top", "permissions": ["manage_roles"], "position": 200 },
                { "id": "r-admin", "name": "Admin", "permissions": ["manage_members"], "position": 100 },
                { "id": "r-admin2", "name": "Admin2", "permissions": ["manage_space"], "position": 50 }
            ],
            "members": [
                { "id": ACCOUNT_ID, "roleIds": ["r-top"], "joinedAt": "2026-01-01T00:00:00Z" },
                { "id": "admin-user", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" }
            ]
        }),
    );
}

async fn patch_sole_admin_space(patch: serde_json::Value) -> serde_json::Value {
    let backend = MemoryBackend::new();
    sole_admin_space(&backend);
    let HandlerOutput { primary: resp, .. } = handle_space_set(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { SPACE_ID: patch } }),
        &[],
    )
    .await
    .expect("handle_space_set");
    resp
}

/// Oracle: draft §Space/set `removeMembers` (jmap-chat-spec 33702a5
/// L1356) — "The same protection applies to an `updateMembers` role
/// change, a `removeRoles` entry, or an `updateRoles` permission change
/// that would leave the Space without any member holding
/// `"manage_members"` or `"manage_space"`; a refusal names the patch key
/// concerned."
#[tokio::test]
async fn last_admin_refusal_names_update_members() {
    let resp =
        patch_sole_admin_space(json!({ "updateMembers": [{ "id": "admin-user", "roleIds": [] }] }))
            .await;
    let err = &resp["notUpdated"][SPACE_ID];
    assert_eq!(err["type"], "invalidProperties", "{resp:?}");
    assert_eq!(err["properties"], json!(["updateMembers"]), "{resp:?}");
}

/// Oracle: draft §Space/set `removeMembers` (33702a5 L1356), as above,
/// for "a `removeRoles` entry".
#[tokio::test]
async fn last_admin_refusal_names_remove_roles() {
    let resp = patch_sole_admin_space(json!({ "removeRoles": ["r-admin"] })).await;
    let err = &resp["notUpdated"][SPACE_ID];
    assert_eq!(err["type"], "invalidProperties", "{resp:?}");
    assert_eq!(err["properties"], json!(["removeRoles"]), "{resp:?}");
}

/// Oracle: draft §Space/set `removeMembers` (33702a5 L1356), as above,
/// for "an `updateRoles` permission change".
#[tokio::test]
async fn last_admin_refusal_names_update_roles() {
    let resp = patch_sole_admin_space(
        json!({ "updateRoles": [{ "id": "r-admin", "permissions": ["send"] }] }),
    )
    .await;
    let err = &resp["notUpdated"][SPACE_ID];
    assert_eq!(err["type"], "invalidProperties", "{resp:?}");
    assert_eq!(err["properties"], json!(["updateRoles"]), "{resp:?}");
}

/// Oracle: draft §Space/set `removeMembers` (33702a5 L1356) refuses only
/// a change that "would leave the Space without any member holding
/// `"manage_members"` or `"manage_space"`". Swapping the admin's
/// `manage_members` for `manage_space` keeps an administrator.
#[tokio::test]
async fn update_roles_keeping_an_admin_permission_is_permitted() {
    let resp = patch_sole_admin_space(
        json!({ "updateRoles": [{ "id": "r-admin", "permissions": ["manage_space"] }] }),
    )
    .await;
    assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp:?}");
}

/// Oracle: as above. Removing `r-admin` while moving its only holder to
/// `r-admin2` (`manage_space`) in the same patch keeps an administrator.
#[tokio::test]
async fn remove_role_with_admin_moved_to_another_admin_role_is_permitted() {
    let resp = patch_sole_admin_space(json!({
        "removeRoles": ["r-admin"],
        "updateMembers": [{ "id": "admin-user", "roleIds": ["r-admin2"] }]
    }))
    .await;
    assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp:?}");
}
