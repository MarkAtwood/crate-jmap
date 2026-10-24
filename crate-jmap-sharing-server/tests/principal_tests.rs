//! Integration tests for Principal/* method handlers (RFC 9670 §2).
//!
//! Each test dispatches through `register_sharing_handlers` with the
//! in-memory `MemoryBackend`; results are asserted against RFC wire shapes.

mod common;

use jmap_types::Invocation;
use std::sync::Arc;

use jmap_server::{Dispatcher, JmapRequest, State};
use jmap_sharing_server::{SharingBackend, register_sharing_handlers};
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
        vec![Invocation::new(method, args, call_id)],
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
    let principal = move |id: &Id| -> Principal {
        let mut with_id = p;
        with_id["id"] = json!(id);
        serde_json::from_value(with_id).expect("test fixture must deserialize")
    };
    let (server_id, _) = backend
        .create_object(&(), &Id::from(account_id), "seed", principal)
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
    let Invocation {
        arguments: args, ..
    } = &resp.method_responses[0];

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

    // `id`, `capabilities` and `accounts` are server-set (RFC 9670 §2), so
    // the create body omits them (RFC 8620 §1.1).
    let mut create_body = alice_json();
    let body = create_body
        .as_object_mut()
        .expect("alice_json is an object");
    body.remove("capabilities");
    body.remove("accounts");

    let req = single_call(
        "Principal/set",
        json!({
            "accountId": "acc1",
            "create": { "c1": create_body }
        }),
        "c1",
    );
    let resp = dispatcher.dispatch(req, (), State::from("s0")).await;
    let Invocation {
        arguments: args, ..
    } = &resp.method_responses[0];

    assert!(args.get("type").is_none(), "must not be an error: {args}");
    let created = &args["created"];
    assert!(
        !created["c1"].is_null(),
        "created[\"c1\"] must be present: {args}"
    );
    let assigned_id = created["c1"]["id"].as_str().expect("id must be a string");
    assert!(
        !assigned_id.is_empty(),
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
    let Invocation {
        arguments: args, ..
    } = &resp.method_responses[0];

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
    let Invocation {
        arguments: args2, ..
    } = &resp2.method_responses[0];

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
    let Invocation {
        arguments: state_args,
        ..
    } = &state_resp.method_responses[0];
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
    let Invocation {
        arguments: changes_args,
        ..
    } = &changes_resp.method_responses[0];

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

// ---------------------------------------------------------------------------
// Principal/query at the RFC 8620 §1.3 Int/UnsignedInt extremes
// (bd:JMAP-uqhn.6).
//
// Oracle: RFC 8620 §1.3 bounds `Int` to "-2^53+1 <= value <= 2^53-1" and
// `UnsignedInt` to "0 <= value <= 2^53-1". §5.5 position: "the negative
// value MUST be added to the total number of results given the filter,
// and if still negative, it's clamped to "0"", and "If the index is
// greater than or equal to the total number of objects in the results
// list, then the "ids" array in the response will be empty, but this is
// not an error."
// ---------------------------------------------------------------------------

/// Seeds two Principals into "acc1"; returns the backend and their ids.
async fn backend_with_two_principals() -> (Arc<MemoryBackend>, Vec<Id>) {
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let a = seed_principal(&backend, "acc1", alice_json()).await;
    let mut bob = alice_json();
    bob["name"] = json!("Bob Jones");
    bob["email"] = json!("bob@example.com");
    let b = seed_principal(&backend, "acc1", bob).await;
    (backend, vec![a, b])
}

async fn principal_query(
    backend: &Arc<MemoryBackend>,
    args: serde_json::Value,
) -> serde_json::Value {
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(backend));
    let resp = dispatcher
        .dispatch(
            single_call("Principal/query", args, "q0"),
            (),
            State::from("s0"),
        )
        .await;
    resp.method_responses[0].arguments.clone()
}

