//! Shared test helpers for jmap-vtc-client integration tests.
//!
//! Provides a mock-server–backed session and client factory, a responder
//! for one method response, and an accessor for the invocation the client
//! sent.

#![allow(dead_code)]

use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Build a [`jmap_base_client::Session`] whose `apiUrl` points at the mock
/// server and whose primary `urn:ietf:params:jmap:vtc` account is
/// `account_id`.
///
/// Oracle: RFC 8620 §2.1 example session JSON shape; draft-atwood-jmap-vtc-00
/// L192 "The value of `capabilities["urn:ietf:params:jmap:vtc"]` at the
/// session level is an empty JSON object `{}`."
pub fn make_session(server: &MockServer, account_id: &str) -> jmap_base_client::Session {
    serde_json::from_value(session_json(server, account_id))
        .expect("make_session: RFC 8620 §2.1 session must deserialize")
}

/// The session JSON [`make_session`] decodes.
fn session_json(server: &MockServer, account_id: &str) -> Value {
    json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:vtc": {}
        },
        "accounts": {
            account_id: {
                "name": "bob@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": { "urn:ietf:params:jmap:vtc": {} }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:vtc": account_id },
        "username": "bob@example.com",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}", server.uri()),
        "uploadUrl": format!("{}/ul/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/sse/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}", server.uri()),
        "state": "s1"
    })
}

/// Build a [`jmap_vtc_client::SessionClient`] for `account_id` pointed at
/// the mock server.
pub fn make_client(server: &MockServer, account_id: &str) -> jmap_vtc_client::SessionClient {
    use jmap_vtc_client::JmapVtcExt;
    let client = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("make_client: JmapClient construction must succeed");
    client.with_vtc_session(make_session(server, account_id))
}

/// Build a [`jmap_vtc_client::SessionClient`] whose primary VTC account is
/// `personal` and which targets `hosting` through
/// [`jmap_vtc_client::SessionClient::for_account`].
///
/// Oracle: draft-atwood-jmap-vtc-00 L1652 "Other users reach these records
/// by accessing the hosting account as a shared account ({{RFC8620}}
/// Section 1.6.2): the account appears in the user's Session with
/// `isPersonal` set to `false`."
pub fn make_shared_client(
    server: &MockServer,
    personal: &str,
    hosting: &str,
) -> jmap_vtc_client::SessionClient {
    use jmap_vtc_client::JmapVtcExt;
    let mut session = session_json(server, personal);
    session["accounts"][hosting] = json!({
        "name": "shared calls",
        "isPersonal": false,
        "isReadOnly": false,
        "accountCapabilities": { "urn:ietf:params:jmap:vtc": {} }
    });
    let session: jmap_base_client::Session = serde_json::from_value(session)
        .expect("make_shared_client: RFC 8620 §2.1 session must deserialize");
    let client = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("make_shared_client: JmapClient construction must succeed");
    client
        .with_vtc_session(session)
        .for_account(hosting)
        .expect("make_shared_client: hosting account is a VTC account")
}

/// Answer every POST to the API URL with one `[name, args, "r1"]`
/// response invocation (RFC 8620 §3.4).
pub async fn respond(server: &MockServer, name: &str, args: Value) {
    let body = json!({
        "sessionState": "s1",
        "methodResponses": [[name, args, "r1"]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(server)
        .await;
}

/// The request body of the single request the client sent.
pub async fn sent_request(server: &MockServer) -> Value {
    let reqs = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(reqs.len(), 1, "exactly one request must be sent");
    serde_json::from_slice(&reqs[0].body).expect("request body must be JSON")
}

/// The `[name, arguments]` of the single invocation the client sent,
/// without the call id (the draft's examples use their own call ids).
pub async fn sent_invocation(server: &MockServer) -> Value {
    let body = sent_request(server).await;
    let calls = body["methodCalls"]
        .as_array()
        .expect("methodCalls must be an array");
    assert_eq!(calls.len(), 1, "exactly one method call must be sent");
    json!([calls[0][0], calls[0][1]])
}

/// A minimal RFC 8620 §5.3 `/set` response carrying `extra` fields.
pub fn set_response(account_id: &str, extra: Value) -> Value {
    let mut v = json!({
        "accountId": account_id,
        "oldState": "s1",
        "newState": "s2"
    });
    for (k, val) in extra.as_object().expect("extra must be an object") {
        v[k] = val.clone();
    }
    v
}
