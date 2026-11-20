//! Server-initiated media transitions through `media_report::MediaReporter`
//! (bd JMAP-po6q.2, docs/design/vtc-broadcast-research.md Recommendation 2).
//!
//! Oracles: draft-atwood-jmap-vtc-00 §Livestream States ("`"starting"` →
//! `"live"`: server-initiated", "`"starting"` or `"live"` → `"failed"`:
//! server-initiated, with `failureReason` set", "`"stopped"` and `"failed"`
//! are final"), §VTCLivestream (`startedAt` "Time the stream went live";
//! `stoppedAt` "Time the stream stopped or failed"; `playbackEndpoints`
//! "Always empty in push mode" and "empty once it is `"stopped"` or
//! `"failed"`"), §VTCRecording (`blobId`, `size`, `mediaType` "Available
//! only when `state` is `"available"`") and §Pausing, Resuming, and
//! Stopping a Recording ("After stopping, the server transitions the
//! recording through `"processing"` ... to `"available"`").

#[macro_use]
mod common;

use common::{BOB, media_backend, seed_as, stored_livestream};
use jmap_types::Id;
use jmap_vtc_server::media_report::{MediaReporter, RecordingFile, ReportError};
use jmap_vtc_server::{
    MediaEffect, handle_vtc_livestream_changes, handle_vtc_livestream_get,
    handle_vtc_livestream_set, handle_vtc_recording_get, handle_vtc_recording_set,
};
use jmap_vtc_types::{
    LatencyHint, LivestreamFailureReason, LivestreamState, PlaybackProtocol, RecordingState,
    VtcPlaybackEndpoint,
};
use serde_json::{Value, json};

/// The deployment's service identity for the reports.
const SYSTEM: &str = "service:media";

fn account() -> Id {
    Id::from(common::ACCOUNT)
}

fn hls() -> VtcPlaybackEndpoint {
    VtcPlaybackEndpoint::new(
        PlaybackProtocol::Hls,
        "https://cdn.example.com/live/01J4XQ7V/index.m3u8",
        LatencyHint::Standard,
    )
}

async fn hosted(b: &jmap_vtc_server::memory::MemoryBackend) -> Id {
    let r = call!(ids handle_vtc_livestream_set, b, BOB,
        json!({"create": {"s1": {"callId": "C1", "mode": "hosted"}}}));
    Id::from(r["created"]["s1"]["id"].as_str().expect("created"))
}

async fn livestream(b: &jmap_vtc_server::memory::MemoryBackend, id: &Id) -> Value {
    let r = call!(handle_vtc_livestream_get, b, BOB, json!({"ids": [id]}));
    r["list"][0].clone()
}

async fn recording(b: &jmap_vtc_server::memory::MemoryBackend, id: &Id) -> Value {
    let r = call!(handle_vtc_recording_get, b, BOB, json!({"ids": [id]}));
    r["list"][0].clone()
}

/// `"starting"` → `"live"` sets `startedAt` and the endpoints, and is an
/// ordinary change: `/changes` reports it as `updated`.
#[tokio::test]
async fn livestream_live_sets_started_at_and_endpoints() {
    let b = media_backend();
    let id = hosted(&b).await;
    let since = call!(handle_vtc_livestream_get, &b, BOB, json!({"ids": []}))["state"].clone();

    let caller = jmap_types::Id::from(SYSTEM);
    let reporter = MediaReporter::new(&b, &caller, account());
    reporter
        .livestream_live(&id, vec![hls()])
        .await
        .expect("live");

    let ls = livestream(&b, &id).await;
    assert_eq!(ls["state"], "live", "{ls}");
    assert!(ls["startedAt"].is_string(), "{ls}");
    assert_eq!(
        ls["playbackEndpoints"][0]["url"],
        "https://cdn.example.com/live/01J4XQ7V/index.m3u8"
    );
    let changes = call!(
        handle_vtc_livestream_changes,
        &b,
        BOB,
        json!({"sinceState": since})
    );
    assert_eq!(changes["updated"], json!([id]), "{changes}");
}

/// "`"stopped"` and `"failed"` are final", and `"live"` is reached only
/// from `"starting"`: a second report is refused with the stored state.
#[tokio::test]
async fn livestream_live_twice_is_wrong_state() {
    let b = media_backend();
    let id = hosted(&b).await;
    let caller = jmap_types::Id::from(SYSTEM);
    let reporter = MediaReporter::new(&b, &caller, account());
    reporter.livestream_live(&id, vec![]).await.expect("live");
    let err = reporter.livestream_live(&id, vec![]).await.unwrap_err();
    assert!(
        matches!(err, ReportError::WrongState(LivestreamState::Live)),
        "{err:?}"
    );
}

/// "Always empty in push mode".
#[tokio::test]
async fn push_livestream_takes_no_endpoints() {
    let b = media_backend();
    seed_as(
        &b,
        "VTCLivestream",
        stored_livestream("L1", "starting", Value::Null),
    );
    let caller = jmap_types::Id::from(SYSTEM);
    let reporter = MediaReporter::new(&b, &caller, account());
    let err = reporter
        .livestream_live(&Id::from("L1"), vec![hls()])
        .await
        .unwrap_err();
    assert!(matches!(err, ReportError::Invalid(_)), "{err:?}");
    assert_eq!(livestream(&b, &Id::from("L1")).await["state"], "starting");
    reporter
        .livestream_live(&Id::from("L1"), vec![])
        .await
        .expect("a push livestream goes live without endpoints");
}

