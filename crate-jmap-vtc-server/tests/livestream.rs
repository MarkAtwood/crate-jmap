//! `VTCLivestream/*` (draft-atwood-jmap-vtc-00 §VTCLivestream Methods,
//! L1340-1501) and §Livestream Key Exposure (L1933-1935). Line numbers cite
//! draft-atwood-jmap-vtc-00.md at jmap-chat-spec 77c3474.

#[macro_use]
mod common;

use common::*;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{
    handle_vtc_livestream_changes, handle_vtc_livestream_get, handle_vtc_livestream_query,
    handle_vtc_livestream_set,
};
use serde_json::{Value, json};

async fn set(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(ids handle_vtc_livestream_set, backend, caller, args)
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

/// L1382-1388: the example create body, with this fixture's call id.
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
    // L1406 "The server sets `id` and `state` (to `"starting"`)"; L606
    // `startedAt` "`null` before the stream is live."
    assert!(created["id"].is_string(), "{created}");
    assert_eq!(created["state"], "starting");
    assert_eq!(created["startedAt"], Value::Null);
    assert_eq!(created["stoppedAt"], Value::Null);
    assert_eq!(created["platform"], "youtube");
    // L1802-1803: a `VTCRecordingStateEvent` carries `livestreamId` only for
    // a key-holding livestream bridge, which this server does not run.
    assert!(backend.recording_events_for_test().is_empty());
}

#[tokio::test]
async fn stream_uri_and_key_are_required() {
    let backend = media_backend();
    // L1368 "`streamUri` (String, required in push mode)", L1371
    // "`streamKey` (String, required in push mode)"; L579 `mode` "Default:
    // `"push"`."
    let err = create(&backend, BOB, json!({ "callId": "C1" }))
        .await
        .expect_err("required");
    assert_invalid(&err, json!(["streamUri", "streamKey"]));
}

#[tokio::test]
async fn non_moderator_is_forbidden_everything() {
    let backend = media_backend();
    live(&backend);
    // jmap-chat-spec 77c3474 L1356 "Only participants with
    // `role: "moderator"` on the associated VTCCall MAY create, update, or
    // destroy VTCLivestream objects. Non-moderators SHOULD receive
    // `forbidden`." This crate enforces it by choice.
    let err = create(&backend, ALICE, start()).await.expect_err("create");
    assert_eq!(err["type"], "forbidden", "{err}");
    let err = update(&backend, ALICE, "L1", json!({ "state": "stopped" }))
        .await
        .expect_err("update");
    assert_eq!(err["type"], "forbidden", "{err}");
    // Destroy: same L1356 rule.
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
    // L1458 "When the call has `e2eeEnabled: true`, a livestream that would
    // require server-side media access is rejected as
    // {{e2ee-media-participants}} describes." L1834 "The server SHOULD
    // reject with `forbidden` a recording or livestream request that can
    // only be met by such a path." This crate enforces it by choice. This
    // server has no key-holding livestream bridge.
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
    // L1458 "The server MUST return `forbidden` when the account-level
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
    // L1458 "The server MUST return `invalidProperties` naming `callId` when
    // the call is not in state `"active"`."
    let err = create(&backend, BOB, start()).await.expect_err("ended");
    assert_invalid(&err, json!(["callId"]));
}

#[tokio::test]
async fn stream_key_is_blank_for_non_moderators() {
    let backend = media_backend();
    live(&backend);
    // L600 "the server SHOULD return an empty string in `VTCLivestream/get`
    // responses for participants whose role is not `"moderator"`"; L1935
    // "the server SHOULD return `streamUri` as an empty string for
    // non-moderator participants".
    let seen = get_as(&backend, ALICE, json!({ "ids": ["L1"] })).await;
    assert_eq!(seen["streamKey"], "");
    assert_eq!(seen["streamUri"], "");
    assert_eq!(seen["platform"], "youtube");

    // L600: moderators are not covered by the rule; Bob initiated C1.
    let seen = get_as(&backend, BOB, json!({ "ids": ["L1"] })).await;
    assert_eq!(seen["streamKey"], "sk_live_abc123def456");
    assert_eq!(seen["streamUri"], "rtmp://live.example.com/ingest");
}

