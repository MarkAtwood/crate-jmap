//! RFC 8620 §5.3 `maxObjectsInSet` enforcement on the tasks-server
//! `/set` handlers (bd:JMAP-ayoz.41.5).
//!
//! Every test asserts the wire-format method error `requestTooLarge`
//! (description `"maxObjectsInSet"`) against a hand-built independent
//! oracle — the helper under test is never the oracle. Oversize batches use 501 entries against the
//! default `JmapBackend::max_objects_in_set` cap of 500.
//!
//! Mirrors the canonical extension-server test layout established by
//! `crate-jmap-mail-server/tests/max_objects_in_set.rs` (bd:JMAP-ayoz.41.2).

#![allow(async_fn_in_trait)]

mod common;

use common::MemoryBackend;
use jmap_tasks_server::{handle_task_list_set, handle_task_notification_set, handle_task_set};
use jmap_types::Id;
use serde_json::{Value, json};

/// Default cap value returned by `JmapBackend::max_objects_in_set` in
/// the foundation crate. Tests below construct batches relative to
/// this value rather than hardcoding 500 in multiple places.
const DEFAULT_CAP: usize = 500;

/// Helper: build a `/set` create-map of `n` entries. The wire shape
/// is independent of the typed object's internal struct — the test
/// only cares about the pre-handler cap check, so any
/// `Value::Object` shape suffices.
fn create_map_of_size(n: usize) -> Value {
    let mut map = serde_json::Map::with_capacity(n);
    for i in 0..n {
        map.insert(format!("c{i}"), json!({"name": format!("entry-{i}")}));
    }
    Value::Object(map)
}

/// Helper: build a `/set` destroy array of `n` Id-shaped strings.
fn destroy_array_of_size(n: usize) -> Value {
    Value::Array((0..n).map(|i| json!(format!("id-{i}"))).collect())
}

/// Independent oracle for the method error `requestTooLarge` with
/// description `"maxObjectsInSet"`.
///
/// Type from RFC 8620 §5.3 (bd:JMAP-yt1w.30): ""requestTooLarge": The
/// total number of objects to create, update, or destroy exceeds the
/// maximum number the server is willing to process in a single method
/// call." The description names the exceeded cap. The helper under
/// test is NOT used to produce the expected value.
fn expected_limit_error_shape() -> (&'static str, &'static str) {
    ("requestTooLarge", "maxObjectsInSet")
}

/// Set up a `MemoryBackend` with one account ready for `/set` dispatch.
fn one_account_backend() -> (MemoryBackend, Id) {
    let backend = MemoryBackend::new();
    let account_id = Id::from("account1");
    backend.register_account(&account_id);
    (backend, account_id)
}

// ---------------------------------------------------------------------------
// Task/set
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_set_over_limit_create_returns_limit_error() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "create": create_map_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
        .await
        .expect_err("501-entry Task/set create batch must trip maxObjectsInSet cap");
    let (etype, edesc) = expected_limit_error_shape();
    assert_eq!(err.error_type.to_string(), etype);
    assert_eq!(err.description.as_deref(), Some(edesc));
}

// ---------------------------------------------------------------------------
// TaskList/set
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_list_set_over_limit_create_returns_limit_error() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "create": create_map_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_task_list_set(&backend, &(), args, &[])
        .await
        .expect_err("501-entry TaskList/set create batch must trip maxObjectsInSet cap");
    let (etype, edesc) = expected_limit_error_shape();
    assert_eq!(err.error_type.to_string(), etype);
    assert_eq!(err.description.as_deref(), Some(edesc));
}

// ---------------------------------------------------------------------------
// TaskNotification/set
// ---------------------------------------------------------------------------

#[tokio::test]
async fn task_notification_set_over_limit_destroy_returns_limit_error() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "destroy": destroy_array_of_size(DEFAULT_CAP + 1),
    });
    let err = handle_task_notification_set(&backend, &(), args)
        .await
        .expect_err("501-entry TaskNotification/set destroy batch must trip maxObjectsInSet cap");
    let (etype, edesc) = expected_limit_error_shape();
    assert_eq!(err.error_type.to_string(), etype);
    assert_eq!(err.description.as_deref(), Some(edesc));
}

// ---------------------------------------------------------------------------
// At-limit positive control (Task/set)
// ---------------------------------------------------------------------------

/// Exactly `DEFAULT_CAP` create entries MUST pass the cap check. The
/// handler proceeds to per-entry validation; entries may fail
/// downstream for other reasons but the cap itself MUST NOT reject
/// the request.
#[tokio::test]
async fn task_set_at_limit_passes_cap_check() {
    let (backend, account_id) = one_account_backend();
    let args = json!({
        "accountId": account_id.as_ref(),
        "create": create_map_of_size(DEFAULT_CAP),
    });
    let result =
        handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[]).await;
    if let Err(ref err) = result {
        assert_ne!(
            err.error_type.to_string(),
            "requestTooLarge",
            "exactly {DEFAULT_CAP} entries must NOT trip the maxObjectsInSet cap; \
             got JmapError type=requestTooLarge with description={:?}",
            err.description,
        );
    }
}
