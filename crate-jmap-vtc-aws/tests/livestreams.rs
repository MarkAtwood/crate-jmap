//! The IVS livestream adapter end to end: `jmap-vtc-server` handlers and
//! `MemoryBackend` (which records the media effects), the adapter against
//! a local IVS stub, and IVS events fed back through `MediaReporter`.
//!
//! Oracles: draft-atwood-jmap-vtc-00 §Livestream States, §VTCLivestream
//! (`playbackEndpoints` "Always empty in push mode", `failureReason`
//! values), §Playback Authorization ("a server can carry the authorization
//! in `url` itself and set `credential` to `null`"); the IVS pages cited in
//! `tests/ivs_api.rs`; "Using Amazon EventBridge with IVS Low-Latency
//! Streaming" (event JSON), "Generate and Sign IVS Playback Tokens"
//! (`?token=` query parameter), and DeleteChannel ("If you try to delete a
//! live channel, you will get an error (409 ConflictException)").

mod common;

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use common::{
    CHANNEL_ARN, PLAYBACK_URL, aws_error, body, client, create_channel_response, on, sent,
};
use jmap_types::Id;
use jmap_vtc_aws::AwsError;
use jmap_vtc_aws::ivs::{
    ChannelTemplate, ChannelType, IvsChannel, IvsEvent, IvsLivestreams, PlaybackKey, StopOutcome,
};
use jmap_vtc_server::MediaEffect;
use jmap_vtc_server::media_report::{MediaReporter, ReportError};
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{handle_vtc_livestream_get, handle_vtc_livestream_set};
use jmap_vtc_types::{LivestreamFailureReason, LivestreamState, VtcCapability, VtcLivestream};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P384_SHA384_FIXED_SIGNING, EcdsaKeyPair};
use serde_json::{Value, json};
use wiremock::{MockServer, ResponseTemplate};

const ACCOUNT: &str = "A1";
const BOB: &str = "user:bob@example.com";
const SYSTEM: &str = "service:media";

/// A backend whose account allows hosted and push livestreams, holding
/// active room call C1 started by BOB (its moderator).
fn backend() -> MemoryBackend {
    let b = MemoryBackend::new();
    let cap: VtcCapability = serde_json::from_value(json!({
        "mayCreateCall": true,
        "supportsRingCalls": true,
        "supportsRoomCalls": true,
        "supportsScheduledCalls": true,
        "supportsRecording": true,
        "supportsLivestream": true,
        "livestreamModes": ["push", "hosted"],
        "supportsLobby": true,
        "supportsBreakoutRooms": true,
        "supportedMediaTypes": ["audio", "video"],
        "maxParticipantsPerCall": null,
        "maxConcurrentCalls": null,
        "maxBreakoutRooms": null
    }))
    .unwrap();
    b.register_account(&Id::from(ACCOUNT), cap);
    b.insert_object_for_test(
        "VTCCall",
        &Id::from(ACCOUNT),
        "C1",
        json!({
            "id": "C1", "accountId": ACCOUNT, "callType": "room", "state": "active",
            "createdAt": "2026-11-20T14:30:00Z", "endedAt": null, "endReason": null,
            "initiatorId": BOB, "subject": null,
            "joinUri": "https://meet.example.com/room/xkz", "joinPassword": null,
            "mediaTypes": ["audio", "video"], "activeParticipantCount": 0,
            "lobbyEnabled": false, "parentCallId": null, "breakoutRoomIds": [],
            "scheduledStartAt": null, "gatewayPin": null, "e2eeEnabled": false
        }),
    );
    b
}

async fn livestream_set(b: &MemoryBackend, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    handle_vtc_livestream_set(b, &jmap_types::Id::from(BOB), &BTreeMap::new(), args)
        .await
        .unwrap()
        .primary
}

async fn livestream_get(b: &MemoryBackend, id: &Id) -> Value {
    handle_vtc_livestream_get(
        b,
        &jmap_types::Id::from(BOB),
        json!({"accountId": ACCOUNT, "ids": [id]}),
    )
    .await
    .unwrap()
    .primary["list"][0]
        .clone()
}

