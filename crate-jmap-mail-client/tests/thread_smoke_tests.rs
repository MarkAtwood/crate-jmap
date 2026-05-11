//! Wiremock smoke tests for Thread/changes.
//!
//! Focus: `since_state` passthrough — Thread/changes is one of the few methods
//! whose only required arg beyond `accountId` is the state token. The deleted
//! vacuous test (JMAP-tco1.5) hand-built JSON; this version exercises the
//! production builder.
//!
//! Oracle: RFC 8621 §3.2 (Thread/changes), RFC 8620 §5.2 (/changes envelope).

#[path = "helpers.rs"]
mod helpers;

use jmap_types::State;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Thread/changes must pass `since_state` through verbatim and emit
/// `sinceState` (camelCase) on the wire (RFC 8620 §5.2).
#[tokio::test]
async fn thread_changes_since_state_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Thread/changes",
            {
                "accountId": "A13824",
                "oldState": "t-old",
                "newState": "t-new",
                "hasMoreChanges": false,
                "created": ["T1"],
                "updated": [],
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

    let sc = helpers::make_client(&server);
    let since = State::from("t-old");
    let _ = sc
        .thread_changes(&since, None)
        .await
        .expect("thread_changes: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(args["accountId"], json!("A13824"), "accountId mismatch");
    assert_eq!(
        args["sinceState"],
        json!("t-old"),
        "sinceState must be passed through as the caller-supplied token"
    );
    // RFC 8620 §5.2 — maxChanges is optional; we passed None so the wire
    // request MUST omit it (not send null).
    assert!(
        args.get("maxChanges").is_none(),
        "maxChanges must be omitted when caller passes None"
    );
}

/// Thread/changes with `max_changes: Some(50)` must include the camelCase
/// `maxChanges` wire key (RFC 8620 §5.2).
#[tokio::test]
async fn thread_changes_max_changes_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Thread/changes",
            {
                "accountId": "A13824",
                "oldState": "t-old",
                "newState": "t-new",
                "hasMoreChanges": true,
                "created": [],
                "updated": [],
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

    let sc = helpers::make_client(&server);
    let since = State::from("t-old");
    let _ = sc
        .thread_changes(&since, Some(50))
        .await
        .expect("thread_changes: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(args["sinceState"], json!("t-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(50), "maxChanges mismatch");
}

/// Thread/changes with an empty `since_state` must be rejected client-side
/// before any HTTP request is made (defence-in-depth against pathological
/// `State::from("")` constructions; see thread.rs production code).
#[tokio::test]
async fn thread_changes_empty_since_state_rejected_before_send() {
    let server = MockServer::start().await;
    // No Mock mounted: if the production code accidentally sends a request,
    // wiremock will record it and the assertion below will fail.

    let sc = helpers::make_client(&server);
    let empty = State::from("");
    let err = sc
        .thread_changes(&empty, None)
        .await
        .expect_err("thread_changes must reject empty since_state");

    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("since_state may not be empty"),
                "error message must explain the validation: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }

    let reqs = server
        .received_requests()
        .await
        .expect("recorded_requests must succeed");
    assert!(
        reqs.is_empty(),
        "no HTTP request must be sent when since_state is empty"
    );
}
