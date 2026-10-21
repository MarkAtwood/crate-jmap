//! Shared test helpers for jmap-chat-mimi-client integration tests.

use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The account-level `urn:ietf:params:jmap:chat:mimi` object, with the
/// fields of draft-atwood-jmap-chat-mimi-00 §Account-Level Capability
/// Object.
pub fn mimi_account_capability() -> serde_json::Value {
    serde_json::json!({
        "userUri": "mimi://a.example/u/alice",
        "roomIdPrefix": "mimi://a.example/r/",
        "hubSender": "IAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f",
        "cipherSuites": [1],
        "maxKeyPackages": 100
    })
}

/// A [`jmap_base_client::Session`] whose `apiUrl` points at the mock
/// server, with account `u1` (the account of the draft's §Examples)
/// primary for `urn:ietf:params:jmap:chat:mimi` when `mimi` is set.
///
/// Oracle: RFC 8620 §2.1 Session shape.
pub fn make_session(server: &MockServer, mimi: bool) -> jmap_base_client::Session {
    let mut primary = serde_json::json!({ "urn:ietf:params:jmap:chat": "u1" });
    if mimi {
        primary["urn:ietf:params:jmap:chat:mimi"] = "u1".into();
    }
    let json = serde_json::json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:chat": {},
            "urn:ietf:params:jmap:chat:mimi": {}
        },
        "accounts": {
            "u1": {
                "name": "alice@a.example",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:chat": {},
                    "urn:ietf:params:jmap:chat:mimi": mimi_account_capability()
                }
            }
        },
        "primaryAccounts": primary,
        "username": "alice@a.example",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}", server.uri()),
        "uploadUrl": format!("{}/ul/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/sse/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}", server.uri()),
        "state": "s1"
    });
    serde_json::from_value(json).expect("session must deserialize from RFC 8620 §2.1 shape")
}

/// A [`jmap_chat_mimi_client::SessionClient`] pointed at the mock server.
pub fn make_client(server: &MockServer, mimi: bool) -> jmap_chat_mimi_client::SessionClient {
    use jmap_chat_mimi_client::JmapMimiExt;
    let client = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("JmapClient construction must succeed");
    client.with_mimi_session(make_session(server, mimi))
}

/// Start a mock server answering every POST with `name` and `response`.
pub async fn serve(name: &str, response: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sessionState": "s1",
            "methodResponses": [[name, response, "r1"]]
        })))
        .mount(&server)
        .await;
    server
}

/// The one request the server received: its `using` and its single
/// method call `[name, args, callId]`.
pub async fn sent(server: &MockServer) -> (Value, Value) {
    let requests = server.received_requests().await.expect("recording on");
    assert_eq!(requests.len(), 1, "exactly one request");
    let body: Value = serde_json::from_slice(&requests[0].body).expect("JSON body");
    let calls = body["methodCalls"].as_array().expect("methodCalls");
    assert_eq!(calls.len(), 1, "exactly one method call");
    (body["using"].clone(), calls[0].clone())
}

/// Oracle: draft L149: "This capability is meaningful only when
/// `urn:ietf:params:jmap:chat` is also present."
pub fn assert_using(using: &Value) {
    let using = using.as_array().expect("using array");
    for uri in [
        "urn:ietf:params:jmap:core",
        "urn:ietf:params:jmap:chat",
        "urn:ietf:params:jmap:chat:mimi",
    ] {
        assert!(using.contains(&json!(uri)), "using must include {uri}");
    }
}
