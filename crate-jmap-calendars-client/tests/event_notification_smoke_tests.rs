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

use jmap_calendars_types::NotificationFilterCondition;
use jmap_types::{Id, State};
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
    assert_eq!(
        resp.not_found.as_deref(),
        Some(&[Id::from("n-missing")][..]),
        "notFound mismatch"
    );

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
        .calendar_event_notification_set(Some(&ids))
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
        .calendar_event_notification_set(None)
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
    filter.after = Some("2024-06-01T00:00:00Z".to_owned());
    filter.notification_type = Some("invitation".to_owned());
    filter.calendar_event_ids = Some(vec![Id::from("ev-1")]);
    let sort = [json!({ "property": "created", "isAscending": false })];

    let _ = sc
        .calendar_event_notification_query(Some(&filter), Some(&sort), None, Some(20))
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
        .calendar_event_notification_query_changes(&since, None)
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
