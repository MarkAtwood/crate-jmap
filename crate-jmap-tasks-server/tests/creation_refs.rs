//! `#creationId` references in `Task/set` `taskListId`, through the full
//! dispatcher (bd:JMAP-1p88).
//!
//! Oracle: RFC 8620 §5.3 (rfc8620.txt:1983-1996): "Some records may hold
//! references to other records (foreign keys). That reference may be set
//! (via create or update) in the same request as the referenced record is
//! created. To do this, the client refers to the new record using its
//! creation id prefixed with a "#". [...] the server MUST keep a simple map
//! for the duration of the request of creation id to record id for each
//! newly created record, so it can substitute in the correct value if
//! necessary in later method calls."
//!
//! Each assertion compares `taskListId` with the id the server reported
//! for the referenced TaskList in the same response, or with a
//! hand-written SetError shape.

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_server::Dispatcher;
use jmap_tasks_server::register_tasks_handlers;
use jmap_types::{JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";

async fn dispatch(method_calls: Value) -> JmapResponse {
    let mut dispatcher = Dispatcher::new();
    register_tasks_handlers(
        &mut dispatcher,
        Arc::new(MemoryBackend::new().with_account(ACCOUNT)),
    );
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:tasks"],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher.dispatch(request, (), State::from("s0")).await
}

fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

#[tokio::test]
async fn task_list_id_resolves_on_create() {
    let resp = dispatch(json!([
        ["TaskList/set", { "accountId": ACCOUNT, "create": { "l": { "name": "Home", "isSubscribed": true } } }, "0"],
        ["Task/set", { "accountId": ACCOUNT, "create": {
            "t": { "@type": "Task", "uid": "t-uid", "title": "Buy milk", "taskListId": "#l" }
        } }, "1"]
    ]))
    .await;
    let home = &args(&resp, 0)["created"]["l"]["id"];
    assert!(home.is_string(), "{}", args(&resp, 0));
    let task = &args(&resp, 1)["created"]["t"];
    assert_eq!(&task["taskListId"], home, "{}", args(&resp, 1));
}

#[tokio::test]
async fn task_list_id_resolves_on_update() {
    // An update names the Task by its id as a map key, so the Task is
    // created by a first request and updated by a second against the same
    // backend.
    let backend = Arc::new(MemoryBackend::new().with_account(ACCOUNT));
    let mut dispatcher = Dispatcher::new();
    register_tasks_handlers(&mut dispatcher, Arc::clone(&backend));
    let run = |calls: Value| {
        let request: JmapRequest = serde_json::from_value(json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:tasks"],
            "methodCalls": calls
        }))
        .expect("request JSON must deserialize");
        dispatcher.dispatch(request, (), State::from("s0"))
    };
    let first = run(json!([
        ["TaskList/set", { "accountId": ACCOUNT, "create": { "l": { "name": "Home", "isSubscribed": true } } }, "0"],
        ["Task/set", { "accountId": ACCOUNT, "create": {
            "t": { "@type": "Task", "uid": "t-uid", "title": "Buy milk", "taskListId": "#l" }
        } }, "1"]
    ]))
    .await;
    let task_id = args(&first, 1)["created"]["t"]["id"]
        .as_str()
        .expect("task id")
        .to_owned();
    let second = run(json!([
        ["TaskList/set", { "accountId": ACCOUNT, "create": { "w": { "name": "Work", "isSubscribed": true } } }, "0"],
        ["Task/set", { "accountId": ACCOUNT,
            "update": { (task_id.clone()): { "taskListId": "#w" } } }, "1"],
        ["Task/get", { "accountId": ACCOUNT, "ids": [task_id], "properties": ["taskListId"] }, "2"]
    ]))
    .await;
    let work = &args(&second, 0)["created"]["w"]["id"];
    assert!(work.is_string(), "{}", args(&second, 0));
    assert!(
        args(&second, 1)["updated"].get(&task_id).is_some(),
        "{}",
        args(&second, 1)
    );
    assert_eq!(
        &args(&second, 2)["list"][0]["taskListId"],
        work,
        "{}",
        args(&second, 2)
    );
}

#[tokio::test]
async fn unknown_task_list_reference_is_invalid_properties() {
    let resp = dispatch(json!([
        ["Task/set", { "accountId": ACCOUNT, "create": {
            "t": { "@type": "Task", "uid": "t-uid", "title": "x", "taskListId": "#nope" }
        } }, "0"]
    ]))
    .await;
    let err = &args(&resp, 0)["notCreated"]["t"];
    assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, 0));
    assert_eq!(err["properties"], json!(["taskListId"]), "{err}");
}