/// The livestream of the last `StartLivestream` effect.
fn started(b: &MemoryBackend) -> VtcLivestream {
    b.media_effects_for_test()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            MediaEffect::StartLivestream(ls) => Some(ls),
            _ => None,
        })
        .expect("a StartLivestream effect")
}

fn event(name: &str) -> IvsEvent {
    IvsEvent::from_eventbridge(&json!({
        "version": "0",
        "id": "01234567-0123-0123-0123-012345678901",
        "detail-type": "IVS Stream State Change",
        "source": "aws.ivs",
        "account": "123456789012",
        "time": "2026-11-20T17:02:11Z",
        "region": "us-west-2",
        "resources": [CHANNEL_ARN],
        "detail": {
            "event_name": name,
            "channel_name": "jmap-vtc-L1",
            "stream_id": "st-1A2b3c4D5e6F78ghij9Klmn"
        }
    }))
    .unwrap()
}

/// Create (effect) → CreateChannel → Stream Start → `"live"` with the HLS
/// endpoint → moderator stop (effect) → StopStream + DeleteChannel.
#[tokio::test]
async fn hosted_livestream_lifecycle() {
    let server = MockServer::start().await;
    on(
        &server,
        "CreateChannel",
        ResponseTemplate::new(200).set_body_json(create_channel_response(false)),
    )
    .await;
    on(&server, "StopStream", ResponseTemplate::new(200)).await;
    on(&server, "DeleteChannel", ResponseTemplate::new(204)).await;
    let ivs = IvsLivestreams::new(
        client(&server),
        ChannelTemplate::default().with_channel_type(ChannelType::Basic),
    );
    let b = backend();

    let r = livestream_set(
        &b,
        json!({"create": {"s1": {"callId": "C1", "mode": "hosted"}}}),
    )
    .await;
    let id = Id::from(r["created"]["s1"]["id"].as_str().expect("created"));
    let channel = ivs.start(&started(&b)).await.unwrap();
    assert_eq!(channel.livestream_id, id);
    assert_eq!(
        channel.rtmps_url(),
        "rtmps://a1b2c3d4e5f6.global-contribute.live-video.net:443/app/"
    );
    let create = body(&sent(&server, "CreateChannel").await[0]);
    assert_eq!(create["name"], json!(format!("jmap-vtc-{id}")));
    assert_eq!(create["type"], "BASIC");
    assert_eq!(create["authorized"], false);
    assert_eq!(create["tags"]["jmap-vtc:livestream-id"], json!(id));
    assert_eq!(create["tags"]["jmap-vtc:call-id"], "C1");

    // The channel is serializable, so the deployment can keep it with the
    // livestream; the stored form still works.
    let channel: IvsChannel =
        serde_json::from_value(serde_json::to_value(&channel).unwrap()).unwrap();

    let caller = jmap_types::Id::from(SYSTEM);
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));
    ivs.report_event(&reporter, &channel, &event("Session Created"))
        .await
        .unwrap();
    assert_eq!(livestream_get(&b, &id).await["state"], "starting");
    ivs.report_event(&reporter, &channel, &event("Stream Start"))
        .await
        .unwrap();
    let ls = livestream_get(&b, &id).await;
    assert_eq!(ls["state"], "live", "{ls}");
    assert_eq!(ls["playbackEndpoints"][0]["protocol"], "hls");
    assert_eq!(ls["playbackEndpoints"][0]["url"], PLAYBACK_URL);
    assert_eq!(ls["playbackEndpoints"][0]["latencyHint"], "low");

    let r = livestream_set(&b, json!({"update": {id.as_ref(): {"state": "stopped"}}})).await;
    assert!(r["notUpdated"].is_null(), "{r}");
    assert!(matches!(
        b.media_effects_for_test().last(),
        Some(MediaEffect::StopLivestream(_))
    ));
    assert_eq!(ivs.stop(&channel).await.unwrap(), StopOutcome::Deleted);
    assert_eq!(
        body(&sent(&server, "StopStream").await[0]),
        json!({"channelArn": CHANNEL_ARN})
    );
    assert_eq!(
        body(&sent(&server, "DeleteChannel").await[0]),
        json!({"arn": CHANNEL_ARN})
    );

    // The Stream End that follows the stop changes nothing.
    let late = ivs
        .report_event(&reporter, &channel, &event("Stream End"))
        .await
        .unwrap_err();
    assert!(
        matches!(late, ReportError::WrongState(LivestreamState::Stopped)),
        "{late:?}"
    );
}

