//! `SpaceInvite/get` and `SpaceInvite/changes` show each caller only the
//! invites the spec lets it see (bd:JMAP-x2gd.9).
//!
//! Oracle: draft-atwood-jmap-chat-00 §SpaceInvite/get and
//! §SpaceInvite/changes, quoted on each test.

mod common;

use common::{ACCOUNT_ID, IdentityBackend, SPACE_ID, seed_space};
use jmap_chat_server::{handle_invite_changes, handle_invite_get, handle_invite_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const OWN: &str = "inv-own";
const OTHERS: &str = "inv-others";

/// Who the caller is in the seeded Space. The caller's member entry is
/// the accountId (draft §Terminology).
#[derive(Clone, Copy)]
enum Caller {
    Manager,
    Member,
    NonMember,
}

/// Seed [`SPACE_ID`] so the caller has the given standing, alongside a
/// member `other` without roles.
fn seed(backend: &IdentityBackend, caller: Caller) {
    let caller_member = |roles: Value| json!({ "id": ACCOUNT_ID, "roleIds": roles, "joinedAt": "2026-01-01T00:00:00Z" });
    let other = json!({ "id": "other", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" });
    let members = match caller {
        Caller::Manager => json!([caller_member(json!(["r-mm"])), other]),
        Caller::Member => json!([caller_member(json!([])), other]),
        Caller::NonMember => json!([other]),
    };
    seed_space(
        backend,
        json!([{ "id": "r-mm", "name": "Mod", "permissions": ["manage_members"], "position": 10 }]),
        members,
    );
}

fn invite(id: &str, created_by: &str) -> Value {
    json!({
        "id": id,
        "code": format!("code-{id}"),
        "spaceId": SPACE_ID,
        "createdBy": created_by,
        "uses": 0,
        "createdAt": "2026-01-01T00:00:00Z",
    })
}

/// A backend holding one invite created by the caller and one created by
/// `other`, both in [`SPACE_ID`].
fn seeded(caller: Caller) -> IdentityBackend {
    let backend = IdentityBackend::new();
    seed(&backend, caller);
    for (id, by) in [(OWN, ACCOUNT_ID), (OTHERS, "other")] {
        backend
            .inner()
            .insert_object_for_test("SpaceInvite", ACCOUNT_ID, id, invite(id, by));
    }
    backend
}

async fn get(backend: &IdentityBackend, ids: Value) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": ids });
    let HandlerOutput { primary: resp, .. } = handle_invite_get(backend, &Id::from("owner"), args)
        .await
        .expect("SpaceInvite/get");
    resp
}

fn listed_ids(resp: &Value) -> Vec<&str> {
    let mut ids: Vec<&str> = resp["list"]
        .as_array()
        .expect("list")
        .iter()
        .map(|o| o["id"].as_str().expect("id"))
        .collect();
    ids.sort_unstable();
    ids
}

/// "Members with `"manage_members"` permission see all invites"
#[tokio::test]
async fn manager_sees_all_invites() {
    let resp = get(&seeded(Caller::Manager), Value::Null).await;
    assert_eq!(listed_ids(&resp), [OTHERS, OWN], "{resp:?}");
    assert_eq!(resp["notFound"], json!([]), "{resp:?}");
}

/// "other members see only invites they created."
#[tokio::test]
async fn member_sees_only_own_invites() {
    let backend = seeded(Caller::Member);

    let all = get(&backend, Value::Null).await;
    assert_eq!(listed_ids(&all), [OWN], "{all:?}");
    assert_eq!(all["notFound"], json!([]), "{all:?}");

    let by_id = get(&backend, json!([OWN, OTHERS])).await;
    assert_eq!(listed_ids(&by_id), [OWN], "{by_id:?}");
    assert_eq!(by_id["notFound"], json!([OTHERS]), "{by_id:?}");
}

/// "Only members of the Space may retrieve its invites." This holds even
/// for an invite whose `createdBy` is the caller.
#[tokio::test]
async fn non_member_sees_no_invites() {
    let resp = get(&seeded(Caller::NonMember), json!([OWN, OTHERS])).await;
    assert_eq!(resp["list"], json!([]), "{resp:?}");
    assert_eq!(resp["notFound"], json!([OWN, OTHERS]), "{resp:?}");
}

/// "The same visibility rules as `SpaceInvite/get` apply: members with
/// `"manage_members"` permission see all changes; other members see only
/// changes to invites they created."
///
/// An invite is created through `SpaceInvite/set` so the change log
/// records it, and then the caller loses `manage_members`. `createdBy`
/// is server-set to the creator, so the stored record is then rewritten
/// to have been created by `other` to model an invite the caller did not
/// create.
#[tokio::test]
async fn changes_omit_invites_hidden_from_caller() {
    for (created_by, expect_visible) in [(ACCOUNT_ID, true), ("other", false)] {
        let backend = IdentityBackend::new();
        seed(&backend, Caller::Manager);
        let since = get(&backend, json!([])).await["state"].clone();

        let HandlerOutput { primary: set, .. } = handle_invite_set(
            &backend,
            &Id::from("owner"),
            &std::collections::BTreeMap::new(),
            json!({ "accountId": ACCOUNT_ID, "create": { "i0": { "spaceId": SPACE_ID } } }),
        )
        .await
        .expect("SpaceInvite/set");
        let id = set["created"]["i0"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("invite created: {set:?}"))
            .to_owned();

        seed(&backend, Caller::Member);
        backend.inner().insert_object_for_test(
            "SpaceInvite",
            ACCOUNT_ID,
            &id,
            invite(&id, created_by),
        );

        let HandlerOutput {
            primary: changes, ..
        } = handle_invite_changes(
            &backend,
            &Id::from("owner"),
            json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
        )
        .await
        .expect("SpaceInvite/changes");
        let expected = if expect_visible {
            json!([id])
        } else {
            json!([])
        };
        assert_eq!(
            changes["created"], expected,
            "createdBy={created_by}: {changes:?}"
        );
    }
}
