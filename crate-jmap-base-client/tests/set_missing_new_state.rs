//! `extract_response` logs a `Foo/set` response that has no `newState`
//! (docs/design/DECISIONS.md decision 67, bd:JMAP-cyub.5).
//!
//! RFC 8620 §5.3 requires `newState`; Stalwart 1.1.1 leaves it out of a
//! `/set` that changes nothing. [`jmap_types::SetResponse`] decodes such a
//! response with an empty `new_state`, and `extract_response` emits one
//! `WARN` event naming the method, the account and the missing key. These
//! tests pin that event, its absence when `newState` is present, and that
//! the response's objects never reach the log.
//!
//! [`LogCapture`] is thread-local, so every test here is synchronous:
//! `extract_response` runs on the test thread.

mod common;

use common::log_capture::LogCapture;
use jmap_base_client::{ClientError, extract_response};
use jmap_types::{JmapResponse, PartialObject, SetResponse};
use serde_json::json;

/// The stable message of the event.
const MESSAGE: &str = "set response has no newState";

/// A literal placed in the response's object contents; it must never be
/// logged.
const CANARY: &str = "CANARY-SET-OBJECT-CONTENT-4f1d";

/// A one-invocation response `[name, args, "c1"]`.
fn response(name: &str, args: serde_json::Value) -> JmapResponse {
    serde_json::from_value(json!({
        "sessionState": "s0",
        "methodResponses": [[name, args, "c1"]]
    }))
    .expect("fixture is a JMAP response")
}

/// Harness self-test: the capture sees a `WARN` event, so the negative
/// assertions below cannot pass vacuously.
#[test]
fn log_capture_sees_warn_events() {
    let capture = LogCapture::new();
    tracing::warn!(probe = "x", "harness probe");
    capture.assert_contains("WARN");
    capture.assert_contains("harness probe");
}

#[test]
fn set_response_without_new_state_decodes_and_warns() {
    let capture = LogCapture::new();
    let resp = response(
        "Identity/set",
        json!({
            "accountId": "A13824",
            "updated": { "I1": { "name": CANARY } },
            "notDestroyed": { "I9": { "type": "notFound", "description": CANARY } }
        }),
    );
    let set: SetResponse<PartialObject> = extract_response(&resp, "c1").expect("decodes");
    assert_eq!(set.new_state, "");
    assert_eq!(set.old_state, None);
    assert_eq!(set.updated.len(), 1);
    assert_eq!(set.not_destroyed.len(), 1);

    let out = capture.contents();
    assert_eq!(out.lines().count(), 1, "exactly one event: {out}");
    let line = out.lines().next().unwrap();
    assert!(line.contains("WARN"), "level: {line}");
    assert!(line.contains(MESSAGE), "message: {line}");
    assert!(line.contains("method=\"Identity/set\""), "method: {line}");
    assert!(line.contains("account_id=\"A13824\""), "account_id: {line}");
    assert!(line.contains("missing=\"newState\""), "missing: {line}");
    capture.assert_does_not_contain(CANARY);
}

/// A response that also lacks `accountId` still decodes as far as the
/// log is concerned: the event records an empty `account_id`. (It does
/// not decode as `SetResponse`, which requires `accountId`, so `T` is
/// `serde_json::Value` here.)
#[test]
fn missing_account_id_is_logged_empty() {
    let capture = LogCapture::new();
    let resp = response("Mailbox/set", json!({ "destroyed": ["M1"] }));
    let _: serde_json::Value = extract_response(&resp, "c1").expect("decodes");
    capture.assert_contains(MESSAGE);
    capture.assert_contains("account_id=\"\"");
    capture.assert_contains("method=\"Mailbox/set\"");
}

#[test]
fn set_response_with_new_state_does_not_warn() {
    let capture = LogCapture::new();
    // No oldState either: Stalwart's EmailSubmission/set create. Only a
    // missing newState is logged.
    let resp = response(
        "EmailSubmission/set",
        json!({ "accountId": "A1", "newState": "s2", "destroyed": ["ES1"] }),
    );
    let set: SetResponse<PartialObject> = extract_response(&resp, "c1").expect("decodes");
    assert_eq!(set.new_state, "s2");
    assert_eq!(capture.contents(), "", "no event expected");
}

/// RFC 8620 §7.2.2: a `PushSubscription/set` response has no `newState`
/// by definition, so it is not logged.
#[test]
fn push_subscription_set_does_not_warn() {
    let capture = LogCapture::new();
    let resp = response(
        "PushSubscription/set",
        json!({ "created": { "k1": { "id": "P1" } } }),
    );
    let _: jmap_base_client::PushSubscriptionSetResponse =
        extract_response(&resp, "c1").expect("decodes");
    assert_eq!(capture.contents(), "", "no event expected");
}

/// Only `/set` responses are checked: a `/get` has no `newState`.
#[test]
fn non_set_response_does_not_warn() {
    let capture = LogCapture::new();
    let resp = response(
        "Mailbox/get",
        json!({ "accountId": "A1", "state": "s1", "list": [], "notFound": [] }),
    );
    let _: serde_json::Value = extract_response(&resp, "c1").expect("decodes");
    assert_eq!(capture.contents(), "", "no event expected");
}

/// A response that fails to decode is reported by the error alone, not
/// also by the warning.
#[test]
fn undecodable_set_response_does_not_warn() {
    let capture = LogCapture::new();
    let resp = response("Mailbox/set", json!({ "destroyed": ["M1"] }));
    let err =
        extract_response::<SetResponse<PartialObject>>(&resp, "c1").expect_err("no accountId");
    assert!(matches!(err, ClientError::Parse(_)), "{err:?}");
    assert_eq!(capture.contents(), "", "no event expected");
}
