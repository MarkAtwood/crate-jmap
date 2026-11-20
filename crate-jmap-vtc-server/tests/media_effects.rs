//! Media effects requested after the write (bd JMAP-po6q.2,
//! docs/design/vtc-broadcast-research.md Recommendation 1).
//!
//! Oracles: draft-atwood-jmap-vtc-00 §Starting a Livestream ("The server
//! sets `id` and `state` (to `"starting"`)"), §Stopping a Livestream,
//! §Destroying a Livestream ("the server first stops it, so that no
//! orphaned stream keeps running"), §Pausing, Resuming, and Stopping a
//! Recording, §Ending a Call ("the server stops ... every VTCLivestream in
//! state `"starting"` or `"live"`"); draft-ietf-jmap-conditional-02 §4.2
//! (an atomic failure changes nothing) and the `ConditionalBackend::begin_set`
//! rule that irreversible external effects wait for commit.

#[macro_use]
mod common;

use std::sync::Arc;

use common::{ACCOUNT, BOB, media_backend};
use jmap_server::Dispatcher;
use jmap_types::{JmapRequest, State};
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{
    MediaEffect, handle_vtc_call_set, handle_vtc_livestream_set, handle_vtc_recording_set,
    register_vtc_conditional, register_vtc_handlers,
};
use jmap_vtc_types::{LivestreamMode, LivestreamState, RecordingState};
use serde_json::{Value, json};

async fn livestream_set(b: &MemoryBackend, args: Value) -> Value {
    call!(ids handle_vtc_livestream_set, b, BOB, args)
}

async fn recording_set(b: &MemoryBackend, args: Value) -> Value {
    call!(ids handle_vtc_recording_set, b, BOB, args)
}

async fn create_hosted(b: &MemoryBackend) -> String {
    let r = livestream_set(
        b,
        json!({"create": {"s1": {"callId": "C1", "mode": "hosted"}}}),
    )
    .await;
    r["created"]["s1"]["id"]
        .as_str()
        .expect("created")
        .to_owned()
}

/// A create requests the start, with the livestream as stored
/// (`"starting"`), once; nothing else.
#[tokio::test]
async fn livestream_create_requests_start_after_the_insert() {
    let b = media_backend();
    let id = create_hosted(&b).await;
    let effects = b.media_effects_for_test();
    assert_eq!(effects.len(), 1, "{effects:?}");
    let MediaEffect::StartLivestream(ls) = &effects[0] else {
        panic!("{effects:?}");
    };
    assert_eq!(ls.id.as_ref(), id);
    assert_eq!(ls.state, LivestreamState::Starting);
    assert_eq!(ls.mode, LivestreamMode::Hosted);
}

/// A push create hands the backend its ingest configuration.
#[tokio::test]
async fn push_create_carries_the_stream_config() {
    let b = media_backend();
    livestream_set(
        &b,
        json!({"create": {"s0": {
            "callId": "C1",
            "streamUri": "rtmp://live.example.com/ingest",
            "streamKey": "sk_live_abc123def456",
            "platform": "youtube"
        }}}),
    )
    .await;
    let effects = b.media_effects_for_test();
    let [MediaEffect::StartLivestream(ls)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        ls.stream_uri.as_deref(),
        Some("rtmp://live.example.com/ingest")
    );
    assert_eq!(ls.stream_key.as_deref(), Some("sk_live_abc123def456"));
}

