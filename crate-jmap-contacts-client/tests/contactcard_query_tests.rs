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

    let sc = helpers::make_client(&server);
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

    let sc = helpers::make_client(&server);
    let resp = sc
        .contact_card_query_changes(
            &jmap_types::State::from("qs5"),
            None,
            None,
            None,
            None,
            None,
        )
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

/// `ContactCard/queryChanges` with filter, sort, upToId, and
/// calculateTotal must emit all four optional args on the wire
/// (RFC 8620 §5.6).
///
/// Oracle: RFC 9610 §3.4 — `inAddressBook` is a valid ContactCard
/// filter field; §3.3.2 mandates standard sort properties; the
/// comparator shape (property + isAscending) follows RFC 8620 §5.5.
#[tokio::test]
async fn contact_card_query_changes_with_filter_sort_upto_calculatetotal() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ContactCard/queryChanges",
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

    let sc = helpers::make_client(&server);
    let since = jmap_types::State::from("qs5");
    let up_to = jmap_types::Id::from("CC-100");
    sc.contact_card_query_changes(
        &since,
        None,
        Some(json!({ "inAddressBook": "ab-1" })),
        Some(json!([{ "property": "uid", "isAscending": true }])),
        Some(&up_to),
        Some(true),
    )
    .await
    .expect("contact_card_query_changes_with_filter_sort_upto_calculatetotal: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filter"]["inAddressBook"],
        json!("ab-1"),
        "filter.inAddressBook must be 'ab-1'"
    );
    assert_eq!(
        args["sort"][0]["property"],
        json!("uid"),
        "sort[0].property must be 'uid'"
    );
    assert_eq!(
        args["upToId"],
        json!("CC-100"),
        "upToId must be on the wire (RFC 8620 §5.6)"
    );
    assert_eq!(
        args["calculateTotal"],
        json!(true),
        "calculateTotal must be on the wire (RFC 8620 §5.6)"
    );
}

/// `ContactCard/queryChanges` with all None optional args must NOT emit
/// any of filter/sort/upToId/calculateTotal/maxChanges on the wire.
///
/// Oracle: RFC 8620 §5.6 — all five are optional; the wire shape with
/// `None` for each must be byte-identical to the minimal
/// `sinceQueryState`-only call.
#[tokio::test]
async fn contact_card_query_changes_all_none_omits_optional_wire_keys() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ContactCard/queryChanges",
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

    let sc = helpers::make_client(&server);
    let since = jmap_types::State::from("qs5");
    sc.contact_card_query_changes(&since, None, None, None, None, None)
        .await
        .expect("contact_card_query_changes_all_none_omits_optional_wire_keys: must succeed");

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
