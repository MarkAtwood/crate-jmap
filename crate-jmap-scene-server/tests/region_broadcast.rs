//! `SceneRegion/broadcast`.
//!
//! Oracle: draft-atwood-jmap-scene-00 at jmap-chat-spec 93f2258,
//! §SceneRegion/broadcast (arguments, response and SetError table) and
//! §Account-Level Capability Object (`supportsRegionBroadcast`), quoted at
//! each assertion. Fixtures are hand-written JSON.

#[macro_use]
mod common;

use std::sync::Arc;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{handle_scene_region_broadcast, register_scene_handlers};
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapError, JmapRequest};
use serde_json::{Value, json};

/// A public region owned by [`ALICE`].
const R1: &str = "01J5ABC0000000000000000001";
/// An `"invite"` region owned by [`ALICE`]; [`BOB`] is not invited.
const VAULT: &str = "01J5ABC0000000000000000002";

fn capability(broadcast: bool) -> Value {
    let mut cap = common::capability();
    if broadcast {
        cap["supportsRegionBroadcast"] = json!(true);
    }
    cap
}

fn scene(broadcast: bool) -> MemoryBackend {
    let backend = backend_with(capability(broadcast));
    seed_as(
        &backend,
        "SceneRegion",
        stored_region(R1, "Arena", "2026-06-01T10:00:00Z"),
    );
    let mut vault = stored_region(VAULT, "Vault", "2026-06-01T11:00:00Z");
    vault["accessPolicy"] = json!("invite");
    seed_as(&backend, "SceneRegion", vault);
    backend
}

fn round_start() -> Value {
    json!({
        "regionId": R1,
        "event": "com.example.game.round-start",
        "data": {"round": 6, "buyTimeSeconds": 20}
    })
}

async fn broadcast(backend: &MemoryBackend, caller: &str, events: Value) -> Value {
    call!(
        handle_scene_region_broadcast,
        backend,
        caller,
        json!({ "events": events })
    )
}

/// "The response has the shape of the `SceneObject/interact` response";
/// the owner's events are accepted and handed on as `SceneRegionEvent`s
/// with the hosting `accountId`.
#[tokio::test]
async fn owner_broadcast_is_accepted_and_delivered() {
    let backend = scene(true);
    let resp = broadcast(
        &backend,
        ALICE,
        json!({
            "e0": round_start(),
            "e1": {"regionId": R1, "event": "com.example.game.timer-expired"}
        }),
    )
    .await;
    assert_eq!(
        resp,
        json!({"accountId": ACCOUNT, "accepted": ["e0", "e1"], "notAccepted": null})
    );
    let delivered = backend.accepted_region_events();
    assert_eq!(delivered.len(), 2);
    assert_eq!(
        serde_json::to_value(&delivered[0]).unwrap(),
        json!({
            "accountId": ACCOUNT,
            "regionId": R1,
            "event": "com.example.game.round-start",
            "data": {"round": 6, "buyTimeSeconds": 20}
        })
    );
    // "`data` (Object|null): ... Default: `null`."
    assert_eq!(delivered[1].data, None);
}

/// "Only the region owner and users with deployment-defined
/// administrative privileges may broadcast to a region": an admin may,
/// another user gets `forbidden`.
#[tokio::test]
async fn admin_may_broadcast_others_are_forbidden() {
    let backend = scene(true);
    let resp = broadcast(&backend, ADMIN, json!({"e0": round_start()})).await;
    assert_eq!(resp["accepted"], json!(["e0"]));
    let resp = broadcast(&backend, BOB, json!({"e0": round_start()})).await;
    assert_eq!(resp["accepted"], json!(null));
    assert_eq!(resp["notAccepted"]["e0"]["type"], "forbidden");
    assert_eq!(backend.accepted_region_events().len(), 1);
}

/// "`notFound` | The region does not exist or the caller has no access to
/// it": a missing region and one Bob cannot access look the same, and
/// neither is `forbidden`.
#[tokio::test]
async fn missing_or_inaccessible_region_is_not_found() {
    let backend = scene(true);
    let resp = broadcast(
        &backend,
        BOB,
        json!({
            "gone": {"regionId": "01J5ABC0000000000000000099", "event": "com.example.x"},
            "vault": {"regionId": VAULT, "event": "com.example.x"}
        }),
    )
    .await;
    assert_eq!(resp["notAccepted"]["gone"], json!({"type": "notFound"}));
    assert_eq!(resp["notAccepted"]["vault"], json!({"type": "notFound"}));
}