#[tokio::test]
async fn principal_query_extreme_position_and_limit_saturate() {
    let (backend, mut seeded) = backend_with_two_principals().await;

    let resp = principal_query(
        &backend,
        json!({
            "accountId": "acc1",
            "position": -9_007_199_254_740_991_i64,
            "limit": 9_007_199_254_740_991_u64
        }),
    )
    .await;

    assert!(resp.get("type").is_none(), "must not be an error: {resp}");
    let mut got: Vec<Id> = serde_json::from_value(resp["ids"].clone()).expect("ids array");
    got.sort_by(|x, y| x.as_ref().cmp(y.as_ref()));
    seeded.sort_by(|x, y| x.as_ref().cmp(y.as_ref()));
    assert_eq!(got, seeded, "{resp}");
    assert_eq!(resp["position"], 0, "{resp}");
}

#[tokio::test]
async fn principal_query_largest_position_is_empty_page_not_error() {
    let (backend, _) = backend_with_two_principals().await;

    let resp = principal_query(
        &backend,
        json!({ "accountId": "acc1", "position": 9_007_199_254_740_991_i64 }),
    )
    .await;

    assert!(resp.get("type").is_none(), "must not be an error: {resp}");
    assert_eq!(resp["ids"], json!([]), "{resp}");
}

// ---------------------------------------------------------------------------
// Oracle: RFC 8620 §5.5: "A server MUST reject the call with an
// unsupportedFilter error if it cannot process the given filter" and "with
// an unsupportedSort error if it cannot sort by the given properties." The
// reference MemoryBackend evaluates no RFC 9670 §2.4.1 Principal condition,
// no §3.4.1 ShareNotification condition and no comparator, so each request
// below must fail loud instead of returning every id. A filter with no
// condition set matches everything, so it is not an error. Bead: JMAP-yt1w.47.
// ---------------------------------------------------------------------------

async fn query_error_type(
    backend: &Arc<MemoryBackend>,
    method: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_sharing_handlers(&mut dispatcher, Arc::clone(backend));
    let resp = dispatcher
        .dispatch(single_call(method, args, "q0"), (), State::from("s0"))
        .await;
    let inv = &resp.method_responses[0];
    assert_eq!(inv.name, "error", "{method} must fail: {:?}", inv.arguments);
    inv.arguments["type"].clone()
}

#[tokio::test]
async fn query_rejects_filter_and_sort_backend_cannot_apply() {
    let (backend, _) = backend_with_two_principals().await;

    for (method, args, expected) in [
        (
            "Principal/query",
            json!({ "accountId": "acc1", "filter": { "name": "Alice" } }),
            "unsupportedFilter",
        ),
        (
            "Principal/query",
            json!({ "accountId": "acc1", "filter": {
                "operator": "NOT", "conditions": [{ "type": "group" }]
            } }),
            "unsupportedFilter",
        ),
        (
            "Principal/query",
            json!({ "accountId": "acc1", "sort": [{ "property": "name" }] }),
            "unsupportedSort",
        ),
        (
            "ShareNotification/query",
            json!({ "accountId": "acc1", "filter": { "objectType": "Principal" } }),
            "unsupportedFilter",
        ),
        (
            "ShareNotification/query",
            json!({ "accountId": "acc1", "sort": [{ "property": "created" }] }),
            "unsupportedSort",
        ),
    ] {
        let got = query_error_type(&backend, method, args.clone()).await;
        assert_eq!(got, expected, "{method} {args}");
    }
}

#[tokio::test]
async fn principal_query_empty_filter_matches_all() {
    let (backend, mut seeded) = backend_with_two_principals().await;

    let resp = principal_query(&backend, json!({ "accountId": "acc1", "filter": {} })).await;

    let mut got: Vec<Id> = serde_json::from_value(resp["ids"].clone()).expect("ids array");
    got.sort_by(|x, y| x.as_ref().cmp(y.as_ref()));
    seeded.sort_by(|x, y| x.as_ref().cmp(y.as_ref()));
    assert_eq!(got, seeded, "{resp}");
}
