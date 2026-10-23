//! `Space/get` membership and visibility key on the owner's member entry
//! (the accountId) even when the backend wires no caller identity:
//! `MemoryBackend` uses `CallerCtx = ()`, so `JmapBackend::principal_id`
//! returns `None` (bd:JMAP-yt1w.36).
//!
//! Oracle: draft-atwood-jmap-chat-00 (jmap-chat-spec cefc217),
//! §Terminology "Owner": "The owner has no ChatContact: a client-facing
//! reference to the owner carries the accountId of the owner's account".
//! §SpaceMember `id`: "The participant's ChatContact.id, or the accountId
//! for the owner." Each test also quotes the §Space/get rule it enforces.

mod common;

use common::{ACCOUNT_ID, MemoryBackend, SPACE_ID};
use jmap_chat_server::{handle_space_get, handle_space_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

/// A backend holding one Space whose only member is someone other than
/// the owner, so the owner (accountId) is a non-member.
fn foreign_space(previewable: bool) -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    backend.insert_object_for_test(
        "Space",
        ACCOUNT_ID,
        SPACE_ID,
        json!({
            "id": SPACE_ID,
            "name": "Elsewhere",
            "description": "not ours",
            "createdAt": "2026-01-01T00:00:00Z",
            "memberCount": 1,
            "categories": [],
            "uncategorizedChannelIds": [],
            "isPublic": false,
            "isPubliclyPreviewable": previewable,
            "roles": [{ "id": "r-admin", "name": "Admin", "permissions": ["view"], "position": 10 }],
            "members": [{ "id": "contact-bob", "roleIds": ["r-admin"], "joinedAt": "2026-01-01T00:00:00Z" }],
        }),
    );
    backend
}

async fn get_space(backend: &MemoryBackend, id: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_space_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
    )
    .await
    .expect("Space/get");
    resp
}

/// draft-atwood-jmap-chat-00 §Space/get (cefc217): "If the Space has
/// `isPubliclyPreviewable: false` (or the Space does not exist), the
/// server MUST include the requested Space `id` in the `notFound` array
/// of the response and MUST NOT include the Space in the `list` array."
/// The owner is a non-member because no member entry carries the
/// accountId.
#[tokio::test]
async fn owner_without_identity_non_member_private_space_is_not_found() {
    let resp = get_space(&foreign_space(false), SPACE_ID).await;
    assert_eq!(resp["list"], json!([]), "{resp:?}");
    assert_eq!(resp["notFound"], json!([SPACE_ID]), "{resp:?}");
}

/// draft-atwood-jmap-chat-00 §Space/get (cefc217): "If the Space has
/// `isPubliclyPreviewable: true`, the server MUST return a restricted
/// view of the Space containing only the fields `id`, `name`,
/// `description`, `iconBlobId`, `memberCount`, `createdAt`, `isPublic`,
/// `isPubliclyPreviewable`, and `closedAt` (when set). All other Space
/// fields MUST be omitted from the returned object".
#[tokio::test]
async fn owner_without_identity_non_member_previewable_space_is_restricted() {
    let resp = get_space(&foreign_space(true), SPACE_ID).await;
    assert_eq!(resp["notFound"], json!([]), "{resp:?}");
    let space = resp["list"][0]
        .as_object()
        .unwrap_or_else(|| panic!("previewable Space missing from list: {resp:?}"));
    let allowed = [
        "id",
        "name",
        "description",
        "iconBlobId",
        "memberCount",
        "createdAt",
        "isPublic",
        "isPubliclyPreviewable",
        "closedAt",
    ];
    for key in space.keys() {
        assert!(
            allowed.contains(&key.as_str()),
            "non-member view leaks {key}: {resp:?}"
        );
    }
}

/// draft-atwood-jmap-chat-00 §Space/set "Creating a Space" (cefc217):
/// "The server assigns the Space's `id` and adds the caller to `members`
/// with at least one role granting permissions sufficient to administer
/// the Space." The owner is therefore a member of a Space it created, and
/// the §Space/get non-member rules ("When the caller is not a member of a
/// requested Space") do not apply: the full object, including `members`
/// and `roles`, is returned.
#[tokio::test]
async fn owner_without_identity_sees_full_view_of_created_space() {
    let backend = MemoryBackend::new();
    backend.register_account(&Id::from(ACCOUNT_ID));
    let HandlerOutput {
        primary: created, ..
    } = handle_space_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "s0": { "name": "Mine", "isPubliclyPreviewable": false } }
        }),
        &[],
    )
    .await
    .expect("Space/set create");
    let space_id = created["created"]["s0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no server-assigned id: {created:?}"))
        .to_owned();

    let resp = get_space(&backend, &space_id).await;
    assert_eq!(resp["notFound"], json!([]), "{resp:?}");
    let space = &resp["list"][0];
    assert_eq!(space["id"], space_id.as_str(), "{resp:?}");
    assert!(
        space["members"].is_array(),
        "full view lacks members: {resp:?}"
    );
    assert!(space["roles"].is_array(), "full view lacks roles: {resp:?}");
}
