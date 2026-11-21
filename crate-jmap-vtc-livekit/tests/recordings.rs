//! The recording adapter end to end: create and stop through the
//! `jmap-vtc-server` handlers, the effects applied to the adapter against
//! a local Egress stub, and the egress's end fed back through
//! `MediaReporter`.
//!
//! Oracles: draft-atwood-jmap-vtc-00 §VTCRecording, §Pausing, Resuming,
//! and Stopping a Recording (`"stopped"` → `"processing"` →
//! `"available"` with `blobId`, `size`, `duration` and `mediaType`, or
//! `"failed"`), §Capability (`supportsRecordingPause`); LiveKit's Egress
//! API reference (`EncodedFileOutput`, `FileInfo` "duration
//! (nanoseconds), size (bytes), location"), the composite-recording page
//! (the recording stops when everyone leaves) and `StopEgress`
//! (`failed_precondition` for an ended egress).

mod common;

use common::{
    ACCOUNT, ROOM, assert_egress_auth, backend, body, client, egress_info, last_effect, on,
    recording_get, recording_set, sent, started_recording, system, twirp_error,
};
use jmap_types::Id;
use jmap_vtc_livekit::egress::{EgressInfo, S3Upload};
use jmap_vtc_livekit::{
    EgressJob, EgressOutcome, EgressReportError, JobObject, LiveKitConfig, LiveKitMedia, Reported,
};
use jmap_vtc_server::MediaEffect;
use jmap_vtc_server::media_report::{MediaReporter, ReportError};
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_types::RecordingState;
use serde_json::json;
use wiremock::{MockServer, ResponseTemplate};

const LOCATION: &str = "https://media.s3.us-west-2.amazonaws.com/jmap-vtc/recordings/R.mp4";

/// An `egress_ended` info as a webhook carries it, with one MP4 of 65.4 s
/// and 7 MiB.
fn ended(egress_id: &str, status: &str, with_file: bool) -> EgressInfo {
    let mut v = json!({
        "egressId": egress_id, "roomName": ROOM, "status": status,
        "startedAt": "1700000000000000000", "endedAt": "1700000065400000000"
    });
    if with_file {
        v["fileResults"] = json!([{
            "filename": "jmap-vtc/recordings/R.mp4",
            "startedAt": "1700000000000000000",
            "endedAt": "1700000065400000000",
            "duration": "65400000000",
            "size": "7340032",
            "location": LOCATION
        }]);
    }
    serde_json::from_value(v).unwrap()
}

