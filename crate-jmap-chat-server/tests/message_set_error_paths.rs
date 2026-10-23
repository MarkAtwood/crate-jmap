//! `Message/set` request-validation and backend-failure branches
//! (bd:JMAP-w8fu.12).
//!
//! Oracles, quoted from RFC 8620 §5.3
//! (https://www.rfc-editor.org/rfc/rfc8620.txt) and
//! draft-atwood-jmap-chat-00 at jmap-chat-spec 29e9a41:
//!
//! - RFC 8620 §5.3: "stateMismatch": An "ifInState" argument was supplied,
//!   and it does not match the current state.
//! - RFC 8620 §5.3: "notFound": (update; destroy). The id given to
//!   update/destroy cannot be found.
//! - RFC 8620 §5.3: "invalidPatch": (update). The PatchObject given to
//!   update the record was not a valid patch.
//! - RFC 8620 §3.6.1: "invalidArguments": One of the arguments is of the
//!   wrong type or is otherwise invalid.
//! - draft L1206 (§Creating a Message): "`chatId` (String, required),
//!   `body` (String, required), `bodyType` (String, required), `sentAt`
//!   (UTCDate, required)."
//! - draft L692: "`senderId` (String, immutable, server-set)".
//! - draft L1754 (§Input Validation): a failed check with no other named
//!   error is "an `invalidProperties` SetError ... whose `properties`
//!   names the offending property."
//! - Backend failures surface as a per-entry `serverFail` whose
//!   description is the static [`jmap_server::SERVER_FAIL_INTERNAL_DESC`],
//!   never the backend error's Display text (workspace redaction policy,
//!   as in `tests/integration.rs`).

mod common;

use common::{ACCOUNT_ID, INJECTABLE_BACKEND_CANARY, InjectableBackend};
use jmap_chat_server::ChatBackend;
use jmap_chat_server::message::handle_message_set;
use jmap_server::HandlerOutput;
use jmap_types::JmapError;
use serde_json::{Value, json};

const SENT_AT: &str = "2024-01-01T00:00:00Z";

async fn try_set<B: ChatBackend<CallerCtx = ()>>(
    backend: &B,
    args: Value,
) -> Result<Value, JmapError> {
    handle_message_set(backend, &(), &std::collections::BTreeMap::new(), args)
        .await
        .map(|HandlerOutput { primary, .. }| primary)
}

async fn set<B: ChatBackend<CallerCtx = ()>>(backend: &B, args: Value) -> Value {
    try_set(backend, args).await.expect("Message/set")
}

fn wire(err: JmapError) -> Value {
    serde_json::to_value(&err).expect("JmapError must serialize")
}

/// A backend holding group Chat `c1`.
fn backend() -> InjectableBackend {
    let backend = InjectableBackend::new();
    common::seed_group_chat(&backend.inner, "c1");
    backend
}

/// Create a Message in `c1` with no fault queued; return its id.
async fn create_message(backend: &InjectableBackend) -> String {
    let resp = set(
        backend,
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m": { "chatId": "c1", "body": "hi", "sentAt": SENT_AT } }
        }),
    )
    .await;
    resp["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created id: {resp}"))
        .to_owned()
}

fn create_args(obj: Value) -> Value {
    json!({ "accountId": ACCOUNT_ID, "create": { "m": obj } })
}

fn assert_invalid_properties(err: &Value, property: &str) {
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!([property]), "{err}");
}

fn assert_redacted_server_fail(resp: &Value, map: &str, key: &str) {
    let wire = resp.to_string();
    assert!(
        !wire.contains(INJECTABLE_BACKEND_CANARY),
        "backend error text must not reach the wire: {wire}"
    );
    assert_eq!(resp[map][key]["type"], "serverFail", "{resp}");
    assert_eq!(
        resp[map][key]["description"].as_str(),
        Some(jmap_server::SERVER_FAIL_INTERNAL_DESC),
        "{resp}"
    );
}

#[tokio::test]
async fn if_in_state_mismatch_is_method_error() {
    let backend = backend();
    let err = try_set(
        &backend,
        json!({
            "accountId": ACCOUNT_ID,
            "ifInState": "not-the-current-state",
            "create": { "m": { "chatId": "c1", "body": "hi", "sentAt": SENT_AT } }
        }),
    )
    .await
    .expect_err("a stale ifInState must abort the call");
    assert_eq!(wire(err)["type"], "stateMismatch");
}

