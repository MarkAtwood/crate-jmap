//! Integration tests for ShareNotification/* method handlers (RFC 9670 §3).
//!
//! Each test dispatches through `register_sharing_handlers` with the
//! in-memory `MemoryBackend`; results are asserted against RFC wire shapes.

mod common;

use std::sync::Arc;

use jmap_server::{Dispatcher, JmapRequest, State};
use jmap_sharing_server::{register_sharing_handlers, SharingBackend};
use jmap_sharing_types::ShareNotification;
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

/// Minimal valid ShareNotification JSON for seeding.
///
/// Uses a placeholder `id` — MemoryBackend overwrites it with a server id.
fn notif_json(id_hint: &str) -> serde_json::Value {
    json!({
        "id": id_hint,
        "created": "2024-06-01T10:00:00Z",
        "changedBy": {
            "name": "Bob",
            "email": "bob@example.com",
            "principalId": null
        },
        "objectType": "Mailbox",
        "objectAccountId": "acc1",
        "objectId": "obj1",
        "oldRights": null,
        "newRights": null,
        "name": "Shared Inbox"
    })
}

/// Seed a ShareNotification into `backend` for `account_id`.
///
/// Returns the server-assigned [`Id`].
async fn seed_notification(backend: &MemoryBackend, account_id: &str, v: serde_json::Value) -> Id {
    let notif: ShareNotification =
        serde_json::from_value(v).expect("test fixture must deserialize");
    let (server_id, _) = backend
        .create_object::<ShareNotification>(&(), &Id::from(account_id), "seed", notif)
        .await
        .expect("seed must succeed");
    server_id
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Oracle: RFC 8620 §5.1 — /get with ids:null returns all objects.
///
/// Seed two notifications; dispatch ShareNotification/get with ids:null;
/// assert the response list contains both.
#[tokio::test]
async fn notification_get_all_returns_list() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));

    seed_notification(&backend, "acc1", notif_json("n1")).await;
    seed_notification(&backend, "acc1", notif_json("n2")).await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "ShareNotification/get",
        json!({ "accountId": "acc1", "ids": null }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    assert!(args.get("type").is_none(), "must not be an error: {args}");
    let list = args["list"].as_array().expect("list must be an array");
    assert_eq!(list.len(), 2, "expected 2 notifications, got: {args}");
}

/// Oracle: RFC 8620 §5.3 — /set destroy removes the object; subsequent /get
/// returns it in `notFound`.
///
/// Seed one notification, dispatch ShareNotification/set destroy, assert
/// the `destroyed` list contains the id; then get by id and assert notFound.
#[tokio::test]
async fn notification_set_destroy_succeeds() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let id = seed_notification(&backend, "acc1", notif_json("n1")).await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "ShareNotification/set",
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
        "destroyed must contain {id}: {args}"
    );

    // Confirm the object is gone.
    let req2 = single_call(
        "ShareNotification/get",
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
        "destroyed id must be in notFound: {args2}"
    );
}

/// Oracle: RFC 8620 §5.3 — /set destroy of a non-existent id yields
/// `notDestroyed[id]["type"] == "notFound"`.
#[tokio::test]
async fn notification_set_destroy_not_found() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

    let req = single_call(
        "ShareNotification/set",
        json!({ "accountId": "acc1", "destroy": ["missing"] }),
        "c0",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let (_, args, _) = &resp.method_responses[0];

    assert!(
        args.get("type").is_none(),
        "must not be a top-level error: {args}"
    );
    assert_eq!(
        args["notDestroyed"]["missing"]["type"], "notFound",
        "notDestroyed[\"missing\"] must have type=notFound: {args}"
    );
}

/// Oracle: RFC 8620 §5.2 — /changes with sinceState records destroys.
///
/// Seed one notification; record sinceState; destroy it via the dispatcher;
/// call ShareNotification/changes with sinceState; assert the id is in
/// `destroyed`.
#[tokio::test]
async fn notification_changes_after_destroy() {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let id = seed_notification(&backend, "acc1", notif_json("n1")).await;

    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(&backend));

    // Capture state before the destroy.
    let state_req = single_call(
        "ShareNotification/get",
        json!({ "accountId": "acc1", "ids": [] }),
        "c0",
    );
    let state_resp = dispatcher.dispatch(state_req, (), State::from("s0")).await;
    let (_, state_args, _) = &state_resp.method_responses[0];
    let since_state = state_args["state"]
        .as_str()
        .expect("state must be a string")
        .to_owned();

    // Destroy via the dispatcher.
    let destroy_req = single_call(
        "ShareNotification/set",
        json!({ "accountId": "acc1", "destroy": [id.as_ref()] }),
        "c1",
    );
    dispatcher
        .dispatch(destroy_req, (), State::from("s0"))
        .await;

    // Call /changes and check that the id is in `destroyed`.
    let changes_req = single_call(
        "ShareNotification/changes",
        json!({ "accountId": "acc1", "sinceState": since_state }),
        "c2",
    );
    let changes_resp = dispatcher
        .dispatch(changes_req, (), State::from("s0"))
        .await;
    let (_, changes_args, _) = &changes_resp.method_responses[0];

    assert!(
        changes_args.get("type").is_none(),
        "changes must not error: {changes_args}"
    );
    let destroyed = changes_args["destroyed"]
        .as_array()
        .expect("destroyed must be array");
    assert!(
        destroyed.iter().any(|v| v.as_str() == Some(id.as_ref())),
        "id {id} must appear in changes.destroyed: {changes_args}"
    );
}
