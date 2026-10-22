//! Wiremock smoke tests for CalendarEventNotification/* methods.
//!
//! Verifies wire-format passthrough for the methods that previously had
//! no production-path coverage (JMAP-uuoi.1).
//!
//! Oracle for response shapes:
//!   - CalendarEventNotification/get: draft-ietf-jmap-calendars-26 §7.1 + RFC 8620 §5.1
//!   - CalendarEventNotification/changes: §7.2 + RFC 8620 §5.2
//!   - CalendarEventNotification/set: §7.3 + RFC 8620 §5.3 (destroy-only)
//!   - CalendarEventNotification/query: §7.4 + RFC 8620 §5.5
//!   - CalendarEventNotification/queryChanges: §7.5 + RFC 8620 §5.6

#[path = "helpers.rs"]
mod helpers;

use jmap_calendars_types::{NotificationFilter, NotificationFilterCondition};
use jmap_types::query::{Filter, FilterOperator, Operator};
use jmap_types::{Id, State, UTCDate};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// NotificationFilterCondition is `#[non_exhaustive]`; constructed via
// `Default::default()` plus field mutation in the filter test below.

/// Oracle: draft-ietf-jmap-calendars-26 §7.1 / RFC 8620 §5.1 — basic
/// `CalendarEventNotification/get` shape. Verifies that `ids` and
/// `properties` (when supplied) thread through verbatim.
#[tokio::test]
async fn calendar_event_notification_get_basic_shape() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEventNotification/get",
            {
                "accountId": "A13824",
                "state": "ns1",
                "list": [],
                "notFound": ["n-missing"]
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
    let ids = [Id::from("n-1"), Id::from("n-2")];
    let resp = sc
        .calendar_event_notification_get(Some(&ids), Some(&["type", "created"]))
        .await
        .expect("calendar_event_notification_get: must succeed");
    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    assert_eq!(resp.not_found, [Id::from("n-missing")], "notFound mismatch");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["ids"], json!(["n-1", "n-2"]), "ids array mismatch");
    assert_eq!(
        args["properties"],
        json!(["type", "created"]),
        "properties array mismatch"
    );
}

/// Oracle: draft-ietf-jmap-calendars-26 §7.2 / RFC 8620 §5.2 —
/// `CalendarEventNotification/changes` requires `sinceState`.
#[tokio::test]
async fn calendar_event_notification_changes_basic_shape() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEventNotification/changes",
            {
                "accountId": "A13824",
                "oldState": "ns1",
                "newState": "ns2",
                "hasMoreChanges": false,
                "created": ["n-new"],
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
    let since = State::from("ns1");
    let _ = sc
        .calendar_event_notification_changes(&since, None)
        .await
        .expect("calendar_event_notification_changes: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["sinceState"], json!("ns1"), "sinceState mismatch");
    assert!(
        args.get("maxChanges").is_none(),
        "maxChanges must be absent when None"
    );
}

/// Oracle: draft-ietf-jmap-calendars-26 §7.3 — `CalendarEventNotification/set`
/// is destroy-only. The server MUST reject create and update operations
/// with `forbidden`. This client method only sends a `destroy` array;
/// `create` and `update` keys MUST be absent from the wire.
///
/// When the caller passes `Some(&ids)`, the array carries the ids.
#[tokio::test]
async fn calendar_event_notification_set_destroy_only_with_ids() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEventNotification/set",
            {
                "accountId": "A13824",
                "oldState": "ns2",
                "newState": "ns3",
                "created": null,
                "updated": null,
                "destroyed": ["n-1", "n-2"],
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": null
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
    let ids = [Id::from("n-1"), Id::from("n-2")];
    let resp = sc
        .calendar_event_notification_set(Some(&ids), None)
        .await
        .expect("calendar_event_notification_set: must succeed");
    assert_eq!(
        resp.destroyed.as_deref(),
        Some(&[Id::from("n-1"), Id::from("n-2")][..]),
        "destroyed list mismatch"
    );

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["destroy"],
        json!(["n-1", "n-2"]),
        "destroy array mismatch"
    );
    assert!(
        args.get("create").is_none(),
        "create key MUST be absent (destroy-only method)"
    );
    assert!(
        args.get("update").is_none(),
        "update key MUST be absent (destroy-only method)"
    );
}