/// "`invalidProperties` | A property is missing or has the wrong type
/// ...; `properties` names it."
#[tokio::test]
async fn malformed_events_are_invalid_properties() {
    let backend = scene(true);
    let resp = broadcast(
        &backend,
        ALICE,
        json!({
            "noRegion": {"event": "com.example.x"},
            "noEvent": {"regionId": R1},
            "badData": {"regionId": R1, "event": "com.example.x", "data": [1]},
            "emptyEvent": {"regionId": R1, "event": ""},
            "notObject": 7
        }),
    )
    .await;
    let na = &resp["notAccepted"];
    assert_eq!(na["noRegion"]["type"], "invalidProperties");
    assert_eq!(na["noRegion"]["properties"], json!(["regionId"]));
    assert_eq!(na["noEvent"]["properties"], json!(["event"]));
    assert_eq!(na["badData"]["properties"], json!(["data"]));
    assert_eq!(na["emptyEvent"]["properties"], json!(["event"]));
    assert_eq!(na["notObject"]["type"], "invalidProperties");
    assert!(backend.accepted_region_events().is_empty());
}

/// "`rateLimit` | The caller has exceeded a deployment-defined rate for
/// region events."
#[tokio::test]
async fn over_the_rate_is_rate_limit() {
    let backend = scene(true);
    backend.rate_limit_region_events(ALICE);
    let resp = broadcast(&backend, ALICE, json!({"e0": round_start()})).await;
    assert_eq!(resp["notAccepted"]["e0"]["type"], "rateLimit");
    assert!(backend.accepted_region_events().is_empty());
}

/// "When `false`, a call to that method returns an `unknownMethod`
/// error"; "An absent field has the same meaning as `false`".
#[tokio::test]
async fn without_the_capability_the_method_is_unknown() {
    let backend = scene(false);
    let err = handle_scene_region_broadcast(
        &backend,
        &Id::from(ALICE),
        json!({"accountId": ACCOUNT, "events": {"e0": round_start()}}),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type.as_str(), "unknownMethod");
}

/// RFC 8620 §3.6.2: an unknown account is `accountNotFound`; a missing
/// `events` map is `invalidArguments`.
#[tokio::test]
async fn request_level_errors() {
    let backend = scene(true);
    let call = |args: Value| {
        let backend = &backend;
        async move { handle_scene_region_broadcast(backend, &Id::from(ALICE), args).await }
    };
    let err: JmapError = call(json!({"accountId": "nope", "events": {}}))
        .await
        .unwrap_err();
    assert_eq!(err.error_type.as_str(), "accountNotFound");
    let err = call(json!({"accountId": ACCOUNT, "events": []}))
        .await
        .unwrap_err();
    assert_eq!(err.error_type.as_str(), "invalidArguments");
}

/// Region events are ephemeral: "the method does not change any record or
/// state string".
#[tokio::test]
async fn changes_no_state() {
    let backend = scene(true);
    let state = |b: &MemoryBackend| {
        let b = b.clone();
        async move {
            use jmap_server::JmapReads;
            b.get_state::<jmap_scene_types::SceneRegion>(&Id::from(ALICE), &Id::from(ACCOUNT))
                .await
                .unwrap()
        }
    };
    let before = state(&backend).await;
    broadcast(&backend, ALICE, json!({"e0": round_start()})).await;
    assert_eq!(state(&backend).await, before);
}

/// `register_scene_handlers` wires `SceneRegion/broadcast`.
#[tokio::test]
async fn registered_with_the_dispatcher() {
    let backend = Arc::new(scene(true));
    let mut dispatcher: Dispatcher<Id> = Dispatcher::new();
    register_scene_handlers(&mut dispatcher, Arc::clone(&backend));
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:scene"],
        "methodCalls": [["SceneRegion/broadcast",
            {"accountId": ACCOUNT, "events": {"e0": round_start()}}, "c0"]]
    }))
    .unwrap();
    let response = dispatcher
        .dispatch(request, Id::from(ALICE), jmap_types::State::from("s0"))
        .await;
    let out = serde_json::to_value(&response).unwrap();
    assert_eq!(out["methodResponses"][0][0], "SceneRegion/broadcast");
    assert_eq!(out["methodResponses"][0][1]["accepted"], json!(["e0"]));
}
