//! Wiremock integration tests for TaskList/* client methods.
//!
//! Each test verifies both the round-trip response parsing and the wire
//! request shape produced by the client.
//!
//! Oracle: draft-ietf-jmap-tasks-06 §3 (TaskList object), RFC 8620 §5 (method shapes).

#[path = "helpers.rs"]
mod helpers;

use jmap_types::{Id, State};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Test 1: TaskList/get — round-trip and wire request shape
// ---------------------------------------------------------------------------

/// TaskList/get with ids=None sends null for ids and returns a populated list.
///
/// Oracle: draft-tasks-06 §3.5 — ids=null means "all TaskLists for the account".
/// Response shape: RFC 8620 §5.1 GetResponse.
#[tokio::test]
async fn task_list_get_sends_correct_wire_request() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "TaskList/get",
            {
                "accountId": "A13824",
                "state": "s5",
                "list": [{
                    "id": "list1",
                    "name": "My Tasks",
                    "sortOrder": 0,
                    "isSubscribed": true,
                    "myRights": {
                        "mayReadItems": true,
                        "mayWriteAll": false,
                        "mayWriteOwn": false,
                        "mayUpdatePrivate": false,
                        "mayRSVP": false,
                        "mayAdmin": false,
                        "mayDelete": false
                    }
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

    let sc = helpers::make_client(&server);
    let resp = sc
        .task_list_get(None, None)
        .await
        .expect("task_list_get_sends_correct_wire_request: must succeed");

    // Response assertions.
    assert_eq!(resp.list.len(), 1, "list must contain one TaskList");
    assert_eq!(
        resp.list[0].id.as_ref(),
        "list1",
        "list[0].id must be 'list1'"
    );
    assert_eq!(
        resp.list[0].name, "My Tasks",
        "list[0].name must be 'My Tasks'"
    );

    // Wire request assertions.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let calls = body["methodCalls"]
        .as_array()
        .expect("methodCalls must be an array");
    assert_eq!(
        calls[0][0],
        json!("TaskList/get"),
        "method name must be 'TaskList/get'"
    );
    assert!(
        calls[0][1]["ids"].is_null(),
        "ids must be null when None is passed: {}",
        calls[0][1]
    );
}

// ---------------------------------------------------------------------------
// Test 2: TaskList/changes — sinceState and maxChanges on the wire
// ---------------------------------------------------------------------------

/// TaskList/changes sends sinceState and maxChanges and parses the response correctly.
///
/// Oracle: RFC 8620 §5.2 — /changes request must include sinceState; maxChanges is optional.
#[tokio::test]
async fn task_list_changes_sends_since_state() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "TaskList/changes",
            {
                "accountId": "A13824",
                "oldState": "state-1",
                "newState": "state-2",
                "hasMoreChanges": false,
                "created": ["list2"],
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
    let resp = sc
        .task_list_changes(&State::from("state-1"), Some(50))
        .await
        .expect("task_list_changes_sends_since_state: must succeed");

    // Response assertions.
    assert_eq!(resp.old_state, "state-1", "oldState must round-trip");
    assert_eq!(resp.new_state, "state-2", "newState must be 'state-2'");
    assert!(!resp.has_more_changes, "hasMoreChanges must be false");
    assert!(
        resp.created.iter().any(|id| id.as_ref() == "list2"),
        "created must contain 'list2'"
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
        args["sinceState"],
        json!("state-1"),
        "sinceState must be 'state-1' on the wire"
    );
    assert_eq!(
        args["maxChanges"],
        json!(50),
        "maxChanges must be 50 on the wire"
    );
}

// ---------------------------------------------------------------------------
// Test 3: TaskList/set — onDestroyRemoveTasks round-trip
// ---------------------------------------------------------------------------

/// TaskList/set with destroy and onDestroyRemoveTasks=true sends both fields and
/// parses the destroyed list correctly.
///
/// Oracle: draft-tasks-06 §3.3 — onDestroyRemoveTasks controls task cascade on destroy.
#[tokio::test]
async fn task_list_set_on_destroy_remove_tasks_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "TaskList/set",
            {
                "accountId": "A13824",
                "oldState": "s5",
                "newState": "s6",
                "created": null,
                "updated": null,
                "destroyed": ["list1"],
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
    let resp = sc
        .task_list_set(None, None, Some(vec![Id::from("list1")]), Some(true))
        .await
        .expect("task_list_set_on_destroy_remove_tasks_round_trip: must succeed");

    // Response assertions.
    assert_eq!(resp.new_state, "s6", "newState must be 's6'");
    let destroyed = resp
        .destroyed
        .as_deref()
        .expect("destroyed must be Some when a list was destroyed");
    assert!(
        destroyed.iter().any(|id| id.as_ref() == "list1"),
        "destroyed must contain 'list1'"
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
        args["onDestroyRemoveTasks"],
        json!(true),
        "onDestroyRemoveTasks must be true on the wire"
    );
    assert_eq!(
        args["destroy"][0],
        json!("list1"),
        "destroy[0] must be 'list1' on the wire"
    );
}

// ---------------------------------------------------------------------------
// Test 4: TaskList/set — onDestroyRemoveTasks absent when None
// ---------------------------------------------------------------------------

/// TaskList/set with on_destroy_remove_tasks=None omits the key entirely from the wire.
///
/// Oracle: RFC 8620 §5.3 — optional method arguments MUST be absent when not requested,
/// not present as null.
#[tokio::test]
async fn task_list_set_without_on_destroy_omits_field() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "TaskList/set",
            {
                "accountId": "A13824",
                "oldState": null,
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
    // Need at least one of create/update/destroy to be Some — the
    // all-None /set is rejected client-side by the defence-in-depth
    // guard (bd:JMAP-tjvm.24). An empty destroy Vec is the smallest
    // valid input; the test's actual oracle (onDestroyRemoveTasks must
    // be omitted from the wire when the caller passes None) is
    // preserved.
    let destroy_ids: Vec<jmap_types::Id> = vec![];
    sc.task_list_set(None, None, Some(destroy_ids), None)
        .await
        .expect("task_list_set_without_on_destroy_omits_field: must succeed");

    // Wire request assertions.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert!(
        args.get("onDestroyRemoveTasks").is_none(),
        "onDestroyRemoveTasks must be absent from the wire when None is passed: {args}"
    );
}
