//! Wiremock smoke tests for CalendarEvent/* methods (excluding /set basic
//! create — that path lives in calendar_smoke_tests.rs).
//!
//! These tests verify wire-format passthrough for the parameter shapes
//! that previously lacked production-path coverage (JMAP-uuoi.1).
//!
//! Oracle for response shapes:
//!   - CalendarEvent/get extra args: draft-ietf-jmap-calendars-26 §5.4
//!   - CalendarEvent/changes: §5.5 + RFC 8620 §5.2
//!   - CalendarEvent/query: §5.11
//!   - CalendarEvent/queryChanges: §5.12 + RFC 8620 §5.6
//!   - CalendarEvent/copy: §5.10 + RFC 8620 §5.4

#[path = "helpers.rs"]
mod helpers;

use std::collections::HashMap;

use jmap_calendars_client::methods::CalendarEventGetParams;
use jmap_calendars_types::{CalendarEventComparator, CalendarEventFilterCondition};
use jmap_types::{Id, State};

// CalendarEventGetParams, CalendarEventFilterCondition, and
// CalendarEventComparator are all `#[non_exhaustive]`, so they cannot be
// constructed with struct-literal syntax from outside their defining
// crate. The tests below build them through `Default::default()` plus
// field mutation, matching the idiom established elsewhere in the
// workspace for non-exhaustive types.
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Oracle: draft-ietf-jmap-calendars-26 §5.4 — `expandRecurrences`,
/// `reducedParticipants`, `fetchCalendars` are `CalendarEvent/get`
/// extra args. All three are optional booleans; absent means "use
/// server default".
///
/// Verifies that production `calendar_event_get` threads each flag set
/// in `CalendarEventGetParams` to the wire under its exact spec name.
#[tokio::test]
async fn calendar_event_get_params_all_three_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEvent/get",
            {
                "accountId": "A13824",
                "state": "s7",
                "list": [],
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

    let sc = helpers::make_client(&server);
    let params = CalendarEventGetParams {
        expand_recurrences: Some(true),
        reduced_participants: Some(false),
        fetch_calendars: Some(true),
        ..Default::default()
    };
    let _ = sc
        .calendar_event_get(None, None, Some(params))
        .await
        .expect("calendar_event_get: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["expandRecurrences"],
        json!(true),
        "expandRecurrences mismatch"
    );
    assert_eq!(
        args["reducedParticipants"],
        json!(false),
        "reducedParticipants mismatch"
    );
    assert_eq!(
        args["fetchCalendars"],
        json!(true),
        "fetchCalendars mismatch"
    );
}

/// Oracle: draft-ietf-jmap-calendars-26 §5.4 — when no `params` are
/// provided, none of the three extra arg keys may appear on the wire.
#[tokio::test]
async fn calendar_event_get_no_params_omits_all_three_keys() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEvent/get",
            {
                "accountId": "A13824",
                "state": "s7",
                "list": [],
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

    let sc = helpers::make_client(&server);
    let _ = sc
        .calendar_event_get(None, None, None)
        .await
        .expect("calendar_event_get: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert!(
        args.get("expandRecurrences").is_none(),
        "expandRecurrences must be absent when params=None"
    );
    assert!(
        args.get("reducedParticipants").is_none(),
        "reducedParticipants must be absent when params=None"
    );
    assert!(
        args.get("fetchCalendars").is_none(),
        "fetchCalendars must be absent when params=None"
    );
}