async fn create(b: &MemoryBackend, layout: Option<&str>) -> Id {
    let mut c = json!({"callId": "C1"});
    if let Some(l) = layout {
        c["layout"] = json!(l);
    }
    let r = recording_set(b, json!({"create": {"r1": c}})).await;
    Id::from(
        r["created"]["r1"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{r}")),
    )
}

/// Record in `speaker`, stop, `egress_ended` with the file → `"processing"`
/// with the duration → import → `"available"`. A duplicate webhook hands
/// out nothing.
#[tokio::test]
async fn recording_lifecycle() {
    let server = MockServer::start().await;
    on(
        &server,
        "StartRoomCompositeEgress",
        ResponseTemplate::new(200).set_body_json(egress_info("EG_rec1", "EGRESS_STARTING")),
    )
    .await;
    on(
        &server,
        "StopEgress",
        ResponseTemplate::new(200).set_body_json(egress_info("EG_rec1", "EGRESS_ENDING")),
    )
    .await;
    let media = LiveKitMedia::new(
        client(&server),
        LiveKitConfig::default().with_storage(
            S3Upload::new("AKIDEXAMPLE", "s3-secret", "media").with_region("us-west-2"),
        ),
    );
    let b = backend();
    let id = create(&b, Some("speaker")).await;
    let job = media
        .start_recording(&started_recording(&b), ROOM)
        .await
        .unwrap();
    assert_eq!(job.object, JobObject::Recording { id: id.clone() });
    let r = &sent(&server, "StartRoomCompositeEgress").await[0];
    assert_egress_auth(r);
    assert_eq!(
        body(r),
        json!({
            "room_name": ROOM,
            "layout": "speaker",
            "file_outputs": [{
                "file_type": "MP4",
                "filepath": format!("jmap-vtc/recordings/{id}.mp4"),
                "s3": {
                    "access_key": "AKIDEXAMPLE",
                    "secret": "s3-secret",
                    "region": "us-west-2",
                    "bucket": "media"
                }
            }]
        })
    );

    let r = recording_set(&b, json!({"update": {id.as_ref(): {"state": "stopped"}}})).await;
    assert!(r["notUpdated"].is_null(), "{r}");
    assert!(matches!(
        last_effect(&b),
        Some(MediaEffect::StopRecording(_))
    ));
    media.stop(&job).await.unwrap();
    assert_eq!(
        body(&sent(&server, "StopEgress").await[0]),
        json!({"egress_id": "EG_rec1"})
    );

    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    let Reported::RecordingProcessing(file) = media
        .report_egress(&reporter, &job, &ended("EG_rec1", "EGRESS_COMPLETE", true))
        .await
        .unwrap()
    else {
        panic!("no file");
    };
    assert_eq!(file.location, LOCATION);
    assert_eq!(file.size, 7_340_032);
    assert_eq!(file.duration, 65);
    assert_eq!(file.media_type, "video/mp4");
    let rec = recording_get(&b, &id).await;
    assert_eq!(rec["state"], "processing", "{rec}");
    assert_eq!(rec["duration"], 65);

    // The deployment fetches `location` into its blob store.
    reporter
        .recording_available(&id, file.recording_file(Id::from("B42")))
        .await
        .unwrap();
    let rec = recording_get(&b, &id).await;
    assert_eq!(rec["state"], "available");
    assert_eq!(rec["blobId"], "B42");
    assert_eq!(rec["size"], 7_340_032);
    assert_eq!(rec["mediaType"], "video/mp4");

    // LiveKit retries webhooks; the duplicate finds it past "stopped".
    let dup = media
        .report_egress(&reporter, &job, &ended("EG_rec1", "EGRESS_COMPLETE", true))
        .await;
    assert!(
        matches!(
            dup,
            Err(EgressReportError::Recording(ReportError::WrongState(
                RecordingState::Available
            )))
        ),
        "{dup:?}"
    );
}

/// `supportsRecordingPause: false`: the server refuses `"paused"`, so the
/// adapter never sees a pause effect.
#[tokio::test]
async fn pause_is_refused_by_the_capability() {
    let b = backend();
    let id = create(&b, None).await;
    let r = recording_set(&b, json!({"update": {id.as_ref(): {"state": "paused"}}})).await;
    assert_eq!(
        r["notUpdated"][id.as_ref() as &str]["type"],
        "invalidProperties",
        "{r}"
    );
    assert!(matches!(
        last_effect(&b),
        Some(MediaEffect::StartRecording(_))
    ));
}

/// Everyone left: the egress completes while the recording is still
/// `"recording"`; it is stopped, then processed, and the stop effect that
/// follows is answered `failed_precondition`, which is fine.
#[tokio::test]
async fn room_emptied_stops_the_recording() {
    let server = MockServer::start().await;
    on(
        &server,
        "StartRoomCompositeEgress",
        ResponseTemplate::new(200).set_body_json(egress_info("EG_rec2", "EGRESS_STARTING")),
    )
    .await;
    on(
        &server,
        "StopEgress",
        twirp_error(
            412,
            "failed_precondition",
            "egress with status EGRESS_COMPLETE cannot be stopped",
        ),
    )
    .await;
    let media = LiveKitMedia::new(client(&server), LiveKitConfig::default());
    let b = backend();
    let id = create(&b, None).await;
    let job = media
        .start_recording(&started_recording(&b), ROOM)
        .await
        .unwrap();
    // `layout: null` is drawn in the default layout.
    assert_eq!(
        body(&sent(&server, "StartRoomCompositeEgress").await[0])["layout"],
        "speaker"
    );
    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    let reported = media
        .report_egress(&reporter, &job, &ended("EG_rec2", "EGRESS_COMPLETE", true))
        .await
        .unwrap();
    assert!(matches!(reported, Reported::RecordingProcessing(_)));
    let rec = recording_get(&b, &id).await;
    assert_eq!(rec["state"], "processing", "{rec}");
    assert!(rec["stoppedAt"].is_string());
    let effects = b.media_effects_for_test();
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, MediaEffect::StopRecording(r) if r.id == id))
    );
    media.stop(&job).await.unwrap();
}

/// A failed egress fails the recording; one that completes without a file
/// does too.
#[tokio::test]
async fn failed_egress_fails_the_recording() {
    let media = LiveKitMedia::new(client(&MockServer::start().await), LiveKitConfig::default());
    let b = backend();
    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    for (egress, status, with_file) in [
        ("EG_f1", "EGRESS_FAILED", false),
        ("EG_f2", "EGRESS_ABORTED", false),
        ("EG_f3", "EGRESS_COMPLETE", false),
    ] {
        let id = create(&b, None).await;
        let job: EgressJob = serde_json::from_value(json!({
            "egressId": egress, "object": {"type": "recording", "id": id}
        }))
        .unwrap();
        let info = ended(egress, status, with_file);
        assert_eq!(
            media.egress_outcome(&job, &info).unwrap(),
            EgressOutcome::RecordingFailed
        );
        assert_eq!(
            media.report_egress(&reporter, &job, &info).await.unwrap(),
            Reported::RecordingFailed
        );
        let rec = recording_get(&b, &id).await;
        assert_eq!(rec["state"], "failed", "{status}: {rec}");
    }
}

/// `EGRESS_LIMIT_REACHED` (LiveKit's time limit) still uploads the file
/// written so far.
#[tokio::test]
async fn limit_reached_keeps_the_file() {
    let media = LiveKitMedia::new(client(&MockServer::start().await), LiveKitConfig::default());
    let job: EgressJob = serde_json::from_value(json!({
        "egressId": "EG_l", "object": {"type": "recording", "id": "R1"}
    }))
    .unwrap();
    let outcome = media
        .egress_outcome(&job, &ended("EG_l", "EGRESS_LIMIT_REACHED", true))
        .unwrap();
    assert!(matches!(outcome, EgressOutcome::RecordingUploaded(f) if f.duration == 65));
    // While it runs there is nothing to report.
    for status in ["EGRESS_STARTING", "EGRESS_ACTIVE", "EGRESS_ENDING"] {
        assert_eq!(
            media
                .egress_outcome(&job, &ended("EG_l", status, false))
                .unwrap(),
            EgressOutcome::Nothing
        );
    }
}
