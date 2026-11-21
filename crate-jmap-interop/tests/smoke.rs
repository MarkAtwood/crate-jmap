//! Harness smoke test: the configured server answers a Session for the
//! first account (RFC 8620 §2.2). Run with `-- --ignored`; see
//! docs/interop/README.md.

use jmap_interop::InteropEnv;

/// RFC 8620 §2: the Session resource lists the core capability and
/// names the authenticated user.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn server_answers_session() {
    let env = InteropEnv::require();
    let client = env.client(&env.user).expect("client");
    let session = client.fetch_session().await.expect("Session");
    assert!(
        session
            .capabilities
            .contains_key("urn:ietf:params:jmap:core"),
        "RFC 8620 §2: capabilities MUST include urn:ietf:params:jmap:core"
    );
    assert!(!session.accounts.is_empty(), "the user has an account");
}
