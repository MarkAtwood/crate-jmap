//! RFC 8620 §5.3 `maxObjectsInSet` enforcement across the 6 mail-server
//! /set handlers (bd:JMAP-ayoz.41.2).
//!
//! Every test asserts the wire-format `JmapError::limit("maxObjectsInSet")`
//! shape against a hand-built independent oracle — the helper under test
//! is never the oracle. Oversize batches use 501 entries against the
//! default `JmapBackend::max_objects_in_set` cap of 500.

#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_mail_server::{
    handle_email_set, handle_identity_set, handle_mailbox_set, handle_sieve_set,
    handle_submission_set, handle_vacation_set,
};
use jmap_types::Id;
use serde_json::{json, Value};

// Default cap value returned by `JmapBackend::max_objects_in_set` in the
// foundation crate. Tests below construct batches relative to this value
// rather than hardcoding 500 in multiple places.
const DEFAULT_CAP: usize = 500;

/// Helper: build a `Mailbox/set` create-map of `n` entries, each a
/// minimal mailbox with a unique name. The wire shape is independent
/// of `Mailbox`'s internal struct — the test only cares about the
/// pre-handler cap check, so any `Value::Object` shape suffices.
fn create_map_of_size(n: usize) -> Value {
    let mut map = serde_json::Map::with_capacity(n);
    for i in 0..n {
        map.insert(format!("c{i}"), json!({"name": format!("box-{i}")}));
    }
    Value::Object(map)
}

/// Helper: build a `/set` destroy array of `n` Id-shaped strings.
fn destroy_array_of_size(n: usize) -> Value {
    Value::Array((0..n).map(|i| json!(format!("id-{i}"))).collect())
}

/// Independent oracle for `JmapError::limit("maxObjectsInSet")`.
///
/// Constructed against the public `JmapError::limit` API documented in
/// `crate-jmap-types/src/error.rs` (description carries the limit name);
/// the helper under test is NOT used to produce the expected value.
fn expected_limit_error_shape() -> (&'static str, &'static str) {
    ("limit", "maxObjectsInSet")
}

/// Set up a MemoryBackend with one account ready for /set dispatch.
fn one_account_backend() -> (MemoryBackend, Id) {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    backend.register_account(&account_id);
    (backend, account_id)
}

// ---------------------------------------------------------------------------
// Mailbox/set
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mailbox_set_over_limit_returns_limit_error() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "create": create_map_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_mailbox_set(&backend, &(), args)
        .await
        .expect_err("501-entry batch must trip maxObjectsInSet cap");
    let (etype, edesc) = expected_limit_error_shape();
    assert_eq!(err.error_type.as_str(), etype);
    assert_eq!(err.description.as_deref(), Some(edesc));
}

/// At-limit positive control for Mailbox/set: exactly `DEFAULT_CAP`
/// create entries MUST pass the cap check. The handler proceeds to
/// per-entry validation; entries may fail downstream for other reasons
/// (missing required fields, role conflicts, etc.) but the cap itself
/// MUST NOT reject the request.
#[tokio::test]
async fn mailbox_set_at_limit_passes_cap_check() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "create": create_map_of_size(DEFAULT_CAP),
    });
    // The cap check must not return a method-level error. The handler
    // returns Ok(...) and reports per-entry results in notCreated /
    // created. Any other JmapError is acceptable to fail this test
    // because it means the cap path is wrong; the per-entry validation
    // failures live inside the Ok(...) wire shape, not in the method-
    // level Err.
    let result = handle_mailbox_set(&backend, &(), args).await;
    if let Err(ref err) = result {
        assert_ne!(
            err.error_type.as_str(),
            "limit",
            "exactly {DEFAULT_CAP} entries must NOT trip the maxObjectsInSet cap; \
             got JmapError type=limit with description={:?}",
            err.description,
        );
    }
}

// ---------------------------------------------------------------------------
// Email/set
// ---------------------------------------------------------------------------

#[tokio::test]
async fn email_set_over_limit_returns_limit_error() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "create": create_map_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_email_set(&backend, &(), args)
        .await
        .expect_err("501-entry Email/set batch must trip maxObjectsInSet cap");
    let (etype, edesc) = expected_limit_error_shape();
    assert_eq!(err.error_type.as_str(), etype);
    assert_eq!(err.description.as_deref(), Some(edesc));
}

// ---------------------------------------------------------------------------
// Identity/set
// ---------------------------------------------------------------------------

#[tokio::test]
async fn identity_set_over_limit_returns_limit_error() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "create": create_map_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_identity_set(&backend, &(), args)
        .await
        .expect_err("501-entry Identity/set batch must trip maxObjectsInSet cap");
    let (etype, edesc) = expected_limit_error_shape();
    assert_eq!(err.error_type.as_str(), etype);
    assert_eq!(err.description.as_deref(), Some(edesc));
}

// ---------------------------------------------------------------------------
// EmailSubmission/set
// ---------------------------------------------------------------------------

#[tokio::test]
async fn submission_set_over_limit_returns_limit_error() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "destroy": destroy_array_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_submission_set(&backend, &(), args, "c0")
        .await
        .expect_err("501-entry EmailSubmission/set batch must trip maxObjectsInSet cap");
    let (etype, edesc) = expected_limit_error_shape();
    assert_eq!(err.error_type.as_str(), etype);
    assert_eq!(err.description.as_deref(), Some(edesc));
}

// ---------------------------------------------------------------------------
// SieveScript/set
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sieve_set_over_limit_returns_limit_error() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "create": create_map_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_sieve_set(&backend, &(), args)
        .await
        .expect_err("501-entry SieveScript/set batch must trip maxObjectsInSet cap");
    let (etype, edesc) = expected_limit_error_shape();
    assert_eq!(err.error_type.as_str(), etype);
    assert_eq!(err.description.as_deref(), Some(edesc));
}

// ---------------------------------------------------------------------------
// VacationResponse/set
// ---------------------------------------------------------------------------
//
// VacationResponse is a singleton (RFC 8621 §8); legitimate /set batches
// are always 1 create or 1 update. The cap is wired for canonical-template
// uniformity across the 6 mail-server /set handlers — applying the same
// over-limit defence here costs nothing and pins the behaviour against a
// future refactor that drops the call.

#[tokio::test]
async fn vacation_set_over_limit_returns_limit_error() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "destroy": destroy_array_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_vacation_set(&backend, &(), args)
        .await
        .expect_err("501-entry VacationResponse/set batch must trip maxObjectsInSet cap");
    let (etype, edesc) = expected_limit_error_shape();
    assert_eq!(err.error_type.as_str(), etype);
    assert_eq!(err.description.as_deref(), Some(edesc));
}
