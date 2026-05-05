//! Shared test helpers for jmap-sharing-client integration tests.
//!
//! Provides mock-server–backed session and client factories used by all
//! wiremock test files.

use wiremock::MockServer;

/// Build a [`jmap_base_client::Session`] whose `apiUrl` points at the mock server.
///
/// Account: `u33084183` / `john@example.com`, primary for
/// `urn:ietf:params:jmap:principals` and `urn:ietf:params:jmap:principals:owner`.
///
/// Oracle: RFC 8620 §2.1 example session JSON shape.
pub fn make_session(server: &MockServer) -> jmap_base_client::Session {
    let json = serde_json::json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:principals": {},
            "urn:ietf:params:jmap:principals:owner": {}
        },
        "accounts": {
            "u33084183": {
                "name": "john@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:principals": {},
                    "urn:ietf:params:jmap:principals:owner": {}
                }
            }
        },
        "primaryAccounts": {
            "urn:ietf:params:jmap:principals": "u33084183"
        },
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

/// Build a [`jmap_sharing_client::SessionClient`] pointed at the mock server.
///
/// Uses `DefaultTransport` (standard TLS) and `NoneAuth` (no credentials) — appropriate
/// for wiremock test servers which do not verify auth headers.
///
/// The extension trait method is [`JmapSharingExt::with_sharing_session`].
pub async fn make_client(server: &MockServer) -> jmap_sharing_client::SessionClient {
    use jmap_sharing_client::JmapSharingExt;
    let client = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("make_client: JmapClient construction must succeed");
    client.with_sharing_session(make_session(server))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Confirms that make_session deserializes correctly and the primary account
    /// id matches the RFC 8620 §2.1 session shape.
    ///
    /// Oracle: RFC 8620 §2.1 — primaryAccounts field maps capability URI to account id.
    #[tokio::test]
    async fn build_session_has_correct_primary_account() {
        let server = MockServer::start().await;
        let session = make_session(&server);
        assert_eq!(
            session.primary_account_id("urn:ietf:params:jmap:principals"),
            Some("u33084183"),
            "primary account must be u33084183"
        );
    }

    /// Confirms that make_client builds successfully — construction must succeed
    /// regardless of whether the mock server handles any requests.
    ///
    /// Oracle: RFC 8620 §2.1 session shape — apiUrl, accounts, primaryAccounts fields.
    #[tokio::test]
    async fn helpers_compile() {
        let server = MockServer::start().await;
        let sc = make_client(&server).await;
        // SessionClient is opaque — confirming construction succeeds
        let _ = sc;
    }
}
