//! `Chat/set` update applies the group member patch keys `addMembers`,
//! `removeMembers` and `updateMemberRoles` (bd:JMAP-yt1w.7).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 §Chat/set "Updating a
//! Chat": "Member list changes use the following patch keys (all require
//! admin role, except leaving the group as described under
//! `removeMembers`)". The last test follows the implementer guide's
//! "Deployments SHOULD avoid admin-less open groups by refusing to let the
//! last admin demote themselves via `updateMemberRoles`."
//!
//! The owner's own member entry is the accountId (§Terminology: "a
//! client-facing reference to the owner carries the accountId of the
//! owner's account").

mod common;

use common::{ACCOUNT_ID, IdentityBackend};
use jmap_chat_server::{ChatLimits, handle_chat_get, handle_chat_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

/// Create a group Chat as the owner (admin) with `bob` and `carol` as
/// members and return its id.
async fn group_of_three(backend: &IdentityBackend) -> String {
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        backend,
        &Id::from(ACCOUNT_ID),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "g": { "kind": "group", "name": "Team", "memberIds": ["bob", "carol"] } },
        }),
    )
    .await
    .expect("Chat/set create");
    resp["created"]["g"]["id"]
        .as_str()
        .expect("created")
        .to_owned()
}

/// Store a group Chat in which `alice` is the admin and the owner and
/// `carol` are members, and return its id.
fn group_with_owner_as_member(backend: &IdentityBackend) -> String {
    backend.inner().register_account(&Id::from(ACCOUNT_ID));
    backend.inner().insert_object_for_test(
        "Chat",
        ACCOUNT_ID,
        "g1",
        json!({
            "id": "g1",
            "kind": "group",
            "name": "Team",
            "createdAt": "2026-01-01T00:00:00Z",
            "unreadCount": 0,
            "members": [
                { "id": "alice", "role": "admin", "joinedAt": "2026-01-01T00:00:00Z" },
                { "id": ACCOUNT_ID, "role": "member", "joinedAt": "2026-01-01T00:00:00Z" },
                { "id": "carol", "role": "member", "joinedAt": "2026-01-01T00:00:00Z" }
            ],
        }),
    );
    "g1".to_owned()
}

/// Send one `Chat/set` update as `caller` and return the response.
async fn update_as(backend: &IdentityBackend, caller: &str, chat_id: &str, patch: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        backend,
        &Id::from(caller),
        json!({ "accountId": ACCOUNT_ID, "update": { chat_id: patch } }),
    )
    .await
    .expect("Chat/set update");
    resp
}

/// The stored Chat as returned by `Chat/get`.
async fn stored_chat(backend: &IdentityBackend, chat_id: &str) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_chat_get(
        backend,
        &Id::from(ACCOUNT_ID),
        json!({ "accountId": ACCOUNT_ID, "ids": [chat_id] }),
    )
    .await
    .expect("Chat/get");
    resp["list"][0].clone()
}

/// Sorted `(id, role)` of every entry in the stored Chat's `members`.
async fn stored_members(backend: &IdentityBackend, chat_id: &str) -> Vec<(String, String)> {
    let mut members: Vec<(String, String)> = stored_chat(backend, chat_id).await["members"]
        .as_array()
        .expect("group Chat must carry members")
        .iter()
        .map(|m| {
            (
                m["id"].as_str().expect("member id").to_owned(),
                m["role"].as_str().expect("member role").to_owned(),
            )
        })
        .collect();
    members.sort();
    members
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(id, role)| ((*id).to_owned(), (*role).to_owned()))
        .collect()
}

/// "`addMembers` (Object[]): Each entry: `id` (String, ChatContact.id) and
/// optional `role` (String, default `"member"`)." The key is an operation
/// on `members`, not a property, so it must not be stored on the Chat.
#[tokio::test]
async fn admin_add_members_adds_with_default_role() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        &chat_id,
        json!({ "addMembers": [{ "id": "dave" }, { "id": "erin", "role": "admin" }] }),
    )
    .await;
    assert!(resp["updated"].get(&chat_id).is_some(), "{resp}");

    assert_eq!(
        stored_members(&backend, &chat_id).await,
        pairs(&[
            (ACCOUNT_ID, "admin"),
            ("bob", "member"),
            ("carol", "member"),
            ("dave", "member"),
            ("erin", "admin"),
        ])
    );
    assert!(
        stored_chat(&backend, &chat_id)
            .await
            .get("addMembers")
            .is_none()
    );
}

/// "Member list changes use the following patch keys (all require admin
/// role, except leaving the group as described under `removeMembers`)".
#[tokio::test]
async fn non_admin_add_members_is_forbidden() {
    let backend = IdentityBackend::new();
    let chat_id = group_with_owner_as_member(&backend);

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        &chat_id,
        json!({ "addMembers": [{ "id": "dave" }] }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&chat_id]["type"], "forbidden", "{resp}");
    assert_eq!(
        stored_members(&backend, &chat_id).await,
        pairs(&[
            (ACCOUNT_ID, "member"),
            ("alice", "admin"),
            ("carol", "member")
        ])
    );
}

/// "`removeMembers` (String[]): ChatContact.ids to remove."
#[tokio::test]
async fn admin_remove_members_removes() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        &chat_id,
        json!({ "removeMembers": ["carol"] }),
    )
    .await;
    assert!(resp["updated"].get(&chat_id).is_some(), "{resp}");
    assert_eq!(
        stored_members(&backend, &chat_id).await,
        pairs(&[(ACCOUNT_ID, "admin"), ("bob", "member")])
    );
    assert!(
        stored_chat(&backend, &chat_id)
            .await
            .get("removeMembers")
            .is_none()
    );
}

