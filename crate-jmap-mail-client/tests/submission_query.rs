//! Wiremock integration tests for EmailSubmission/query and EmailSubmission/queryChanges.
//!
//! Oracle for all response shapes: RFC 8621 §7.3 (query) and §7.4 (queryChanges).
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.

#[path = "helpers.rs"]
mod helpers;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test 1: EmailSubmission/query with a filter sends the filter in the request.
///
/// Oracle: RFC 8621 §7.3 filter condition fields — identityIds is a valid filter field.
#[tokio::test]
async fn email_submission_query_with_filter() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/query",
            {
                "accountId": "A13824",
                "queryState": "qs1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": ["ES-1", "ES-2"],
                "total": 2
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
        .email_submission_query(Some(json!({"identityIds": ["I1"]})), None, None, None)
        .await
        .expect("email_submission_query_with_filter: must succeed");

    assert_eq!(resp.ids.len(), 2, "must return 2 ids");
    assert_eq!(resp.query_state, "qs1", "queryState mismatch");

    // Verify filter was sent correctly in the request.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filter"]["identityIds"][0],
        json!("I1"),
        "filter.identityIds[0] must be I1"
    );
}

/// Test 2: EmailSubmission/query with no filter omits the filter key from the request.
///
/// Oracle: RFC 8621 §7.3 — filter is optional; MUST be absent when None is passed.
#[tokio::test]
async fn email_submission_query_no_filter() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/query",
            {
                "accountId": "A13824",
                "queryState": "qs-empty",
                "canCalculateChanges": true,
                "position": 0,
                "ids": [],
                "total": 0
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
        .email_submission_query(None, None, None, None)
        .await
        .expect("email_submission_query_no_filter: must succeed");

    assert!(resp.ids.is_empty(), "ids must be empty");
    assert_eq!(resp.query_state, "qs-empty", "queryState mismatch");

    // Verify filter key is absent from the request.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be JSON");
    let args = &body["methodCalls"][0][1];
    assert!(
        args.get("filter").is_none(),
        "filter must be absent from request when None: {args}"
    );
}

/// Test 3: EmailSubmission/queryChanges returns old/new queryState and change lists.
///
/// Oracle: RFC 8620 §5.6 /queryChanges response shape.
#[tokio::test]
async fn email_submission_query_changes_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/queryChanges",
            {
                "accountId": "A13824",
                "oldQueryState": "qs1",
                "newQueryState": "qs2",
                "total": 5,
                "removed": ["ES-old"],
                "added": [{"id": "ES-new", "index": 0}]
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
        .email_submission_query_changes("qs1", None, None, None)
        .await
        .expect("email_submission_query_changes_round_trip: must succeed");

    assert_eq!(resp.old_query_state, "qs1", "oldQueryState mismatch");
    assert_eq!(resp.new_query_state, "qs2", "newQueryState mismatch");
    assert!(
        resp.removed.iter().any(|id| id.as_ref() == "ES-old"),
        "removed must contain ES-old"
    );
    assert_eq!(resp.added.len(), 1, "added must have 1 item");
    assert_eq!(
        resp.added[0].id.as_ref(),
        "ES-new",
        "added[0].id must be ES-new"
    );
    assert_eq!(resp.added[0].index, 0, "added[0].index must be 0");
}

/// Test 4: EmailSubmission/queryChanges with filter and sort sends both in the request.
///
/// Oracle: RFC 8621 §7.3 — undoStatus is a valid filter field;
/// the sort property name is "sentAt" (§7.3 line 4513), NOT "sendAt" (the object field).
#[tokio::test]
async fn email_submission_query_changes_with_filter_and_sort() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "EmailSubmission/queryChanges",
            {
                "accountId": "A13824",
                "oldQueryState": "qs5",
                "newQueryState": "qs6",
                "total": 0,
                "removed": [],
                "added": []
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
    sc.email_submission_query_changes(
        "qs5",
        None,
        Some(json!({"undoStatus": "pending"})),
        Some(json!([{"property": "sentAt", "isAscending": false}])),
    )
    .await
    .expect("email_submission_query_changes_with_filter_and_sort: must succeed");

    // Verify both filter and sort are present in the request.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filter"]["undoStatus"],
        json!("pending"),
        "filter.undoStatus must be 'pending'"
    );
    assert_eq!(
        args["sort"][0]["property"],
        json!("sentAt"),
        "sort[0].property must be 'sentAt' (RFC 8621 §7.3 line 4513)"
    );
}
