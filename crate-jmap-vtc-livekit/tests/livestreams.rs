//! The livestream adapter end to end: `jmap-vtc-server` handlers and
//! `MemoryBackend` (which records the media effects), the adapter against
//! a local Egress stub, and egress states fed back through
//! `MediaReporter`.
//!
//! Oracles: draft-atwood-jmap-vtc-00 §Livestream States, §VTCLivestream
//! (`playbackEndpoints` "Always empty in push mode", `failureReason`
//! values), §Presenting the Stream Key (RTMP stream name appended to the
//! application path, SRT stream ID), §Composition Layouts; LiveKit's
//! Egress API reference and `livekit_egress.proto` (request fields,
//! `EgressStatus`), the composite-recording page ("When all of the
//! participants leave the room, the recording would stop automatically")
//! and LiveKit's `StopEgress` (`failed_precondition` for an ended egress).

mod common;

use common::{
    ACCOUNT, ROOM, assert_egress_auth, backend, body, client, egress_info, last_effect,
    livestream_get, livestream_set, on, sent, started_livestream, system, twirp_error,
};
use jmap_types::Id;
use jmap_vtc_livekit::egress::{EgressInfo, S3Upload};
use jmap_vtc_livekit::url::Url;
use jmap_vtc_livekit::{
    EgressJob, EgressReportError, HlsOutput, HostedOutput, JobObject, LiveKitConfig, LiveKitMedia,
    OriginEndpoint, OriginOutput, Reported,
};
use jmap_vtc_server::MediaEffect;
use jmap_vtc_server::media_report::{MediaReporter, ReportError};
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_types::{LatencyHint, LivestreamMode, LivestreamState, PlaybackProtocol};
use serde_json::{Value, json};
use wiremock::{MockServer, ResponseTemplate};

/// An `EgressInfo` as a webhook carries it (lowerCamelCase, defaults
/// omitted).
fn info(egress_id: &str, status: &str, started: bool) -> EgressInfo {
    let mut v = json!({"egressId": egress_id, "roomName": ROOM, "status": status});
    if started {
        v["startedAt"] = json!("1700000000000000000");
    }
    serde_json::from_value(v).unwrap()
}

async fn stub(server: &MockServer, egress_id: &str) {
    on(
        server,
        "StartEgress",
        ResponseTemplate::new(200).set_body_json(egress_info(egress_id, "EGRESS_STARTING")),
    )
    .await;
    on(
        server,
        "StopEgress",
        ResponseTemplate::new(200).set_body_json(egress_info(egress_id, "EGRESS_ENDING")),
    )
    .await;
    on(
        server,
        "UpdateLayout",
        ResponseTemplate::new(200).set_body_json(egress_info(egress_id, "EGRESS_ACTIVE")),
    )
    .await;
}