/// §Stopping a Livestream: a moderator's stop requests the stop of a
/// livestream now `"stopped"` with `stoppedAt` set.
#[tokio::test]
async fn moderator_stop_requests_stop() {
    let b = media_backend();
    let id = create_hosted(&b).await;
    let r = livestream_set(&b, json!({"update": {&id: {"state": "stopped"}}})).await;
    assert!(r["notUpdated"].is_null(), "{r}");
    let effects = b.media_effects_for_test();
    let [_, MediaEffect::StopLivestream(ls)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(ls.state, LivestreamState::Stopped);
    assert!(ls.stopped_at.is_some());
    assert!(ls.playback_endpoints.is_empty());
}

/// A refused update requests nothing.
#[tokio::test]
async fn refused_stop_requests_nothing() {
    let b = media_backend();
    let id = create_hosted(&b).await;
    livestream_set(&b, json!({"update": {&id: {"state": "stopped"}}})).await;
    let r = livestream_set(&b, json!({"update": {&id: {"state": "stopped"}}})).await;
    assert_eq!(r["notUpdated"][&id]["type"], "invalidProperties", "{r}");
    assert_eq!(b.media_effects_for_test().len(), 2);
}

/// §Destroying a Livestream: an active livestream is stopped first.
#[tokio::test]
async fn destroying_an_active_livestream_requests_stop() {
    let b = media_backend();
    let id = create_hosted(&b).await;
    let r = livestream_set(&b, json!({"destroy": [&id]})).await;
    assert_eq!(r["destroyed"], json!([&id]), "{r}");
    let effects = b.media_effects_for_test();
    assert!(
        matches!(effects.as_slice(), [_, MediaEffect::StopLivestream(_)]),
        "{effects:?}"
    );
}

/// §Pausing, Resuming, and Stopping a Recording: each transition requests
/// the matching effect with the recording in its new state.
#[tokio::test]
async fn recording_transitions_request_effects() {
    let b = media_backend();
    let r = recording_set(&b, json!({"create": {"r0": {"callId": "C1"}}})).await;
    let id = r["created"]["r0"]["id"]
        .as_str()
        .expect("created")
        .to_owned();
    for state in ["paused", "recording", "stopped"] {
        let r = recording_set(&b, json!({"update": {&id: {"state": state}}})).await;
        assert!(r["notUpdated"].is_null(), "{r}");
    }
    let effects = b.media_effects_for_test();
    let [
        MediaEffect::StartRecording(start),
        MediaEffect::PauseRecording(pause),
        MediaEffect::ResumeRecording(resume),
        MediaEffect::StopRecording(stop),
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(start.state, RecordingState::Recording);
    assert_eq!(pause.state, RecordingState::Paused);
    assert_eq!(resume.state, RecordingState::Recording);
    assert_eq!(stop.state, RecordingState::Stopped);
    assert!(stop.stopped_at.is_some());
}

/// §Ending a Call: the call's end stops its livestream and recording,
/// each with its stop effect.
#[tokio::test]
async fn call_end_requests_stops() {
    let b = media_backend();
    create_hosted(&b).await;
    recording_set(&b, json!({"create": {"r0": {"callId": "C1"}}})).await;
    let r = call!(ids handle_vtc_call_set, &b, BOB, json!({"update": {"C1": {"state": "ended"}}}));
    assert!(r["notUpdated"].is_null(), "{r}");
    let effects = b.media_effects_for_test();
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, MediaEffect::StopLivestream(_))),
        "{effects:?}"
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, MediaEffect::StopRecording(_))),
        "{effects:?}"
    );
}

/// conditional-02 §4.2: an atomic VTCLivestream/set that fails as a whole
/// requests no media effect, so no stream starts for a livestream that
/// was never created.
#[tokio::test]
async fn atomic_failure_requests_no_media_effect() {
    let b = Arc::new(media_backend());
    let mut d: Dispatcher<String> = Dispatcher::new();
    register_vtc_handlers(&mut d, Arc::clone(&b));
    register_vtc_conditional(&mut d, Arc::clone(&b));
    let req: JmapRequest = serde_json::from_value(json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:vtc",
            "urn:ietf:params:jmap:conditional"
        ],
        "methodCalls": [["VTCLivestream/set", {
            "accountId": ACCOUNT,
            "atomic": true,
            "create": {
                "ok": {"callId": "C1", "mode": "hosted"},
                "bad": {"callId": "nope", "mode": "hosted"}
            }
        }, "0"]]
    }))
    .expect("request");
    let resp = serde_json::to_value(d.dispatch(req, BOB.to_owned(), State::from("s")).await)
        .expect("response");
    assert_eq!(
        resp["methodResponses"][0][1]["type"], "atomicFailure",
        "{resp}"
    );
    assert!(
        b.media_effects_for_test().is_empty(),
        "{:?}",
        b.media_effects_for_test()
    );
}