/// A CreateChannel failure is reported as `"failed"` with the mapped
/// reason; the report releases the pipeline with a stop effect.
#[tokio::test]
async fn quota_failure_reports_unavailable() {
    let server = MockServer::start().await;
    on(
        &server,
        "CreateChannel",
        aws_error(402, "ServiceQuotaExceededException", "quota"),
    )
    .await;
    let ivs = IvsLivestreams::new(client(&server), ChannelTemplate::default());
    let b = backend();
    livestream_set(
        &b,
        json!({"create": {"s1": {"callId": "C1", "mode": "hosted"}}}),
    )
    .await;
    let ls = started(&b);
    let err = ivs.start(&ls).await.unwrap_err();
    assert_eq!(err.failure_reason(), LivestreamFailureReason::Unavailable);

    let caller = jmap_types::Id::from(SYSTEM);
    MediaReporter::new(&b, &caller, Id::from(ACCOUNT))
        .livestream_failed(&ls.id, err.failure_reason())
        .await
        .unwrap();
    let got = livestream_get(&b, &ls.id).await;
    assert_eq!(got["state"], "failed");
    assert_eq!(got["failureReason"], "unavailable");
}

/// A push livestream has no IVS path: IVS cannot push to the moderator's
/// `streamUri`. No AWS call is made.
#[tokio::test]
async fn push_mode_is_rejected_without_a_call() {
    let server = MockServer::start().await;
    let ivs = IvsLivestreams::new(client(&server), ChannelTemplate::default());
    let b = backend();
    livestream_set(
        &b,
        json!({"create": {"s0": {
            "callId": "C1",
            "streamUri": "rtmp://live.example.com/ingest",
            "streamKey": "sk_live_abc123def456"
        }}}),
    )
    .await;
    let err = ivs.start(&started(&b)).await.unwrap_err();
    assert!(matches!(err, AwsError::UnsupportedMode(_)), "{err:?}");
    assert_eq!(err.failure_reason(), LivestreamFailureReason::Rejected);
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// Stream End while live is `"interrupted"`; an ingest limit breach while
/// starting is `"rejected"`.
#[tokio::test]
async fn events_map_to_failure_reasons() {
    let server = MockServer::start().await;
    on(
        &server,
        "CreateChannel",
        ResponseTemplate::new(200).set_body_json(create_channel_response(false)),
    )
    .await;
    let ivs = IvsLivestreams::new(client(&server), ChannelTemplate::default());
    let b = backend();
    let caller = jmap_types::Id::from(SYSTEM);
    let reporter = MediaReporter::new(&b, &caller, Id::from(ACCOUNT));

    livestream_set(
        &b,
        json!({"create": {"s1": {"callId": "C1", "mode": "hosted"}}}),
    )
    .await;
    let one = ivs.start(&started(&b)).await.unwrap();
    ivs.report_event(&reporter, &one, &event("Stream Start"))
        .await
        .unwrap();
    ivs.report_event(&reporter, &one, &event("Stream End"))
        .await
        .unwrap();
    assert_eq!(
        livestream_get(&b, &one.livestream_id).await["failureReason"],
        "interrupted"
    );

    livestream_set(
        &b,
        json!({"create": {"s2": {"callId": "C1", "mode": "hosted"}}}),
    )
    .await;
    let two = ivs.start(&started(&b)).await.unwrap();
    let breach = IvsEvent::from_eventbridge(&json!({
        "detail-type": "IVS Limit Breach",
        "source": "aws.ivs",
        "resources": [CHANNEL_ARN],
        "detail": {
            "limit_name": "Ingest Resolution",
            "limit_value": 495000,
            "exceeded_by": 426600,
            "limit_unit": "total pixels",
            "channel_name": "jmap-vtc-L2",
            "stream_id": "st-1A2b3c4D5e6F78ghij9Klmn"
        }
    }))
    .unwrap();
    ivs.report_event(&reporter, &two, &breach).await.unwrap();
    let got = livestream_get(&b, &two.livestream_id).await;
    assert_eq!(got["state"], "failed");
    assert_eq!(got["failureReason"], "rejected");

    // An event for another channel is refused before any write.
    let mut other = event("Stream Start");
    other.channel_arn = Some("arn:aws:ivs:us-west-2:123456789012:channel/other".to_owned());
    assert!(matches!(
        ivs.report_event(&reporter, &two, &other).await,
        Err(ReportError::Invalid(_))
    ));
}

/// DeleteChannel on a channel IVS still counts as live is a 409; `stop`
/// says to retry after Stream End.
#[tokio::test]
async fn delete_conflict_is_pending() {
    let server = MockServer::start().await;
    on(&server, "StopStream", ResponseTemplate::new(200)).await;
    on(
        &server,
        "DeleteChannel",
        aws_error(409, "ConflictException", "channel is live"),
    )
    .await;
    let ivs = IvsLivestreams::new(client(&server), ChannelTemplate::default());
    let channel: IvsChannel = serde_json::from_value(json!({
        "livestreamId": "L1",
        "arn": CHANNEL_ARN,
        "playbackUrl": PLAYBACK_URL,
        "ingestEndpoint": common::INGEST,
        "streamKey": common::STREAM_KEY,
        "authorized": false,
        "latencyMode": "LOW"
    }))
    .unwrap();
    assert_eq!(
        ivs.stop(&channel).await.unwrap(),
        StopOutcome::DeletePending
    );

    // A channel already gone is deleted, not an error.
    let gone = MockServer::start().await;
    on(
        &gone,
        "StopStream",
        aws_error(404, "ResourceNotFoundException", "no such channel"),
    )
    .await;
    let ivs = IvsLivestreams::new(client(&gone), ChannelTemplate::default());
    assert_eq!(ivs.stop(&channel).await.unwrap(), StopOutcome::Deleted);
}

/// A private channel: CreateChannel `authorized: true`, and each viewer's
/// endpoint URL carries `?token=<jwt>` with `expiresAt`, `credential` null.
#[tokio::test]
async fn private_channel_endpoints_carry_a_token() {
    let server = MockServer::start().await;
    on(
        &server,
        "CreateChannel",
        ResponseTemplate::new(200).set_body_json(create_channel_response(true)),
    )
    .await;
    let rng = SystemRandom::new();
    let der = EcdsaKeyPair::generate_pkcs8(&ECDSA_P384_SHA384_FIXED_SIGNING, &rng).unwrap();
    let key = PlaybackKey::from_pkcs8_der(der.as_ref()).unwrap();
    let ivs = IvsLivestreams::new(client(&server), ChannelTemplate::default())
        .with_private_playback(key, Duration::from_secs(300))
        .with_allowed_origin("https://app.example.com");
    let b = backend();
    livestream_set(
        &b,
        json!({"create": {"s1": {"callId": "C1", "mode": "hosted"}}}),
    )
    .await;
    let channel = ivs.start(&started(&b)).await.unwrap();
    assert_eq!(
        body(&sent(&server, "CreateChannel").await[0])["authorized"],
        true
    );

    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_763_658_000);
    let stored = vec![ivs.playback_endpoint(&channel)];
    let shown = ivs
        .endpoints_for(&channel, stored, Some("viewer-7"), now)
        .unwrap();
    let ep = &shown[0];
    let token = ep
        .url
        .strip_prefix(&format!("{PLAYBACK_URL}?token="))
        .unwrap_or_else(|| panic!("{}", ep.url));
    assert_eq!(token.split('.').count(), 3);
    assert!(ep.credential.is_none());
    // 1_763_658_000 + 300 s = 2025-11-20T17:05:00Z.
    assert_eq!(
        serde_json::to_value(&ep.expires_at).unwrap(),
        json!("2025-11-20T17:05:00Z")
    );
}
