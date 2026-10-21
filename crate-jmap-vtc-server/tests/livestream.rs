//! `VTCLivestream/*` (draft-atwood-jmap-vtc-00 §VTCLivestream Methods,
//! L1133-1230) and §Livestream Key Exposure (L1653-1655). Line numbers cite
//! draft-atwood-jmap-vtc-00.md at jmap-chat-spec 3227c0f.

#[macro_use]
mod common;

use common::*;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{
    handle_vtc_livestream_changes, handle_vtc_livestream_get, handle_vtc_livestream_query,
    handle_vtc_livestream_set,
};
use serde_json::{json, Value};

async fn set(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(handle_vtc_livestream_set, backend, caller, args)
}

async fn create(backend: &MemoryBackend, caller: &str, obj: Value) -> Result<Value, Value> {
    let resp = set(backend, caller, json!({ "create": { "s0": obj } })).await;
    match resp["notCreated"].get("s0") {
        Some(e) => Err(e.clone()),
        None => Ok(resp["created"]["s0"].clone()),
    }
}

async fn update(
    backend: &MemoryBackend,
    caller: &str,
    id: &str,
    patch: Value,
) -> Result<Value, Value> {
    let resp = set(backend, caller, json!({ "update": { id: patch } })).await;
    match resp["notUpdated"].get(id) {
        Some(e) => Err(e.clone()),
        None => Ok(resp["updated"][id].clone()),
    }
}

async fn get_as(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(handle_vtc_livestream_get, backend, caller, args)["list"][0].clone()
}

/// L1170-1176: the example create body, with this fixture's call id.
fn start() -> Value {
    json!({
        "callId": "C1",
        "streamUri": "rtmp://live.example.com/ingest",
        "streamKey": "sk_live_abc123def456",
        "platform": "youtube"
    })
}

fn live(backend: &MemoryBackend) {
    seed_as(
        backend,
        "VTCLivestream",
        stored_livestream("L1", "live", json!("2026-06-05T14:41:00Z")),
    );
}

fn assert_invalid(err: &Value, properties: Value) {
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], properties, "{err}");
}

#[tokio::test]
async fn moderator_starts_a_livestream() {
    let backend = media_backend();
    let created = create(&backend, BOB, start()).await.expect("start");
    // L1181 "The server sets `id` and `state` (to `"starting"`)"; L514
    // `startedAt` "`null` before the stream is live."
    assert!(created["id"].is_string(), "{created}");
    assert_eq!(created["state"], "starting");
    assert_eq!(created["startedAt"], Value::Null);
    assert_eq!(created["stoppedAt"], Value::Null);
    assert_eq!(created["platform"], "youtube");
    // L1528-1531: a `VTCRecordingStateEvent` carries `livestreamId` only for
    // a key-holding livestream bridge, which this server does not run.
    assert!(backend.recording_events_for_test().is_empty());
}

#[tokio::test]
async fn stream_uri_and_key_are_required() {
    let backend = media_backend();
    // L1158 "`streamUri` (String, required)", L1161 "`streamKey` (String,
    // required)".
    let err = create(&backend, BOB, json!({ "callId": "C1" }))
        .await
        .expect_err("required");
    assert_invalid(&err, json!(["streamUri", "streamKey"]));
}

#[tokio::test]
async fn non_moderator_is_forbidden_everything() {
    let backend = media_backend();
    live(&backend);
    // L1149 "Only participants with `role: "moderator"` on the associated
    // VTCCall MAY create, update, or destroy VTCLivestream objects.
    // Non-moderators MUST receive `forbidden`."
    let err = create(&backend, ALICE, start()).await.expect_err("create");
    assert_eq!(err["type"], "forbidden", "{err}");
    let err = update(&backend, ALICE, "L1", json!({ "state": "stopped" }))
        .await
        .expect_err("update");
    assert_eq!(err["type"], "forbidden", "{err}");
    // L1207 "The server MUST return `forbidden` when the caller is not a
    // moderator."
    let resp = set(&backend, ALICE, json!({ "destroy": ["L1"] })).await;
    assert_eq!(resp["notDestroyed"]["L1"]["type"], "forbidden", "{resp}");
    assert_eq!(
        get_as(&backend, BOB, json!({ "ids": ["L1"] })).await["state"],
        "live"
    );
}

