//! `SpaceInvite/set` create and destroy are gated on the `manage_members`
//! Space permission, with destroy also allowed to the invite's owner
//! (bd:JMAP-x2gd.3).
//!
//! Oracle: draft-atwood-jmap-chat-00 §SpaceInvite/set, quoted on each test.

mod common;

use common::{ACCOUNT_ID, IdentityBackend, SPACE_ID, seed_space};
use jmap_chat_server::handle_invite_set;
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const INVITE_ID: &str = "inv1";

/// A backend with a Space where exactly one of the owner, whose member
/// entry is the accountId (draft §Terminology), and `manager` holds a
/// role granting `manage_members`, plus one existing SpaceInvite in that
/// Space created by `invite_creator`.
fn seeded(owner_may_manage: bool, invite_creator: &str) -> IdentityBackend {
    let backend = IdentityBackend::new();
    let (owner_roles, manager_roles) = if owner_may_manage {
        (json!(["r-mm"]), json!([]))
    } else {
        (json!([]), json!(["r-mm"]))
    };
    seed_space(
        &backend,
        json!([{ "id": "r-mm", "name": "Mod", "permissions": ["manage_members"], "position": 10 }]),
        json!([
            { "id": ACCOUNT_ID, "roleIds": owner_roles, "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "manager", "roleIds": manager_roles, "joinedAt": "2026-01-01T00:00:00Z" },
        ]),
    );
    backend.inner().insert_object_for_test(
        "SpaceInvite",
        ACCOUNT_ID,
        INVITE_ID,
        json!({
            "id": INVITE_ID,
            "code": "code1",
            "spaceId": SPACE_ID,
            "createdBy": invite_creator,
            "uses": 0,
            "createdAt": "2026-01-01T00:00:00Z",
        }),
    );
    backend
}

/// Send `SpaceInvite/set` with `body` merged into the request as the owner.
async fn set(backend: &IdentityBackend, body: Value) -> Value {
    let mut args = json!({ "accountId": ACCOUNT_ID });
    let (Value::Object(args_map), Value::Object(body_map)) = (&mut args, body) else {
        unreachable!("both are object literals");
    };
    args_map.extend(body_map);
    let HandlerOutput { primary: resp, .. } = handle_invite_set(
        backend,
        &Id::from("owner"),
        &std::collections::BTreeMap::new(),
        args,
    )
    .await
    .expect("SpaceInvite/set");
    resp
}

/// "`create` accepts: `spaceId` ... Requires `"manage_members"` permission."
#[tokio::test]
async fn create_requires_manage_members() {
    let body = json!({ "create": { "i0": { "spaceId": SPACE_ID } } });

    let denied = set(&seeded(false, "manager"), body.clone()).await;
    assert_eq!(denied["created"], Value::Null, "{denied:?}");
    assert_eq!(
        denied["notCreated"]["i0"]["type"], "forbidden",
        "{denied:?}"
    );

    let allowed = set(&seeded(true, "manager"), body).await;
    assert_eq!(allowed["notCreated"], Value::Null, "{allowed:?}");
    assert!(allowed["created"].get("i0").is_some(), "{allowed:?}");
}

/// A backend that cannot mint an invite code fails that create with
/// `serverFail` (RFC 8620 §3.6.1: "An unexpected error occurred during
/// the processing of the call"); nothing is created.
#[tokio::test]
async fn create_fails_with_server_fail_when_code_generation_fails() {
    let backend = seeded(true, "manager");
    backend.fail_invite_codes();

    let resp = set(
        &backend,
        json!({ "create": { "i0": { "spaceId": SPACE_ID } } }),
    )
    .await;
    assert_eq!(resp["created"], Value::Null, "{resp:?}");
    assert_eq!(resp["notCreated"]["i0"]["type"], "serverFail", "{resp:?}");
}

/// "`destroy` revokes an invite. Requires `"manage_members"` permission
/// or ownership of the invite." Here the invite belongs to someone else.
#[tokio::test]
async fn destroy_of_others_invite_requires_manage_members() {
    let body = json!({ "destroy": [INVITE_ID] });

    let denied = set(&seeded(false, "manager"), body.clone()).await;
    assert_eq!(denied["destroyed"], Value::Null, "{denied:?}");
    assert_eq!(
        denied["notDestroyed"][INVITE_ID]["type"], "forbidden",
        "{denied:?}"
    );

    let allowed = set(&seeded(true, "manager"), body).await;
    assert_eq!(allowed["notDestroyed"], Value::Null, "{allowed:?}");
    assert_eq!(allowed["destroyed"], json!([INVITE_ID]), "{allowed:?}");
}

/// "... or ownership of the invite": the owner's own invite (`createdBy`
/// is the accountId, draft §SpaceInvite.createdBy) is destroyable without
/// `manage_members`.
#[tokio::test]
async fn destroy_of_own_invite_needs_no_manage_members() {
    let resp = set(
        &seeded(false, ACCOUNT_ID),
        json!({ "destroy": [INVITE_ID] }),
    )
    .await;
    assert_eq!(resp["notDestroyed"], Value::Null, "{resp:?}");
    assert_eq!(resp["destroyed"], json!([INVITE_ID]), "{resp:?}");
}
