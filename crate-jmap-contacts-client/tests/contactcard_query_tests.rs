//! Wiremock integration tests for ContactCard/query and ContactCard/queryChanges.
//!
//! Oracle for all response shapes: RFC 9610 §3.4–3.5 and
//! RFC 8620 §5.5–5.6.
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.

#[path = "helpers.rs"]
mod helpers;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test JMAP-kh21.3 #1 — ContactCard/query with filter returns matching ids.
///
/// Oracle: RFC 9610 §3.4 — filter is an optional argument.
/// RFC 8620 §5.5 — query response shape (queryState, position, ids, canCalculateChanges).
#[tokio::test]
async fn contact_card_query_with_filter() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ContactCard/query",
            {
                "accountId": "A13824",
                "queryState": "qs1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": ["card1", "card2"],
                "total": 2,
                "limit": null
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
    let filter = json!({"inAddressBook": "ab1"});
    let resp = sc
        .contact_card_query(Some(filter), None, None, None)
        .await
        .expect("contact_card_query_with_filter: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    assert_eq!(resp.query_state, "qs1", "queryState mismatch");
    assert!(
        resp.can_calculate_changes,
        "canCalculateChanges must be true"
    );
    assert_eq!(resp.position, 0, "position must be 0");
    assert_eq!(resp.ids.len(), 2, "ids must have 2 entries");
    assert_eq!(resp.ids[0].as_ref(), "card1", "first id mismatch");
    assert_eq!(resp.ids[1].as_ref(), "card2", "second id mismatch");

    // Verify filter was sent in the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("contact_card_query_with_filter: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("contact_card_query_with_filter: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filter"]["inAddressBook"],
        json!("ab1"),
        "filter.inAddressBook must be ab1 in wire request"
    );
}

/// Test JMAP-kh21.3 #2 — ContactCard/queryChanges returns removed and added ids.
///
/// Oracle: RFC 9610 §3.5 — sinceQueryState is required.
/// RFC 8620 §5.6 — queryChanges response shape (removed, added, newQueryState).
#[tokio::test]
async fn contact_card_query_changes_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ContactCard/queryChanges",
            {
                "accountId": "A13824",
                "oldQueryState": "qs5",
                "newQueryState": "qs6",
                "total": 3,
                "removed": ["card-old"],
                "added": [{"id": "card-new", "index": 0}]
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
        .contact_card_query_changes("qs5", None)
        .await
        .expect("contact_card_query_changes_round_trip: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    assert_eq!(resp.old_query_state, "qs5", "oldQueryState mismatch");
    assert_eq!(resp.new_query_state, "qs6", "newQueryState mismatch");
    assert_eq!(resp.total, Some(3), "total mismatch");
    assert_eq!(resp.removed.len(), 1, "removed must have 1 entry");
    assert_eq!(resp.removed[0].as_ref(), "card-old", "removed id mismatch");
    assert_eq!(resp.added.len(), 1, "added must have 1 entry");
    assert_eq!(resp.added[0].id.as_ref(), "card-new", "added id mismatch");
    assert_eq!(resp.added[0].index, 0, "added index mismatch");

    // Verify sinceQueryState was sent in the wire request.
    let reqs = server
        .received_requests()
        .await
        .expect("contact_card_query_changes_round_trip: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("contact_card_query_changes_round_trip: request body must be valid JSON");
    assert_eq!(
        body["methodCalls"][0][1]["sinceQueryState"],
        json!("qs5"),
        "sinceQueryState must be qs5 in wire request"
    );
}