#[tokio::test]
async fn e2ee_call_is_forbidden() {
    let backend = backend_with(media_capability());
    let mut c = stored_call("C1", "room", "active", "2026-06-05T14:30:00Z");
    c["e2eeEnabled"] = json!(true);
    seed(&backend, c);
    // L1183 "The server MUST return `forbidden` when the call has
    // `e2eeEnabled: true` and the livestream would require server-side media
    // access." This server has no key-holding livestream bridge.
    let err = create(&backend, BOB, start()).await.expect_err("e2ee");
    assert_eq!(err["type"], "forbidden", "{err}");
}

#[tokio::test]
async fn unsupported_livestream_is_forbidden() {
    let backend = backend(); // supportsLivestream: false
    seed(
        &backend,
        stored_call("C1", "room", "active", "2026-06-05T14:30:00Z"),
    );
    // L1183 "The server MUST return `forbidden` when the account-level
    // `supportsLivestream` is `false`."
    let err = create(&backend, BOB, start()).await.expect_err("off");
    assert_eq!(err["type"], "forbidden", "{err}");
}

#[tokio::test]
async fn ended_call_is_invalid_call_id() {
    let backend = backend_with(media_capability());
    seed(
        &backend,
        stored_call("C1", "room", "ended", "2026-06-05T14:30:00Z"),
    );
    // L1183 "The server MUST return `invalidProperties` naming `callId` when
    // the call is not in state `"active"`."
    let err = create(&backend, BOB, start()).await.expect_err("ended");
    assert_invalid(&err, json!(["callId"]));
}

#[tokio::test]
async fn stream_key_is_blank_for_non_moderators() {
    let backend = media_backend();
    live(&backend);
    // L562 "the server SHOULD return an empty string in `VTCLivestream/get`
    // responses for participants whose role is not `"moderator"`"; L1731
    // "the server SHOULD return `streamUri` as an empty string for
    // non-moderator participants".
    let seen = get_as(&backend, ALICE, json!({ "ids": ["L1"] })).await;
    assert_eq!(seen["streamKey"], "");
    assert_eq!(seen["streamUri"], "");
    assert_eq!(seen["platform"], "youtube");

    // L562: moderators are not covered by the rule; Bob initiated C1.
    let seen = get_as(&backend, BOB, json!({ "ids": ["L1"] })).await;
    assert_eq!(seen["streamKey"], "sk_live_abc123def456");
    assert_eq!(seen["streamUri"], "rtmp://live.example.com/ingest");
}

#[tokio::test]
async fn stream_key_stays_blank_when_call_id_is_not_requested() {
    let backend = media_backend();
    live(&backend);
    // L562 "the server SHOULD return an empty string in `VTCLivestream/get`
    // responses for participants whose role is not `"moderator"`", whatever
    // the `properties`. RFC 8620 §5.1: only the requested properties (and `id`)
    // are returned.
    let seen = get_as(
        &backend,
        ALICE,
        json!({ "ids": ["L1"], "properties": ["streamKey"] }),
    )
    .await;
    assert_eq!(seen["streamKey"], "");
    assert!(seen.get("callId").is_none(), "{seen}");
}

#[tokio::test]
async fn moderator_stops_a_live_stream() {
    let backend = media_backend();
    live(&backend);
    // L1187 "`update` with `state: "stopped"` ends the stream. The server
    // sets `stoppedAt` to the current time."
    let echo = update(&backend, BOB, "L1", json!({ "state": "stopped" }))
        .await
        .expect("stop");
    let seen = get_as(&backend, BOB, json!({ "ids": ["L1"] })).await;
    assert_eq!(seen["state"], "stopped");
    assert!(seen["stoppedAt"].is_string(), "{seen}");
    assert_eq!(echo["stoppedAt"], seen["stoppedAt"]);

    // L1199 "The server MUST return `invalidProperties` naming `state` if
    // the livestream is not in state `"starting"` or `"live"`."
    let err = update(&backend, BOB, "L1", json!({ "state": "stopped" }))
        .await
        .expect_err("already stopped");
    assert_invalid(&err, json!(["state"]));
}