#[tokio::test]
async fn stream_key_stays_blank_when_call_id_is_not_requested() {
    let backend = media_backend();
    live(&backend);
    // L600 "the server SHOULD return an empty string in `VTCLivestream/get`
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
    // L1462 "`update` with `state: "stopped"` ends the stream. The server
    // sets `stoppedAt` to the current time."
    let echo = update(&backend, BOB, "L1", json!({ "state": "stopped" }))
        .await
        .expect("stop");
    let seen = get_as(&backend, BOB, json!({ "ids": ["L1"] })).await;
    assert_eq!(seen["state"], "stopped");
    assert!(seen["stoppedAt"].is_string(), "{seen}");
    assert_eq!(echo["stoppedAt"], seen["stoppedAt"]);

    // L1474 "The server MUST return `invalidProperties` naming `state` if
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
    // L615 "`"starting"` → `"live"`: server-initiated"; L616 "`"starting"`
    // or `"live"` → `"failed"`: server-initiated".
    for to in ["live", "failed"] {
        let err = update(&backend, BOB, &id, json!({ "state": to }))
            .await
            .expect_err("server-initiated");
        assert_invalid(&err, json!(["state"]));
    }
}

#[tokio::test]
async fn stream_configuration_is_immutable() {
    // §VTCLivestream: `platform`, `streamUri` and `streamKey` are each
    // "(String|null, immutable)", and "`"stopped"` and `"failed"` are
    // final: to stream again, a client creates a new VTCLivestream".
    // jmap-chat-spec 3227c0f allowed reconfiguring a stopped stream; 77c3474
    // removed §Updating Stream Configuration (JMAP-t8ej.10).
    let backend = media_backend();
    live(&backend);
    let err = update(
        &backend,
        BOB,
        "L1",
        json!({ "streamKey": "sk_new", "platform": "twitch" }),
    )
    .await
    .expect_err("live");
    assert_invalid(&err, json!(["platform", "streamKey"]));

    update(&backend, BOB, "L1", json!({ "state": "stopped" }))
        .await
        .expect("stop");
    let err = update(&backend, BOB, "L1", json!({ "streamKey": "sk_new" }))
        .await
        .expect_err("immutable after stop too");
    assert_invalid(&err, json!(["streamKey"]));
    // An identical value is not a change (RFC 8620 §5.3).
    update(
        &backend,
        BOB,
        "L1",
        json!({ "streamKey": "sk_live_abc123def456" }),
    )
    .await
    .expect("identical value");
}

#[tokio::test]
async fn stopped_is_final() {
    // §Livestream States: "`"stopped"` and `"failed"` are final".
    let backend = media_backend();
    for (id, state) in [("L1", "stopped"), ("L2", "failed")] {
        seed_as(
            &backend,
            "VTCLivestream",
            stored_livestream(id, state, json!("2026-06-05T14:41:00Z")),
        );
        for to in ["starting", "live", "stopped"] {
            let err = update(&backend, BOB, id, json!({ "state": to }))
                .await
                .expect_err("final");
            assert_invalid(&err, json!(["state"]));
        }
    }
}

#[tokio::test]
async fn push_is_the_default_mode() {
    // §Starting a Livestream: "`mode` (String, optional): `"push"`
    // (default)"; failureReason "`null` unless `state` is `"failed"`";
    // playbackEndpoints "Always empty in push mode".
    let backend = media_backend();
    let created = create(&backend, BOB, start()).await.expect("start");
    let id = created["id"].as_str().expect("id");
    let seen = get_as(&backend, BOB, json!({ "ids": [id] })).await;
    assert_eq!(seen["mode"], "push", "{seen}");
    assert_eq!(seen["failureReason"], Value::Null, "{seen}");
    assert_eq!(seen["playbackEndpoints"], json!([]), "{seen}");
}

#[tokio::test]
async fn hosted_create_supplies_only_call_id_and_mode() {
    // §Starting a Livestream: "In hosted mode the client supplies only
    // `callId` and `mode`"; §VTCLivestream: platform, streamUri and
    // streamKey are "Always `null` in hosted mode"; playbackEndpoints "MAY
    // be empty while the livestream is `"starting"`".
    let backend = media_backend();
    let created = create(&backend, BOB, json!({ "callId": "C1", "mode": "hosted" }))
        .await
        .expect("hosted");
    let id = created["id"].as_str().expect("id");
    assert_eq!(created["state"], "starting", "{created}");
    let seen = get_as(&backend, BOB, json!({ "ids": [id] })).await;
    assert_eq!(seen["mode"], "hosted", "{seen}");
    for k in ["platform", "streamUri", "streamKey"] {
        assert_eq!(seen[k], Value::Null, "{k}: {seen}");
    }
    assert_eq!(seen["playbackEndpoints"], json!([]), "{seen}");
}

