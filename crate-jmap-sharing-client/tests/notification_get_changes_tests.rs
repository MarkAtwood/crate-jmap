//! Wiremock integration tests for ShareNotification/get and ShareNotification/changes.
//!
//! Oracle for response shapes: RFC 9670 §3.1 (get), §3.2 (changes).
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.
//! Oracle for fixture: tests/fixtures/sharing/share_notification_get_response.json
//!   (hand-written from RFC 9670 §3 ShareNotification field descriptions).

#[path = "common/mod.rs"]
mod common;

use jmap_types::State;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test 1: ShareNotification/get using the committed fixture file.
///
/// Oracle: RFC 9670 §3.1 — ids=null returns all notifications for the
/// primary account. Response must contain a list with at least 1 notification.
/// The fixture encodes a "Mailbox" share notification from Joe Bloggs.
#[tokio::test]
async fn share_notification_get_round_trip() {
    let server = MockServer::start().await;

    // Load the fixture that lives at tests/fixtures/sharing/share_notification_get_response.json.
    // The path is relative to the workspace root at cargo test time.
    let fixture_bytes =
        std::fs::read("tests/fixtures/sharing/share_notification_get_response.json")
            .expect("share_notification_get_round_trip: fixture file must exist");
    let resp_body: serde_json::Value = serde_json::from_slice(&fixture_bytes)
        .expect("share_notification_get_round_trip: fixture must be valid JSON");

    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = common::make_client(&server).await;
    let resp = sc
        .share_notification_get(None, None)
        .await
        .expect("share_notification_get_round_trip: must succeed");

    assert_eq!(resp.account_id.as_ref(), "u33084183", "accountId mismatch");
    assert_eq!(resp.list.len(), 1, "list must have 1 notification");

    let notif = &resp.list[0];
    assert_eq!(notif.id.as_ref(), "notif-1", "notification id mismatch");
    assert_eq!(
        notif.object_type, "Mailbox",
        "objectType must be Mailbox per fixture"
    );
    assert_eq!(
        notif.changed_by.name, "Joe Bloggs",
        "changedBy.name must be Joe Bloggs per fixture"
    );
}

/// Test 2: ShareNotification/changes sends sinceState in the request.
///
/// Oracle: RFC 9670 §3.2 / RFC 8620 §5.2 — sinceState is required;
/// maxChanges is optional and MUST be absent when None is passed.
#[tokio::test]
async fn share_notification_changes_sends_since_state() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ShareNotification/changes",
            {
                "accountId": "u33084183",
                "oldState": "n-s1",
                "newState": "n-s2",
                "hasMoreChanges": false,
                "created": ["notif-new"],
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

    let sc = common::make_client(&server).await;
    let resp = sc
        .share_notification_changes(&State::from("n-s1"), None)
        .await
        .expect("share_notification_changes_sends_since_state: must succeed");

    assert_eq!(resp.old_state, "n-s1", "oldState mismatch");
    assert_eq!(resp.new_state, "n-s2", "newState mismatch");
    assert!(!resp.has_more_changes, "hasMoreChanges must be false");

    // Inspect the wire request: sinceState must be sent, maxChanges must be absent.
    let reqs = server
        .received_requests()
        .await
        .expect("share_notification_changes_sends_since_state: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("share_notification_changes_sends_since_state: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["sinceState"],
        json!("n-s1"),
        "sinceState must be n-s1 in request"
    );
    assert!(
        args.get("maxChanges").is_none(),
        "maxChanges must be absent from request when None is passed: {args}"
    );
}
