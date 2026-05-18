//! Integration tests using the [`common::log_capture::LogCapture`]
//! harness to assert that no credential-grade canary literal ever
//! appears in `tracing::*` output for code paths that touch
//! [`BearerAuth`] or [`BasicAuth`].
//!
//! Tracks bd:JMAP-sc1b.102.
//!
//! ## Why this exists when no `tracing::*` call sites live in
//! `crate-jmap-*/src/` today
//!
//! Today the workspace has zero `tracing::*` instrumentation. A naive
//! reading of these tests is "trivially passing because no logging
//! ever happens". That is not the test:
//!
//! - [`log_capture_captures_traced_output`] is the harness self-test.
//!   It proves the [`LogCapture`] mechanism actually captures what
//!   `tracing` emits before any negative assertion relies on it.
//!
//! - [`auth_token_never_appears_in_tracing_capture`] forces the
//!   logging paths the bead `description` warns about — the test
//!   itself calls `tracing::info!(auth = ?auth, ...)` and
//!   `tracing::debug!("{auth:?}")`, the exact shapes a future
//!   contributor adding instrumentation is likely to write. The
//!   assertion verifies that the type-level Debug-redaction landed in
//!   bd:JMAP-sc1b.79 still defends when those shapes are added — i.e.
//!   the canary literal does not leak through `?`-formatted args.
//!
//! ## Do not simplify the four-test structure (bd:JMAP-6r7c.15)
//!
//! This file deliberately ships **four** tests: a harness self-test
//! (`log_capture_captures_traced_output`), a negative control
//! (`log_capture_detects_a_leak_when_one_exists`), and two canaries
//! (`auth_token_never_appears_in_tracing_capture`,
//! `basic_auth_credentials_never_appear_in_tracing_capture`). A future
//! contributor may look at this file and suggest "the first two tests
//! are scaffolding — delete them, keep only the actual canaries." This
//! is the wrong simplification.
//!
//! Without the harness self-test, a future regression in
//! [`LogCapture`] (subscriber-install failure, `MakeWriter` mis-routing,
//! `tracing_subscriber` feature-set drift) silently makes the canary
//! tests pass *vacuously* — no captured output means the canary needle
//! is not found means `assert_does_not_contain` passes for the wrong
//! reason.
//!
//! Without the negative control, a future regression in the
//! negative-assertion plumbing itself (e.g. a refactor that filters
//! output before [`LogCapture::contents`] returns it, or a clippy-
//! placating change that breaks the `?`-formatted Debug path) silently
//! makes the canaries pass without actually defending anything.
//!
//! The canary tests themselves carry a per-test proof-of-capture
//! (`assert_contains("constructed auth provider")` and
//! `assert_contains("[REDACTED]")`) so each canary independently
//! verifies the subscriber installed and the `?`-formatted args
//! reached the buffer. This is the workspace AGENTS.md "Security
//! testing" Pattern 2 executed correctly: harness self-test +
//! negative control + per-test proof-of-capture + negative-assertion
//! canary. All four are load-bearing. Removing any of them leaves the
//! remaining tests defending nothing.

mod common;

use base64::prelude::*;
use common::log_capture::LogCapture;
use jmap_base_client::auth::{BasicAuth, BearerAuth};
use jmap_base_client::{AccountName, Username};

/// Sanity check: the [`LogCapture`] harness actually captures the
/// output of `tracing::*` calls emitted while it is installed.
///
/// Without this self-test, the negative-assertion tests below would be
/// vacuously true if [`LogCapture::new`] silently failed to install
/// the subscriber.
#[test]
fn log_capture_captures_traced_output() {
    let capture = LogCapture::new();
    tracing::info!("sentinel-message-12345 emitted by harness self-test");
    capture.assert_contains("sentinel-message-12345");
}

