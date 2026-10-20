//! Burn-on-read sets `readAt` and hard-deletes in one atomic backend call,
//! `ChatBackend::read_and_burn_message` (bd:JMAP-x2gd.6).
//!
//! Oracle: draft-atwood-jmap-chat-00 §Message `burnOnRead`: "When `true`,
//! the receiving server MUST permanently hard-delete this message (row
//! removal, not tombstone) immediately after setting `readAt`." The
//! `readAt` write must therefore never be stored without the deletion:
//! a failed burn commits nothing, and retrying the same patch burns.

mod common;

use common::{seed_group_chat, InjectableBackend};
use jmap_chat_server::{handle_message_changes, handle_message_get, handle_message_set};
use jmap_server::HandlerOutput;
use jmap_types::Id;
use serde_json::{json, Value};

const ACCOUNT_ID: &str = "a1";
const READ_AT: &str = "2024-01-02T00:00:00Z";

/// A backend holding one `burnOnRead` message; returns it and its id.
async fn seeded() -> (InjectableBackend, String) {
    let backend = InjectableBackend::new();
    backend.inner.register_account(&Id::from(ACCOUNT_ID));
    seed_group_chat(&backend.inner, "c1");
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        &backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": {
                "m0": {
                    "chatId": "c1",
                    "body": "burn me",
                    "sentAt": "2024-01-01T00:00:00Z",
                    "burnOnRead": true
                }
            }
        }),
    )
    .await
    .expect("create");
    let id = resp["created"]["m0"]["id"]
        .as_str()
        .expect("created id")
        .to_owned();
    (backend, id)
}

async fn set_update(backend: &InjectableBackend, id: &str, patch: Value) -> Value {
    handle_message_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "update": { id: patch } }),
    )
    .await
    .expect("Message/set")
    .primary
}

async fn get(backend: &InjectableBackend, id: &str) -> Value {
    handle_message_get(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
    )
    .await
    .expect("Message/get")
    .primary
}

async fn state(backend: &InjectableBackend, id: &str) -> String {
    get(backend, id).await["state"]
        .as_str()
        .expect("state")
        .to_owned()
}

/// A failed burn leaves the message unread and unchanged; the retry burns
/// it and `Message/changes` reports it `destroyed`, not `updated` (draft
/// §Message `senderExpiresAt`: "A hard-deleted message appears in the
/// `destroyed` list of subsequent `Message/changes` responses, not
/// `updated`.").
#[tokio::test]
async fn failed_burn_commits_nothing_and_retry_burns() {
    let (backend, id) = seeded().await;
    let before = state(&backend, &id).await;

    backend.inject("Message", "read_and_burn");
    let resp = set_update(&backend, &id, json!({ "readAt": READ_AT })).await;
    assert_eq!(
        resp["notUpdated"][&id]["type"], "serverFail",
        "resp: {resp}"
    );
    assert!(resp["updated"].get(&id).is_none(), "resp: {resp}");

    let after_fail = get(&backend, &id).await;
    assert_eq!(after_fail["list"][0]["id"], json!(id), "{after_fail}");
    assert_eq!(after_fail["list"][0]["readAt"], Value::Null, "{after_fail}");
    assert_eq!(after_fail["state"], json!(before), "no state change");

    let retry = set_update(&backend, &id, json!({ "readAt": READ_AT })).await;
    assert_eq!(retry["updated"][&id], Value::Null, "retry: {retry}");
    assert_eq!(retry["notUpdated"], Value::Null, "retry: {retry}");

    let gone = get(&backend, &id).await;
    assert_eq!(gone["list"], json!([]), "{gone}");
    assert_eq!(gone["notFound"], json!([id]), "{gone}");

    let HandlerOutput {
        primary: changes, ..
    } = handle_message_changes(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "sinceState": before }),
    )
    .await
    .expect("Message/changes");
    assert_eq!(changes["destroyed"], json!([id]), "{changes}");
    assert_eq!(changes["updated"], json!([]), "{changes}");
}

/// A `readAt` patch the backend cannot store is rejected and burns
/// nothing. RFC 8620 §5.3: "invalidProperties: The record given is
/// invalid in some way." `readAt` is a UTCDate, so a number is invalid.
#[tokio::test]
async fn invalid_read_at_patch_is_rejected_without_burning() {
    let (backend, id) = seeded().await;
    let before = state(&backend, &id).await;

    let resp = set_update(&backend, &id, json!({ "readAt": 42 })).await;
    assert_eq!(
        resp["notUpdated"][&id]["type"], "invalidProperties",
        "resp: {resp}"
    );

    let still = get(&backend, &id).await;
    assert_eq!(still["list"][0]["id"], json!(id), "{still}");
    assert_eq!(still["list"][0]["readAt"], Value::Null, "{still}");
    assert_eq!(still["state"], json!(before), "no state change");
}
