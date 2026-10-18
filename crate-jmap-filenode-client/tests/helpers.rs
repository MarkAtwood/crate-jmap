//! Shared test helpers for jmap-filenode-client integration tests.
//!
//! Provides mock-server–backed session and client factories used by all
//! wiremock test files.

use wiremock::MockServer;

/// Build a [`jmap_base_client::Session`] whose `apiUrl` points at the mock server.
///
/// Account: `A13824` / `john@example.com`, primary for `urn:ietf:params:jmap:filenode`.
///
/// Oracle: RFC 8620 §2.1 example session JSON shape.
pub fn make_session(server: &MockServer) -> jmap_base_client::Session {
    let json = serde_json::json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:filenode": {}
        },
        "accounts": {
            "A13824": {
                "name": "john@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    // draft-ietf-jmap-filenode-16 §2.1.1 example value.
                    "urn:ietf:params:jmap:filenode": {
                        "maxFileNodeDepth": 50,
                        "maxSizeFileNodeName": 255,
                        "fileNodeQuerySortOptions": [
                            "name", "mediaType", "size", "created", "modified",
                            "nodeType", "tree"
                        ],
                        "supportedNodeTypes": ["Directory", "File", "SymLink"],
                        "forbiddenNameChars": "/<>:\"\\|?*",
                        "forbiddenNodeNames": [".", "..", "CON", "PRN", "AUX",
                            "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5",
                            "COM6", "COM7", "COM8", "COM9", "LPT0", "LPT1", "LPT2",
                            "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"
                        ],
                        "caseInsensitiveNames": false,
                        "mayCreateTopLevelFileNode": false,
                        "webTrashUrl": "https://files.example.com/trash",
                        "webUrlTemplate": "https://files.example.com/view/{id}",
                        "webWriteUrlTemplate": "https://files.example.com/write/{id}"
                    }
                }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:filenode": "A13824" },
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

/// Build a [`jmap_filenode_client::SessionClient`] pointed at the mock server.
///
/// Uses `DefaultTransport` (standard TLS) and `NoneAuth` (no credentials) — appropriate
/// for wiremock test servers which do not verify auth headers.
pub fn make_client(server: &MockServer) -> jmap_filenode_client::SessionClient {
    use jmap_filenode_client::JmapFilenodeExt;
    let client = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("make_client: JmapClient construction must succeed");
    client.with_filenode_session(make_session(server))
}
