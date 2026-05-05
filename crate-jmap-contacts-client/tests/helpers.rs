//! Shared test helpers for jmap-contacts-client integration tests.
//!
//! Provides mock-server–backed session and client factories used by all
//! wiremock test files.

use wiremock::MockServer;

/// Build a [`jmap_base_client::Session`] whose `apiUrl` points at the mock server.
///
/// Account: `A13824` / `john@example.com`, primary for `urn:ietf:params:jmap:contacts`.
///
/// Oracle: RFC 8620 §2.1 example session JSON shape.
pub fn make_session(server: &MockServer) -> jmap_base_client::Session {
    let json = serde_json::json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:contacts": {}
        },
        "accounts": {
            "A13824": {
                "name": "john@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": { "urn:ietf:params:jmap:contacts": {} }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:contacts": "A13824" },
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

/// Build a [`jmap_contacts_client::SessionClient`] pointed at the mock server.
///
/// Uses `DefaultTransport` (standard TLS) and `NoneAuth` (no credentials) — appropriate
/// for wiremock test servers which do not verify auth headers.
pub async fn make_client(server: &MockServer) -> jmap_contacts_client::SessionClient {
    use jmap_contacts_client::JmapContactsExt;
    let client = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("make_client: JmapClient construction must succeed");
    client.with_contacts_session(make_session(server))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Confirms that make_session and make_client build successfully and the session
    /// deserializes from the RFC 8620 §2.1 shape.
    ///
    /// Oracle: RFC 8620 §2.1 session JSON shape — apiUrl, accounts, primaryAccounts fields.
    #[tokio::test]
    async fn helpers_compile() {
        let server = MockServer::start().await;
        let sc = make_client(&server).await;
        let _ = sc;
    }
}