#[tokio::test]
async fn create_rejects_server_set_sender_id() {
    let backend = backend();
    let resp = set(
        &backend,
        create_args(json!({
            "chatId": "c1", "body": "hi", "sentAt": SENT_AT, "senderId": "someone-else"
        })),
    )
    .await;
    assert_invalid_properties(&resp["notCreated"]["m"], "senderId");
}

#[tokio::test]
async fn create_rejects_malformed_chat_id() {
    let backend = backend();
    let resp = set(
        &backend,
        create_args(json!({ "chatId": "bad id!", "body": "hi", "sentAt": SENT_AT })),
    )
    .await;
    assert_invalid_properties(&resp["notCreated"]["m"], "chatId");
}

#[tokio::test]
async fn create_rejects_missing_body() {
    let backend = backend();
    let resp = set(
        &backend,
        create_args(json!({ "chatId": "c1", "sentAt": SENT_AT })),
    )
    .await;
    assert_invalid_properties(&resp["notCreated"]["m"], "body");
}

#[tokio::test]
async fn create_rejects_missing_sent_at() {
    let backend = backend();
    let resp = set(
        &backend,
        create_args(json!({ "chatId": "c1", "body": "hi" })),
    )
    .await;
    assert_invalid_properties(&resp["notCreated"]["m"], "sentAt");
}

/// A malformed id cannot name a stored Message, so the update fails per
/// entry and the state does not move.
#[tokio::test]
async fn update_rejects_malformed_id() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { "bad id!": { "readAt": SENT_AT } } }),
    )
    .await;
    assert!(resp["notUpdated"]["bad id!"]["type"].is_string(), "{resp}");
    assert!(resp["updated"].get("bad id!").is_none(), "{resp}");
    assert_eq!(resp["oldState"], resp["newState"], "{resp}");
}

#[tokio::test]
async fn update_rejects_non_object_patch() {
    let backend = backend();
    let id = create_message(&backend).await;
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { &id: "not a patch" } }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&id]["type"], "invalidPatch", "{resp}");
}

#[tokio::test]
async fn destroy_rejects_non_string_element() {
    let backend = backend();
    let err = try_set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "destroy": [42] }),
    )
    .await
    .expect_err("a non-string destroy element is a wrongly-typed argument");
    assert_eq!(wire(err)["type"], "invalidArguments");
}

#[tokio::test]
async fn destroy_rejects_malformed_id() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "destroy": ["bad id!"] }),
    )
    .await;
    assert!(
        resp["notDestroyed"]["bad id!"]["type"].is_string(),
        "{resp}"
    );
    assert_eq!(resp["oldState"], resp["newState"], "{resp}");
}

#[tokio::test]
async fn destroy_unknown_id_is_not_found() {
    let backend = backend();
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "destroy": ["Mnope"] }),
    )
    .await;
    assert_eq!(resp["notDestroyed"]["Mnope"]["type"], "notFound", "{resp}");
}

#[tokio::test]
async fn create_store_failure_is_redacted_server_fail() {
    let backend = backend();
    backend.inject("Message", "create");
    let resp = set(
        &backend,
        create_args(json!({ "chatId": "c1", "body": "hi", "sentAt": SENT_AT })),
    )
    .await;
    assert_redacted_server_fail(&resp, "notCreated", "m");
}

#[tokio::test]
async fn create_chat_lookup_failure_is_redacted_server_fail() {
    let backend = backend();
    backend.inject("Chat", "get");
    let resp = set(
        &backend,
        create_args(json!({ "chatId": "c1", "body": "hi", "sentAt": SENT_AT })),
    )
    .await;
    assert_redacted_server_fail(&resp, "notCreated", "m");
}

#[tokio::test]
async fn update_message_lookup_failure_is_redacted_server_fail() {
    let backend = backend();
    let id = create_message(&backend).await;
    backend.inject("Message", "get");
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { &id: { "body": "edited" } } }),
    )
    .await;
    assert_redacted_server_fail(&resp, "notUpdated", &id);
}

#[tokio::test]
async fn update_chat_lookup_failure_is_redacted_server_fail() {
    let backend = backend();
    let id = create_message(&backend).await;
    backend.inject("Chat", "get");
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { &id: { "body": "edited" } } }),
    )
    .await;
    assert_redacted_server_fail(&resp, "notUpdated", &id);
}

#[tokio::test]
async fn destroy_failure_is_redacted_server_fail() {
    let backend = backend();
    let id = create_message(&backend).await;
    backend.inject("Message", "destroy");
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "destroy": [&id] }),
    )
    .await;
    assert_redacted_server_fail(&resp, "notDestroyed", &id);
}
