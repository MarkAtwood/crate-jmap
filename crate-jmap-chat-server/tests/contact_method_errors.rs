//! `ChatContact/*` argument, validation and backend-failure branches
//! (bd:JMAP-w8fu.7).
//!
//! Oracles: draft-atwood-jmap-chat-00 §ChatContact/set (jmap-chat-spec
//! 6c38e2f) and RFC 8620 §3.6, §5.1, §5.2, §5.3 and §5.5, quoted on each
//! test; expected responses are hand-written.

mod common;

use common::{ACCOUNT_ID, INJECTABLE_BACKEND_CANARY, InjectableBackend, account_backend};
use jmap_chat_server::ChatBackend;
use jmap_chat_server::memory::MemoryBackend;
use jmap_chat_server::{
    handle_contact_changes, handle_contact_get, handle_contact_query, handle_contact_set,
};
use jmap_server::HandlerOutput;
use jmap_types::JmapError;
use serde_json::{Value, json};

const CONTACT_ID: &str = "ct1";

fn seed_contact(backend: &MemoryBackend, id: &str, identity: &str) {
    backend.insert_object_for_test(
        "ChatContact",
        ACCOUNT_ID,
        id,
        json!({
            "id": id,
            "identity": identity,
            "login": identity,
            "firstSeenAt": "2026-01-01T00:00:00Z",
            "lastSeenAt": "2026-01-01T00:00:00Z",
            "blocked": false,
        }),
    );
}

fn with_account(body: Value) -> Value {
    let mut args = json!({ "accountId": ACCOUNT_ID });
    let (Value::Object(args_map), Value::Object(body_map)) = (&mut args, body) else {
        unreachable!("both are object literals");
    };
    args_map.extend(body_map);
    args
}

async fn set<B: ChatBackend<CallerCtx = ()>>(backend: &B, body: Value) -> Result<Value, JmapError> {
    handle_contact_set(backend, &(), with_account(body))
        .await
        .map(|HandlerOutput { primary, .. }| primary)
}

async fn query(body: Value) -> Result<Value, JmapError> {
    handle_contact_query(&account_backend(), &(), with_account(body))
        .await
        .map(|HandlerOutput { primary, .. }| primary)
}

fn assert_error_type(result: Result<Value, JmapError>, expected: &str) {
    let err = result.expect_err(expected);
    assert_eq!(err.error_type.to_string(), expected, "{err:?}");
}

/// RFC 8620 §3.6.2 `invalidArguments`: "One of the arguments is of the
/// wrong type or otherwise invalid, or a required argument is missing."
/// `accountId` is required on every standard method (§5.1).
#[tokio::test]
async fn get_without_account_id_is_invalid_arguments() {
    let result = handle_contact_get(&account_backend(), &(), json!({}))
        .await
        .map(|HandlerOutput { primary, .. }| primary);
    assert_error_type(result, "invalidArguments");
}

/// RFC 8620 §5.1 `notFound`: "This array contains the ids passed to the
/// method for records that do not exist."
#[tokio::test]
async fn get_unknown_id_is_reported_not_found() {
    let backend = account_backend();
    seed_contact(&backend, CONTACT_ID, "alice@example.com");
    let HandlerOutput { primary: resp, .. } = handle_contact_get(
        &backend,
        &(),
        with_account(json!({ "ids": [CONTACT_ID, "missing"] })),
    )
    .await
    .expect("ChatContact/get");
    assert_eq!(resp["notFound"], json!(["missing"]), "{resp:?}");
    assert_eq!(resp["list"][0]["id"], CONTACT_ID, "{resp:?}");
    assert_eq!(resp["list"].as_array().map(Vec::len), Some(1), "{resp:?}");
}

/// RFC 8620 §5.1: `ids` is "Id[]|null"; any other type is
/// `invalidArguments` (§3.6.2).
#[tokio::test]
async fn get_with_non_array_ids_is_invalid_arguments() {
    let result = handle_contact_get(
        &account_backend(),
        &(),
        with_account(json!({ "ids": "ct1" })),
    )
    .await
    .map(|HandlerOutput { primary, .. }| primary);
    assert_error_type(result, "invalidArguments");
}

