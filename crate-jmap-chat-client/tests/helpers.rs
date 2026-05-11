//! Shared test helpers for jmap-chat-client integration tests.
//!
//! Provides mock-server–backed session and client factories used by all
//! wiremock test files.

use wiremock::MockServer;

/// Build a [`jmap_base_client::Session`] whose `apiUrl` points at the mock server.
///
/// Account: `A13824` / `john@example.com`, primary for `urn:ietf:params:jmap:chat`.
///
/// Oracle: RFC 8620 §2.1 example session JSON shape;
/// draft-atwood-jmap-chat-00 §3 (chat capability URI).
pub fn make_session(server: &MockServer) -> jmap_base_client::Session {
    let json = serde_json::json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:chat": {}
        },
        "accounts": {
            "A13824": {
                "name": "john@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": { "urn:ietf:params:jmap:chat": {} }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:chat": "A13824" },
        "username": "john@example.com",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}", server.uri()),
        "uploadUrl": format!("{}/ul/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/sse/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}", server.uri()),
        "state": "s1"
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
