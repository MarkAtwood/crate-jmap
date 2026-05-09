//! Wiremock integration tests for TaskNotification/* client methods.
//!
//! Each test verifies both the round-trip response parsing and the wire
//! request shape produced by the client.
//!
//! Oracle: draft-ietf-jmap-tasks-06 §5 (TaskNotification methods),
//!         RFC 8620 §5 (method shapes).

#[path = "helpers.rs"]
mod helpers;

use jmap_types::{Id, State};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Test 1: TaskNotification/get — round-trip with a real notification
// ---------------------------------------------------------------------------

/// TaskNotification/get returns a populated list with a correctly deserialised
/// TaskNotification object.
///
/// Oracle: draft-tasks-06 §5.1 — TaskNotification fields: id, created,
/// changedBy (Person), type, taskId.
#[tokio::test]
async fn task_notification_get_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "TaskNotification/get",
            {
                "accountId": "A13824",
                "state": "sn1",
                "list": [{
                    "id": "notif1",
                    "created": "2026-01-15T10:00:00Z",
                    "changedBy": {
                        "@type": "Person",
                        "name": "Alice"
                    },
                    "type": "created",
                    "taskId": "task1"
                }],
                "notFound": null
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
        .task_notification_get(Some(&[Id::from("notif1")]), None)
        .await
        .expect("task_notification_get_round_trip: must succeed");

    // Response assertions.
    assert_eq!(resp.list.len(), 1, "list must contain one TaskNotification");
    assert_eq!(
        resp.list[0].id.as_ref(),
        "notif1",
        "list[0].id must be 'notif1'"
    );
    assert_eq!(
        resp.list[0].task_id.as_ref(),
        "task1",
        "list[0].task_id must be 'task1'"
    );
    assert_eq!(
        resp.list[0].created, "2026-01-15T10:00:00Z",
        "list[0].created must round-trip"
    );
    assert_eq!(
        resp.list[0].changed_by.name.as_deref(),
        Some("Alice"),
        "list[0].changed_by.name must be 'Alice'"
    );

    // Wire request assertions.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["ids"],
        json!(["notif1"]),
        "ids must be [\"notif1\"] on the wire"
    );
}

// ---------------------------------------------------------------------------
// Test 2: TaskNotification/changes — destroyed round-trip, maxChanges absent
// ---------------------------------------------------------------------------

/// TaskNotification/changes with maxChanges=None omits maxChanges from the wire
/// and returns a destroyed list correctly.
///
/// Oracle: RFC 8620 §5.2 — maxChanges is optional; absent when not requested.
#[tokio::test]
async fn task_notification_changes_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "TaskNotification/changes",
            {
                "accountId": "A13824",
                "oldState": "sn1",
                "newState": "sn2",
                "hasMoreChanges": false,
                "created": [],
                "updated": [],
                "destroyed": ["notif-old"]
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
        .task_notification_changes(&State::from("sn1"), None)
        .await
        .expect("task_notification_changes_round_trip: must succeed");

    // Response assertions.
    assert!(
        resp.destroyed.iter().any(|id| id.as_ref() == "notif-old"),
        "destroyed must contain 'notif-old'"
    );

    // Wire request assertions — maxChanges must be absent when None.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert!(
        args.get("maxChanges").is_none(),
        "maxChanges must be absent from the wire when None is passed: {args}"
    );
    assert_eq!(
        args["sinceState"],
        json!("sn1"),
        "sinceState must be 'sn1' on the wire"
    );
}

// ---------------------------------------------------------------------------
// Test 3: TaskNotification/set — destroy-only wire format (key correctness test)
// ---------------------------------------------------------------------------