/// "The owner MAY leave the group by including only the accountId in
/// `removeMembers`; the server MUST permit this whether or not the owner
/// holds the admin role".
#[tokio::test]
async fn non_admin_may_leave_via_remove_members() {
    let backend = IdentityBackend::new();
    let chat_id = group_with_owner_as_member(&backend);

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        &chat_id,
        json!({ "removeMembers": [ACCOUNT_ID] }),
    )
    .await;
    assert!(resp["updated"].get(&chat_id).is_some(), "{resp}");
    assert_eq!(
        stored_members(&backend, &chat_id).await,
        pairs(&[("alice", "admin"), ("carol", "member")])
    );
}

/// "(all require admin role, except leaving the group as described under
/// `removeMembers`)": removing someone else is not leaving.
#[tokio::test]
async fn non_admin_remove_other_member_is_forbidden() {
    let backend = IdentityBackend::new();
    let chat_id = group_with_owner_as_member(&backend);

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        &chat_id,
        json!({ "removeMembers": ["carol"] }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&chat_id]["type"], "forbidden", "{resp}");
    assert_eq!(
        stored_members(&backend, &chat_id).await,
        pairs(&[
            (ACCOUNT_ID, "member"),
            ("alice", "admin"),
            ("carol", "member")
        ])
    );
}

/// "If the resulting membership would exceed a server-defined limit on the
/// number of members per group Chat, the server MUST return an `overQuota`
/// SetError ({{RFC8620}} §5.3) for that entry."
#[tokio::test]
async fn add_members_over_limit_is_over_quota() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;
    backend
        .inner()
        .set_limits_for_test(Some(ChatLimits::default().with_max_group_chat_members(3)));

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        &chat_id,
        json!({ "addMembers": [{ "id": "dave" }] }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&chat_id]["type"], "overQuota", "{resp}");
    assert_eq!(
        stored_members(&backend, &chat_id).await,
        pairs(&[
            (ACCOUNT_ID, "admin"),
            ("bob", "member"),
            ("carol", "member")
        ])
    );
}

/// "The `role` value in `addMembers` and `updateMemberRoles` entries MUST
/// be `"admin"` or `"member"` ({{chat-member}}); the server MUST reject any
/// other value with an `invalidProperties` SetError."
#[tokio::test]
async fn add_members_rejects_unknown_role() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        &chat_id,
        json!({ "addMembers": [{ "id": "dave", "role": "owner" }] }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&chat_id]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"][&chat_id]["properties"],
        json!(["addMembers"])
    );
}

/// "The `role` value in `addMembers` and `updateMemberRoles` entries MUST
/// be `"admin"` or `"member"` ({{chat-member}}); the server MUST reject any
/// other value with an `invalidProperties` SetError."
#[tokio::test]
async fn update_member_roles_rejects_unknown_role() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        &chat_id,
        json!({ "updateMemberRoles": [{ "id": "bob", "role": "moderator" }] }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&chat_id]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"][&chat_id]["properties"],
        json!(["updateMemberRoles"])
    );
}

/// "`updateMemberRoles` (Object[]): Each entry: `id` (String) and `role`
/// (String)."
#[tokio::test]
async fn admin_update_member_roles_changes_role() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        &chat_id,
        json!({ "updateMemberRoles": [{ "id": "bob", "role": "admin" }] }),
    )
    .await;
    assert!(resp["updated"].get(&chat_id).is_some(), "{resp}");
    assert_eq!(
        stored_members(&backend, &chat_id).await,
        pairs(&[(ACCOUNT_ID, "admin"), ("bob", "admin"), ("carol", "member")])
    );
    assert!(
        stored_chat(&backend, &chat_id)
            .await
            .get("updateMemberRoles")
            .is_none()
    );
}

/// "Member list changes use the following patch keys (all require admin
/// role, except leaving the group as described under `removeMembers`)".
#[tokio::test]
async fn non_admin_update_member_roles_is_forbidden() {
    let backend = IdentityBackend::new();
    let chat_id = group_with_owner_as_member(&backend);

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        &chat_id,
        json!({ "updateMemberRoles": [{ "id": ACCOUNT_ID, "role": "admin" }] }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&chat_id]["type"], "forbidden", "{resp}");
    assert_eq!(
        stored_members(&backend, &chat_id).await,
        pairs(&[
            (ACCOUNT_ID, "member"),
            ("alice", "admin"),
            ("carol", "member")
        ])
    );
}

/// Implementer guide: "Deployments SHOULD avoid admin-less open groups by
/// refusing to let the last admin demote themselves via
/// `updateMemberRoles`."
#[tokio::test]
async fn last_admin_self_demote_is_refused() {
    let backend = IdentityBackend::new();
    let chat_id = group_of_three(&backend).await;

    let resp = update_as(
        &backend,
        ACCOUNT_ID,
        &chat_id,
        json!({ "updateMemberRoles": [{ "id": ACCOUNT_ID, "role": "member" }] }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&chat_id]["type"], "forbidden", "{resp}");
    assert_eq!(
        stored_members(&backend, &chat_id).await,
        pairs(&[
            (ACCOUNT_ID, "admin"),
            ("bob", "member"),
            ("carol", "member")
        ])
    );
}