/// `"live"` → `"failed"`: `failureReason`, `stoppedAt`, no endpoints, and
/// the stop effect so the pipeline is released.
#[tokio::test]
async fn livestream_failed_sets_reason_and_requests_stop() {
    let b = media_backend();
    let id = hosted(&b).await;
    let caller = jmap_types::Id::from(SYSTEM);
    let reporter = MediaReporter::new(&b, &caller, account());
    reporter
        .livestream_live(&id, vec![hls()])
        .await
        .expect("live");
    reporter
        .livestream_failed(&id, LivestreamFailureReason::Interrupted)
        .await
        .expect("failed");

    let ls = livestream(&b, &id).await;
    assert_eq!(ls["state"], "failed", "{ls}");
    assert_eq!(ls["failureReason"], "interrupted");
    assert!(ls["stoppedAt"].is_string());
    assert_eq!(ls["playbackEndpoints"], json!([]));
    let effects = b.media_effects_for_test();
    let Some(MediaEffect::StopLivestream(stopped)) = effects.last() else {
        panic!("{effects:?}");
    };
    assert_eq!(stopped.state, LivestreamState::Failed);
}

/// The idle stop moves the livestream to `"stopped"` "as if a moderator
/// had set it", stop effect included.
#[tokio::test]
async fn livestream_stopped_is_a_server_stop() {
    let b = media_backend();
    let id = hosted(&b).await;
    let caller = jmap_types::Id::from(SYSTEM);
    let reporter = MediaReporter::new(&b, &caller, account());
    reporter.livestream_stopped(&id).await.expect("stopped");
    assert_eq!(livestream(&b, &id).await["state"], "stopped");
    assert!(matches!(
        b.media_effects_for_test().last(),
        Some(MediaEffect::StopLivestream(_))
    ));
    let err = reporter
        .livestream_failed(&id, LivestreamFailureReason::Timeout)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ReportError::WrongState(LivestreamState::Stopped)),
        "{err:?}"
    );
}

#[tokio::test]
async fn unknown_livestream_is_not_found() {
    let b = media_backend();
    let caller = jmap_types::Id::from(SYSTEM);
    let reporter = MediaReporter::new(&b, &caller, account());
    let err = reporter
        .livestream_stopped(&Id::from("nope"))
        .await
        .unwrap_err();
    assert!(matches!(err, ReportError::NotFound), "{err:?}");
}

/// `"stopped"` → `"processing"` → `"available"` with the file properties.
#[tokio::test]
async fn recording_processing_then_available() {
    let b = media_backend();
    let r =
        call!(ids handle_vtc_recording_set, &b, BOB, json!({"create": {"r0": {"callId": "C1"}}}));
    let id = Id::from(r["created"]["r0"]["id"].as_str().expect("created"));
    let caller = jmap_types::Id::from(SYSTEM);
    let reporter = MediaReporter::new(&b, &caller, account());

    let err = reporter
        .recording_processing(&id, Some(61))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ReportError::WrongState(RecordingState::Recording)),
        "{err:?}"
    );

    reporter.recording_stopped(&id).await.expect("stopped");
    let events = b.recording_events_for_test();
    let last = events.last().expect("event");
    assert_eq!(last.state, RecordingState::Stopped);
    assert_eq!(last.initiated_by, SYSTEM);
    assert!(matches!(
        b.media_effects_for_test().last(),
        Some(MediaEffect::StopRecording(_))
    ));

    reporter
        .recording_processing(&id, Some(61))
        .await
        .expect("processing");
    assert_eq!(recording(&b, &id).await["state"], "processing");
    reporter
        .recording_available(
            &id,
            RecordingFile::new(Id::from("B1"), 4_200_000, 61, "video/mp4"),
        )
        .await
        .expect("available");
    let rec = recording(&b, &id).await;
    assert_eq!(rec["state"], "available", "{rec}");
    assert_eq!(rec["blobId"], "B1");
    assert_eq!(rec["size"], 4_200_000);
    assert_eq!(rec["duration"], 61);
    assert_eq!(rec["mediaType"], "video/mp4");

    let err = reporter.recording_failed(&id).await.unwrap_err();
    assert!(
        matches!(err, ReportError::WrongState(RecordingState::Available)),
        "{err:?}"
    );
}

/// A server pause delivers the same consent event as a moderator's.
#[tokio::test]
async fn recording_paused_delivers_the_event() {
    let b = media_backend();
    let r =
        call!(ids handle_vtc_recording_set, &b, BOB, json!({"create": {"r0": {"callId": "C1"}}}));
    let id = Id::from(r["created"]["r0"]["id"].as_str().expect("created"));
    let caller = jmap_types::Id::from(SYSTEM);
    let reporter = MediaReporter::new(&b, &caller, account());
    reporter.recording_paused(&id).await.expect("paused");
    reporter.recording_resumed(&id).await.expect("resumed");
    let states: Vec<RecordingState> = b
        .recording_events_for_test()
        .into_iter()
        .map(|e| e.state)
        .collect();
    assert_eq!(
        states,
        [
            RecordingState::Recording,
            RecordingState::Paused,
            RecordingState::Recording
        ]
    );
    reporter.recording_stopped(&id).await.expect("stopped");
    reporter.recording_failed(&id).await.expect("failed");
    assert_eq!(recording(&b, &id).await["state"], "failed");
}
