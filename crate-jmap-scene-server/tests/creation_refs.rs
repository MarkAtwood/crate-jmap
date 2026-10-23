//! `#creationId` references in SceneObject/set and SceneAvatar/set
//! Id-typed properties, through the full dispatcher (bd:JMAP-1p88).
//!
//! Oracle: RFC 8620 §5.3 (rfc8620.txt:1983-1996): "Some records may hold
//! references to other records (foreign keys). That reference may be set
//! (via create or update) in the same request as the referenced record is
//! created. To do this, the client refers to the new record using its
//! creation id prefixed with a "#". [...] the server MUST keep a simple map
//! for the duration of the request of creation id to record id for each
//! newly created record, so it can substitute in the correct value if
//! necessary in later method calls. In the case of records with
//! references to the same type, the server MUST order the creates and
//! updates within a single method call so that creates happen before their
//! creation ids are referenced by another create/update/destroy in the same
//! call."
//!
//! Each assertion compares the referencing property with the id the server
//! reported for the referenced record, or with a hand-written SetError
//! shape.

#[macro_use]
mod common;

use std::sync::Arc;

use common::*;
use jmap_scene_server::register_scene_handlers;
use jmap_server::Dispatcher;
use jmap_types::{JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

fn dispatcher() -> Dispatcher<String> {
    let mut dispatcher = Dispatcher::new();
    register_scene_handlers(&mut dispatcher, Arc::new(backend()));
    dispatcher
}

async fn run(dispatcher: &Dispatcher<String>, method_calls: Value) -> JmapResponse {
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:scene"],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher
        .dispatch(request, ALICE.to_owned(), State::from("s0"))
        .await
}

fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

fn created(resp: &JmapResponse, i: usize, key: &str) -> Value {
    let entry = args(resp, i)["created"][key].clone();
    assert!(entry["id"].is_string(), "created {key}: {}", args(resp, i));
    entry
}

fn region() -> Value {
    json!({
        "name": "Main Plaza",
        "bounds": { "min": [-500, -10, -500], "max": [500, 200, 500] }
    })
}

/// SceneRegion/set, then SceneObject/set whose child create sorts before
/// its parent and both name the region by creation id, then
/// SceneAvatar/set entering the same region, then SceneObject/get.
#[tokio::test]
async fn region_and_parent_references_resolve() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["SceneRegion/set", { "accountId": ACCOUNT, "create": { "r": region() } }, "0"],
            ["SceneObject/set", { "accountId": ACCOUNT, "create": {
                "a-child": { "regionId": "#r", "parentId": "#z-parent", "position": [0, 1, 0] },
                "z-parent": { "regionId": "#r", "position": [0, 0, 0] }
            } }, "1"],
            ["SceneAvatar/set", { "accountId": ACCOUNT, "create": {
                "av": { "regionId": "#r" }
            } }, "2"]
        ]),
    )
    .await;
    let region = created(&resp, 0, "r");
    let parent = created(&resp, 1, "z-parent");
    let child = created(&resp, 1, "a-child");
    let avatar = created(&resp, 2, "av");
    assert_eq!(avatar["regionId"], region["id"], "{avatar}");

    let resp = run(
        &d,
        json!([["SceneObject/get", { "accountId": ACCOUNT,
            "ids": [&child["id"], &parent["id"]],
            "properties": ["regionId", "parentId"] }, "0"]]),
    )
    .await;
    let list = &args(&resp, 0)["list"];
    assert_eq!(list[0]["id"], child["id"], "{list}");
    assert_eq!(list[0]["regionId"], region["id"], "{list}");
    assert_eq!(list[0]["parentId"], parent["id"], "{list}");
    assert_eq!(list[1]["regionId"], region["id"], "{list}");
}

#[tokio::test]
async fn unknown_reference_is_invalid_properties() {
    let resp = run(
        &dispatcher(),
        json!([
            ["SceneObject/set", { "accountId": ACCOUNT, "create": {
                "o": { "regionId": "#nope", "position": [0, 0, 0] }
            } }, "0"],
            ["SceneAvatar/set", { "accountId": ACCOUNT, "create": {
                "av": { "regionId": "#nope" }
            } }, "1"]
        ]),
    )
    .await;
    for (i, key) in [(0, "o"), (1, "av")] {
        let err = &args(&resp, i)["notCreated"][key];
        assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, i));
        assert_eq!(err["properties"], json!(["regionId"]), "{err}");
    }
}
