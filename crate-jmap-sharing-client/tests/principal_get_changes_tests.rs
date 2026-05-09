//! Wiremock integration tests for Principal/get and Principal/changes.
//!
//! Oracle for response shapes: RFC 9670 §2.1 (get), §2.2 (changes).
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.

#[path = "common/mod.rs"]
mod common;

use jmap_types::{Id, State};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test 1: Principal/get with no ids filter returns all principals.
///
/// Oracle: RFC 9670 §2.1 — passing ids=null returns all Principals for the
/// primary account. Response fields from RFC 9670 §2 Principal object definition.
#[tokio::test]
async fn principal_get_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Principal/get",
            {
                "accountId": "u33084183",
                "state": "p-s1",
                "list": [
                    {
                        "id": "p-joe",
                        "type": "individual",
                        "name": "Joe Bloggs",
                        "email": "joe@example.com",
                        "description": null,
                        "timeZone": null,
                        "capabilities": {},
                        "accounts": null
                    }
                ],
                "notFound": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = common::make_client(&server).await;
    let resp = sc
        .principal_get(None, None)
        .await
        .expect("principal_get_round_trip: must succeed");

    assert_eq!(resp.account_id.as_ref(), "u33084183", "accountId mismatch");
    assert_eq!(resp.list.len(), 1, "list must have 1 principal");
    assert_eq!(
        resp.list[0].id.as_ref(),
        "p-joe",
        "principal id must be p-joe"
    );
    assert_eq!(
        resp.list[0].name, "Joe Bloggs",
        "principal name must be Joe Bloggs"
    );
}

/// Test 2: Principal/get with specific ids sends ids array in the request.
///
/// Oracle: RFC 9670 §2.1 — ids argument is an array of Principal IDs.
/// The wire request must carry the ids array, not null.
#[tokio::test]
async fn principal_get_specific_ids_sends_array() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Principal/get",
            {
                "accountId": "u33084183",
                "state": "p-s1",
                "list": [
                    {
                        "id": "p-joe",
                        "type": "individual",
                        "name": "Joe Bloggs",
                        "email": "joe@example.com",
                        "description": null,
                        "timeZone": null,
                        "capabilities": {},
                        "accounts": null
                    }
                ],
                "notFound": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = common::make_client(&server).await;
    sc.principal_get(Some(&[Id::from("p-joe")]), None)
        .await
        .expect("principal_get_specific_ids_sends_array: must succeed");

    // Inspect the request body to verify ids were sent in the wire call.
    let reqs = server
        .received_requests()
        .await
        .expect("principal_get_specific_ids_sends_array: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("principal_get_specific_ids_sends_array: request body must be valid JSON");
    let ids = &body["methodCalls"][0][1]["ids"];
    assert!(ids.is_array(), "ids must be an array in the request");
    assert_eq!(ids[0], json!("p-joe"), "first id in request must be p-joe");
}

/// Test 3: Principal/changes sends sinceState and returns old/new states.
///
/// Oracle: RFC 9670 §2.2 — sinceState is required; maxChanges is optional
/// and MUST be absent from the request when None is passed (RFC 8620 §5.2).
#[tokio::test]
async fn principal_changes_sends_since_state() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Principal/changes",
            {
                "accountId": "u33084183",
                "oldState": "s1",
                "newState": "s2",
                "hasMoreChanges": false,
                "created": [],
                "updated": ["p-joe"],
                "destroyed": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = common::make_client(&server).await;
    let resp = sc
        .principal_changes(&State::from("s1"), None)
        .await
        .expect("principal_changes_sends_since_state: must succeed");

    assert_eq!(resp.old_state, "s1", "oldState mismatch");
    assert_eq!(resp.new_state, "s2", "newState mismatch");
    assert!(!resp.has_more_changes, "hasMoreChanges must be false");

    // Inspect the request body: sinceState must be present, maxChanges must be absent.
    let reqs = server
        .received_requests()
        .await
        .expect("principal_changes_sends_since_state: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("principal_changes_sends_since_state: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["sinceState"],
        json!("s1"),
        "sinceState must be s1 in request"
    );
    assert!(
        args.get("maxChanges").is_none(),
        "maxChanges must be absent from request when None is passed: {args}"
    );
}
