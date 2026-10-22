//! Wiremock integration tests for Task/* client methods.
//!
//! Each test verifies both the round-trip response parsing and the wire
//! request shape produced by the client.
//!
//! Oracle: draft-ietf-jmap-tasks-06 §4 (Task methods), RFC 8620 §5 (method shapes).

#[path = "helpers.rs"]
mod helpers;

use jmap_tasks_types::task::{TaskFilter, TaskFilterCondition};
use jmap_tasks_types::TaskComparator;
use jmap_types::query::Filter;
use jmap_types::{Id, State, UTCDate};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Test 1: Task/get — ids and properties round-trip
// ---------------------------------------------------------------------------

/// Task/get with ids=["task1"] and properties=["id","title","isDraft"] sends
/// both fields on the wire and returns a partial Task with the correct values.
///
/// Oracle: draft-tasks-06 §4.5 — ids and properties are passed verbatim to the server.
/// Response shape: RFC 8620 §5.1 GetResponse.
#[tokio::test]
async fn task_get_sends_ids_and_properties() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Task/get",
            {
                "accountId": "A13824",
                "state": "s5",
                "list": [{
                    "id": "task1",
                    "title": "Write tests",
                    "isDraft": true
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
        .task_get(
            Some(&[Id::from("task1")]),
            Some(&["id", "title", "isDraft"]),
        )
        .await
        .expect("task_get_sends_ids_and_properties: must succeed");

    // Response assertions.
    assert_eq!(resp.list.len(), 1, "list must contain one Task");
    assert_eq!(
        resp.list[0].id.as_ref().map(|id| id.as_ref()),
        Some("task1"),
        "list[0].id must be 'task1'"
    );
    assert_eq!(
        resp.list[0].title.as_deref(),
        Some("Write tests"),
        "list[0].title must be 'Write tests'"
    );
    assert_eq!(
        resp.list[0].is_draft,
        Some(true),
        "list[0].is_draft must be Some(true)"
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
        json!(["task1"]),
        "ids must be [\"task1\"] on the wire"
    );
    assert_eq!(
        args["properties"],
        json!(["id", "title", "isDraft"]),
        "properties must be [\"id\",\"title\",\"isDraft\"] on the wire"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Task/get — ids=None sends null
// ---------------------------------------------------------------------------

/// Task/get with ids=None sends null for ids, requesting all Tasks.
///
/// Oracle: draft-tasks-06 §4.5 — ids=null means "all Tasks for the account".
#[tokio::test]
async fn task_get_all_ids_null() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Task/get",
            {
                "accountId": "A13824",
                "state": "s5",
                "list": [],
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
    sc.task_get(None, None)
        .await
        .expect("task_get_all_ids_null: must succeed");

    // Wire request assertions.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert!(
        args["ids"].is_null(),
        "ids must be null when None is passed: {}",
        args
    );
}

// ---------------------------------------------------------------------------
// Test 3: Task/changes — sinceState, maxChanges, hasMoreChanges
// ---------------------------------------------------------------------------

/// Task/changes sends sinceState and maxChanges and parses hasMoreChanges=true.
///
/// Oracle: RFC 8620 §5.2 — /changes request must include sinceState; maxChanges is optional.
#[tokio::test]
async fn task_changes_paginated() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Task/changes",
            {
                "accountId": "A13824",
                "oldState": "s5",
                "newState": "s6",
                "hasMoreChanges": true,
                "created": ["task-new"],
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
        .task_changes(&State::from("s5"), Some(10))
        .await
        .expect("task_changes_paginated: must succeed");

    // Response assertions.
    assert!(resp.has_more_changes, "hasMoreChanges must be true");
    assert_eq!(resp.old_state, "s5", "oldState must round-trip");

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
        json!("s5"),
        "sinceState must be 's5' on the wire"
    );
    assert_eq!(
        args["maxChanges"],
        json!(10),
        "maxChanges must be 10 on the wire"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Task/set — create round-trip
// ---------------------------------------------------------------------------

/// Task/set with create returns the created object in the response.
///
/// Oracle: RFC 8620 §5.3 — created keys are caller-supplied; server returns
/// them in the `created` map with the assigned id and any server-set fields.
#[tokio::test]
async fn task_set_create_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Task/set",
            {
                "accountId": "A13824",
                "oldState": null,
                "newState": "s2",
                "created": {
                    "k1": { "id": "task-new1" }
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
    let resp = sc
        .task_set(
            Some(json!({"k1": {"title": "New task", "isDraft": true}})),
            None,
            None,
        )
        .await
        .expect("task_set_create_round_trip: must succeed");

    // Response assertions.
    let created = resp
        .created
        .as_ref()
        .expect("created must be Some when a task was created");
    assert!(
        created.contains_key("k1"),
        "created map must contain key 'k1'"
    );
    assert_eq!(
        created["k1"].id.as_ref().map(|id| id.as_ref()),
        Some("task-new1"),
        "created[\"k1\"].id must be 'task-new1'"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Task/copy — fromAccountId and accountId on the wire
// ---------------------------------------------------------------------------

/// Task/copy sends fromAccountId from the argument and accountId from the session.
///
/// Oracle: draft-tasks-06 §4.8 — fromAccountId is the source account; accountId
/// is the destination (the session's primary tasks account).
#[tokio::test]
async fn task_copy_includes_from_account_id() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Task/copy",
            {
                "accountId": "A13824",
                "oldState": null,
                "newState": "s2",
                "created": {
                    "c1": { "id": "task-copy1" }
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
    sc.task_copy(
        jmap_tasks_client::TaskCopyParams {
            from_account_id: Id::from("srcacc"),
            if_from_in_state: None,
            if_in_state: None,
            on_success_destroy_original: None,
            destroy_from_if_in_state: None,
            extra: serde_json::Map::new(),
        },
        json!({"c1": {"id": "task-src1", "taskListId": "list1"}}),
    )
    .await
    .expect("task_copy_includes_from_account_id: must succeed");

    // Wire request assertions.
    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["fromAccountId"],
        json!("srcacc"),
        "fromAccountId must be 'srcacc' on the wire"
    );
    assert_eq!(
        args["accountId"],
        json!("A13824"),
        "accountId must be 'A13824' (session primary account) on the wire"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Task/query — filter, position, limit round-trip
// ---------------------------------------------------------------------------

/// Task/query with filter and pagination sends all fields and returns ids.
///
/// Oracle: draft-tasks-06 §4.13 — filter, position, and limit are optional
/// query arguments; ids in the response are server-ordered.
#[tokio::test]
async fn task_query_with_filter() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Task/query",
            {
                "accountId": "A13824",
                "queryState": "qs1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": ["task1", "task2"],
                "total": 2,
                "limit": 20
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
    let mut cond = TaskFilterCondition::default();
    cond.after = Some(UTCDate::from("2026-01-01T00:00:00Z"));
    let resp = sc
        .task_query(
            Some(&Filter::Condition(cond)),
            None,
            &jmap_base_client::QueryParams {
                position: Some(0),
                limit: Some(20),
                ..Default::default()
            },
        )
        .await
        .expect("task_query_with_filter: must succeed");

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
        args["filter"]["after"],
        json!("2026-01-01T00:00:00Z"),
        "filter.after must be '2026-01-01T00:00:00Z' on the wire"
    );
    assert_eq!(args["position"], json!(0), "position must be 0 on the wire");
    assert_eq!(args["limit"], json!(20), "limit must be 20 on the wire");
}

// ---------------------------------------------------------------------------
// Test 7: Task/queryChanges — sinceQueryState, maxChanges, removed, added
// ---------------------------------------------------------------------------

/// Task/queryChanges sends sinceQueryState and maxChanges and parses removed/added.
///
/// Oracle: RFC 8620 §5.6 — /queryChanges response contains removed (Ids) and
/// added (AddedItem with id and index).
#[tokio::test]
async fn task_query_changes_round_trip() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Task/queryChanges",
            {
                "accountId": "A13824",
                "oldQueryState": "qs1",
                "newQueryState": "qs2",
                "total": 1,
                "removed": ["task-old"],
                "added": [{"id": "task-new", "index": 0}]
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
        .task_query_changes(
            &State::from("qs1"),
            Some(5),
            None::<&TaskFilter>,
            None,
            None,
            None,
        )
        .await
        .expect("task_query_changes_round_trip: must succeed");

    // Response assertions.
    assert!(
        resp.removed.iter().any(|id| id.as_ref() == "task-old"),
        "removed must contain 'task-old'"
    );
    assert_eq!(resp.added.len(), 1, "added must contain one entry");
    assert_eq!(
        resp.added[0].id.as_ref(),
        "task-new",
        "added[0].id must be 'task-new'"
    );
    assert_eq!(resp.added[0].index, 0, "added[0].index must be 0");

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
        json!(5),
        "maxChanges must be 5 on the wire"
    );
}

/// `Task/queryChanges` with filter, sort, upToId, and calculateTotal
/// must emit all four optional args on the wire (RFC 8620 §5.6).
///
/// Oracle: draft-ietf-jmap-tasks-06 §4.5 — `taskListId` is a valid
/// Task filter field; the comparator shape (property + isAscending)
/// follows RFC 8620 §5.5.
#[tokio::test]
async fn task_query_changes_with_filter_sort_upto_calculatetotal() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Task/queryChanges",
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
    let up_to = Id::from("task-100");
    let mut cond = TaskFilterCondition::default();
    cond.task_list_id = Some(Id::from("tl-1"));
    let mut cmp = TaskComparator::default();
    cmp.property = "created".into();
    cmp.is_ascending = false;
    sc.task_query_changes(
        &since,
        None,
        Some(&Filter::Condition(cond)),
        Some(&[cmp]),
        Some(&up_to),
        Some(true),
    )
    .await
    .expect("task_query_changes_with_filter_sort_upto_calculatetotal: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["filter"]["taskListId"],
        json!("tl-1"),
        "filter.taskListId must be 'tl-1'"
    );
    assert_eq!(
        args["sort"][0]["property"],
        json!("created"),
        "sort[0].property must be 'created'"
    );
    assert_eq!(
        args["upToId"],
        json!("task-100"),
        "upToId must be on the wire (RFC 8620 §5.6)"
    );
    assert_eq!(
        args["calculateTotal"],
        json!(true),
        "calculateTotal must be on the wire (RFC 8620 §5.6)"
    );
}

/// `Task/queryChanges` with all None optional args must NOT emit any of
/// filter/sort/upToId/calculateTotal/maxChanges on the wire.
///
/// Oracle: RFC 8620 §5.6 — all five are optional; the wire shape with
/// `None` for each must be byte-identical to the minimal
/// `sinceQueryState`-only call.
#[tokio::test]
async fn task_query_changes_all_none_omits_optional_wire_keys() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Task/queryChanges",
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
    sc.task_query_changes(&since, None, None::<&TaskFilter>, None, None, None)
        .await
        .expect("task_query_changes_all_none_omits_optional_wire_keys: must succeed");

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

// ---------------------------------------------------------------------------
// Task/copy — RFC 8620 §5.4 standard /copy arguments
// ---------------------------------------------------------------------------
//
// Oracle: RFC 8620 §5.4 names the optional /copy arguments
// `ifFromInState: "String|null"`, `ifInState: "String|null"`,
// `onSuccessDestroyOriginal: "Boolean" (default: false)` and
// `destroyFromIfInState: "String|null"`. Expected request arguments are
// hand-written from that text; `create` is checked by other tests.

async fn task_copy_sent_args(params: jmap_tasks_client::TaskCopyParams) -> serde_json::Value {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Task/copy",
            {
                "accountId": "A13824",
                "fromAccountId": "src",
                "oldState": "s1",
                "newState": "s2",
                "created": null,
                "notCreated": null
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
    sc.task_copy(params, json!({ "k1": { "id": "task-src" } }))
        .await
        .expect("task_copy must succeed");
    let reqs = server.received_requests().await.expect("recorded requests");
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).expect("JSON body");
    let mut args = body["methodCalls"][0][1].clone();
    args.as_object_mut()
        .expect("args object")
        .remove("create")
        .expect("create must be sent");
    args
}

/// All four RFC 8620 §5.4 optional arguments reach the wire under their
/// spec names; vendor extras pass through, but a colliding typed key
/// keeps its typed value (workspace extras-preservation policy).
#[tokio::test]
async fn task_copy_sends_rfc8620_copy_args() {
    let params = jmap_tasks_client::TaskCopyParams {
        from_account_id: jmap_types::Id::from("src"),
        if_from_in_state: Some("from-s5".into()),
        if_in_state: Some("to-s7".into()),
        on_success_destroy_original: Some(true),
        destroy_from_if_in_state: Some("from-s5".into()),
        extra: serde_json::Map::from_iter([
            ("acmeCorpAudit".to_owned(), json!(true)),
            ("fromAccountId".to_owned(), json!("collides")),
        ]),
    };
    assert_eq!(
        task_copy_sent_args(params).await,
        json!({
            "accountId": "A13824",
            "fromAccountId": "src",
            "ifFromInState": "from-s5",
            "ifInState": "to-s7",
            "onSuccessDestroyOriginal": true,
            "destroyFromIfInState": "from-s5",
            "acmeCorpAudit": true
        })
    );
}

/// Unset optional arguments are omitted, leaving the server defaults of
/// RFC 8620 §5.4 in force.
#[tokio::test]
async fn task_copy_omits_unset_copy_args() {
    let params = jmap_tasks_client::TaskCopyParams {
        from_account_id: jmap_types::Id::from("src"),
        if_from_in_state: None,
        if_in_state: None,
        on_success_destroy_original: None,
        destroy_from_if_in_state: None,
        extra: serde_json::Map::new(),
    };
    assert_eq!(
        task_copy_sent_args(params).await,
        json!({ "accountId": "A13824", "fromAccountId": "src" })
    );
}