#[tokio::test]
async fn going_live_is_server_initiated() {
    let backend = media_backend();
    let id = create(&backend, BOB, start()).await.expect("start")["id"]
        .as_str()
        .expect("id")
        .to_owned();
    // L1181 "The `"starting"` → `"live"` and `"starting"` → `"failed"`
    // transitions are server-initiated."
    for to in ["live", "failed"] {
        let err = update(&backend, BOB, &id, json!({ "state": to }))
            .await
            .expect_err("server-initiated");
        assert_invalid(&err, json!(["state"]));
    }
}

#[tokio::test]
async fn configuration_changes_only_while_not_streaming() {
    let backend = media_backend();
    live(&backend);
    // L1203 "The server MUST return `invalidProperties` naming each patched
    // configuration property for configuration updates while the stream is
    // in state `"starting"` or `"live"`."
    let err = update(
        &backend,
        BOB,
        "L1",
        json!({ "streamKey": "sk_new", "platform": "twitch" }),
    )
    .await
    .expect_err("live");
    assert_invalid(&err, json!(["platform", "streamKey"]));

    // L1203 "`update` supports patching `streamUri`, `streamKey`, or
    // `platform` only while the livestream is in state `"stopped"` or
    // `"failed"`."
    update(&backend, BOB, "L1", json!({ "state": "stopped" }))
        .await
        .expect("stop");
    update(&backend, BOB, "L1", json!({ "streamKey": "sk_new" }))
        .await
        .expect("reconfigure");
    let seen = get_as(&backend, BOB, json!({ "ids": ["L1"] })).await;
    assert_eq!(seen["streamKey"], "sk_new");
}

#[tokio::test]
async fn query_defaults_to_started_at_descending() {
    let backend = media_backend();
    seed_as(
        &backend,
        "VTCLivestream",
        stored_livestream("L1", "stopped", json!("2026-06-05T14:41:00Z")),
    );
    seed_as(
        &backend,
        "VTCLivestream",
        stored_livestream("L2", "live", json!("2026-06-05T15:00:00Z")),
    );
    // L1226 "Default sort: `startedAt` descending."
    let resp = call!(handle_vtc_livestream_query, &backend, BOB, json!({}));
    assert_eq!(resp["ids"], json!(["L2", "L1"]), "{resp}");
    // L1218 `state` "Filter by livestream state."
    let resp = call!(
        handle_vtc_livestream_query,
        &backend,
        BOB,
        json!({ "filter": { "state": "live", "platform": "youtube" } })
    );
    assert_eq!(resp["ids"], json!(["L2"]), "{resp}");
}

#[tokio::test]
async fn destroying_a_streaming_livestream_stops_it_first() {
    // jmap-chat-spec 33702a5 draft-atwood-jmap-vtc-00.md L1274: "`destroy`
    // removes the VTCLivestream metadata. If the livestream is in state
    // `"starting"` or `"live"`, the server first stops it, so that no
    // orphaned stream keeps running after the metadata is removed."
    // MemoryBackend states count commits from "0", and seeding commits
    // nothing: a stop is one commit, the destroy another.
    for (state, commits) in [("starting", 2), ("live", 2), ("stopped", 1), ("failed", 1)] {
        let backend = media_backend();
        seed_as(
            &backend,
            "VTCLivestream",
            stored_livestream("L1", state, Value::Null),
        );
        let resp = set(&backend, BOB, json!({ "destroy": ["L1"] })).await;
        assert_eq!(resp["destroyed"], json!(["L1"]), "{state}: {resp}");
        assert_eq!(resp["oldState"], "0", "{state}: {resp}");
        assert_eq!(resp["newState"], commits.to_string(), "{state}: {resp}");
        assert_eq!(
            get_as(&backend, BOB, json!({ "ids": ["L1"] })).await,
            Value::Null,
            "{state}"
        );
        if commits == 2 {
            // The first commit changed L1 without destroying it; the
            // destroy came after.
            let changes = call!(
                handle_vtc_livestream_changes,
                &backend,
                BOB,
                json!({ "sinceState": "1" })
            );
            assert_eq!(changes["destroyed"], json!(["L1"]), "{state}: {changes}");
        }
    }
}
