//! Shared test helpers for jmap-chat-mimi-client integration tests.

use wiremock::MockServer;

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
