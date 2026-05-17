//! RFC 8620 §5.3 `maxObjectsInSet` enforcement on the metadata-server
//! `Metadata/set` handler (bd:JMAP-ayoz.41.9 — original target of the
//! workspace cookie-cut sweep at bd:JMAP-ayoz.41).
//!
//! Every test asserts the wire-format `JmapError::limit("maxObjectsInSet")`
//! shape against a hand-built independent oracle — the helper under test
//! is never the oracle. Oversize batches use 501 entries against the
//! default `JmapBackend::max_objects_in_set` cap of 500.
//!
//! Mirrors the canonical extension-server test layout established by
//! `crate-jmap-mail-server/tests/max_objects_in_set.rs` (bd:JMAP-ayoz.41.2).

#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_metadata_server::handle_metadata_set;
use jmap_types::Id;
use serde_json::{json, Value};

/// Default cap value returned by `JmapBackend::max_objects_in_set` in
/// the foundation crate. Tests below construct batches relative to
/// this value rather than hardcoding 500 in multiple places.
const DEFAULT_CAP: usize = 500;

/// Helper: build a `Metadata/set` create-map of `n` entries, each a
/// minimal annotation distinguished by its `relatedId`. The wire
/// shape is independent of `Metadata`'s internal struct — the test
/// only cares about the pre-handler cap check, so any
/// `Value::Object` shape suffices.
fn create_map_of_size(n: usize) -> Value {
    let mut map = serde_json::Map::with_capacity(n);
    for i in 0..n {
        map.insert(
            format!("c{i}"),
            json!({
                "@type": "Annotation",
                "relatedType": "Email",
                "relatedId": format!("EM-{i}")
            }),
        );
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

/// Set up a `MemoryBackend` with one account ready for `/set` dispatch.
fn one_account_backend() -> (MemoryBackend, Id) {
    let account_id = Id::from("account1");
    let backend = MemoryBackend::new_with_accounts(&[account_id.as_ref()]);
    (backend, account_id)
}

// ---------------------------------------------------------------------------
// Metadata/set — over-limit (501 creates)
// ---------------------------------------------------------------------------

/// Oracle: a `create` map with `DEFAULT_CAP + 1` entries (501 against
/// the default cap of 500) MUST be rejected with `type: "limit"` and
/// `description: "maxObjectsInSet"` before the handler touches the
/// storage layer.
#[tokio::test]
async fn metadata_set_over_limit_create_returns_limit_error() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "create": create_map_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_metadata_set(&backend, &(), args)
        .await
        .expect_err("501-entry Metadata/set create batch must trip maxObjectsInSet cap");
    let (etype, edesc) = expected_limit_error_shape();
    assert_eq!(err.error_type.as_str(), etype);
    assert_eq!(err.description.as_deref(), Some(edesc));
}

/// Oracle: the `destroy` array is counted alongside `create` toward the
/// same cap (`enforce_max_objects_in_set` sums all three argument
/// branches). A `destroy` array of 501 entries MUST trip the cap.
#[tokio::test]
async fn metadata_set_over_limit_destroy_returns_limit_error() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "destroy": destroy_array_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_metadata_set(&backend, &(), args)
        .await
        .expect_err("501-entry Metadata/set destroy batch must trip maxObjectsInSet cap");
    let (etype, edesc) = expected_limit_error_shape();
    assert_eq!(err.error_type.as_str(), etype);
    assert_eq!(err.description.as_deref(), Some(edesc));
}

// ---------------------------------------------------------------------------
// Metadata/set — at-limit positive control
// ---------------------------------------------------------------------------

/// At-limit positive control: exactly `DEFAULT_CAP` create entries MUST
/// pass the cap check. The handler proceeds to per-entry validation;
/// entries may fail downstream for other reasons (id-minter behavior,
/// uniqueness conflicts, etc.) but the cap itself MUST NOT reject the
/// request.
#[tokio::test]
async fn metadata_set_at_limit_passes_cap_check() {
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
    let result = handle_metadata_set(&backend, &(), args).await;
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
