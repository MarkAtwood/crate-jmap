//! The Egress Twirp client against a local stub.
//!
//! Oracles: the Egress API reference,
//! <https://docs.livekit.io/reference/other/egress/api/> ("POST ... to
//! `https://<your-livekit-host>/twirp/livekit.Egress/<MethodName>`",
//! `Authorization: Bearer`, `Content-Type: application/json`, the
//! `roomRecord` permission); `protobufs/livekit_egress.proto` in
//! livekit/protocol `main` for the RPC name `StartEgress` and every field
//! and enum name (`StartEgressRequest` with its `source` oneof member
//! `template` (`TemplateSource`) and `encoding` oneof member `preset`;
//! `Output` with its `config` oneof (`file`, `stream`, `segments`) and
//! `storage`; `StorageConfig`, `FileOutput`, `S3Upload`,
//! `SegmentedFileOutput`, `StreamOutput`, `UpdateLayoutRequest`,
//! `StopEgressRequest`, `ListEgressRequest`, `EgressInfo`); protojson,
//! which writes a oneof as its member's field; the Twirp wire protocol's
//! error body (`{"code", "msg"}`) and its `bad_route` (404) for a method
//! the server does not have; livekit/livekit v1.13.1's `StartEgress`
//! ("not implemented").

mod common;

use common::{API_KEY, ROOM, assert_egress_auth, body, client, egress_info, on, sent, twirp_error};
use jmap_vtc_livekit::egress::{
    EgressClient, EgressStatus, EncodingPreset, FileOutput, Output, S3Upload, SegmentOutput,
    StartEgressRequest, StreamOutput, StreamProtocol,
};
use jmap_vtc_livekit::token::ApiKey;
use jmap_vtc_livekit::url::Url;
use jmap_vtc_livekit::{LiveKitError, TwirpCode};
use jmap_vtc_types::LivestreamFailureReason;
use serde_json::json;
use wiremock::{MockServer, ResponseTemplate};

#[tokio::test]
async fn start_egress_sends_the_proto_fields() {
    let server = MockServer::start().await;
    on(
        &server,
        "StartEgress",
        ResponseTemplate::new(200).set_body_json(egress_info("EG_abc", "EGRESS_STARTING")),
    )
    .await;
    let s3 = S3Upload::new("AKIDEXAMPLE", "s3-secret", "recordings").with_region("us-west-2");
    let req = StartEgressRequest::room_composite(ROOM)
        .with_layout("grid-light")
        .with_preset(EncodingPreset::H264_1080p30)
        .with_custom_base_url("https://templates.example.com/vtc")
        .with_output(
            Output::from(FileOutput::mp4("jmap-vtc/recordings/R1.mp4")).with_s3(s3.clone()),
        )
        .with_output(StreamOutput::new(
            StreamProtocol::Rtmp,
            vec!["rtmps://live.example.com:443/app/sk_1".to_owned()],
        ))
        .with_output(
            SegmentOutput::hls("live/L1/segment", "live/L1/index.m3u8")
                .with_live_playlist("live/L1/live.m3u8")
                .with_segment_duration(2),
        )
        .with_storage(s3.clone().with_session_token("tok"));
    let info = client(&server).start_egress(&req).await.unwrap();
    assert_eq!(info.egress_id, "EG_abc");
    assert_eq!(info.status, EgressStatus::Starting);
    assert_eq!(info.room_name, ROOM);

    let r = &sent(&server, "StartEgress").await[0];
    assert_egress_auth(r);
    let s3 = json!({
        "access_key": "AKIDEXAMPLE",
        "secret": "s3-secret",
        "region": "us-west-2",
        "bucket": "recordings"
    });
    let mut default_s3 = s3.clone();
    default_s3["session_token"] = json!("tok");
    assert_eq!(
        body(r),
        json!({
            "room_name": ROOM,
            "template": {
                "layout": "grid-light",
                "custom_base_url": "https://templates.example.com/vtc"
            },
            "preset": "H264_1080P_30",
            "outputs": [
                {
                    "file": {"file_type": "MP4", "filepath": "jmap-vtc/recordings/R1.mp4"},
                    "storage": {"s3": s3}
                },
                {
                    "stream": {
                        "protocol": "RTMP",
                        "urls": ["rtmps://live.example.com:443/app/sk_1"]
                    }
                },
                {
                    "segments": {
                        "protocol": "HLS_PROTOCOL",
                        "filename_prefix": "live/L1/segment",
                        "playlist_name": "live/L1/index.m3u8",
                        "live_playlist_name": "live/L1/live.m3u8",
                        "segment_duration": 2
                    }
                }
            ],
            "storage": {"s3": default_s3}
        })
    );
}

/// The smallest request: the `template` oneof member is sent even when
/// empty (it selects the room-composite source), `outputs` always, and
/// nothing else. A second output of a kind replaces the first, since the
/// egress refuses two ("multiple file outputs").
#[test]
fn minimal_request_and_one_output_per_kind() {
    let req = StartEgressRequest::room_composite(ROOM)
        .with_output(FileOutput::mp4("a.mp4"))
        .with_output(StreamOutput::new(
            StreamProtocol::Srt,
            vec!["srt://x:1".to_owned()],
        ))
        .with_output(FileOutput::mp4("b.mp4"));
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({
            "room_name": ROOM,
            "template": {},
            "outputs": [
                {"stream": {"protocol": "SRT", "urls": ["srt://x:1"]}},
                {"file": {"file_type": "MP4", "filepath": "b.mp4"}}
            ]
        })
    );
}

/// A LiveKit server older than v1.13.2 refuses `StartEgress`: through
/// v1.12.0 the Twirp router has no such method (`bad_route`, 404), and
/// v1.13.1 answers "not implemented" (a plain Go error, so `internal`,
/// 500). Both are `unavailable`, not `rejected`: the moderator's request
/// was fine, the deployment's LiveKit is too old.
#[tokio::test]
async fn older_servers_refuse_start_egress() {
    for (status, code, msg, want) in [
        (
            404,
            "bad_route",
            "no handler for path \"/twirp/livekit.Egress/StartEgress\"",
            TwirpCode::BadRoute,
        ),
        (500, "internal", "not implemented", TwirpCode::Internal),
    ] {
        let server = MockServer::start().await;
        on(&server, "StartEgress", twirp_error(status, code, msg)).await;
        let req = StartEgressRequest::room_composite(ROOM).with_output(FileOutput::mp4("a.mp4"));
        let e = client(&server).start_egress(&req).await.unwrap_err();
        assert_eq!(e.code(), Some(&want), "{e}");
        assert_eq!(e.failure_reason(), LivestreamFailureReason::Unavailable);
    }
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