/// RFC 8620 §3.6.1 `serverFail`: "Some internal server resource was
/// temporarily unavailable." The backend's own error text is not wired.
#[tokio::test]
async fn get_backend_failure_is_server_fail_without_backend_text() {
    let backend = InjectableBackend::new();
    backend.inject("ChatContact", "get");
    let err = handle_contact_get(&backend, &(), with_account(json!({})))
        .await
        .expect_err("injected get failure");
    assert_eq!(err.error_type.to_string(), "serverFail", "{err:?}");
    assert!(
        !format!("{err:?}").contains(INJECTABLE_BACKEND_CANARY),
        "leaked backend text: {err:?}"
    );
}

/// RFC 8620 §5.2: `sinceState` is a required argument of `/changes`.
#[tokio::test]
async fn changes_without_since_state_is_invalid_arguments() {
    let result = handle_contact_changes(&account_backend(), &(), with_account(json!({})))
        .await
        .map(|HandlerOutput { primary, .. }| primary);
    assert_error_type(result, "invalidArguments");
}

/// RFC 8620 §5.3 `ifInState`: "If supplied, the string must match the
/// current state, otherwise the method will be aborted and a
/// `stateMismatch` error returned."
#[tokio::test]
async fn set_with_stale_if_in_state_is_state_mismatch() {
    let result = set(
        &account_backend(),
        json!({ "ifInState": "not-the-state", "destroy": [CONTACT_ID] }),
    )
    .await;
    assert_error_type(result, "stateMismatch");
}

/// RFC 8620 §5.3 `created`: "This is null if no objects were
/// successfully created"; `notCreated`: "or null if none". An empty
/// `create` map creates nothing and changes no state.
#[tokio::test]
async fn set_with_empty_create_map_creates_nothing() {
    let resp = set(&account_backend(), json!({ "create": {} }))
        .await
        .expect("ChatContact/set");
    assert_eq!(resp["created"], Value::Null, "{resp:?}");
    assert_eq!(resp["notCreated"], Value::Null, "{resp:?}");
    assert_eq!(resp["oldState"], resp["newState"], "{resp:?}");
}

/// RFC 8620 §1.2: an Id "MUST only contain characters from the
/// URL and Filename Safe base64 alphabet"; an update key that is not a
/// valid Id fails that update with `invalidProperties` naming `id`.
#[tokio::test]
async fn update_with_malformed_id_is_invalid_properties() {
    let resp = set(
        &account_backend(),
        json!({ "update": { "bad id!": { "blocked": true } } }),
    )
    .await
    .expect("ChatContact/set");
    assert_eq!(resp["updated"], Value::Null, "{resp:?}");
    assert_eq!(
        resp["notUpdated"]["bad id!"]["type"], "invalidProperties",
        "{resp:?}"
    );
    assert_eq!(
        resp["notUpdated"]["bad id!"]["properties"],
        json!(["id"]),
        "{resp:?}"
    );
}

/// RFC 8620 §5.3 `update` is "Id[PatchObject]|null"; `invalidPatch`:
/// "The PatchObject given to update the record was not a valid patch".
/// A patch that is not a JSON object is not a PatchObject.
#[tokio::test]
async fn update_with_non_object_patch_is_invalid_patch() {
    let backend = account_backend();
    seed_contact(&backend, CONTACT_ID, "alice@example.com");
    let resp = set(&backend, json!({ "update": { CONTACT_ID: true } }))
        .await
        .expect("ChatContact/set");
    assert_eq!(resp["updated"], Value::Null, "{resp:?}");
    assert_eq!(
        resp["notUpdated"][CONTACT_ID]["type"], "invalidPatch",
        "{resp:?}"
    );
}

/// Draft §ChatContact/set: "`update` supports: `blocked`,
/// `displayName`." A patch naming neither carries nothing the server can
/// apply, so it fails with `invalidPatch` (RFC 8620 §5.3) and the stored
/// contact is unchanged.
#[tokio::test]
async fn update_with_no_supported_property_is_invalid_patch() {
    let backend = account_backend();
    seed_contact(&backend, CONTACT_ID, "alice@example.com");
    let resp = set(
        &backend,
        json!({ "update": { CONTACT_ID: { "nickname": "A" } } }),
    )
    .await
    .expect("ChatContact/set");
    assert_eq!(resp["updated"], Value::Null, "{resp:?}");
    assert_eq!(
        resp["notUpdated"][CONTACT_ID]["type"], "invalidPatch",
        "{resp:?}"
    );
    assert_eq!(resp["oldState"], resp["newState"], "{resp:?}");
}

