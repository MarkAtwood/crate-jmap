//! `#creationId` references in VTCCall/set and VTCParticipant/set Id-typed
//! properties, through the full dispatcher (bd:JMAP-1p88).
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
use jmap_server::Dispatcher;
use jmap_types::{JmapRequest, JmapResponse, State};
use jmap_vtc_server::register_vtc_handlers;
use serde_json::{Value, json};

async fn dispatch(method_calls: Value) -> JmapResponse {
    let mut dispatcher: Dispatcher<jmap_types::Id> = Dispatcher::new();
    register_vtc_handlers(&mut dispatcher, Arc::new(backend()));
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:vtc"],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher
        .dispatch(request, jmap_types::Id::from(BOB), State::from("s0"))
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

fn room() -> Value {
    json!({ "callType": "room", "mediaTypes": ["audio"] })
}

#[tokio::test]
async fn parent_call_id_resolves_same_call_creation_listed_after_child() {
    // The breakout room sorts before its parent in the create map; §5.3
    // still requires the parent to be created first.
    let mut child = room();
    child["parentCallId"] = json!("#z-parent");
    let resp = dispatch(json!([["VTCCall/set", { "accountId": ACCOUNT, "create": {
        "a-child": child,
        "z-parent": room()
    } }, "0"]]))
    .await;
    let parent = created(&resp, 0, "z-parent");
    let child = created(&resp, 0, "a-child");
    assert_eq!(child["parentCallId"], parent["id"], "{child}");
}

#[tokio::test]
async fn participant_call_id_resolves_earlier_call_creation() {
    let resp = dispatch(json!([
        ["VTCCall/set", { "accountId": ACCOUNT, "create": { "c": room() } }, "0"],
        ["VTCParticipant/set", { "accountId": ACCOUNT, "create": {
            "p": { "callId": "#c", "joinMethod": "webrtc" }
        } }, "1"]
    ]))
    .await;
    let call = created(&resp, 0, "c");
    let participant = created(&resp, 1, "p");
    assert_eq!(participant["callId"], call["id"], "{participant}");
}

#[tokio::test]
async fn unknown_reference_is_invalid_properties() {
    let mut child = room();
    child["parentCallId"] = json!("#nope");
    let resp = dispatch(json!([
        ["VTCCall/set", { "accountId": ACCOUNT, "create": { "c": child } }, "0"],
        ["VTCParticipant/set", { "accountId": ACCOUNT, "create": {
            "p": { "callId": "#nope", "joinMethod": "webrtc" }
        } }, "1"]
    ]))
    .await;
    for (i, key, property) in [(0, "c", "parentCallId"), (1, "p", "callId")] {
        let err = &args(&resp, i)["notCreated"][key];
        assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, i));
        assert_eq!(err["properties"], json!([property]), "{err}");
    }
}

/// RFC 8620 §5.3: creates happen "before their creation ids are referenced
/// by another create/update/destroy in the same call", so update keys and
/// destroy ids may be creation ids, from an earlier call (`#a`, `#b`) or
/// this one (`#c`); the response names each by its real id
/// (bd:JMAP-tna1.16).
#[tokio::test]
async fn update_keys_and_destroy_ids_resolve_creations() {
    let obj = room();
    let resp = dispatch(json!([
            ["VTCCall/set", {"accountId": ACCOUNT, "create": {"a": obj.clone(), "b": obj.clone()}}, "0"],
            ["VTCCall/set", {"accountId": ACCOUNT,
                "create": {"c": obj},
                "update": {"#a": {"mediaTypes": ["audio", "video"]}, "#c": {"mediaTypes": ["audio", "video"]}},
                "destroy": ["#b"]
            }, "1"]
        ]))
    .await;
    let id = |i: usize, key: &str| {
        resp.method_responses[i].arguments["created"][key]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("created {key}: {:?}", resp.method_responses[i]))
            .to_owned()
    };
    let (a, b, c) = (id(0, "a"), id(0, "b"), id(1, "c"));
    let set = &resp.method_responses[1].arguments;
    assert!(set["updated"].get(&a).is_some(), "{set}");
    assert!(set["updated"].get(&c).is_some(), "{set}");
    assert_eq!(set["destroyed"], json!([b]), "{set}");
    assert!(set["notUpdated"].is_null(), "{set}");
    assert!(set["notDestroyed"].is_null(), "{set}");
}
