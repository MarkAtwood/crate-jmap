//! Integration tests for Principal/* method handlers (RFC 9670 §2).
//!
//! Each test dispatches through `register_sharing_handlers` with the
//! in-memory `MemoryBackend`; results are asserted against RFC wire shapes.

mod common;

use std::sync::Arc;

use jmap_server::{Dispatcher, JmapRequest, State};
use jmap_sharing_server::{register_sharing_handlers, SharingBackend};
use jmap_sharing_types::Principal;
use jmap_types::Id;
use serde_json::json;

use common::MemoryBackend;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal [`JmapRequest`] with a single method call.
fn single_call(method: &str, args: serde_json::Value, call_id: &str) -> JmapRequest {
    JmapRequest::new(
        vec!["urn:ietf:params:jmap:principals".into()],
        vec![(method.into(), args, call_id.into())],
        None,
    )
}

/// Minimal valid Principal JSON suitable for a `/set create` payload.
fn alice_json() -> serde_json::Value {
    json!({
        "type": "individual",
        "name": "Alice Smith",
        "email": "alice@example.com",
        "description": null,
        "timeZone": null,
        "capabilities": {},
        "accounts": null
    })
}

/// Seed a Principal into `backend` for `account_id` using the backend directly.
///
/// Returns the server-assigned [`Id`].
async fn seed_principal(backend: &MemoryBackend, account_id: &str, p: serde_json::Value) -> Id {
    // Attach a placeholder id so the Principal deserializes; MemoryBackend
    // will overwrite it with the server-assigned id.
    let mut with_id = p;
    with_id["id"] = json!("placeholder");
    let principal: Principal =
        serde_json::from_value(with_id).expect("test fixture must deserialize");
    let (server_id, _) = backend
        .create_object::<Principal>(&Id::from(account_id), "seed", principal)
        .await
        .expect("seed must succeed");
    server_id
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Oracle: RFC 8620 §5.1 — /get with ids:null returns all objects for the account.
///
/// Pre-seed two Principals; dispatch Principal/get with ids:null; assert both
/// are present in the response list.
#[tokio::test]
async fn principal_get_all_returns_list() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));

    seed_principal(
        &backend,
        "acc1",
        json!({
            "type": "individual", "name": "Alice Smith",
            "email": "alice@example.com", "description": null,
            "timeZone": null, "capabilities": {}, "accounts": null
        }),
    )
    .await;
    seed_principal(
        &backend,
        "acc1",
        json!({
            "type": "group", "name": "Engineering",
            "email": null, "description": null,
            "timeZone": null, "capabilities": {}, "accounts": null
        }),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "Principal/get",
        json!({"accountId": "acc1", "ids": null}),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    assert!(args.get("type").is_none(), "must not be an error: {args}");
    let list = args["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 2, "expected 2 principals, got: {args}");
}

/// Oracle: RFC 8620 §5.3 — /set create returns the new id in `created`.
///
/// Dispatch Principal/set with one create entry; assert that `created["c1"]`
/// is present and has a non-empty, non-placeholder id.
#[tokio::test]
async fn principal_set_create_succeeds() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

    // Attach a placeholder id in the create body so the type deserializes.
    let mut create_body = alice_json();
    create_body["id"] = json!("placeholder");

    let req = single_call(
        "Principal/set",
        json!({
            "accountId": "acc1",
            "create": { "c1": create_body }
        }),
        "c1",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    assert!(args.get("type").is_none(), "must not be an error: {args}");
    let created = &args["created"];
    assert!(
        !created["c1"].is_null(),
        "created[\"c1\"] must be present: {args}"
    );
    let assigned_id = created["c1"]["id"].as_str().expect("id must be a string");
    assert!(
        !assigned_id.is_empty() && assigned_id != "placeholder",
        "server must assign a real id, got: {assigned_id}"
    );
}

/// Oracle: RFC 8620 §5.3 — /set destroy removes the object; subsequent /get
/// returns it in `notFound`.
///
/// Create one Principal via the backend, then dispatch Principal/set destroy;
/// assert destroyed contains the id; then get by that id and assert notFound.
#[tokio::test]
async fn principal_set_destroy_succeeds() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));

    let id = seed_principal(
        &backend,
        "acc1",
        json!({
            "type": "individual", "name": "Alice Smith",
            "email": "alice@example.com", "description": null,
            "timeZone": null, "capabilities": {}, "accounts": null
        }),
    )
    .await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

    // Destroy the principal.
    let req = single_call(
        "Principal/set",
        json!({ "accountId": "acc1", "destroy": [id.as_ref()] }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    assert!(args.get("type").is_none(), "set must not error: {args}");
    let destroyed = args["destroyed"]
        .as_array()
        .expect("destroyed must be array");
    assert!(
        destroyed.iter().any(|v| v.as_str() == Some(id.as_ref())),
        "destroyed must contain the id {id}: {args}"
    );

    // Subsequent get must return notFound.
    let req2 = single_call(
        "Principal/get",
        json!({ "accountId": "acc1", "ids": [id.as_ref()] }),
        "c1",
    );
    let resp2 = dispatcher.dispatch(req2, (), State::from("s0")).await;
    let (_, args2, _) = &resp2.method_responses[0];

    let not_found = args2["notFound"]
        .as_array()
        .expect("notFound must be array");
    assert!(
        not_found.iter().any(|v| v.as_str() == Some(id.as_ref())),
        "destroyed id must appear in notFound: {args2}"
    );
}

/// Oracle: RFC 8620 §5.2 — /changes with sinceState records new creates.
///
/// Record the current state; create a Principal; call Principal/changes with
/// sinceState; assert the new id appears in `created`.
#[tokio::test]
async fn principal_changes_after_create() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

    // Capture state before creation.
    let state_req = single_call(
        "Principal/get",
        json!({ "accountId": "acc1", "ids": [] }),
        "c0",
    );
    let state_resp = dispatcher.dispatch(state_req, (), State::from("s0")).await;
    let (_, state_args, _) = &state_resp.method_responses[0];
    let since_state = state_args["state"]
        .as_str()
        .expect("state must be a string")
        .to_owned();

    // Create a Principal via the backend directly (bypasses dispatcher to keep
    // the dispatcher state clean for the changes call).
    let new_id = seed_principal(
        &backend,
        "acc1",
        json!({
            "type": "individual", "name": "Alice Smith",
            "email": "alice@example.com", "description": null,
            "timeZone": null, "capabilities": {}, "accounts": null
        }),
    )
    .await;

    // Call Principal/changes with the captured sinceState.
    let changes_req = single_call(
        "Principal/changes",
        json!({ "accountId": "acc1", "sinceState": since_state }),
        "c1",
    );
    let changes_resp = dispatcher
        .dispatch(changes_req, (), State::from("s0"))
        .await;
    let (_, changes_args, _) = &changes_resp.method_responses[0];

    assert!(
        changes_args.get("type").is_none(),
        "changes must not error: {changes_args}"
    );
    let created = changes_args["created"]
        .as_array()
        .expect("created must be array");
    assert!(
        created.iter().any(|v| v.as_str() == Some(new_id.as_ref())),
        "new id {new_id} must appear in changes.created: {changes_args}"
    );
}
