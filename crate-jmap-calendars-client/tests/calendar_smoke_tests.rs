//! Wiremock smoke tests for Calendar/get, CalendarEvent/set, CalendarEvent/parse.
//!
//! Oracle for response shapes:
//!   - Calendar/get: draft-ietf-jmap-calendars-26 §4.1 and RFC 8620 §5.1
//!   - CalendarEvent/set: draft-ietf-jmap-calendars-26 §5.6 and RFC 8620 §5.3
//!   - CalendarEvent/parse: draft-ietf-jmap-calendars-26 §5.13
//!     Oracle for JMAP batch response envelope: RFC 8620 §3.4.

#[path = "helpers.rs"]
mod helpers;

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

    let sc = helpers::make_client(&server).await;
    let resp = sc
        .calendar_get(None, None)
        .await
        .expect("calendar_get_smoke: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
    assert_eq!(resp.state, "s3", "state mismatch");
    assert_eq!(resp.list.len(), 1, "list must have 1 calendar");
    assert_eq!(resp.list[0].id.as_ref(), "cal-1", "calendar id mismatch");
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

    let sc = helpers::make_client(&server).await;
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
        .calendar_event_set(Some(create_map), None, None)
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

    let sc = helpers::make_client(&server).await;
    let resp = sc
        .calendar_event_parse(&["blob1"], None)
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

    let sc = helpers::make_client(&server).await;

    let event: jmap_calendars_types::CalendarEvent = serde_json::from_value(json!({
        "id": "src-event-id",
    }))
    .expect("CalendarEvent must deserialize from minimal fixture");
    let mut create = std::collections::HashMap::new();
    create.insert(String::new(), event); // empty creation id

    let result = sc.calendar_event_copy("src_acc", create).await;
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

/// Oracle: empty `ids` slice element MUST be rejected with
/// `InvalidArgument` BEFORE any HTTP call. The mock server has no
/// expectations registered, so any HTTP request would result in a 404 from
/// wiremock — the test would still fail (different error), but with a
/// distinct symptom from the validation-fired path.
///
/// Replaces a vacuous inline unit test that re-asserted `"".is_empty()`
/// without actually invoking the public method (JMAP-231o.7).
#[tokio::test]
async fn calendar_get_empty_id_returns_invalid_argument() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server).await;

    let result = sc.calendar_get(Some(&[""]), None).await;
    let err = result.expect_err("calendar_get with empty id must error");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("ids element"),
                "error message must mention 'ids element': {msg}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// Oracle: parallel to calendar_get — `calendar_event_get` MUST reject an
/// empty `ids` slice element with `InvalidArgument` before any HTTP call.
/// Replaces a vacuous inline unit test (JMAP-231o.7).
#[tokio::test]
async fn calendar_event_get_empty_id_returns_invalid_argument() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server).await;

    let result = sc.calendar_event_get(Some(&[""]), None, None).await;
    let err = result.expect_err("calendar_event_get with empty id must error");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("ids element"),
                "error message must mention 'ids element': {msg}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// Oracle: parallel to calendar_get — `calendar_event_notification_get`
/// MUST reject an empty `ids` slice element with `InvalidArgument` before
/// any HTTP call. Replaces a vacuous inline unit test (JMAP-231o.7).
#[tokio::test]
async fn calendar_event_notification_get_empty_id_returns_invalid_argument() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server).await;

    let result = sc.calendar_event_notification_get(Some(&[""]), None).await;
    let err = result.expect_err("calendar_event_notification_get with empty id must error");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("ids element"),
                "error message must mention 'ids element': {msg}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// Oracle: §5.10 — `from_account_id` empty guard is exercised end-to-end.
/// The check is the same shape as the creation-id guard but on a different
/// field; pinning both ensures the validation chain is wired correctly.
#[tokio::test]
async fn calendar_event_copy_empty_from_account_returns_invalid_argument() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server).await;

    let event: jmap_calendars_types::CalendarEvent = serde_json::from_value(json!({
        "id": "src-event-id",
    }))
    .expect("CalendarEvent must deserialize");
    let mut create = std::collections::HashMap::new();
    create.insert("c1".to_owned(), event);

    let result = sc.calendar_event_copy("", create).await;
    let err = result.expect_err("empty from_account_id must error");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("from_account_id"),
                "error message must mention 'from_account_id': {msg}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