#[tokio::test]
async fn hosted_create_rejects_each_stream_property() {
    // §Starting a Livestream: "The server MUST return `invalidProperties`
    // naming each of `streamUri`, `streamKey`, and `platform` that a
    // hosted-mode create sets to a non-null value."
    let backend = media_backend();
    let mut obj = start();
    obj["mode"] = json!("hosted");
    let err = create(&backend, BOB, obj).await.expect_err("hosted");
    assert_invalid(&err, json!(["streamUri", "streamKey", "platform"]));

    // Null is allowed: it is the value hosted mode stores.
    create(
        &backend,
        BOB,
        json!({ "callId": "C1", "mode": "hosted", "platform": null, "streamUri": null }),
    )
    .await
    .expect("nulls are fine");
}

#[tokio::test]
async fn mode_must_be_in_livestream_modes() {
    // §Starting a Livestream: "The server MUST return `invalidProperties`
    // naming `mode` when the requested mode is not listed in the
    // account-level `livestreamModes`."
    let mut cap = media_capability();
    cap["livestreamModes"] = json!(["push"]);
    let backend = common::backend_with(cap);
    seed(
        &backend,
        stored_call("C1", "room", "active", "2026-06-05T14:30:00Z"),
    );
    for mode in [json!("hosted"), json!("x-relay"), json!(7)] {
        let err = create(&backend, BOB, json!({ "callId": "C1", "mode": mode }))
            .await
            .expect_err("mode");
        assert_invalid(&err, json!(["mode"]));
    }
    create(&backend, BOB, start())
        .await
        .expect("push is listed");
}

#[tokio::test]
async fn stopping_empties_playback_endpoints() {
    // §VTCLivestream playbackEndpoints: "is empty once it is `"stopped"` or
    // `"failed"`".
    let backend = media_backend();
    let mut hosted = stored_livestream("L1", "live", json!("2026-06-05T14:41:00Z"));
    hosted["mode"] = json!("hosted");
    for k in ["platform", "streamUri", "streamKey"] {
        hosted[k] = Value::Null;
    }
    hosted["playbackEndpoints"] = json!([{
        "protocol": "hls",
        "url": "https://cdn.example.com/live/L1/index.m3u8",
        "latencyHint": "standard",
        "credential": null,
        "expiresAt": null,
        "timeReference": null,
        "tracks": [],
        "encrypted": false,
        "keySystems": []
    }]);
    seed_as(&backend, "VTCLivestream", hosted);
    update(&backend, BOB, "L1", json!({ "state": "stopped" }))
        .await
        .expect("stop");
    let seen = get_as(&backend, BOB, json!({ "ids": ["L1"] })).await;
    assert_eq!(seen["state"], "stopped", "{seen}");
    assert_eq!(seen["playbackEndpoints"], json!([]), "{seen}");
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
    // L1497 "Default sort: `startedAt` descending."
    let resp = call!(handle_vtc_livestream_query, &backend, BOB, json!({}));
    assert_eq!(resp["ids"], json!(["L2", "L1"]), "{resp}");
    // L1490 `state` "Filter by livestream state."
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
    // L1478: "`destroy`
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

#[tokio::test]
async fn query_other_sort_property_is_unsupported() {
    let backend = media_backend();
    // L1497 "Sort properties: `startedAt`."; RFC 8620 §5.5
    // "unsupportedSort: The sort is syntactically valid, but includes a
    // property the server does not support sorting on".
    let Err(err) = handle_vtc_livestream_query(
        &backend,
        &BOB.to_owned(),
        json!({ "accountId": ACCOUNT, "sort": [{ "property": "state" }] }),
    )
    .await
    else {
        panic!("expected unsupportedSort");
    };
    assert_eq!(err.error_type.to_string(), "unsupportedSort");
}

#[tokio::test]
async fn stream_uri_must_be_an_rfc3986_uri() {
    // draft-atwood-jmap-updates-00 §7.1 (jmap-chat-spec 2b85363): a
    // URI-valued property "SHOULD hold a URI as defined in RFC 3986
    // Section 3". L1372 `streamUri` is "the RTMP or equivalent ingest
    // endpoint"; one with a space is not a URI.
    let backend = media_backend();
    let mut bad = start();
    bad["streamUri"] = json!("rtmp://live.example.com/my ingest");
    let err = create(&backend, BOB, bad).await.expect_err("not a URI");
    assert_invalid(&err, json!(["streamUri"]));

    // §VTCLivestream: `streamUri` is immutable, so an update never
    // carries a new one (stream_configuration_is_immutable).
}