/// RFC 8620 §5.3 `destroy` is "Id[]|null"; an element that is not a
/// string is the wrong type, so the method fails with
/// `invalidArguments` (§3.6.2).
#[tokio::test]
async fn destroy_with_non_string_element_is_invalid_arguments() {
    let result = set(&account_backend(), json!({ "destroy": [CONTACT_ID, 7] })).await;
    assert_error_type(result, "invalidArguments");
}

/// RFC 8620 §3.6.1 `serverFail` surfaced per record in `notCreated`
/// (§5.3) when the store fails; the backend's error text is not wired.
#[tokio::test]
async fn create_backend_failure_is_server_fail_without_backend_text() {
    let backend = InjectableBackend::new();
    backend.inject("ChatContact", "create");
    let resp = set(
        &backend,
        json!({ "create": { "c0": { "identity": "bob@example.com" } } }),
    )
    .await
    .expect("ChatContact/set");
    assert_eq!(resp["created"], Value::Null, "{resp:?}");
    assert_eq!(resp["notCreated"]["c0"]["type"], "serverFail", "{resp:?}");
    assert!(
        !resp.to_string().contains(INJECTABLE_BACKEND_CANARY),
        "leaked backend text: {resp:?}"
    );
}

/// RFC 8620 §3.6.1 `serverFail` surfaced per record in `notUpdated`
/// (§5.3) when the store fails; the backend's error text is not wired.
#[tokio::test]
async fn update_backend_failure_is_server_fail_without_backend_text() {
    let backend = InjectableBackend::new();
    seed_contact(&backend.inner, CONTACT_ID, "alice@example.com");
    backend.inject("ChatContact", "update");
    let resp = set(
        &backend,
        json!({ "update": { CONTACT_ID: { "blocked": true } } }),
    )
    .await
    .expect("ChatContact/set");
    assert_eq!(resp["updated"], Value::Null, "{resp:?}");
    assert_eq!(
        resp["notUpdated"][CONTACT_ID]["type"], "serverFail",
        "{resp:?}"
    );
    assert!(
        !resp.to_string().contains(INJECTABLE_BACKEND_CANARY),
        "leaked backend text: {resp:?}"
    );
}

/// RFC 8620 §5.5 `limit`: "UnsignedInt|null"; a negative number is the
/// wrong type (`invalidArguments`, §3.6.2).
#[tokio::test]
async fn query_with_negative_limit_is_invalid_arguments() {
    assert_error_type(query(json!({ "limit": -1 })).await, "invalidArguments");
}

/// RFC 8620 §5.5 `position`: "Int" (default 0); a string is the wrong
/// type (`invalidArguments`, §3.6.2).
#[tokio::test]
async fn query_with_string_position_is_invalid_arguments() {
    assert_error_type(query(json!({ "position": "0" })).await, "invalidArguments");
}

/// RFC 8620 §5.5 `sort`: "Comparator[]|null"; an object is the wrong
/// type (`invalidArguments`, §3.6.2).
#[tokio::test]
async fn query_with_non_array_sort_is_invalid_arguments() {
    assert_error_type(
        query(json!({ "sort": { "property": "login" } })).await,
        "invalidArguments",
    );
}

/// RFC 8620 §5.5 `total`: "The total number of Foos in the results
/// (given the filter). This argument MUST be omitted if the
/// `calculateTotal` request argument is not true".
#[tokio::test]
async fn query_total_present_only_when_requested() {
    let backend = account_backend();
    seed_contact(&backend, "ct1", "alice@example.com");
    seed_contact(&backend, "ct2", "bob@example.com");

    let HandlerOutput { primary: resp, .. } = handle_contact_query(
        &backend,
        &(),
        with_account(json!({ "calculateTotal": true, "limit": 1 })),
    )
    .await
    .expect("ChatContact/query");
    assert_eq!(resp["total"], json!(2), "{resp:?}");
    assert_eq!(resp["ids"].as_array().map(Vec::len), Some(1), "{resp:?}");

    let HandlerOutput { primary: resp, .. } =
        handle_contact_query(&backend, &(), with_account(json!({})))
            .await
            .expect("ChatContact/query");
    assert!(resp.get("total").is_none(), "{resp:?}");
}
