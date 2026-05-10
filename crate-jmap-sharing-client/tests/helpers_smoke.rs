//! Smoke tests for common test helpers (make_session, make_client).
//!
//! Verifies that the shared test infrastructure builds and `make_session`
//! deserializes from the RFC 8620 §2.1 shape. (Construction of
//! `make_client` is exercised by every other integration-test binary in
//! this crate via `common::make_client(&server)` — no dedicated smoke
//! test for it here, per JMAP-231o.11 reasoning.)

mod common;

use wiremock::MockServer;

/// Confirms that `make_session` deserializes correctly and the primary
/// account id matches the RFC 8620 §2.1 session shape.
///
/// Oracle: RFC 8620 §2.1 — `primaryAccounts` field maps capability URI to
/// account id. Hosted in this dedicated smoke-test binary instead of an
/// inline `#[cfg(test)] mod tests` block in `common/mod.rs` so the test
/// runs once instead of once per consuming test binary.
#[tokio::test]
async fn build_session_has_correct_primary_account() {
    let server = MockServer::start().await;
    let session = common::make_session(&server);
    assert_eq!(
        session.primary_account_id("urn:ietf:params:jmap:principals"),
        Some("u33084183"),
        "primary account must be u33084183"
    );
}
