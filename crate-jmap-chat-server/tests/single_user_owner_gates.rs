//! Chat permission and role-hierarchy gates are enforced against the
//! owner's member entry (the accountId) even when the backend wires no
//! caller identity: `MemoryBackend` uses `CallerCtx = ()`, so
//! `JmapBackend::principal_id` returns `None` (bd:JMAP-yt1w.33).
//!
//! Oracle: draft-atwood-jmap-chat-00 (jmap-chat-spec cefc217),
//! §SpaceMember: "The participant's ChatContact.id, or the accountId for
//! the owner." Each test also quotes the permission clause it enforces.

mod common;

use common::{MemoryBackend, ACCOUNT_ID, SPACE_ID};
use jmap_chat_server::{handle_ban_set, handle_space_set};
use jmap_types::Id;
use serde_json::{json, Value};

/// A backend holding one Space whose only member is the owner (member
/// entry id = accountId) with a role at position 10 granting
/// `permissions`.
fn owner_space(permissions: Value) -> MemoryBackend {
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
            "memberCount": 1,
            "categories": [],
            "uncategorizedChannelIds": [],
            "isPublic": false,
            "isPubliclyPreviewable": false,
            "roles": [{ "id": "r-owner", "name": "Owner", "permissions": permissions, "position": 10 }],
            "members": [{ "id": ACCOUNT_ID, "roleIds": ["r-owner"], "joinedAt": "2026-01-01T00:00:00Z" }],
        }),
    );
    backend
}

/// Send a `Space/set` update of [`SPACE_ID`] with `patch` as the owner.
async fn update_space(backend: &MemoryBackend, patch: Value) -> Value {
    let (resp, _) = handle_space_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { SPACE_ID: patch } }),
    )
    .await
    .expect("Space/set");
    resp
}

fn assert_forbidden(resp: &Value) {
    assert_eq!(resp["updated"], Value::Null, "{resp:?}");
    assert_eq!(
        resp["notUpdated"][SPACE_ID]["type"], "forbidden",
        "{resp:?}"
    );
}

fn assert_updated(resp: &Value) {
    assert_eq!(resp["notUpdated"], Value::Null, "{resp:?}");
    assert!(resp["updated"].get(SPACE_ID).is_some(), "{resp:?}");
}

/// §Space/set, Updating a Space, `name`, `description`, `iconBlobId`:
/// "Metadata fields. Require `"manage_space"` permission."
#[tokio::test]
async fn owner_without_identity_needs_manage_space_for_metadata() {
    let patch = json!({ "description": "renamed" });
    assert_forbidden(&update_space(&owner_space(json!([])), patch.clone()).await);
    assert_updated(&update_space(&owner_space(json!(["manage_space"])), patch).await);
}

/// §Space/set, Updating a Space, `addChannels`: "Requires
/// `"manage_channels"`."
#[tokio::test]
async fn owner_without_identity_needs_manage_channels_for_add_channels() {
    let patch = json!({ "addChannels": [{ "name": "general" }] });
    assert_forbidden(&update_space(&owner_space(json!([])), patch.clone()).await);
    assert_updated(&update_space(&owner_space(json!(["manage_channels"])), patch).await);
}

/// §Space/set, Updating a Space, `addRoles`: "Requires `"manage_roles"`.
/// Members may only add roles whose `position` is strictly less than their
/// own highest-position role; servers MUST enforce this."
#[tokio::test]
async fn owner_without_identity_is_bound_by_add_roles_gates() {
    let role_at = |position: u64| json!({ "addRoles": [{ "name": "Mod", "permissions": [], "position": position }] });
    // Lacks manage_roles.
    assert_forbidden(&update_space(&owner_space(json!([])), role_at(5)).await);
    // Holds manage_roles at position 10: 50 is above it.
    assert_forbidden(&update_space(&owner_space(json!(["manage_roles"])), role_at(50)).await);
    assert_updated(&update_space(&owner_space(json!(["manage_roles"])), role_at(5)).await);
}

/// §SpaceBan/set: "`create` accepts: ... Requires `"ban"` permission in
/// the Space."
#[tokio::test]
async fn owner_without_identity_needs_ban_for_space_ban_create() {
    let create = |backend: MemoryBackend| async move {
        let (resp, _) = handle_ban_set(
            &backend,
            &(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": { "b0": { "spaceId": SPACE_ID, "userId": "target" } },
            }),
        )
        .await
        .expect("SpaceBan/set");
        resp
    };

    let denied = create(owner_space(json!([]))).await;
    assert_eq!(denied["created"], Value::Null, "{denied:?}");
    assert_eq!(
        denied["notCreated"]["b0"]["type"], "forbidden",
        "{denied:?}"
    );

    let allowed = create(owner_space(json!(["ban"]))).await;
    assert_eq!(allowed["notCreated"], Value::Null, "{allowed:?}");
    assert!(allowed["created"].get("b0").is_some(), "{allowed:?}");
}
