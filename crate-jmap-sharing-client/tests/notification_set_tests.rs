//! Wiremock integration tests for ShareNotification/set (destroy-only).
//!
//! Oracle for response shapes: RFC 9670 §3.3 (set, destroy-only).
//! Oracle for JMAP batch response envelope: RFC 8620 §3.4.
//!
//! RFC 9670 §3.3 MUST: "A client MUST NOT attempt to create or update
//! ShareNotification objects." The implementation accepts only destroy to
//! enforce this at the type level.

#[path = "common/mod.rs"]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test 1: ShareNotification/set destroy-only — wire request must not contain
/// "create" or "update" keys; "destroy" must be present.
///
/// Oracle: RFC 9670 §3.3 — ShareNotification/set is destroy-only.
/// The wire request MUST have "destroy" and MUST NOT have "create" or "update".
#[tokio::test]
async fn share_notification_set_destroy_only_wire() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ShareNotification/set",
            {
                "accountId": "u33084183",
                "oldState": "n-s1",
                "newState": "n-s2",
                "created": null,
                "updated": null,
                "destroyed": ["notif-1"],
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

    let sc = common::make_client(&server).await;
    let resp = sc
        .share_notification_set(Some(vec!["notif-1"]))
        .await
        .expect("share_notification_set_destroy_only_wire: must succeed");

    let destroyed = resp
        .destroyed
        .as_ref()
        .expect("share_notification_set_destroy_only_wire: destroyed must be Some");
    assert!(
        destroyed.iter().any(|id| id.as_ref() == "notif-1"),
        "destroyed must contain notif-1"
    );

    // Inspect the wire request: "destroy" must be present; "create" and "update" must be absent.
    let reqs = server
        .received_requests()
        .await
        .expect("share_notification_set_destroy_only_wire: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("share_notification_set_destroy_only_wire: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    // destroy must be present and contain the expected ID.
    let destroy_arr = args["destroy"]
        .as_array()
        .expect("share_notification_set_destroy_only_wire: destroy must be an array");
    assert!(
        destroy_arr.contains(&json!("notif-1")),
        "destroy array must contain notif-1"
    );

    // RFC 9670 §3.3: create and update MUST NOT be present in a destroy-only request.
    assert!(
        args.get("create").is_none(),
        "create must not be present in ShareNotification/set request per RFC 9670 §3.3: {args}"
    );
    assert!(
        args.get("update").is_none(),
        "update must not be present in ShareNotification/set request per RFC 9670 §3.3: {args}"
    );
}

/// Test 2: ShareNotification/set with destroy=None sends empty destroy array.
///
/// Oracle: RFC 9670 §3.3 — The implementation converts None to an empty
/// destroy array rather than omitting the key. An empty destroy is a no-op
/// that must succeed without error.
#[tokio::test]
async fn share_notification_set_empty_destroy() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "ShareNotification/set",
            {
                "accountId": "u33084183",
                "oldState": "n-s1",
                "newState": "n-s1",
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

    let sc = common::make_client(&server).await;
    let resp = sc
        .share_notification_set(None)
        .await
        .expect("share_notification_set_empty_destroy: must succeed with no-op destroy");

    // Old and new state are equal — no changes occurred.
    assert_eq!(resp.new_state, "n-s1", "newState mismatch");
    assert!(
        resp.destroyed.is_none(),
        "destroyed must be None when nothing was destroyed"
    );

    // Inspect the wire request: destroy must be an empty array, not absent.
    let reqs = server
        .received_requests()
        .await
        .expect("share_notification_set_empty_destroy: must have recorded requests");
    assert_eq!(reqs.len(), 1, "must have received exactly one request");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body)
        .expect("share_notification_set_empty_destroy: request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    let destroy_arr = args["destroy"].as_array().expect(
        "share_notification_set_empty_destroy: destroy must be an array even when None passed",
    );
    assert!(
        destroy_arr.is_empty(),
        "destroy array must be empty when None is passed"
    );
}