async fn create(b: &MemoryBackend, create: Value) -> Id {
    let r = livestream_set(b, json!({"create": {"s1": create}})).await;
    Id::from(
        r["created"]["s1"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{r}")),
    )
}

/// Push to RTMPS in the `grid` layout → `"live"` with no endpoints →
/// layout change → moderator stop → the late `egress_ended` changes
/// nothing.
#[tokio::test]
async fn push_livestream_lifecycle() {
    let server = MockServer::start().await;
    stub(&server, "EG_push1").await;
    let media = LiveKitMedia::new(client(&server), LiveKitConfig::default());
    let b = backend();
    let id = create(
        &b,
        json!({
            "callId": "C1", "mode": "push", "layout": "grid",
            "streamUri": "rtmps://live.example.com:443/app",
            "streamKey": "sk_live_1"
        }),
    )
    .await;

    let job = media
        .start_livestream(&started_livestream(&b), ROOM)
        .await
        .unwrap();
    assert_eq!(job.egress_id, "EG_push1");
    assert_eq!(
        job.object,
        JobObject::Livestream {
            id: id.clone(),
            mode: LivestreamMode::Push
        }
    );
    let r = &sent(&server, "StartEgress").await[0];
    assert_egress_auth(r);
    assert_eq!(
        body(r),
        json!({
            "room_name": ROOM,
            "template": {"layout": "grid"},
            "outputs": [{"stream": {
                "protocol": "RTMP",
                "urls": ["rtmps://live.example.com:443/app/sk_live_1"]
            }}]
        })
    );
    // The job is serializable, so the deployment keeps it with the
    // livestream; the stored form still works.
    let job: EgressJob = serde_json::from_value(serde_json::to_value(&job).unwrap()).unwrap();

    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    assert_eq!(
        media
            .report_egress(&reporter, &job, &info("EG_push1", "EGRESS_STARTING", false))
            .await
            .unwrap(),
        Reported::Nothing
    );
    assert_eq!(livestream_get(&b, &id).await["state"], "starting");
    assert_eq!(
        media
            .report_egress(&reporter, &job, &info("EG_push1", "EGRESS_ACTIVE", true))
            .await
            .unwrap(),
        Reported::Livestream
    );
    let ls = livestream_get(&b, &id).await;
    assert_eq!(ls["state"], "live", "{ls}");
    assert_eq!(ls["playbackEndpoints"], json!([]));

    // The moderator switches to single-speaker.
    let r = livestream_set(
        &b,
        json!({"update": {id.as_ref(): {"layout": "single-speaker"}}}),
    )
    .await;
    assert!(r["notUpdated"].is_null(), "{r}");
    let Some(MediaEffect::ChangeLivestreamLayout(ls)) = last_effect(&b) else {
        panic!("no layout effect");
    };
    media.change_layout(&job, ls.layout.as_ref()).await.unwrap();
    assert_eq!(
        body(&sent(&server, "UpdateLayout").await[0]),
        json!({"egress_id": "EG_push1", "layout": "single-speaker"})
    );

    // The moderator stops it.
    let r = livestream_set(&b, json!({"update": {id.as_ref(): {"state": "stopped"}}})).await;
    assert!(r["notUpdated"].is_null(), "{r}");
    assert!(matches!(
        last_effect(&b),
        Some(MediaEffect::StopLivestream(_))
    ));
    media.stop(&job).await.unwrap();
    assert_eq!(
        body(&sent(&server, "StopEgress").await[0]),
        json!({"egress_id": "EG_push1"})
    );
    // The egress_ended that follows finds it stopped.
    let late = media
        .report_egress(&reporter, &job, &info("EG_push1", "EGRESS_COMPLETE", true))
        .await;
    assert!(
        matches!(
            late,
            Err(EgressReportError::Livestream(ReportError::WrongState(
                LivestreamState::Stopped
            )))
        ),
        "{late:?}"
    );
    assert_eq!(livestream_get(&b, &id).await["state"], "stopped");
}

/// Hosted as HLS in object storage: segment output under the key prefix,
/// the live playlist as the endpoint.
#[tokio::test]
async fn hosted_hls_livestream() {
    let server = MockServer::start().await;
    stub(&server, "EG_hls1").await;
    let config = LiveKitConfig::default()
        .with_storage(
            S3Upload::new("AKIDEXAMPLE", "s3-secret", "media")
                .with_endpoint("https://s3.example.net"),
        )
        .with_hosted(HostedOutput::Hls(HlsOutput::new(
            "vtc/live/",
            Url::parse("https://cdn.example.com/live/").unwrap(),
        )));
    let media = LiveKitMedia::new(client(&server), config);
    assert_eq!(
        media.advertised_modes(),
        [LivestreamMode::Push, LivestreamMode::Hosted]
    );
    let b = backend();
    let id = create(&b, json!({"callId": "C1", "mode": "hosted"})).await;
    let job = media
        .start_livestream(&started_livestream(&b), ROOM)
        .await
        .unwrap();
    assert_eq!(
        body(&sent(&server, "StartEgress").await[0]),
        json!({
            "room_name": ROOM,
            "template": {"layout": "speaker"},
            "outputs": [{
                "segments": {
                    "protocol": "HLS_PROTOCOL",
                    "filename_prefix": format!("vtc/live/{id}/segment"),
                    "playlist_name": format!("vtc/live/{id}/index.m3u8"),
                    "live_playlist_name": format!("vtc/live/{id}/live.m3u8"),
                    "segment_duration": 2
                },
                "storage": {"s3": {
                    "access_key": "AKIDEXAMPLE",
                    "secret": "s3-secret",
                    "endpoint": "https://s3.example.net",
                    "bucket": "media",
                    "force_path_style": true
                }}
            }]
        })
    );

    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    media
        .report_egress(&reporter, &job, &info("EG_hls1", "EGRESS_ACTIVE", true))
        .await
        .unwrap();
    let ls = livestream_get(&b, &id).await;
    assert_eq!(ls["state"], "live", "{ls}");
    assert_eq!(ls["playbackEndpoints"].as_array().map(Vec::len), Some(1));
    assert_eq!(ls["playbackEndpoints"][0]["protocol"], "hls");
    assert_eq!(
        ls["playbackEndpoints"][0]["url"],
        format!("https://cdn.example.com/live/{id}/live.m3u8")
    );
    assert_eq!(ls["playbackEndpoints"][0]["latencyHint"], "standard");
}

/// Hosted through a media origin (the MediaMTX deployment option): an SRT
/// publish with the id in the stream ID, WHEP and HLS endpoints.
#[tokio::test]
async fn hosted_origin_livestream() {
    let server = MockServer::start().await;
    stub(&server, "EG_mtx1").await;
    let origin = OriginOutput::new("srt://mtx.example.com:8890?streamid=publish:vtc/{id}")
        .with_endpoint(OriginEndpoint::new(
            PlaybackProtocol::Whep,
            "https://mtx.example.com/vtc/{id}/whep",
            LatencyHint::Interactive,
        ))
        .with_endpoint(OriginEndpoint::new(
            PlaybackProtocol::Hls,
            "https://mtx.example.com/vtc/{id}/index.m3u8",
            LatencyHint::Low,
        ));
    let media = LiveKitMedia::new(
        client(&server),
        LiveKitConfig::default().with_hosted(HostedOutput::Origin(origin)),
    );
    let b = backend();
    let id = create(&b, json!({"callId": "C1", "mode": "hosted"})).await;
    let job = media
        .start_livestream(&started_livestream(&b), ROOM)
        .await
        .unwrap();
    assert_eq!(
        body(&sent(&server, "StartEgress").await[0])["outputs"],
        json!([{"stream": {
            "protocol": "SRT",
            "urls": [format!("srt://mtx.example.com:8890?streamid=publish:vtc/{id}")]
        }}])
    );
    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    media
        .report_egress(&reporter, &job, &info("EG_mtx1", "EGRESS_ACTIVE", true))
        .await
        .unwrap();
    let ls = livestream_get(&b, &id).await;
    let eps = ls["playbackEndpoints"].as_array().unwrap();
    assert_eq!(eps.len(), 2, "{ls}");
    assert_eq!(eps[0]["protocol"], "whep");
    assert_eq!(
        eps[0]["url"],
        format!("https://mtx.example.com/vtc/{id}/whep")
    );
    assert_eq!(eps[0]["latencyHint"], "interactive");
    assert_eq!(eps[1]["protocol"], "hls");
    assert_eq!(eps[1]["latencyHint"], "low");
}

/// Everyone left the room: LiveKit completes the egress on its own, which
/// is the idle stop; the stop effect that follows is answered
/// `failed_precondition` and is fine.
#[tokio::test]
async fn room_emptied_is_the_idle_stop() {
    let server = MockServer::start().await;
    on(
        &server,
        "StartEgress",
        ResponseTemplate::new(200).set_body_json(egress_info("EG_idle", "EGRESS_STARTING")),
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
    let id = create(
        &b,
        json!({"callId": "C1", "mode": "push", "streamUri": "srt://ingest.example.com:9000", "streamKey": "k1"}),
    )
    .await;
    let job = media
        .start_livestream(&started_livestream(&b), ROOM)
        .await
        .unwrap();
    assert_eq!(
        body(&sent(&server, "StartEgress").await[0])["outputs"][0]["stream"]["urls"],
        json!(["srt://ingest.example.com:9000?streamid=k1"])
    );
    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    media
        .report_egress(&reporter, &job, &info("EG_idle", "EGRESS_ACTIVE", true))
        .await
        .unwrap();
    media
        .report_egress(&reporter, &job, &info("EG_idle", "EGRESS_COMPLETE", true))
        .await
        .unwrap();
    let ls = livestream_get(&b, &id).await;
    assert_eq!(ls["state"], "stopped", "{ls}");
    assert!(matches!(
        last_effect(&b),
        Some(MediaEffect::StopLivestream(_))
    ));
    media.stop(&job).await.unwrap();
}

/// Failures: a start LiveKit cannot serve, an egress that fails before
/// and after going live, and an info for another egress.
#[tokio::test]
async fn failures() {
    let server = MockServer::start().await;
    on(
        &server,
        "StartEgress",
        twirp_error(503, "unavailable", "no response from servers"),
    )
    .await;
    let media = LiveKitMedia::new(client(&server), LiveKitConfig::default());
    let b = backend();
    let caller = system();
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));

    // No egress worker: "unavailable".
    let id = create(
        &b,
        json!({"callId": "C1", "mode": "push", "streamUri": "rtmp://live.example.com/app", "streamKey": "k"}),
    )
    .await;
    let e = media
        .start_livestream(&started_livestream(&b), ROOM)
        .await
        .unwrap_err();
    reporter
        .livestream_failed(&id, e.failure_reason())
        .await
        .unwrap();
    let ls = livestream_get(&b, &id).await;
    assert_eq!(ls["state"], "failed");
    assert_eq!(ls["failureReason"], "unavailable");

    // Hosted without a hosted output: refused before any call.
    let hosted = create(&b, json!({"callId": "C1", "mode": "hosted"})).await;
    let e = media
        .start_livestream(&started_livestream(&b), ROOM)
        .await
        .unwrap_err();
    assert_eq!(
        e.set_error().unwrap().properties,
        Some(vec!["mode".to_owned()])
    );
    reporter
        .livestream_failed(&hosted, e.failure_reason())
        .await
        .unwrap();
    assert_eq!(
        livestream_get(&b, &hosted).await["failureReason"],
        "rejected"
    );

    // EGRESS_FAILED before it started: "rejected"; after: "interrupted".
    let job = |id: &Id, egress: &str| -> EgressJob {
        serde_json::from_value(json!({
            "egressId": egress,
            "object": {"type": "livestream", "id": id, "mode": "push"}
        }))
        .unwrap()
    };
    let early = create(
        &b,
        json!({"callId": "C1", "mode": "push", "streamUri": "rtmp://live.example.com/app", "streamKey": "k"}),
    )
    .await;
    media
        .report_egress(
            &reporter,
            &job(&early, "EG_f1"),
            &info("EG_f1", "EGRESS_FAILED", false),
        )
        .await
        .unwrap();
    assert_eq!(
        livestream_get(&b, &early).await["failureReason"],
        "rejected"
    );
    let late = create(
        &b,
        json!({"callId": "C1", "mode": "push", "streamUri": "rtmp://live.example.com/app", "streamKey": "k"}),
    )
    .await;
    let j = job(&late, "EG_f2");
    media
        .report_egress(&reporter, &j, &info("EG_f2", "EGRESS_ACTIVE", true))
        .await
        .unwrap();
    media
        .report_egress(&reporter, &j, &info("EG_f2", "EGRESS_FAILED", true))
        .await
        .unwrap();
    let ls = livestream_get(&b, &late).await;
    assert_eq!(ls["state"], "failed");
    assert_eq!(ls["failureReason"], "interrupted");
    assert_eq!(ls["playbackEndpoints"], json!([]));

    // An info for another egress changes nothing.
    assert!(matches!(
        media
            .report_egress(&reporter, &j, &info("EG_other", "EGRESS_ACTIVE", true))
            .await,
        Err(EgressReportError::OtherEgress)
    ));
}
