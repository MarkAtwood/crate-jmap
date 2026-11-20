//! jmap-chat-compliance-tests 9.2: SpaceBan expiry (bd:JMAP-uf12).
//!
//! Oracles, quoted on each test: jmap-chat-compliance-tests.md §9.2 and
//! draft-atwood-jmap-chat-00 §SpaceBan and §SpaceBan/set.

mod common;

use std::collections::BTreeMap;
use std::time::SystemTime;

use common::{ACCOUNT_ID, MemoryBackend, SPACE_ID, account_backend};
use jmap_chat_server::{handle_ban_changes, handle_ban_get, handle_ban_set, sweep_expired};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{Value, json};

async fn ban_set(backend: &MemoryBackend<()>, body: Value) -> Value {
    let mut args = body;
    args["accountId"] = json!(ACCOUNT_ID);
    let HandlerOutput { primary, .. } = handle_ban_set(backend, &(), &BTreeMap::new(), args)
        .await
        .expect("SpaceBan/set");
    primary
}

async fn ban_get(backend: &MemoryBackend<()>, ids: Value) -> Value {
    let HandlerOutput { primary, .. } =
        handle_ban_get(backend, &(), json!({ "accountId": ACCOUNT_ID, "ids": ids }))
            .await
            .expect("SpaceBan/get");
    primary
}

async fn sweep(backend: &MemoryBackend<()>) {
    sweep_expired(backend, &(), &Id::from(ACCOUNT_ID), SystemTime::now())
        .await
        .expect("sweep");
}

/// 9.2 SpaceBan expiry auto-restore: "Create a SpaceBan with `expiresAt`
/// in the past. Run the expiry sweep. Verify the ban is removed." Draft
/// §SpaceBan `expiresAt`: "If present, the ban expires at this time and
/// the server MUST restore the user's access." The removal is a destroy
/// (§SpaceBan/set: "`destroy` lifts the ban"), so `SpaceBan/changes`
/// reports it.
#[tokio::test]
async fn compliance_9_2_expired_space_ban_is_removed() {
    let backend = account_backend();
    let resp = ban_set(
        &backend,
        json!({ "create": { "b": {
            "spaceId": SPACE_ID, "userId": "bob", "expiresAt": "2020-01-01T00:00:00Z",
        } } }),
    )
    .await;
    let ban_id = resp["created"]["b"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created: {resp}"))
        .to_owned();
    let since = resp["newState"].clone();

    sweep(&backend).await;

    let got = ban_get(&backend, json!([ban_id])).await;
    assert_eq!(got["list"], json!([]), "{got}");
    assert_eq!(got["notFound"], json!([ban_id]), "{got}");

    let HandlerOutput { primary, .. } = handle_ban_changes(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": since }),
    )
    .await
    .expect("SpaceBan/changes");
    assert_eq!(primary["destroyed"], json!([ban_id]), "{primary}");

    // Idempotent: a second run finds nothing to do.
    let state = got["state"].clone();
    sweep(&backend).await;
    assert_eq!(ban_get(&backend, Value::Null).await["state"], state);
}

/// The sweep removes only bans whose `expiresAt` has passed. Draft
/// §SpaceBan `expiresAt`: "If absent, the ban is permanent until
/// explicitly destroyed"; a ban whose time has not come stays in force.
#[tokio::test]
async fn sweep_keeps_space_bans_in_force() {
    let backend = account_backend();
    let resp = ban_set(
        &backend,
        json!({ "create": {
            "permanent": { "spaceId": SPACE_ID, "userId": "bob" },
            "future": {
                "spaceId": SPACE_ID, "userId": "carol", "expiresAt": "2999-01-01T00:00:00Z",
            },
        } }),
    )
    .await;
    assert_eq!(resp["notCreated"], Value::Null, "{resp}");

    sweep(&backend).await;

    let got = ban_get(&backend, Value::Null).await;
    assert_eq!(got["list"].as_array().map(Vec::len), Some(2), "{got}");
    assert_eq!(got["state"], resp["newState"], "{got}");
}
