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

use jmap_mail_types::Keyword;
use jmap_mail_types::{ComparatorProperty, EmailComparator, EmailFilter, EmailFilterCondition};
use jmap_metadata_types::{MetadataTextMatch, WithMetadata};
use jmap_types::query::{Filter, FilterOperator, Operator};
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
    let mut cond = EmailFilterCondition::default();
    cond.has_keyword = Some(Keyword::from("$flagged"));
    let mut cmp = EmailComparator::new(ComparatorProperty::ReceivedAt);
    cmp.is_ascending = false;
    sc.email_query_changes(
        &since,
        None,
        Some(true),
        Some(&Filter::Condition(cond)),
        Some(&[cmp]),
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
    sc.email_query_changes(&since, None, None, None::<&EmailFilter>, None, None, None)
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

/// Mount a minimal `Email/query` response and return the server.
async fn mount_email_query() -> MockServer {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Email/query",
            {
                "accountId": "A13824",
                "queryState": "qs1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;
    server
}

/// Return the `filter` argument of the first recorded request.
async fn sent_filter(server: &MockServer) -> serde_json::Value {
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    body["methodCalls"][0][1]["filter"].clone()
}

/// A typed `NOT` operator reaches the wire as an RFC 8620 §5.5
/// FilterOperator.
///
/// Oracle: RFC 8620 §5.5 — a FilterOperator has "operator: String"
/// ("NOT": "No conditions must match") and "conditions: (FilterOperator|
/// FilterCondition)[]"; RFC 8621 §4.4.1 names the Email conditions
/// `hasKeyword` and `from`. Expected JSON written by hand.
#[tokio::test]
async fn email_query_sends_typed_not_operator() {
    let server = mount_email_query().await;
    let sc = helpers::make_client(&server);

    let mut seen = EmailFilterCondition::default();
    seen.has_keyword = Some(Keyword::from("$seen"));
    let mut from = EmailFilterCondition::default();
    from.from = Some("boss@example.com".into());
    let filter = Filter::Operator(FilterOperator::new(
        Operator::Not,
        vec![Filter::Condition(seen), Filter::Condition(from)],
    ));
    sc.email_query(
        Some(&filter),
        None,
        &jmap_base_client::QueryParams::default(),
        None,
    )
    .await
    .expect("email_query: must succeed");

    assert_eq!(
        sent_filter(&server).await,
        json!({
            "operator": "NOT",
            "conditions": [
                { "hasKeyword": "$seen" },
                { "from": "boss@example.com" }
            ]
        })
    );
}

/// A `WithMetadata<EmailFilterCondition>` leaf inside an `AND` reaches the
/// wire exactly as draft-ietf-jmap-metadata-02 §6.7 shows it.
///
/// Oracle: the `filter` value of the §6.7 example request ("Find all Email
/// objects in a particular Mailbox whose private vendor memo contains the
/// text \"follow up\""), copied from the draft. The -04 §6.7 example
/// drops the middle `privateMetadataExists` leaf; -04 §3.6 still defines
/// that condition, so this -02 filter remains a valid -04 request.
#[tokio::test]
async fn email_query_sends_metadata_02_section_6_7_filter() {
    let server = mount_email_query().await;
    let sc = helpers::make_client(&server);

    let mut in_mailbox = WithMetadata::<EmailFilterCondition>::default();
    in_mailbox.base.in_mailbox = Some(Id::from("MB-inbox"));
    let mut exists = WithMetadata::<EmailFilterCondition>::default();
    exists.private_metadata_exists = Some("acme.example.com/memo".into());
    let mut contains = WithMetadata::<EmailFilterCondition>::default();
    contains.private_metadata_text_contains =
        Some(MetadataTextMatch::new("acme.example.com/memo", "follow up"));
    let filter = Filter::Operator(FilterOperator::new(
        Operator::And,
        vec![
            Filter::Condition(in_mailbox),
            Filter::Condition(exists),
            Filter::Condition(contains),
        ],
    ));
    sc.email_query(
        Some(&filter),
        None,
        &jmap_base_client::QueryParams::default(),
        None,
    )
    .await
    .expect("email_query: must succeed");

    assert_eq!(
        sent_filter(&server).await,
        json!({
            "operator": "AND",
            "conditions": [
                { "inMailbox": "MB-inbox" },
                { "privateMetadataExists": "acme.example.com/memo" },
                {
                    "privateMetadataTextContains": {
                        "path":  "acme.example.com/memo",
                        "value": "follow up"
                    }
                }
            ]
        })
    );
}

/// `QueryParams` window arguments and `collapseThreads` reach the wire
/// under their RFC names.
///
/// Oracle: RFC 8620 §5.5 names `anchor` ("Id|null"), `anchorOffset`
/// ("Int", "This MAY be negative"), `limit` and `calculateTotal`;
/// RFC 8621 §4.4 adds `collapseThreads`. Expected JSON written by hand.
#[tokio::test]
async fn email_query_sends_anchor_window_and_collapse_threads() {
    let server = mount_email_query().await;
    let sc = helpers::make_client(&server);

    let params = jmap_base_client::QueryParams {
        anchor: Some(Id::from("EM-42")),
        anchor_offset: Some(-2),
        limit: Some(10),
        calculate_total: Some(true),
        ..Default::default()
    };
    sc.email_query(None::<&EmailFilter>, None, &params, Some(true))
        .await
        .expect("email_query: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    assert_eq!(
        body["methodCalls"][0][1],
        json!({
            "accountId": "A13824",
            "anchor": "EM-42",
            "anchorOffset": -2,
            "limit": 10,
            "calculateTotal": true,
            "collapseThreads": true
        })
    );
}
