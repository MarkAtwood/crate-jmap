//! Wiremock smoke tests for Email/queryChanges wire-shape regression
//! guards.
//!
//! Sibling propagation of the canonical pattern that landed in
//! `tests/submission_query.rs` under JMAP-tjvm.5 (the workspace
//! follow-up bead is JMAP-tjvm.37).
//!
//! Oracle: RFC 8621 §4.4 (Email/query, §4.5 Email/queryChanges) +
//! RFC 8620 §5.6 (generic /queryChanges arg list).

#[path = "helpers.rs"]
mod helpers;

use jmap_types::{Id, State};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// `Email/queryChanges` with filter, sort, upToId, and calculateTotal
/// must emit all four RFC 8620 §5.6 optional args on the wire alongside
/// the RFC 8621 §4.5 `collapseThreads` extension.
///
/// Oracle: RFC 8621 §4.4 — `hasKeyword` is a valid Email filter field;
/// `receivedAt` is a valid sort property. RFC 8621 §4.5 —
/// `collapseThreads` is the only non-RFC-8620 arg accepted by
/// Email/queryChanges.
#[tokio::test]
async fn email_query_changes_with_filter_sort_upto_calculatetotal() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Email/queryChanges",
            {
                "accountId": "A13824",
                "oldQueryState": "qs1",
                "newQueryState": "qs2",
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
    let since = State::from("qs1");
    let up_to = Id::from("EM-100");
    sc.email_query_changes(
        &since,
        None,
        Some(true),
        Some(json!({ "hasKeyword": "$flagged" })),
        Some(json!([{ "property": "receivedAt", "isAscending": false }])),
        Some(&up_to),
        Some(true),
    )
    .await
    .expect("email_query_changes_with_filter_sort_upto_calculatetotal: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filter"]["hasKeyword"],
        json!("$flagged"),
        "filter.hasKeyword must be '$flagged'"
    );
    assert_eq!(
        args["sort"][0]["property"],
        json!("receivedAt"),
        "sort[0].property must be 'receivedAt' (RFC 8621 §4.4)"
    );
    assert_eq!(
        args["upToId"],
        json!("EM-100"),
        "upToId must be on the wire (RFC 8620 §5.6)"
    );
    assert_eq!(
        args["calculateTotal"],
        json!(true),
        "calculateTotal must be on the wire (RFC 8620 §5.6)"
    );
    assert_eq!(
        args["collapseThreads"],
        json!(true),
        "collapseThreads must be on the wire (RFC 8621 §4.5)"
    );
}

/// `Email/queryChanges` with all None optional args must NOT emit any
/// of filter/sort/upToId/calculateTotal/maxChanges/collapseThreads on
/// the wire.
///
/// Oracle: RFC 8620 §5.6 + RFC 8621 §4.5 — all six are optional; the
/// wire shape with `None` for each must be byte-identical to the
/// minimal `sinceQueryState`-only call.
#[tokio::test]
async fn email_query_changes_all_none_omits_optional_wire_keys() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Email/queryChanges",
            {
                "accountId": "A13824",
                "oldQueryState": "qs1",
                "newQueryState": "qs2",
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
    let since = State::from("qs1");
    sc.email_query_changes(&since, None, None, None, None, None, None)
        .await
        .expect("email_query_changes_all_none_omits_optional_wire_keys: must succeed");

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
    assert!(
        args.get("collapseThreads").is_none(),
        "collapseThreads must be omitted"
    );
}