/// Oracle: draft-ietf-jmap-calendars-26 §5.5 / RFC 8620 §5.2 —
/// `CalendarEvent/changes` carries `sinceState` and an optional
/// `maxChanges`. The response shape matches the canonical
/// `ChangesResponse`.
#[tokio::test]
async fn calendar_event_changes_since_state_and_max_changes_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEvent/changes",
            {
                "accountId": "A13824",
                "oldState": "s20",
                "newState": "s25",
                "hasMoreChanges": true,
                "created": [],
                "updated": ["ev-1", "ev-2"],
                "destroyed": ["ev-3"]
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
    let since = State::from("s20");
    let resp = sc
        .calendar_event_changes(&since, Some(100))
        .await
        .expect("calendar_event_changes: must succeed");
    assert_eq!(resp.old_state, "s20", "oldState mismatch");
    assert!(resp.has_more_changes, "hasMoreChanges must be true");
    assert_eq!(resp.updated.len(), 2, "two updated ids expected");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["sinceState"], json!("s20"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(100), "maxChanges mismatch");
}

/// Oracle: draft-ietf-jmap-calendars-26 §5.11 — `CalendarEvent/query`
/// accepts `filter` (a CalendarEventFilterCondition), `sort` (a list of
/// `CalendarEventComparator`), and optional `position`, `limit`, and
/// `expandRecurrences`. When `expandRecurrences` is `true`, both
/// `filter.before` and `filter.after` MUST be set (server-side
/// validation per §5.11).
///
/// Verifies that the typed filter and sort serialize into the spec
/// camelCase wire shapes and that `expandRecurrences` rides alongside.
#[tokio::test]
async fn calendar_event_query_filter_sort_expand_recurrences_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEvent/query",
            {
                "accountId": "A13824",
                "queryState": "qs1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": ["ev-A"],
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
    let mut filter = CalendarEventFilterCondition::default();
    filter.in_calendar = Some(Id::from("cal-1"));
    filter.after = Some("2024-06-01T00:00:00".to_owned());
    filter.before = Some("2024-07-01T00:00:00".to_owned());
    filter.text = Some("standup".to_owned());
    let mut comparator = CalendarEventComparator::default();
    comparator.property = "start".to_owned();
    comparator.is_ascending = false;
    let sort = vec![comparator];
    let _ = sc
        .calendar_event_query(Some(&filter), Some(&sort), Some(0), Some(10), Some(true))
        .await
        .expect("calendar_event_query: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    assert_eq!(
        args["filter"]["inCalendar"],
        json!("cal-1"),
        "filter.inCalendar mismatch"
    );
    assert_eq!(
        args["filter"]["after"],
        json!("2024-06-01T00:00:00"),
        "filter.after mismatch"
    );
    assert_eq!(
        args["filter"]["before"],
        json!("2024-07-01T00:00:00"),
        "filter.before mismatch"
    );
    assert_eq!(
        args["filter"]["text"],
        json!("standup"),
        "filter.text mismatch"
    );

    // Sort serializes as an array of comparators.
    let sort_arr = args["sort"]
        .as_array()
        .expect("sort must serialize as array");
    assert_eq!(sort_arr.len(), 1, "sort must have one entry");
    assert_eq!(
        sort_arr[0]["property"],
        json!("start"),
        "sort[0].property mismatch"
    );
    assert_eq!(
        sort_arr[0]["isAscending"],
        json!(false),
        "sort[0].isAscending must serialize as camelCase"
    );

    assert_eq!(args["position"], json!(0), "position mismatch");
    assert_eq!(args["limit"], json!(10), "limit mismatch");
    assert_eq!(
        args["expandRecurrences"],
        json!(true),
        "expandRecurrences mismatch"
    );
}

/// Oracle: draft-ietf-jmap-calendars-26 §5.11 / RFC 8620 §5.5 — when
/// no filter, sort, position, or limit is supplied, those wire keys
/// MUST be omitted; only `accountId` remains.
#[tokio::test]
async fn calendar_event_query_no_args_omits_optional_keys() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEvent/query",
            {
                "accountId": "A13824",
                "queryState": "qs0",
                "canCalculateChanges": false,
                "position": 0,
                "ids": [],
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
    let _ = sc
        .calendar_event_query(None, None, None, None, None)
        .await
        .expect("calendar_event_query: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    for key in ["filter", "sort", "position", "limit", "expandRecurrences"] {
        assert!(
            args.get(key).is_none(),
            "{key} must be absent when caller passes None"
        );
    }
    assert_eq!(args["accountId"], json!("A13824"), "accountId mismatch");
}

