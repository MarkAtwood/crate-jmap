//! Per-id `/set` `serverFail` redaction tests for `Principal/set`.
//!
//! Ported from the canonical `jmap-mail-server` tests
//! `set_per_id_server_fail_redacts_backend_display_{create,update,destroy}`
//! (bd:JMAP-ic0j.68, bd:JMAP-ops7.28). [`FaultyBackend`] injects a
//! `BackendSetError::Other` whose Display text carries
//! [`FAULTY_BACKEND_CANARY`].
//!
//! Oracle: the workspace redaction contract (bd:JMAP-ic0j.53,
//! bd:JMAP-ic0j.68) — RFC 8620 §3.6.1 `serverFail` is "An unexpected or
//! unknown error occurred during the processing of the call", and backend
//! Display text never reaches the wire. The per-id entry must carry
//! `type: "serverFail"` with the fixed
//! [`jmap_server::SERVER_FAIL_INTERNAL_DESC`], and the canary must appear
//! nowhere in the response.

mod common;

use common::{FAULTY_BACKEND_CANARY, FaultyBackend};
use jmap_sharing_server::handle_principal_set;
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";

async fn set(backend: &FaultyBackend, args: Value) -> Value {
    handle_principal_set(backend, &(), args, &[])
        .await
        .expect("/set must not return a top-level error")
        .primary
}

/// Create one Principal with no fault injected and return its server id.
async fn create_ok(backend: &FaultyBackend) -> String {
    let resp = set(
        backend,
        json!({ "accountId": ACCOUNT, "create": { "c0": { "type": "individual", "name": "Alice Smith" } } }),
    )
    .await;
    resp["created"]["c0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("setup create must succeed: {resp}"))
        .to_owned()
}

fn new_backend() -> FaultyBackend {
    let backend = FaultyBackend::new();
    backend.inner.register_account(&Id::from(ACCOUNT));
    backend
}

/// Assert `resp[map][key]` is the redacted `serverFail` and that the
/// canary appears nowhere on the wire.
fn assert_redacted(resp: &Value, map: &str, key: &str) {
    let wire = resp.to_string();
    assert!(
        !wire.contains(FAULTY_BACKEND_CANARY),
        "backend-error Display canary must not appear in /set response; wire: {wire}"
    );
    assert_eq!(
        resp[map][key]["type"].as_str(),
        Some("serverFail"),
        "{map}[{key}] must have type serverFail; resp: {resp}"
    );
    assert_eq!(
        resp[map][key]["description"].as_str(),
        Some(jmap_server::SERVER_FAIL_INTERNAL_DESC),
        "description must be the static SERVER_FAIL_INTERNAL_DESC; resp: {resp}"
    );
}

#[tokio::test]
async fn set_per_id_server_fail_redacts_backend_display_create() {
    let backend = new_backend();
    backend.inject("Principal", "create");
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT, "create": { "c1": { "type": "individual", "name": "Alice Smith" } } }),
    )
    .await;
    assert_redacted(&resp, "notCreated", "c1");
}

#[tokio::test]
async fn set_per_id_server_fail_redacts_backend_display_update() {
    let backend = new_backend();
    let id = create_ok(&backend).await;
    backend.inject("Principal", "update");
    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT, "update": { &id: { "name": "Alice Jones" } } }),
    )
    .await;
    assert_redacted(&resp, "notUpdated", &id);
}

#[tokio::test]
async fn set_per_id_server_fail_redacts_backend_display_destroy() {
    let backend = new_backend();
    let id = create_ok(&backend).await;
    backend.inject("Principal", "destroy");
    let resp = set(&backend, json!({ "accountId": ACCOUNT, "destroy": [&id] })).await;
    assert_redacted(&resp, "notDestroyed", &id);
}
