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
//
// `#[allow(dead_code)]` is required because `cargo` compiles each
// integration-test file in `tests/` as a separate binary, and each binary
// independently checks for unused items in `mod common;`. Binaries that
// don't call `make_client` (notably `helpers_smoke.rs`) would otherwise
// trigger `dead_code`. This is a documented cargo limitation; see
// https://doc.rust-lang.org/cargo/reference/cargo-targets.html#integration-tests
#[allow(dead_code)]
pub fn make_client(server: &MockServer) -> jmap_sharing_client::SessionClient {
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


