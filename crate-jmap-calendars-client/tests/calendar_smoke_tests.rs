//! Wiremock smoke tests for Calendar/get, CalendarEvent/set, CalendarEvent/parse.
//!
//! Oracle for response shapes:
//!   - Calendar/get: draft-ietf-jmap-calendars-26 §4.1 and RFC 8620 §5.1
//!   - CalendarEvent/set: draft-ietf-jmap-calendars-26 §5.6 and RFC 8620 §5.3
//!   - CalendarEvent/parse: draft-ietf-jmap-calendars-26 §5.13
//!     Oracle for JMAP batch response envelope: RFC 8620 §3.4.

#[path = "helpers.rs"]
mod helpers;

use jmap_types::Id;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test JMAP-7i4v.4 #1 — Calendar/get smoke: mock returns 1 Calendar; accountId correct.
///
/// Oracle: draft-ietf-jmap-calendars-26 §4.1 — Calendar object fields.
/// RFC 8620 §5.1 — /get response shape.
#[tokio::test]
async fn calendar_get_smoke() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Calendar/get",
            {
                "accountId": "A13824",
                "state": "s3",
                "list": [
                    {
                        "id": "cal-1",
                        "name": "Personal",
                        "description": null,
                        "color": "#4a90d9",
                        "sortOrder": 0,
                        "isSubscribed": true,
                        "isVisible": true,
                        "isDefault": true,
                        "includeInAvailability": "all",
                        "defaultAlertsWithTime": null,
                        "defaultAlertsWithoutTime": null,
                        "timeZone": "America/New_York",
                        "shareWith": null,
                        "myRights": {
                            "mayReadFreeBusy": true,
                            "mayReadItems": true,
                            "mayWriteAll": true,
                            "mayWriteOwn": true,
                            "mayUpdatePrivate": true,
                            "mayRSVP": true,
                            "mayShare": false,
                            "mayDelete": false
                        }
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

    let sc = helpers::make_client(&server);
    let resp = sc
        .calendar_get(None, None)
        .await
        .expect("calendar_get_smoke: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    assert_eq!(resp.state, "s3", "state mismatch");
    assert_eq!(resp.list.len(), 1, "list must have 1 calendar");
    assert_eq!(
        resp.list[0]
            .id
            .as_ref()
            .expect("calendar id must be present in /get response")
            .as_ref(),
        "cal-1",
        "calendar id mismatch"
    );
    assert_eq!(resp.list[0].name, "Personal", "name mismatch");
    assert!(resp.list[0].is_default, "isDefault must be true");
    assert!(
        resp.list[0].my_rights.may_read_items,
        "mayReadItems must be true"
    );
}

/// Test JMAP-7i4v.4 #2 — CalendarEvent/set smoke: create returns server-assigned id.
///
/// Oracle: draft-ietf-jmap-calendars-26 §5.6 — /set create response.
/// RFC 8620 §5.3 — created map keyed by caller-supplied creation key.
#[tokio::test]
async fn calendar_event_set_smoke() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEvent/set",
            {
                "accountId": "A13824",
                "oldState": "s1",
                "newState": "s2",
                "created": {
                    "newEv": {
                        "id": "server-ev-id",
                        "calendarIds": { "cal-1": true },
                        "isDraft": false,
                        "isOrigin": true
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
    // CalendarEvent has all-Option fields, so a sparse JSON deserialization
    // suffices to construct a typed event for the create map.
    let event: jmap_calendars_types::CalendarEvent = serde_json::from_value(json!({
        "calendarIds": { "cal-1": true },
        "title": "Team standup",
        "start": "2024-06-15T09:00:00",
        "duration": "PT30M",
        "timeZone": "America/New_York",
        "showWithoutTime": false
    }))
    .expect("CalendarEvent must deserialize from spec example fields");
    let mut create_map = std::collections::HashMap::new();
    create_map.insert("newEv".to_owned(), event);
    let resp = sc
        .calendar_event_set(Some(create_map), None, None, None)
        .await
        .expect("calendar_event_set_smoke: must succeed");

    assert_eq!(resp.new_state, "s2", "newState mismatch");
    let created = resp.created.expect("created must be present");
    assert!(
        created.contains_key("newEv"),
        "created must contain 'newEv' key"
    );
    let ev_id = created["newEv"]
        .id
        .as_ref()
        .expect("server id must be present");
    assert_eq!(
        ev_id.as_ref(),
        "server-ev-id",
        "server-assigned id mismatch"
    );

    // RFC 8620 §5.3: id MUST NOT be set by the client on creation. The
    // server-side handler will reject any present id with invalidProperties.
    // Pinning the wire shape here so a regression that re-adds id to the
    // create-map (e.g. by making CalendarEvent.id required again) is
    // caught immediately rather than at deploy time against a strict server.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    let create = args["create"]
        .as_object()
        .expect("create must be a JSON object");
    assert!(
        create["newEv"].get("id").is_none(),
        "id must not be set by client on creation per RFC 8620 §5.3, got: {}",
        create["newEv"]
    );
}

/// Test JMAP-7i4v.4 #3 — CalendarEvent/parse smoke: notParsable returned for bad blob.
///
/// Oracle: draft-ietf-jmap-calendars-26 §5.13 — notParsable lists blob ids that
/// could not be parsed as iCalendar data.
#[tokio::test]
async fn calendar_event_parse_smoke() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "CalendarEvent/parse",
            {
                "accountId": "A13824",
                "parsed": null,
                "notFound": null,
                "notParsable": ["blob1"]
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
    let blob_ids = [Id::from("blob1")];
    let resp = sc
        .calendar_event_parse(&blob_ids, None)
        .await
        .expect("calendar_event_parse_smoke: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    let not_parsable = resp.not_parsable.expect("notParsable must be present");
    assert!(
        not_parsable.iter().any(|id| id.as_ref() == "blob1"),
        "notParsable must contain 'blob1'"
    );
}

/// Oracle: §5.10 / RFC 8620 §5.4 — `create` keys are caller-chosen
/// creation ids. Empty creation ids would produce a malformed wire request,
/// so calendar_event_copy MUST reject them client-side with InvalidArgument
/// BEFORE making any HTTP call. The mock server has no expectations, so any
/// HTTP request would result in a 404 from wiremock (which would still
/// surface as a different error, not InvalidArgument).
#[tokio::test]
async fn calendar_event_copy_empty_creation_id_returns_invalid_argument() {
    let server = MockServer::start().await;
    // Deliberately register no mock expectations. If the guard fails to fire,
    // the call reaches wiremock and the test will fail with the wrong error.

    let sc = helpers::make_client(&server);

    let event: jmap_calendars_types::CalendarEvent = serde_json::from_value(json!({
        "id": "src-event-id",
    }))
    .expect("CalendarEvent must deserialize from minimal fixture");
    let mut create = std::collections::HashMap::new();
    create.insert(String::new(), event); // empty creation id

    let from_account = Id::from("src_acc");
    let result = sc.calendar_event_copy(&from_account, create).await;
    let err = result.expect_err("calendar_event_copy with empty creation id must error");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("creation id"),
                "error message must mention 'creation id': {msg}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

// The following tests were deleted in JMAP-6by7.1 (typed-Id refactor):
//
//   - calendar_get_empty_id_returns_invalid_argument
//   - calendar_event_get_empty_id_returns_invalid_argument
//   - calendar_event_notification_get_empty_id_returns_invalid_argument
//   - calendar_event_copy_empty_from_account_returns_invalid_argument
//
// They asserted that the `validate_id_field` / `validate_ids_field`
// helpers rejected `""` passed as a `&str` / `&[&str]` argument. Once the
// method signatures changed to `&Id` / `&[Id]`, the tests would have to
// construct an `Id` whose internal value is `""`. `Id::new_validated("")`
// returns `Err` at the test's input-construction site, so the tests
// would fail before reaching the production code being tested. The
// callers *could* go through `Id::from("")` (which doesn't validate),
// but at that point they're explicitly bypassing the type-system
// guarantee and the client crate has no contract to second-guess them.
// The bug becomes impossible to express through the typed API, so the
// tests are unnecessary.
//
// The `calendar_event_copy_empty_creation_id_returns_invalid_argument`
// test above is preserved because creation-reference keys in the `create`
// map remain `String`, not `Id` — the empty-key guard is still
// meaningful and still has a real test path.

// ---------------------------------------------------------------------------
// Wire-passthrough smoke tests (JMAP-uuoi.1)
// ---------------------------------------------------------------------------
//
// The tests below assert on the captured request body to verify that
// production calendar_set / calendar_changes builders thread caller
// arguments to the wire correctly.

/// Oracle: draft-ietf-jmap-calendars-26 §4.4 — `onDestroyRemoveEvents`
/// is a `Calendar/set`-specific extra argument. When `true`, destroying
/// a calendar also destroys all its events; when `false` (the default),
/// the server MUST reject the destroy with a `calendarHasEvent`
/// SetError if any events remain.
///
/// Verifies that production `calendar_set` threads the flag to the wire
/// verbatim.
#[tokio::test]
async fn calendar_set_on_destroy_remove_events_true_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Calendar/set",
            {
                "accountId": "A13824",
                "oldState": "s1",
                "newState": "s2",
                "created": null,
                "updated": null,
                "destroyed": ["cal-doomed"],
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
    let destroy_ids = [Id::from("cal-doomed")];
    let resp = sc
        .calendar_set(None, None, Some(&destroy_ids), Some(true), None)
        .await
        .expect("calendar_set: must succeed");
    assert_eq!(
        resp.destroyed.as_deref(),
        Some(&[Id::from("cal-doomed")][..]),
        "destroyed must contain the calendar id"
    );

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["onDestroyRemoveEvents"],
        json!(true),
        "onDestroyRemoveEvents must be present and true on the wire"
    );
    assert_eq!(
        args["destroy"],
        json!(["cal-doomed"]),
        "destroy array must thread through unchanged"
    );
    assert_eq!(args["accountId"], json!("A13824"), "accountId mismatch");
    assert!(
        args.get("create").is_none(),
        "create must be absent when None passed"
    );
    assert!(
        args.get("update").is_none(),
        "update must be absent when None passed"
    );
}

/// Oracle: draft-ietf-jmap-calendars-26 §4.4 + RFC 8620 §5.3 —
/// `onDestroyRemoveEvents` MUST be absent from the wire request when the
/// caller passes `None`. Servers default the value to `false` in that
/// case; an explicit `false` on the wire is functionally equivalent but
/// is a different shape, and the production code chooses to omit the
/// field entirely to match the broader "omit-when-None" idiom.
#[tokio::test]
async fn calendar_set_on_destroy_remove_events_none_omits_field() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Calendar/set",
            {
                "accountId": "A13824",
                "oldState": "s1",
                "newState": "s1",
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
        .calendar_set(None, None, None, None, None)
        .await
        .expect("calendar_set: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert!(
        args.get("onDestroyRemoveEvents").is_none(),
        "onDestroyRemoveEvents must be omitted when caller passes None"
    );
}

/// Oracle: draft-ietf-jmap-calendars-26 §4.2 / RFC 8620 §5.2 —
/// `Calendar/changes` carries `sinceState` (the caller-supplied state
/// token) and an optional `maxChanges`.
///
/// Verifies that both flow through to the wire and that the response is
/// parsed back into a typed [`ChangesResponse`].
#[tokio::test]
async fn calendar_changes_since_state_and_max_changes_passthrough() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Calendar/changes",
            {
                "accountId": "A13824",
                "oldState": "s10",
                "newState": "s12",
                "hasMoreChanges": false,
                "created": ["cal-new"],
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
    let since = jmap_types::State::from("s10");
    let resp = sc
        .calendar_changes(&since, Some(50))
        .await
        .expect("calendar_changes: must succeed");
    assert_eq!(resp.old_state, "s10", "oldState mismatch");
    assert_eq!(resp.new_state, "s12", "newState mismatch");
    assert!(!resp.has_more_changes, "hasMoreChanges must be false");
    assert_eq!(
        resp.created,
        vec![Id::from("cal-new")],
        "created list mismatch"
    );

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["sinceState"],
        json!("s10"),
        "sinceState must thread through verbatim"
    );
    assert_eq!(
        args["maxChanges"],
        json!(50),
        "maxChanges must thread through verbatim"
    );
}