/// Oracle: draft-ietf-jmap-calendars-26 §7.3 — when caller passes
/// `None`, the wire request still carries `destroy: []` (the production
/// builder is deliberate about always sending a non-null destroy array,
/// see method docstring).
#[tokio::test]
async fn calendar_event_notification_set_destroy_none_sends_empty_array() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEventNotification/set",
            {
                "accountId": "A13824",
                "oldState": "ns3",
                "newState": "ns3",
                "created": null,
                "updated": null,
                "destroyed": null,
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": null
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
    let _ = sc
        .calendar_event_notification_set(None, None)
        .await
        .expect("calendar_event_notification_set: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["destroy"],
        json!([]),
        "destroy must be an empty array (None → [])"
    );
    assert!(args.get("create").is_none(), "create MUST be absent");
    assert!(args.get("update").is_none(), "update MUST be absent");
}

/// Oracle: draft-ietf-jmap-calendars-26 §7.4 / RFC 8620 §5.5 —
/// `CalendarEventNotification/query` accepts a typed
/// `NotificationFilterCondition` and a sort comparator slice.
#[tokio::test]
async fn calendar_event_notification_query_filter_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEventNotification/query",
            {
                "accountId": "A13824",
                "queryState": "nqs1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": ["n-1"],
                "total": null,
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
    let mut filter = NotificationFilterCondition::default();
    filter.after = Some(UTCDate::from("2024-06-01T00:00:00Z"));
    filter.notification_type = Some("invitation".to_owned());
    filter.calendar_event_ids = Some(vec![Id::from("ev-1")]);
    let sort = [json!({ "property": "created", "isAscending": false })];
    let filter = Filter::Condition(filter);

    let _ = sc
        .calendar_event_notification_query(
            Some(&filter),
            Some(&sort),
            &jmap_base_client::QueryParams {
                limit: Some(20),
                ..Default::default()
            },
        )
        .await
        .expect("calendar_event_notification_query: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(
        args["filter"]["after"],
        json!("2024-06-01T00:00:00Z"),
        "filter.after mismatch"
    );
    // NotificationFilterCondition uses #[serde(rename = "type")] for
    // notification_type — verify the wire key is "type", not
    // "notificationType".
    assert_eq!(
        args["filter"]["type"],
        json!("invitation"),
        "filter.type mismatch (wire key MUST be 'type', not 'notificationType')"
    );
    assert!(
        args["filter"].get("notificationType").is_none(),
        "'notificationType' MUST NOT appear on the wire"
    );
    assert_eq!(
        args["filter"]["calendarEventIds"],
        json!(["ev-1"]),
        "filter.calendarEventIds mismatch"
    );
    assert_eq!(
        args["sort"],
        json!([{ "property": "created", "isAscending": false }]),
        "sort slice mismatch"
    );
    assert_eq!(args["limit"], json!(20), "limit mismatch");
    assert!(
        args.get("position").is_none(),
        "position must be absent when None"
    );
}

/// Oracle: draft-ietf-jmap-calendars-26 §7.5 / RFC 8620 §5.6 —
/// `CalendarEventNotification/queryChanges` carries `sinceQueryState`.
#[tokio::test]
async fn calendar_event_notification_query_changes_basic_shape() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEventNotification/queryChanges",
            {
                "accountId": "A13824",
                "oldQueryState": "nqs1",
                "newQueryState": "nqs2",
                "total": null,
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
    let since = State::from("nqs1");
    let _ = sc
        .calendar_event_notification_query_changes(
            &since,
            None,
            None::<&NotificationFilter>,
            None,
            None,
            None,
        )
        .await
        .expect("calendar_event_notification_query_changes: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["sinceQueryState"],
        json!("nqs1"),
        "sinceQueryState mismatch"
    );
}

