//! Debug-redaction canary for `SessionClient` (JMAP-4nwq).
//!
//! `SessionClient` wraps a `JmapClient` that holds the caller's auth
//! credentials. Its manual `Debug` impl must never render them, so a future
//! `#[derive(Debug)]` on either type cannot silently leak a bearer token
//! into logs (workspace AGENTS.md, Security testing pattern 1).

use jmap_vtc_client::JmapVtcExt;

/// Oracle: the canary is a self-defined literal under the test's control,
/// and the session is hand-written RFC 8620 §2.1 JSON; neither is derived
/// from the code under test.
#[test]
fn session_client_debug_does_not_leak_bearer_token() {
    const CANARY: &str = "CANARY-LEAK-TOKEN-DO-NOT-LOG-4nwq";
    let auth = jmap_base_client::BearerAuth::new(CANARY).expect("canary is a valid bearer token");
    let client = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        auth,
        "https://jmap.example.com",
        jmap_base_client::ClientConfig::default(),
    )
    .expect("JmapClient construction must succeed");
    let session: jmap_base_client::Session = serde_json::from_value(serde_json::json!({
        "capabilities": { "urn:ietf:params:jmap:core": {} },
        "accounts": {},
        "primaryAccounts": {},
        "username": "john@example.com",
        "apiUrl": "https://jmap.example.com/api/",
        "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
        "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
        "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
        "state": "s1"
    }))
    .expect("RFC 8620 §2.1 session must deserialize");

    let dbg = format!("{:?}", client.with_vtc_session(session));
    assert!(
        !dbg.contains(CANARY),
        "SessionClient Debug must not contain the bearer token; got: {dbg}"
    );
}
