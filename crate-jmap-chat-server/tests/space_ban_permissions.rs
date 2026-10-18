//! `SpaceBan/set` create, update and destroy are gated on the `ban`
//! Space permission (bd:JMAP-yt1w.11).
//!
//! Oracle: draft-atwood-jmap-chat-00 §SpaceBan/set, quoted on each test.

mod common;

use common::{seed_space, IdentityBackend, ACCOUNT_ID, SPACE_ID};
use jmap_chat_server::handle_ban_set;
use jmap_types::Id;
use serde_json::{json, Value};

const BAN_ID: &str = "ban1";

/// Seed a Space where `banner` holds a role granting `ban` and `other`
/// holds no roles, plus one existing SpaceBan in that Space.
fn seed(backend: &IdentityBackend) {
    seed_space(
        backend,
        json!([{ "id": "r-ban", "name": "Banner", "permissions": ["ban"], "position": 10 }]),
        json!([
            { "id": "banner", "roleIds": ["r-ban"], "joinedAt": "2026-01-01T00:00:00Z" },
            { "id": "other", "roleIds": [], "joinedAt": "2026-01-01T00:00:00Z" },
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
}

/// Send `SpaceBan/set` with `body` merged into the request as `caller`.
async fn set_as(backend: &IdentityBackend, caller: &str, body: Value) -> Value {
    let mut args = json!({ "accountId": ACCOUNT_ID });
    let (Value::Object(args_map), Value::Object(body_map)) = (&mut args, body) else {
        unreachable!("both are object literals");
    };
    args_map.extend(body_map);
    let (resp, _) = handle_ban_set(backend, &Id::from(caller), args)
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
    let backend = IdentityBackend::new();
    seed(&backend);

    let denied = set_as(&backend, "other", create_body()).await;
    assert_eq!(denied["created"], Value::Null, "{denied:?}");
    assert_eq!(
        denied["notCreated"]["b0"]["type"], "forbidden",
        "{denied:?}"
    );

    let allowed = set_as(&backend, "banner", create_body()).await;
    assert_eq!(allowed["notCreated"], Value::Null, "{allowed:?}");
    assert!(allowed["created"].get("b0").is_some(), "{allowed:?}");
}

/// "`update` supports: `reason`, `expiresAt`. Requires `"ban"`
/// permission in the Space."
#[tokio::test]
async fn update_requires_ban_permission() {
    let backend = IdentityBackend::new();
    seed(&backend);
    let body = json!({ "update": { BAN_ID: { "reason": "spam" } } });

    let denied = set_as(&backend, "other", body.clone()).await;
    assert_eq!(denied["updated"], Value::Null, "{denied:?}");
    assert_eq!(
        denied["notUpdated"][BAN_ID]["type"], "forbidden",
        "{denied:?}"
    );

    let allowed = set_as(&backend, "banner", body).await;
    assert_eq!(allowed["notUpdated"], Value::Null, "{allowed:?}");
    assert!(allowed["updated"].get(BAN_ID).is_some(), "{allowed:?}");
}

/// "`destroy` lifts the ban. Requires `"ban"` permission in the Space."
#[tokio::test]
async fn destroy_requires_ban_permission() {
    let backend = IdentityBackend::new();
    seed(&backend);
    let body = json!({ "destroy": [BAN_ID] });

    let denied = set_as(&backend, "other", body.clone()).await;
    assert_eq!(denied["destroyed"], Value::Null, "{denied:?}");
    assert_eq!(
        denied["notDestroyed"][BAN_ID]["type"], "forbidden",
        "{denied:?}"
    );

    let allowed = set_as(&backend, "banner", body).await;
    assert_eq!(allowed["notDestroyed"], Value::Null, "{allowed:?}");
    assert_eq!(allowed["destroyed"], json!([BAN_ID]), "{allowed:?}");
}