/// Negative control: prove the harness can actually catch a leak.
///
/// Uses a deliberately non-redacting [`Debug`] impl on a local type to
/// model what a buggy redaction-less type would look like. Verifies
/// that emitting it through `tracing::info!(secret = ?leaky, ...)`
/// causes the canary literal to appear in captured output — and that
/// [`LogCapture::contents`] surfaces it. Without this test the
/// positive `assert_does_not_contain` calls in the other tests could
/// theoretically pass for a reason other than the redaction working
/// (e.g. tracing not being wired up at all).
#[test]
fn log_capture_detects_a_leak_when_one_exists() {
    // The inner `&str` is only read through the derived `Debug` impl —
    // clippy excludes Debug from dead-code analysis, so without
    // `#[expect(dead_code)]` the field would be flagged.
    #[derive(Debug)]
    struct Leaky(#[expect(dead_code, reason = "consumed only via Debug formatting")] &'static str);

    const CANARY: &str = "CANARY-NEGATIVE-CONTROL-LEAK-789";
    let capture = LogCapture::new();
    let leaky = Leaky(CANARY);

    tracing::info!(secret = ?leaky, "leaky type formatted via Debug");

    let contents = capture.contents();
    assert!(
        contents.contains(CANARY),
        "negative control: expected the canary literal to appear in captured output; \
         if this fires, the harness is not capturing Debug-formatted args and the \
         positive assertions in other tests pass vacuously. Got:\n{contents}"
    );
}

/// Canary: even when a future contributor adds `tracing::*` calls
/// that pass an `AuthProvider` through `?`-formatting, the secret
/// literal that backs the auth provider does not appear in captured
/// log output.
///
/// The oracle is the canary literal `CANARY-LOG-TOKEN-DO-NOT-LEAK-456`
/// — it is under this test's control and is never derived from the
/// `BearerAuth` internal state.
#[test]
fn auth_token_never_appears_in_tracing_capture() {
    const CANARY: &str = "CANARY-LOG-TOKEN-DO-NOT-LEAK-456";
    let capture = LogCapture::new();
    let auth = BearerAuth::new(CANARY).expect("valid ASCII token must construct");

    // Emit the exact `?`-formatted shapes a future contributor adding
    // instrumentation is likely to write.
    tracing::info!(auth = ?auth, "constructed auth provider");
    tracing::debug!("auth provider state: {auth:?}");
    tracing::trace!(?auth, "fine-grained auth trace");
    tracing::warn!("attempting request with {auth:?}");
    tracing::error!("simulated request failure with auth = {auth:?}");

    // Confirm the harness saw at least one of those calls AND that
    // `?auth` actually rendered through `BearerAuth`'s Debug impl, so
    // the negative assertion below cannot pass vacuously.
    capture.assert_contains("constructed auth provider");
    capture.assert_contains("[REDACTED]");

    capture.assert_does_not_contain(CANARY);
}

/// Same canary shape for [`BasicAuth`]: username, password, and the
/// base64-encoded `user:pass` blob must all be absent from captured
/// log output even when the `BasicAuth` value is `?`-formatted.
#[test]
fn basic_auth_credentials_never_appear_in_tracing_capture() {
    const CANARY_USER: &str = "CANARY-LOG-USER-DO-NOT-LEAK";
    const CANARY_PASS: &str = "CANARY-LOG-PASS-DO-NOT-LEAK";
    let capture = LogCapture::new();
    let auth = BasicAuth::new(CANARY_USER, CANARY_PASS).expect("valid credentials must construct");

    tracing::info!(auth = ?auth, "constructed basic auth");
    tracing::debug!("basic auth state: {auth:?}");

    // Same proof-of-capture as the BearerAuth canary: assert both the
    // message string and the redacted Debug form appear so the
    // negative assertions cannot pass vacuously.
    capture.assert_contains("constructed basic auth");
    capture.assert_contains("[REDACTED]");

    capture.assert_does_not_contain(CANARY_USER);
    capture.assert_does_not_contain(CANARY_PASS);
    // Also catch a regression where `header_string` is logged,
    // surfacing the base64-encoded credentials.
    let base64_pair = BASE64_STANDARD.encode(format!("{CANARY_USER}:{CANARY_PASS}"));
    capture.assert_does_not_contain(&base64_pair);
}

/// bd:JMAP-6r7c.63 — [`Username`] is a PII wrapper for `Session.username`.
/// Both `Display` and `Debug` MUST redact the raw value to `[REDACTED]`.
/// A `tracing::info!(user = %session.username, ...)` line — `%` invokes
/// `Display` — and `tracing::debug!("...{user:?}", user = session.username)`
/// — `?` invokes `Debug` — MUST NOT surface the PII canary literal.
#[test]
fn username_never_appears_in_tracing_capture() {
    const CANARY: &str = "CANARY-LOG-USERNAME-DO-NOT-LEAK";
    let capture = LogCapture::new();
    let username = Username::new(CANARY);

    // Exercise both Display (%) and Debug (?) paths, the two
    // formatter shapes a future contributor adding instrumentation is
    // likely to write against a `Session.username` field.
    tracing::info!(user = %username, "constructed username");
    tracing::debug!("username Display path: {username}");
    tracing::trace!(?username, "username Debug path");
    tracing::warn!("username via mixed Debug: {username:?}");

    capture.assert_contains("constructed username");
    capture.assert_contains("[REDACTED]");
    capture.assert_does_not_contain(CANARY);
}

/// bd:JMAP-6r7c.63 — [`AccountName`] mirror of the Username canary.
/// `AccountInfo.name` (also typically PII) must not leak through any
/// `tracing::*` formatter path either.
#[test]
fn account_name_never_appears_in_tracing_capture() {
    const CANARY: &str = "CANARY-LOG-ACCOUNT-NAME-DO-NOT-LEAK";
    let capture = LogCapture::new();
    let account_name = AccountName::new(CANARY);

    tracing::info!(name = %account_name, "constructed account name");
    tracing::debug!("account name Display path: {account_name}");
    tracing::trace!(?account_name, "account name Debug path");
    tracing::warn!("account name via mixed Debug: {account_name:?}");

    capture.assert_contains("constructed account name");
    capture.assert_contains("[REDACTED]");
    capture.assert_does_not_contain(CANARY);
}