/// `CalendarEventNotification/queryChanges` with filter, sort, upToId,
/// and calculateTotal must emit all four optional args on the wire
/// (RFC 8620 §5.6).
///
/// Oracle: draft-ietf-jmap-calendars-26 §7.4 — `calendarEventIds` is a
/// valid NotificationFilterCondition field; the comparator shape
/// (property + isAscending) follows RFC 8620 §5.5.
#[tokio::test]
async fn calendar_event_notification_query_changes_with_filter_sort_upto_calculatetotal() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEventNotification/queryChanges",
            {
                "accountId": "A13824",
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

    let sc = helpers::make_client(&server);
    let since = State::from("nqs1");
    let up_to = Id::from("notif-100");
    let mut condition = NotificationFilterCondition::default();
    condition.calendar_event_ids = Some(vec![Id::from("ev-1")]);
    let filter = Filter::Condition(condition);
    let sort = [json!({ "property": "created", "isAscending": false })];
    sc.calendar_event_notification_query_changes(
        &since,
        None,
        Some(&filter),
        Some(&sort),
        Some(&up_to),
        Some(true),
    )
    .await
    .expect(
        "calendar_event_notification_query_changes_with_filter_sort_upto_calculatetotal: \
             must succeed",
    );

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filter"]["calendarEventIds"][0],
        json!("ev-1"),
        "filter.calendarEventIds[0] must be 'ev-1'"
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

/// `CalendarEventNotification/queryChanges` with all None optional args
/// must NOT emit any of filter/sort/upToId/calculateTotal/maxChanges on
/// the wire.
///
/// Oracle: RFC 8620 §5.6 — all five are optional; the wire shape with
/// `None` for each must be byte-identical to the minimal
/// `sinceQueryState`-only call.
#[tokio::test]
async fn calendar_event_notification_query_changes_all_none_omits_optional_wire_keys() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEventNotification/queryChanges",
            {
                "accountId": "A13824",
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

    let sc = helpers::make_client(&server);
    let since = State::from("nqs1");
    sc.calendar_event_notification_query_changes(
        &since,
        None,
        None::<&NotificationFilter>,
        None,
        None,
        None,
    )
    .await
    .expect(
        "calendar_event_notification_query_changes_all_none_omits_optional_wire_keys: \
             must succeed",
    );

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

/// Mount a canned `CalendarEventNotification/*` response and return the
/// `filter` argument the client sent.
async fn sent_notification_filter(
    method_name: &str,
    response_args: serde_json::Value,
    call: impl AsyncFnOnce(&jmap_calendars_client::SessionClient),
) -> serde_json::Value {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[method_name, response_args, "r1"]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;
    let sc = helpers::make_client(&server);
    call(&sc).await;
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    body["methodCalls"][0][1]["filter"].clone()
}

/// `CalendarEventNotification/query` accepts a compound `NOT` filter.
///
/// Oracle: RFC 8620 §5.5 — FilterOperator is `{"operator": "AND" | "OR" |
/// "NOT", "conditions": Filter[]}`; draft-ietf-jmap-calendars-26 §7.4 —
/// `type` is a CalendarEventNotification FilterCondition property.
/// Expected JSON is hand-written.
#[tokio::test]
async fn calendar_event_notification_query_not_filter_wire_shape() {
    let mut destroyed = NotificationFilterCondition::default();
    destroyed.notification_type = Some("destroyed".to_owned());
    let filter = Filter::Operator(FilterOperator::new(
        Operator::Not,
        vec![Filter::Condition(destroyed)],
    ));

    let sent = sent_notification_filter(
        "CalendarEventNotification/query",
        json!({
            "accountId": "A13824",
            "queryState": "nqs1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": [],
            "total": null,
            "limit": null
        }),
        async |sc| {
            sc.calendar_event_notification_query(
                Some(&filter),
                None,
                &jmap_base_client::QueryParams::default(),
            )
            .await
            .expect("calendar_event_notification_query: must succeed");
        },
    )
    .await;

    assert_eq!(
        sent,
        json!({ "operator": "NOT", "conditions": [{ "type": "destroyed" }] }),
        "NOT FilterOperator wire shape mismatch (RFC 8620 §5.5)"
    );
}

/// `CalendarEventNotification/queryChanges` accepts a compound `OR`
/// filter, matching the shape `/query` accepts (RFC 8620 §5.6).
///
/// Oracle: RFC 8620 §5.5 FilterOperator definition;
/// draft-ietf-jmap-calendars-26 §7.4 `type`. Expected JSON is
/// hand-written.
#[tokio::test]
async fn calendar_event_notification_query_changes_or_filter_wire_shape() {
    let mut created = NotificationFilterCondition::default();
    created.notification_type = Some("created".to_owned());
    let mut updated = NotificationFilterCondition::default();
    updated.notification_type = Some("updated".to_owned());
    let filter = Filter::Operator(FilterOperator::new(
        Operator::Or,
        vec![Filter::Condition(created), Filter::Condition(updated)],
    ));
    let since = State::from("nqs1");

    let sent = sent_notification_filter(
        "CalendarEventNotification/queryChanges",
        json!({
            "accountId": "A13824",
            "oldQueryState": "nqs1",
            "newQueryState": "nqs2",
            "removed": [],
            "added": []
        }),
        async |sc| {
            sc.calendar_event_notification_query_changes(
                &since,
                None,
                Some(&filter),
                None,
                None,
                None,
            )
            .await
            .expect("calendar_event_notification_query_changes: must succeed");
        },
    )
    .await;

    assert_eq!(
        sent,
        json!({
            "operator": "OR",
            "conditions": [{ "type": "created" }, { "type": "updated" }]
        }),
        "OR FilterOperator wire shape mismatch (RFC 8620 §5.5)"
    );
}
