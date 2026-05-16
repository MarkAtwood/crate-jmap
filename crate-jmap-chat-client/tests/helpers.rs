//! Shared test helpers for jmap-chat-client integration tests.
//!
//! Provides mock-server–backed session and client factories used by all
//! wiremock test files, plus envelope/response builders and request-body
//! extraction helpers that collapse the 26-times-duplicated boilerplate
//! flagged in bd:JMAP-26di.11.
//!
//! This module is included into each integration-test binary via
//! `#[path = "helpers.rs"] mod helpers;`. Each binary compiles
//! independently and sees only the items its own test bodies reference,
//! so unused-item warnings here are noise: the module-level
//! `#[allow(dead_code)]` below suppresses them.

#![allow(dead_code)]

use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Canonical account id used by every smoke test. Matches the
/// `make_session` accounts map below.
pub const TEST_ACCOUNT_ID: &str = "A13824";

/// Canonical method-call id used in every smoke test's request envelope
/// — chosen by `JmapRequest` builders and echoed back in
/// `methodResponses[0][2]`.
pub const TEST_CALL_ID: &str = "r1";

/// Canonical Session top-level state token returned by `make_session`
/// and echoed in every `methodResponses` envelope.
pub const TEST_SESSION_STATE: &str = "s1";

/// Chat state-token pair used in every `Chat/set` stock response.
pub const CHAT_STATE_OLD: &str = "c-1";
pub const CHAT_STATE_NEW: &str = "c-2";

/// Space state-token pair used in every `Space/set` stock response.
pub const SPACE_STATE_OLD: &str = "sp-1";
pub const SPACE_STATE_NEW: &str = "sp-2";

/// Message state-token pair used in every `Message/set` stock response.
pub const MESSAGE_STATE_OLD: &str = "ms-1";
pub const MESSAGE_STATE_NEW: &str = "ms-2";

/// Build a [`jmap_base_client::Session`] whose `apiUrl` points at the mock server.
///
/// Account: [`TEST_ACCOUNT_ID`] / `john@example.com`, primary for
/// `urn:ietf:params:jmap:chat`.
///
/// Oracle: RFC 8620 §2.1 example session JSON shape;
/// draft-atwood-jmap-chat-00 §3 (chat capability URI).
pub fn make_session(server: &MockServer) -> jmap_base_client::Session {
    let json = json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:chat": {}
        },
        "accounts": {
            TEST_ACCOUNT_ID: {
                "name": "john@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": { "urn:ietf:params:jmap:chat": {} }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:chat": TEST_ACCOUNT_ID },
        "username": "john@example.com",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}", server.uri()),
        "uploadUrl": format!("{}/ul/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/sse/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}", server.uri()),
        "state": TEST_SESSION_STATE
    });
    serde_json::from_value(json)
        .expect("make_session: session must deserialize from RFC 8620 §2.1 shape")
}

/// Build a [`jmap_chat_client::SessionClient`] pointed at the mock server.
///
/// Uses `DefaultTransport` (standard TLS) and `NoneAuth` (no credentials) — appropriate
/// for wiremock test servers which do not verify auth headers.
pub fn make_client(server: &MockServer) -> jmap_chat_client::SessionClient {
    use jmap_chat_client::JmapChatExt;
    let client = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("make_client: JmapClient construction must succeed");
    client.with_chat_session(make_session(server))
}

/// Mount a `POST /api/` mock on `server` returning the JMAP response
/// `resp_body` with HTTP 200. Replaces the 31-occurrence
/// `Mock::given(...).respond_with(...).mount(...)` block.
pub async fn mock_jmap_post(server: &MockServer, resp_body: Value) {
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(resp_body))
        .mount(server)
        .await;
}

/// Fetch the first recorded request body parsed as JSON. Panics with a
/// clear message if no request was recorded or the body is not valid
/// JSON. Use in tests that need the full envelope (e.g. to assert
/// `using[]`).
pub async fn recorded_body(server: &MockServer) -> Value {
    let reqs = server
        .received_requests()
        .await
        .expect("recorded_body: received_requests must succeed");
    assert!(
        !reqs.is_empty(),
        "recorded_body: expected at least one POST /api/ request"
    );
    serde_json::from_slice(&reqs[0].body).expect("recorded_body: body must be valid JSON")
}

/// Fetch `body["methodCalls"][0][1]` from the first recorded request.
/// Replaces the 33-occurrence pattern of `received_requests` →
/// `from_slice` → `body["methodCalls"][0][1]`.
pub async fn recorded_args(server: &MockServer) -> Value {
    let body = recorded_body(server).await;
    body["methodCalls"][0][1].clone()
}

/// Build a standard single-method JMAP response envelope:
///
/// ```json
/// {
///   "sessionState": "s1",
///   "methodResponses": [[method_name, args, "r1"]]
/// }
/// ```
///
/// Matches the [`TEST_SESSION_STATE`] / [`TEST_CALL_ID`] constants.
pub fn jmap_response(method_name: &str, args: Value) -> Value {
    json!({
        "sessionState": TEST_SESSION_STATE,
        "methodResponses": [[
            method_name,
            args,
            TEST_CALL_ID,
        ]]
    })
}

/// Build a `/set` response body with stock null fields, overlaid with
/// the caller-supplied entries from `fields`. The base shape is:
///
/// ```json
/// {
///   "accountId": "A13824",
///   "oldState": <old_state>,
///   "newState": <new_state>,
///   "created": null,
///   "updated": null,
///   "destroyed": null,
///   "notCreated": null,
///   "notUpdated": null,
///   "notDestroyed": null
/// }
/// ```
///
/// Any key in `fields` overrides the null default for that key. Returns
/// the full top-level envelope ready to feed to [`mock_jmap_post`].
///
/// Oracle: RFC 8620 §5.3 `/set` response shape (all six result keys
/// are nullable arrays/maps; canonical successful destroy/update bodies
/// in extension-server reference impls keep the irrelevant ones null).
pub fn set_response(method_name: &str, old_state: &str, new_state: &str, fields: Value) -> Value {
    let mut args = json!({
        "accountId": TEST_ACCOUNT_ID,
        "oldState": old_state,
        "newState": new_state,
        "created": null,
        "updated": null,
        "destroyed": null,
        "notCreated": null,
        "notUpdated": null,
        "notDestroyed": null,
    });
    if let Value::Object(overlay) = fields {
        let args_map = args.as_object_mut().expect("set_response: args is object");
        for (k, v) in overlay {
            args_map.insert(k, v);
        }
    }
    jmap_response(method_name, args)
}

/// Stock `/set` response asserting one id was destroyed; all other
/// result keys are null. Replaces the 3 near-verbatim destroy bodies
/// flagged in bd:JMAP-26di.11 (chat/space/message smoke tests).
pub fn set_destroy_response(
    method_name: &str,
    old_state: &str,
    new_state: &str,
    destroyed_id: &str,
) -> Value {
    set_response(
        method_name,
        old_state,
        new_state,
        json!({ "destroyed": [destroyed_id] }),
    )
}

/// Stock `/set` response asserting one id was updated with `null` patch
/// echo (the canonical RFC 8620 §5.3 "server has nothing to add" form).
/// Replaces the 5 duplicated update-success bodies.
pub fn set_update_response(
    method_name: &str,
    old_state: &str,
    new_state: &str,
    updated_id: &str,
) -> Value {
    set_response(
        method_name,
        old_state,
        new_state,
        json!({ "updated": { updated_id: null } }),
    )
}
