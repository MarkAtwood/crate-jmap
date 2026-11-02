//! `SpaceBan/set` create, update and destroy are gated on the `ban`
//! Space permission (bd:JMAP-yt1w.11).
//!
//! Oracle: draft-atwood-jmap-chat-00 §SpaceBan/set, quoted on each test.

mod common;

use common::{ACCOUNT_ID, IdentityBackend, SPACE_ID, seed_space};
use jmap_chat_server::handle_ban_set;
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

const BAN_ID: &str = "ban1";

/// A backend with a Space where exactly one of the owner, whose member
/// entry is the accountId (draft §Terminology), and `banner` holds a role
/// granting `ban`, plus one existing SpaceBan in that Space.
fn seeded(owner_may_ban: bool) -> IdentityBackend {
    let backend = IdentityBackend::new();
    let (owner_roles, banner_roles) = if owner_may_ban {
        (json!(["r-ban"]), json!([]))
    } else {
        (json!([]), json!(["r-ban"]))
    };
    seed_space(
        &backend,
        json!([{ "id": "r-ban", "name": "Banner", "permissions": ["ban"], "position": 10 }]),
        json!([
            { "id": ACCOUNT_ID, "roleIds": owner_roles, "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "banner", "roleIds": banner_roles, "joinedAt": "2026-01-01T00:00:00Z" },
        ]),
    );
    backend.inner().insert_object_for_test(
        "SpaceBan",
        ACCOUNT_ID,
        BAN_ID,
        json!({
            "id": BAN_ID,
            "spaceId": SPACE_ID,
            "userId": "target",
            "bannedBy": "banner",
            "createdAt": "2026-01-01T00:00:00Z",
        }),
    );
    backend
}

/// Send `SpaceBan/set` with `body` merged into the request as the owner.
async fn set(backend: &IdentityBackend, body: Value) -> Value {
    let mut args = json!({ "accountId": ACCOUNT_ID });
    let (Value::Object(args_map), Value::Object(body_map)) = (&mut args, body) else {
        unreachable!("both are object literals");
    };
    args_map.extend(body_map);
    let HandlerOutput { primary: resp, .. } = handle_ban_set(
        backend,
        &Id::from("owner"),
        &std::collections::BTreeMap::new(),
        args,
    )
    .await
    .expect("SpaceBan/set");
    resp
}

fn create_body() -> Value {
    json!({ "create": { "b0": { "spaceId": SPACE_ID, "userId": "target2" } } })
}

/// "`create` accepts: ... Requires `"ban"` permission in the Space."
#[tokio::test]
async fn create_requires_ban_permission() {
    let denied = set(&seeded(false), create_body()).await;
    assert_eq!(denied["created"], Value::Null, "{denied:?}");
    assert_eq!(
        denied["notCreated"]["b0"]["type"], "forbidden",
        "{denied:?}"
    );

    let allowed = set(&seeded(true), create_body()).await;
    assert_eq!(allowed["notCreated"], Value::Null, "{allowed:?}");
    assert!(allowed["created"].get("b0").is_some(), "{allowed:?}");
}

/// "`update` supports: `reason`, `expiresAt`. Requires `"ban"`
/// permission in the Space."
#[tokio::test]
async fn update_requires_ban_permission() {
    let body = json!({ "update": { BAN_ID: { "reason": "spam" } } });

    let denied = set(&seeded(false), body.clone()).await;
    assert_eq!(denied["updated"], Value::Null, "{denied:?}");
    assert_eq!(
        denied["notUpdated"][BAN_ID]["type"], "forbidden",
        "{denied:?}"
    );

    let allowed = set(&seeded(true), body).await;
    assert_eq!(allowed["notUpdated"], Value::Null, "{allowed:?}");
    assert!(allowed["updated"].get(BAN_ID).is_some(), "{allowed:?}");
}

/// jmap-chat-spec 77c3474 L1533 "`update` supports: `reason`,
/// `expiresAt`. Requires `"ban"` permission in the Space." and RFC 8620
/// §5.3 `forbidden`: "The action would violate an ACL or other permissions
/// policy." An empty patch and a server-set key are updates too, so the
/// permission gate refuses them before any no-op or read-only check.
#[tokio::test]
async fn empty_or_server_set_update_requires_ban_permission() {
    for patch in [json!({}), json!({ "bannedBy": "someone-else" })] {
        let body = json!({ "update": { BAN_ID: patch.clone() } });
        let denied = set(&seeded(false), body).await;
        assert_eq!(denied["updated"], Value::Null, "{patch}: {denied}");
        assert_eq!(
            denied["notUpdated"][BAN_ID]["type"], "forbidden",
            "{patch}: {denied}"
        );
    }

    // With the permission, `{}` is the no-op success RFC 8620 §5.3
    // `updated` describes: "null if none" changed.
    let allowed = set(&seeded(true), json!({ "update": { BAN_ID: {} } })).await;
    assert_eq!(allowed["notUpdated"], Value::Null, "{allowed}");
    assert_eq!(allowed["updated"], json!({ BAN_ID: null }), "{allowed}");
}

/// "`destroy` lifts the ban. Requires `"ban"` permission in the Space."
#[tokio::test]
async fn destroy_requires_ban_permission() {
    let body = json!({ "destroy": [BAN_ID] });

    let denied = set(&seeded(false), body.clone()).await;
    assert_eq!(denied["destroyed"], Value::Null, "{denied:?}");
    assert_eq!(
        denied["notDestroyed"][BAN_ID]["type"], "forbidden",
        "{denied:?}"
    );

    let allowed = set(&seeded(true), body).await;
    assert_eq!(allowed["notDestroyed"], Value::Null, "{allowed:?}");
    assert_eq!(allowed["destroyed"], json!([BAN_ID]), "{allowed:?}");
}
