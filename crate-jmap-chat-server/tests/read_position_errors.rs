//! `ReadPosition/*` argument, validation and backend-failure branches
//! (bd:JMAP-w8fu.3).
//!
//! Oracles: draft-atwood-jmap-chat-00 §ReadPosition/set (jmap-chat-spec
//! 29e9a41 L1430-1436) and RFC 8620 §1.2, §3.6, §5.2 and §5.3, quoted on
//! each test; expected responses are hand-written.

mod common;

use std::collections::BTreeMap;

use common::{ACCOUNT_ID, INJECTABLE_BACKEND_CANARY, InjectableBackend, account_backend};
use jmap_chat_server::memory::MemoryBackend;
use jmap_chat_server::{
    ChatBackend, handle_position_changes, handle_position_get, handle_position_set,
};
use jmap_server::HandlerOutput;
use jmap_types::JmapError;
use serde_json::{Value, json};

const RP_ID: &str = "rp1";

fn seeded() -> MemoryBackend {
    let backend = account_backend();
    seed(&backend);
    backend
}

fn seed(backend: &MemoryBackend) {
    backend.insert_object_for_test(
        "ReadPosition",
        ACCOUNT_ID,
        RP_ID,
        json!({ "id": RP_ID, "chatId": "c1" }),
    );
}

async fn set<B: ChatBackend<CallerCtx = ()>>(backend: &B, body: Value) -> Result<Value, JmapError> {
    let mut args = body;
    args["accountId"] = json!(ACCOUNT_ID);
    handle_position_set(backend, &(), &BTreeMap::new(), args)
        .await
        .map(|HandlerOutput { primary, .. }| primary)
}

async fn stored<B: ChatBackend<CallerCtx = ()>>(backend: &B) -> Value {
    let HandlerOutput { primary, .. } =
        handle_position_get(backend, &(), json!({ "accountId": ACCOUNT_ID }))
            .await
            .expect("ReadPosition/get");
    primary["list"][0].clone()
}

fn assert_error_type(result: Result<Value, JmapError>, expected: &str) {
    let err = result.expect_err(expected);
    assert_eq!(err.error_type.to_string(), expected, "{err:?}");
}

/// RFC 8620 §5.2: `sinceState` is a required argument ("The current state
/// of the client"); §3.6.2 `invalidArguments`: "a required argument is
/// missing."
#[tokio::test]
async fn changes_without_since_state_is_invalid_arguments() {
    let result = handle_position_changes(&seeded(), &(), json!({ "accountId": ACCOUNT_ID }))
        .await
        .map(|HandlerOutput { primary, .. }| primary);
    assert_error_type(result, "invalidArguments");
}

/// RFC 8620 §5.3 `ifInState`: "If supplied, the string must match the
/// current state; otherwise, the method will be aborted and a
/// "stateMismatch" error returned."
#[tokio::test]
async fn set_with_stale_if_in_state_is_state_mismatch() {
    let backend = seeded();
    let result = set(
        &backend,
        json!({
            "ifInState": "not-the-current-state",
            "update": { RP_ID: { "lastReadMessageId": "m1" } }
        }),
    )
    .await;
    assert_error_type(result, "stateMismatch");
    assert!(stored(&backend).await.get("lastReadMessageId").is_none());
}

/// RFC 8620 §1.2: an Id "MUST only contain characters from the 'URL and
/// Filename Safe' base64 alphabet"; `"bad id"` contains a space, so the
/// entry fails with `invalidProperties` naming `id`.
#[tokio::test]
async fn update_with_malformed_id_is_invalid_properties() {
    let resp = set(
        &seeded(),
        json!({ "update": { "bad id": { "lastReadMessageId": "m1" } } }),
    )
    .await
    .expect("ReadPosition/set");
    assert_eq!(resp["updated"], Value::Null, "{resp}");
    assert_eq!(
        resp["notUpdated"]["bad id"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notUpdated"]["bad id"]["properties"], json!(["id"]));
}

/// RFC 8620 §5.3: "the client refers to the new record using its creation
/// id prefixed with a "#"" and the server substitutes the id it created in
/// this request. No record was created under `nope`, so the reference
/// cannot resolve and the update fails naming `lastReadMessageId`.
#[tokio::test]
async fn update_with_unknown_creation_ref_is_invalid_properties() {
    let backend = seeded();
    let resp = set(
        &backend,
        json!({ "update": { RP_ID: { "lastReadMessageId": "#nope" } } }),
    )
    .await
    .expect("ReadPosition/set");
    assert_eq!(resp["updated"], Value::Null, "{resp}");
    assert_eq!(
        resp["notUpdated"][RP_ID]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"][RP_ID]["properties"],
        json!(["lastReadMessageId"])
    );
    assert!(stored(&backend).await.get("lastReadMessageId").is_none());
}

/// RFC 8620 §5.3 `update` is "Id[PatchObject]|null" and a PatchObject is
/// a JSON object; `invalidPatch`: "The PatchObject given to update the
/// record was not a valid patch".
#[tokio::test]
async fn update_with_non_object_patch_is_invalid_patch() {
    let resp = set(&seeded(), json!({ "update": { RP_ID: "m1" } }))
        .await
        .expect("ReadPosition/set");
    assert_eq!(resp["updated"], Value::Null, "{resp}");
    assert_eq!(resp["notUpdated"][RP_ID]["type"], "invalidPatch", "{resp}");
}

/// RFC 8620 §3.6.1 `serverFail` surfaced per record in `notUpdated`
/// (§5.3) when the store fails; the backend's error text is not wired.
#[tokio::test]
async fn update_backend_failure_is_server_fail_without_backend_text() {
    let backend = InjectableBackend::new();
    seed(&backend.inner);
    backend.inject("ReadPosition", "update");
    let resp = set(
        &backend,
        json!({ "update": { RP_ID: { "lastReadMessageId": "m1" } } }),
    )
    .await
    .expect("ReadPosition/set");
    assert_eq!(resp["updated"], Value::Null, "{resp}");
    assert_eq!(resp["notUpdated"][RP_ID]["type"], "serverFail", "{resp}");
    assert!(
        !resp.to_string().contains(INJECTABLE_BACKEND_CANARY),
        "leaked backend text: {resp}"
    );
    assert!(stored(&backend).await.get("lastReadMessageId").is_none());
}

/// RFC 8620 §5.3 `destroy` is "Id[]|null"; a number element is the wrong
/// type (§3.6.2 `invalidArguments`: "One of the arguments is of the wrong
/// type").
#[tokio::test]
async fn destroy_with_non_string_element_is_invalid_arguments() {
    assert_error_type(
        set(&seeded(), json!({ "destroy": [RP_ID, 7] })).await,
        "invalidArguments",
    );
}
