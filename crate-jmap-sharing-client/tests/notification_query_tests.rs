//! Wiremock integration tests for ShareNotification/query and
//! ShareNotification/queryChanges.
//!
//! Oracle for response shapes: RFC 9670 §3.4 (query), §3.5 (queryChanges).
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.

#[path = "common/mod.rs"]
mod common;

use jmap_types::State;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test 1: ShareNotification/query with filter sends filter in request.
///
/// Oracle: RFC 9670 §3.4.1 — filter is a ShareNotificationFilterCondition.
/// The wire request must carry the filter and the response ids array contains
/// matching ShareNotification IDs (RFC 8620 §5.5).
#[tokio::test]
async fn share_notification_query_with_filter() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ShareNotification/query",
            {
                "accountId": "u33084183",
                "queryState": "nqs1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": ["notif-1", "notif-2"],
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

    let sc = common::make_client(&server);
    let resp = sc
        .share_notification_query(Some(json!({"objectType": "Mailbox"})), None, None, None)
        .await
        .expect("share_notification_query_with_filter: must succeed");

    assert_eq!(resp.ids.len(), 2, "ids list must have 2 entries");
    assert_eq!(resp.query_state, "nqs1", "queryState mismatch");
    assert!(
        resp.can_calculate_changes,
        "canCalculateChanges must be true"
    );

    // Inspect the request body: filter must carry the objectType condition.
    let reqs = server
        .received_requests()
        .await
        .expect("share_notification_query_with_filter: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("share_notification_query_with_filter: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filter"]["objectType"],
        json!("Mailbox"),
        "filter objectType must be Mailbox in request"
    );
}

/// Test 2: ShareNotification/queryChanges returns old/new states and change sets.
///
/// Oracle: RFC 9670 §3.5 / RFC 8620 §5.6 — queryChanges response contains
/// oldQueryState, newQueryState, removed IDs, and added items (id + index).
#[tokio::test]
async fn share_notification_query_changes_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ShareNotification/queryChanges",
            {
                "accountId": "u33084183",
                "oldQueryState": "nqs1",
                "newQueryState": "nqs2",
                "total": 3,
                "removed": ["notif-old"],
                "added": [
                    {"id": "notif-new", "index": 0}
                ]
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = common::make_client(&server);
    let resp = sc
        .share_notification_query_changes(&State::from("nqs1"), None, None, None, None, None)
        .await
        .expect("share_notification_query_changes_round_trip: must succeed");

    assert_eq!(resp.old_query_state, "nqs1", "oldQueryState mismatch");
    assert_eq!(resp.new_query_state, "nqs2", "newQueryState mismatch");
    assert_eq!(resp.total, Some(3), "total mismatch");
    assert_eq!(resp.removed.len(), 1, "removed must have 1 entry");
    assert_eq!(
        resp.removed[0].as_ref(),
        "notif-old",
        "removed id must be notif-old"
    );
    assert_eq!(resp.added.len(), 1, "added must have 1 entry");
    assert_eq!(
        resp.added[0].id.as_ref(),
        "notif-new",
        "added id must be notif-new"
    );
    assert_eq!(resp.added[0].index, 0, "added index must be 0");
}

/// `ShareNotification/queryChanges` with filter, sort, upToId, and
/// calculateTotal must emit all four optional args on the wire
/// (RFC 8620 §5.6).
///
/// Oracle: RFC 9670 §3.4.1 — `objectType` is a valid
/// ShareNotificationFilterCondition field; the comparator shape
/// (property + isAscending) follows RFC 8620 §5.5.
#[tokio::test]
async fn share_notification_query_changes_with_filter_sort_upto_calculatetotal() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ShareNotification/queryChanges",
            {
                "accountId": "u33084183",
                "oldQueryState": "nqs1",
                "newQueryState": "nqs2",
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

    let sc = common::make_client(&server);
    let since = State::from("nqs1");
    let up_to = jmap_types::Id::from("notif-100");
    sc.share_notification_query_changes(
        &since,
        None,
        Some(json!({ "objectType": "Mailbox" })),
        Some(json!([{ "property": "created", "isAscending": false }])),
        Some(&up_to),
        Some(true),
    )
    .await
    .expect("share_notification_query_changes_with_filter_sort_upto_calculatetotal: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filter"]["objectType"],
        json!("Mailbox"),
        "filter.objectType must be 'Mailbox'"
    );
    assert_eq!(
        args["sort"][0]["property"],
        json!("created"),
        "sort[0].property must be 'created'"
    );
    assert_eq!(
        args["upToId"],
        json!("notif-100"),
        "upToId must be on the wire (RFC 8620 §5.6)"
    );
    assert_eq!(
        args["calculateTotal"],
        json!(true),
        "calculateTotal must be on the wire (RFC 8620 §5.6)"
    );
}

/// `ShareNotification/queryChanges` with all None optional args must
/// NOT emit any of filter/sort/upToId/calculateTotal/maxChanges on the
/// wire.
///
/// Oracle: RFC 8620 §5.6 — all five are optional; the wire shape with
/// `None` for each must be byte-identical to the minimal
/// `sinceQueryState`-only call.
#[tokio::test]
async fn share_notification_query_changes_all_none_omits_optional_wire_keys() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ShareNotification/queryChanges",
            {
                "accountId": "u33084183",
                "oldQueryState": "nqs1",
                "newQueryState": "nqs2",
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

    let sc = common::make_client(&server);
    let since = State::from("nqs1");
    sc.share_notification_query_changes(&since, None, None, None, None, None)
        .await
        .expect("share_notification_query_changes_all_none_omits_optional_wire_keys: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert!(args.get("filter").is_none(), "filter must be omitted");
    assert!(args.get("sort").is_none(), "sort must be omitted");
    assert!(args.get("upToId").is_none(), "upToId must be omitted");
    assert!(
        args.get("calculateTotal").is_none(),
        "calculateTotal must be omitted"
    );
    assert!(
        args.get("maxChanges").is_none(),
        "maxChanges must be omitted"
    );
}
