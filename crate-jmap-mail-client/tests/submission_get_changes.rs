//! Wiremock integration tests for EmailSubmission/get and EmailSubmission/changes.
//!
//! Oracle for all response shapes: RFC 8621 §7.1 (get) and §7.2 (changes).
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.

#[path = "helpers.rs"]
mod helpers;

use jmap_types::{Id, State};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test 1: EmailSubmission/get with no ids filter returns all submissions.
///
/// Oracle: RFC 8621 §7.1 — passing ids=null returns all submissions.
/// Response fields from RFC 8621 §7 EmailSubmission object definition.
#[tokio::test]
async fn email_submission_get_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/get",
            {
                "accountId": "A13824",
                "state": "s5",
                "list": [
                    {
                        "id": "ES-1",
                        "identityId": "I1",
                        "emailId": "M1",
                        "threadId": "T1",
                        "envelope": null,
                        "sendAt": "2024-06-01T10:00:00Z",
                        "undoStatus": "final",
                        "deliveryStatus": null,
                        "dsnBlobIds": [],
                        "mdnBlobIds": []
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

    let sc = helpers::make_client(&server).await;
    let resp = sc
        .email_submission_get(None, None)
        .await
        .expect("email_submission_get_round_trip: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    assert_eq!(resp.state, "s5", "state mismatch");
    assert_eq!(resp.list.len(), 1, "list must have 1 submission");
    assert_eq!(resp.list[0].id.as_ref(), "ES-1", "submission id mismatch");
}

/// Test 2: EmailSubmission/get with specific ids sends them in the request.
///
/// Oracle: RFC 8621 §7.1 — ids argument is an array of EmailSubmission IDs.
/// Server returns exactly the requested submission.
#[tokio::test]
async fn email_submission_get_specific_ids() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/get",
            {
                "accountId": "A13824",
                "state": "s7",
                "list": [
                    {
                        "id": "ES-abc",
                        "identityId": "I-ident",
                        "emailId": "M-email",
                        "threadId": "T-thread",
                        "envelope": null,
                        "sendAt": "2024-07-01T08:00:00Z",
                        "undoStatus": "final",
                        "deliveryStatus": null,
                        "dsnBlobIds": [],
                        "mdnBlobIds": []
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

    let sc = helpers::make_client(&server).await;
    let ids = [Id::from("ES-abc")];
    let resp = sc
        .email_submission_get(Some(&ids), None)
        .await
        .expect("email_submission_get_specific_ids: must succeed");

    assert_eq!(resp.list.len(), 1, "list must have 1 submission");
    assert_eq!(
        resp.list[0].id.as_ref(),
        "ES-abc",
        "id must match requested id"
    );

    // Inspect the request body to verify ids were sent in the wire call.
    let reqs = server
        .received_requests()
        .await
        .expect("email_submission_get_specific_ids: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("email_submission_get_specific_ids: request body must be valid JSON");
    let ids = &body["methodCalls"][0][1]["ids"];
    assert!(ids.is_array(), "ids must be an array in the request");
    assert_eq!(ids[0], json!("ES-abc"), "first id must be ES-abc");
}

/// Test 3: EmailSubmission/changes returns old/new state and change lists.
///
/// Oracle: RFC 8620 §5.2 /changes response shape — oldState, newState,
/// hasMoreChanges, created, updated, destroyed arrays.
#[tokio::test]
async fn email_submission_changes_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/changes",
            {
                "accountId": "A13824",
                "oldState": "s10",
                "newState": "s11",
                "hasMoreChanges": false,
                "created": ["ES-new"],
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

    let sc = helpers::make_client(&server).await;
    let since = State::from("s10");
    let resp = sc
        .email_submission_changes(&since, None)
        .await
        .expect("email_submission_changes_round_trip: must succeed");

    assert_eq!(resp.old_state, "s10", "oldState mismatch");
    assert_eq!(resp.new_state, "s11", "newState mismatch");
    assert!(!resp.has_more_changes, "hasMoreChanges must be false");
    assert!(
        resp.created.iter().any(|id| id.as_ref() == "ES-new"),
        "created must contain ES-new"
    );
}

/// Test 4: EmailSubmission/changes with no maxChanges omits the key from the request.
///
/// Oracle: RFC 8620 §5.2 — maxChanges is optional; MUST be absent from the request
/// when None is passed (not sent as null).
#[tokio::test]
async fn email_submission_changes_no_max_changes() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/changes",
            {
                "accountId": "A13824",
                "oldState": "s1",
                "newState": "s2",
                "hasMoreChanges": false,
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

    let sc = helpers::make_client(&server).await;
    let since = State::from("s1");
    sc.email_submission_changes(&since, None)
        .await
        .expect("email_submission_changes_no_max_changes: must succeed");

    // Verify that maxChanges key is absent from the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("email_submission_changes_no_max_changes: must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert!(
        args.get("maxChanges").is_none(),
        "maxChanges must be absent from request when None is passed: {args}"
    );
}
