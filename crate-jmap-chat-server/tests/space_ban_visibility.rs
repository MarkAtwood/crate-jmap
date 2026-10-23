//! `SpaceBan/get` shows each caller only the bans the spec lets it see
//! (bd:JMAP-x2gd.9).
//!
//! Oracle: draft-atwood-jmap-chat-00 §SpaceBan/get, quoted on each test.

mod common;

use common::{ACCOUNT_ID, IdentityBackend, SPACE_ID, seed_space};
use jmap_chat_server::handle_ban_get;
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

/// A ban whose `userId` is the caller's accountId.
const OF_CALLER: &str = "ban-caller";
/// A ban on someone else.
const OF_TARGET: &str = "ban-target";

/// A backend with a Space where the caller, whose member entry is the
/// accountId (draft §Terminology), holds `ban` iff `caller_may_ban`, plus
/// one ban on the caller and one on `target`.
fn seeded(caller_may_ban: bool) -> IdentityBackend {
    let backend = IdentityBackend::new();
    let roles = if caller_may_ban {
        json!(["r-ban"])
    } else {
        json!([])
    };
    seed_space(
        &backend,
        json!([{ "id": "r-ban", "name": "Banner", "permissions": ["ban"], "position": 10 }]),
        json!([{ "id": ACCOUNT_ID, "roleIds": roles, "joinedAt": "2026-01-01T00:00:00Z" }]),
    );
    for (id, user) in [(OF_CALLER, ACCOUNT_ID), (OF_TARGET, "target")] {
        backend.inner().insert_object_for_test(
            "SpaceBan",
            ACCOUNT_ID,
            id,
            json!({
                "id": id,
                "spaceId": SPACE_ID,
                "userId": user,
                "bannedBy": "banner",
                "createdAt": "2026-01-01T00:00:00Z",
            }),
        );
    }
    backend
}

async fn get(backend: &IdentityBackend, ids: Value) -> Value {
    let args = json!({ "accountId": ACCOUNT_ID, "ids": ids });
    let HandlerOutput { primary: resp, .. } = handle_ban_get(backend, &Id::from("owner"), args)
        .await
        .expect("SpaceBan/get");
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

/// "Members with `"ban"` permission in the Space see all SpaceBan records
/// for that Space."
#[tokio::test]
async fn ban_holder_sees_all_bans() {
    let resp = get(&seeded(true), Value::Null).await;
    assert_eq!(listed_ids(&resp), [OF_CALLER, OF_TARGET], "{resp:?}");
    assert_eq!(resp["notFound"], json!([]), "{resp:?}");
}

/// "A banned user fetching their own account sees only SpaceBan records
/// where `userId` is the accountId."
#[tokio::test]
async fn caller_without_ban_sees_only_own_bans() {
    let backend = seeded(false);

    let all = get(&backend, Value::Null).await;
    assert_eq!(listed_ids(&all), [OF_CALLER], "{all:?}");
    assert_eq!(all["notFound"], json!([]), "{all:?}");

    let by_id = get(&backend, json!([OF_CALLER, OF_TARGET])).await;
    assert_eq!(listed_ids(&by_id), [OF_CALLER], "{by_id:?}");
    assert_eq!(by_id["notFound"], json!([OF_TARGET]), "{by_id:?}");
}
