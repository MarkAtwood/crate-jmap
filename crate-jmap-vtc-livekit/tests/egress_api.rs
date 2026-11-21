//! The Egress Twirp client against a local stub.
//!
//! Oracles: the Egress API reference,
//! <https://docs.livekit.io/reference/other/egress/api/> ("POST ... to
//! `https://<your-livekit-host>/twirp/livekit.Egress/<MethodName>`",
//! `Authorization: Bearer`, `Content-Type: application/json`, the
//! `roomRecord` permission); `protobufs/livekit_egress.proto` in
//! livekit/protocol for every field and enum name
//! (`RoomCompositeEgressRequest`, `EncodedFileOutput`, `S3Upload`,
//! `SegmentedFileOutput`, `StreamOutput`, `UpdateLayoutRequest`,
//! `StopEgressRequest`, `ListEgressRequest`, `EgressInfo`); the Twirp
//! wire protocol's error body (`{"code", "msg"}`).

mod common;

use common::{API_KEY, ROOM, assert_egress_auth, body, client, egress_info, on, sent, twirp_error};
use jmap_vtc_livekit::egress::{
    EgressClient, EgressStatus, EncodingPreset, FileOutput, RoomCompositeRequest, S3Upload,
    SegmentOutput, StreamOutput, StreamProtocol,
};
use jmap_vtc_livekit::token::ApiKey;
use jmap_vtc_livekit::url::Url;
use jmap_vtc_livekit::{LiveKitError, TwirpCode};
use serde_json::json;
use wiremock::{MockServer, ResponseTemplate};

#[tokio::test]
async fn start_room_composite_sends_the_proto_fields() {
    let server = MockServer::start().await;
    on(
        &server,
        "StartRoomCompositeEgress",
        ResponseTemplate::new(200).set_body_json(egress_info("EG_abc", "EGRESS_STARTING")),
    )
    .await;
    let s3 = S3Upload::new("AKIDEXAMPLE", "s3-secret", "recordings").with_region("us-west-2");
    let req = RoomCompositeRequest::new(ROOM)
        .with_layout("grid-light")
        .with_preset(EncodingPreset::H264_1080p30)
        .with_file(FileOutput::mp4("jmap-vtc/recordings/R1.mp4").with_s3(s3.clone()))
        .with_stream(StreamOutput::new(
            StreamProtocol::Rtmp,
            vec!["rtmps://live.example.com:443/app/sk_1".to_owned()],
        ))
        .with_segments(
            SegmentOutput::hls("live/L1/segment", "live/L1/index.m3u8")
                .with_live_playlist("live/L1/live.m3u8")
                .with_segment_duration(2)
                .with_s3(s3),
        );
    let info = client(&server)
        .start_room_composite_egress(&req)
        .await
        .unwrap();
    assert_eq!(info.egress_id, "EG_abc");
    assert_eq!(info.status, EgressStatus::Starting);
    assert_eq!(info.room_name, ROOM);

    let r = &sent(&server, "StartRoomCompositeEgress").await[0];
    assert_egress_auth(r);
    let s3 = json!({
        "access_key": "AKIDEXAMPLE",
        "secret": "s3-secret",
        "region": "us-west-2",
        "bucket": "recordings"
    });
    assert_eq!(
        body(r),
        json!({
            "room_name": ROOM,
            "layout": "grid-light",
            "preset": "H264_1080P_30",
            "file_outputs": [{
                "file_type": "MP4",
                "filepath": "jmap-vtc/recordings/R1.mp4",
                "s3": s3
            }],
            "stream_outputs": [{
                "protocol": "RTMP",
                "urls": ["rtmps://live.example.com:443/app/sk_1"]
            }],
            "segment_outputs": [{
                "protocol": "HLS_PROTOCOL",
                "filename_prefix": "live/L1/segment",
                "playlist_name": "live/L1/index.m3u8",
                "live_playlist_name": "live/L1/live.m3u8",
                "segment_duration": 2,
                "s3": s3
            }]
        })
    );
}

#[tokio::test]
async fn update_layout_stop_and_list() {
    let server = MockServer::start().await;
    on(
        &server,
        "UpdateLayout",
        ResponseTemplate::new(200).set_body_json(egress_info("EG_abc", "EGRESS_ACTIVE")),
    )
    .await;
    on(
        &server,
        "StopEgress",
        ResponseTemplate::new(200).set_body_json(egress_info("EG_abc", "EGRESS_ENDING")),
    )
    .await;
    let mut active = egress_info("EG_abc", "EGRESS_ACTIVE");
    active["started_at"] = json!("1700000000000000000");
    on(
        &server,
        "ListEgress",
        ResponseTemplate::new(200)
            .set_body_json(json!({"items": [active], "next_page_token": null})),
    )
    .await;
    let c = client(&server);

    let info = c.update_layout("EG_abc", "single-speaker").await.unwrap();
    assert_eq!(info.status, EgressStatus::Active);
    let r = &sent(&server, "UpdateLayout").await[0];
    assert_egress_auth(r);
    assert_eq!(
        body(r),
        json!({"egress_id": "EG_abc", "layout": "single-speaker"})
    );

    assert_eq!(
        c.stop_egress("EG_abc").await.unwrap().status,
        EgressStatus::Ending
    );
    let r = &sent(&server, "StopEgress").await[0];
    assert_egress_auth(r);
    assert_eq!(body(r), json!({"egress_id": "EG_abc"}));

    let items = c.list_egress(Some(ROOM), None, true).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].started_at, 1_700_000_000_000_000_000);
    let r = &sent(&server, "ListEgress").await[0];
    assert_egress_auth(r);
    assert_eq!(body(r), json!({"room_name": ROOM, "active": true}));
}

#[tokio::test]
async fn twirp_errors() {
    let server = MockServer::start().await;
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
    on(
        &server,
        "UpdateLayout",
        ResponseTemplate::new(502).set_body_string("<html>bad gateway</html>"),
    )
    .await;
    let c = client(&server);
    let e = c.stop_egress("EG_done").await.unwrap_err();
    match &e {
        LiveKitError::Twirp { status, code, msg } => {
            assert_eq!(*status, 412);
            assert_eq!(*code, TwirpCode::FailedPrecondition);
            assert_eq!(
                msg.as_deref(),
                Some("egress with status EGRESS_COMPLETE cannot be stopped")
            );
        }
        other => panic!("{other:?}"),
    }
    let e = c.update_layout("EG_x", "grid").await.unwrap_err();
    assert_eq!(e.code(), Some(&TwirpCode::Other(String::new())));
}

/// A server URL with a path (a reverse proxy) keeps it; `wss://` maps to
/// `https://` (checked in the unit tests), `ws://` to `http://`.
#[tokio::test]
async fn path_prefix_and_ws_scheme() {
    let server = MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::path(
        "/livekit/twirp/livekit.Egress/StopEgress",
    ))
    .respond_with(ResponseTemplate::new(200).set_body_json(egress_info("EG_1", "EGRESS_ENDING")))
    .mount(&server)
    .await;
    let ws = server.uri().replacen("http://", "ws://", 1);
    let c = EgressClient::new(
        &Url::parse(&format!("{ws}/livekit")).unwrap(),
        ApiKey::new(API_KEY, "s"),
    )
    .unwrap();
    assert_eq!(
        c.stop_egress("EG_1").await.unwrap().status,
        EgressStatus::Ending
    );
}