/// Oracle: draft-ietf-jmap-calendars-26 §5.12 / RFC 8620 §5.6 —
/// `CalendarEvent/queryChanges` requires `sinceQueryState`. The response
/// reports ids `removed` and `added` (each `added` entry has `id` and
/// `index`).
#[tokio::test]
async fn calendar_event_query_changes_since_state_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEvent/queryChanges",
            {
                "accountId": "A13824",
                "oldQueryState": "qs5",
                "newQueryState": "qs6",
                "total": null,
                "removed": ["ev-old"],
                "added": [
                    {"id": "ev-new", "index": 0}
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

    let sc = helpers::make_client(&server);
    let since = State::from("qs5");
    let resp = sc
        .calendar_event_query_changes(&since, Some(25), None, None, None, None)
        .await
        .expect("calendar_event_query_changes: must succeed");
    assert_eq!(resp.old_query_state, "qs5", "oldQueryState mismatch");
    assert_eq!(resp.new_query_state, "qs6", "newQueryState mismatch");
    assert_eq!(resp.removed.len(), 1, "one removed expected");
    assert_eq!(resp.added.len(), 1, "one added expected");
    assert_eq!(resp.added[0].id.as_ref(), "ev-new", "added id mismatch");
    assert_eq!(resp.added[0].index, 0, "added index mismatch");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["sinceQueryState"],
        json!("qs5"),
        "sinceQueryState mismatch"
    );
    assert_eq!(args["maxChanges"], json!(25), "maxChanges mismatch");
}

/// Oracle: draft-ietf-jmap-calendars-26 §5.10 / RFC 8620 §5.4 —
/// `CalendarEvent/copy` cross-account copy. The wire shape carries
/// `fromAccountId`, `accountId` (the destination), and a `create` map
/// keyed by caller-supplied creation ids. Each value in `create` is a
/// `CalendarEvent` carrying the source `id`.
#[tokio::test]
async fn calendar_event_copy_success_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEvent/copy",
            {
                "accountId": "A13824",
                "oldState": null,
                "newState": "s50",
                "created": {
                    "newCopy": {
                        "id": "ev-dest",
                        "calendarIds": { "cal-1": true },
                        "isDraft": false,
                        "isOrigin": false
                    }
                },
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
    let event: jmap_calendars_types::CalendarEvent = serde_json::from_value(json!({
        "id": "ev-src",
        "calendarIds": { "cal-1": true },
    }))
    .expect("CalendarEvent must deserialize from minimal fixture");
    let mut create = HashMap::new();
    create.insert("newCopy".to_owned(), event);

    let from_account = Id::from("src-acc");
    let resp = sc
        .calendar_event_copy(&from_account, create)
        .await
        .expect("calendar_event_copy: must succeed");
    let created = resp.created.expect("created must be present");
    assert!(
        created.contains_key("newCopy"),
        "created must contain 'newCopy' key"
    );
    assert_eq!(
        created["newCopy"]
            .id
            .as_ref()
            .expect("server id must be present")
            .as_ref(),
        "ev-dest",
        "destination id mismatch"
    );

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["fromAccountId"],
        json!("src-acc"),
        "fromAccountId mismatch"
    );
    assert_eq!(
        args["accountId"],
        json!("A13824"),
        "destination accountId mismatch"
    );
    let create_map = args["create"]
        .as_object()
        .expect("create must be a JSON object");
    assert!(
        create_map.contains_key("newCopy"),
        "create map must contain 'newCopy' key"
    );
    assert_eq!(
        create_map["newCopy"]["id"],
        json!("ev-src"),
        "source id must appear inside create map"
    );
}