/// TaskNotification/set sends destroy=[...] and no create or update keys.
///
/// Oracle: draft-tasks-06 §5.4 — TaskNotification/set is destroy-only.
/// The server rejects any create or update with `forbidden`; the client must
/// never send those keys.
#[tokio::test]
async fn task_notification_set_destroy_only_wire_format() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "TaskNotification/set",
            {
                "accountId": "A13824",
                "oldState": "sn1",
                "newState": "sn2",
                "created": null,
                "updated": null,
                "destroyed": ["notif1", "notif2"],
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
    sc.task_notification_set(vec![Id::from("notif1"), Id::from("notif2")])
        .await
        .expect("task_notification_set_destroy_only_wire_format: must succeed");

    // Wire request assertions — destroy present, create and update absent.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    let destroy = args["destroy"]
        .as_array()
        .expect("destroy must be an array on the wire");
    assert_eq!(destroy.len(), 2, "destroy must contain 2 entries");
    assert!(
        destroy.contains(&json!("notif1")),
        "destroy must contain 'notif1'"
    );
    assert!(
        destroy.contains(&json!("notif2")),
        "destroy must contain 'notif2'"
    );

    assert!(
        args.get("create").is_none() || args["create"].is_null(),
        "destroy-only set must not send 'create' key: {args}"
    );
    assert!(
        args.get("update").is_none() || args["update"].is_null(),
        "destroy-only set must not send 'update' key: {args}"
    );
}

// ---------------------------------------------------------------------------
// Test 4: TaskNotification/set — empty destroy list succeeds
// ---------------------------------------------------------------------------

/// TaskNotification/set with an empty destroy list sends destroy=[] on the wire.
///
/// Oracle: draft-tasks-06 §5.4 — an empty destroy is valid; the server returns
/// an empty /set response.
#[tokio::test]
async fn task_notification_set_empty_destroy_succeeds() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "TaskNotification/set",
            {
                "accountId": "A13824",
                "oldState": "sn1",
                "newState": "sn1",
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

    let sc = helpers::make_client(&server).await;
    sc.task_notification_set(vec![])
        .await
        .expect("task_notification_set_empty_destroy_succeeds: must succeed");

    // Wire request assertions.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];

    let destroy = args["destroy"]
        .as_array()
        .expect("destroy must be an array on the wire even when empty");
    assert!(destroy.is_empty(), "destroy must be [] on the wire");

    assert!(
        args.get("create").is_none() || args["create"].is_null(),
        "create must be absent from the wire: {args}"
    );
    assert!(
        args.get("update").is_none() || args["update"].is_null(),
        "update must be absent from the wire: {args}"
    );
}

// ---------------------------------------------------------------------------
// Test 5: TaskNotification/query — filter and sort on the wire
// ---------------------------------------------------------------------------

/// TaskNotification/query sends filter and sort correctly on the wire.
///
/// Oracle: draft-tasks-06 §5.5 — filter and sort are optional; when present
/// they are passed verbatim to the server.
#[tokio::test]
async fn task_notification_query_with_filter() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "TaskNotification/query",
            {
                "accountId": "A13824",
                "queryState": "qs1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": ["notif1", "notif2"],
                "total": 2
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
        .task_notification_query(
            Some(json!({"type": "created", "taskIds": ["task1"]})),
            Some(json!([{"property": "created"}])),
            None,
            None,
        )
        .await
        .expect("task_notification_query_with_filter: must succeed");

    // Response assertions.
    assert_eq!(resp.ids.len(), 2, "ids must contain 2 entries");

    // Wire request assertions.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filter"]["type"],
        json!("created"),
        "filter.type must be 'created' on the wire"
    );
    assert_eq!(
        args["sort"][0]["property"],
        json!("created"),
        "sort[0].property must be 'created' on the wire"
    );
}

// ---------------------------------------------------------------------------
// Test 6: TaskNotification/queryChanges — sinceQueryState and maxChanges
// ---------------------------------------------------------------------------

/// TaskNotification/queryChanges sends sinceQueryState and maxChanges correctly.
///
/// Oracle: RFC 8620 §5.6 — sinceQueryState is required; maxChanges is optional.
#[tokio::test]
async fn task_notification_query_changes_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "TaskNotification/queryChanges",
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

    let sc = helpers::make_client(&server).await;
    sc.task_notification_query_changes(&State::from("qs1"), Some(20))
        .await
        .expect("task_notification_query_changes_round_trip: must succeed");

    // Wire request assertions.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["sinceQueryState"],
        json!("qs1"),
        "sinceQueryState must be 'qs1' on the wire"
    );
    assert_eq!(
        args["maxChanges"],
        json!(20),
        "maxChanges must be 20 on the wire"
    );
}
