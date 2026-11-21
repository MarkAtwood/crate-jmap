//! Shared fixtures: an Egress client against a local wiremock server, a
//! `MemoryBackend` account with an active call, and the `jmap-vtc-server`
//! handlers. No LiveKit server is ever contacted.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::time::SystemTime;

use jmap_types::Id;
use jmap_vtc_livekit::egress::EgressClient;
use jmap_vtc_livekit::token::ApiKey;
use jmap_vtc_livekit::url::Url;
use jmap_vtc_server::MediaEffect;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{
    handle_vtc_livestream_get, handle_vtc_livestream_set, handle_vtc_recording_get,
    handle_vtc_recording_set,
};
use jmap_vtc_types::{VtcCapability, VtcLivestream, VtcRecording};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

pub const ACCOUNT: &str = "A1";
pub const BOB: &str = "user:bob@example.com";
pub const SYSTEM: &str = "service:media";
pub const API_KEY: &str = "APIjmapvtc";
pub const API_SECRET: &str = "test-secret-not-a-real-livekit-secret";
pub const ROOM: &str = "vtc-C1";

pub fn api_key() -> ApiKey {
    ApiKey::new(API_KEY, API_SECRET)
}

pub fn client(server: &MockServer) -> EgressClient {
    EgressClient::new(&Url::parse(&server.uri()).unwrap(), api_key()).unwrap()
}

/// An `EgressInfo` as LiveKit's Twirp server writes it: proto field names,
/// every field present, `int64` as strings, enums by name
/// (`livekit_egress.proto`, `UseProtoNames`, `EmitUnpopulated`).
pub fn egress_info(egress_id: &str, status: &str) -> Value {
    json!({
        "egress_id": egress_id,
        "room_id": "RM_hycBMAjmt6Ub",
        "room_name": ROOM,
        "source_type": "EGRESS_SOURCE_TYPE_WEB",
        "status": status,
        "started_at": "0",
        "ended_at": "0",
        "updated_at": "1700000000000000000",
        "details": "",
        "error": "",
        "error_code": 0,
        "egress": {
            "room_name": ROOM,
            "template": {"layout": "speaker", "audio_only": false, "video_only": false, "custom_base_url": ""},
            "outputs": [],
            "storage": null,
            "webhooks": []
        },
        "stream_results": [],
        "file_results": [],
        "segment_results": [],
        "image_results": [],
        "manifest_location": "",
        "backup_storage_used": false,
        "retry_count": 0
    })
}

/// Answer `POST /twirp/livekit.Egress/{method}` with `response`.
pub async fn on(server: &MockServer, rpc: &str, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path(format!("/twirp/livekit.Egress/{rpc}")))
        .respond_with(response)
        .mount(server)
        .await;
}

/// A Twirp error response (`{"code", "msg"}`).
pub fn twirp_error(status: u16, code: &str, msg: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({"code": code, "msg": msg}))
}

/// The requests sent to `rpc`.
pub async fn sent(server: &MockServer, rpc: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == format!("/twirp/livekit.Egress/{rpc}"))
        .collect()
}

pub fn header(r: &Request, name: &str) -> String {
    r.headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

pub fn body(r: &Request) -> Value {
    serde_json::from_slice(&r.body).unwrap()
}

/// The request's bearer token verifies with the API key and grants
/// `roomRecord`.
pub fn assert_egress_auth(r: &Request) {
    assert_eq!(header(r, "content-type"), "application/json");
    let auth = header(r, "authorization");
    let token = auth.strip_prefix("Bearer ").expect("a Bearer token");
    let claims = api_key().verify(token, SystemTime::now()).unwrap();
    assert_eq!(claims.issuer, API_KEY);
    assert!(claims.video.expect("a video grant").room_record);
}

/// A backend whose account allows recordings without pause and push and
/// hosted livestreams, with the three draft layouts, holding active room
/// call C1 started by BOB (its moderator).
pub fn backend() -> MemoryBackend {
    let b = MemoryBackend::new();
    let cap: VtcCapability = serde_json::from_value(json!({
        "mayCreateCall": true,
        "supportsRingCalls": true,
        "supportsRoomCalls": true,
        "supportsScheduledCalls": true,
        "supportsRecording": true,
        "supportsRecordingPause": false,
        "supportsLivestream": true,
        "livestreamModes": ["push", "hosted"],
        "livestreamPushSchemes": jmap_vtc_livekit::PUSH_SCHEMES,
        "layouts": ["grid", "speaker", "single-speaker"],
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

pub fn bob() -> Id {
    Id::from(BOB)
}

pub fn system() -> Id {
    Id::from(SYSTEM)
}

pub async fn livestream_set(b: &MemoryBackend, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    handle_vtc_livestream_set(b, &bob(), &BTreeMap::new(), args)
        .await
        .unwrap()
        .primary
}

pub async fn livestream_get(b: &MemoryBackend, id: &Id) -> Value {
    handle_vtc_livestream_get(b, &bob(), json!({"accountId": ACCOUNT, "ids": [id]}))
        .await
        .unwrap()
        .primary["list"][0]
        .clone()
}

pub async fn recording_set(b: &MemoryBackend, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    handle_vtc_recording_set(b, &bob(), &BTreeMap::new(), args)
        .await
        .unwrap()
        .primary
}

pub async fn recording_get(b: &MemoryBackend, id: &Id) -> Value {
    handle_vtc_recording_get(b, &bob(), json!({"accountId": ACCOUNT, "ids": [id]}))
        .await
        .unwrap()
        .primary["list"][0]
        .clone()
}

/// The livestream of the last `StartLivestream` effect.
pub fn started_livestream(b: &MemoryBackend) -> VtcLivestream {
    b.media_effects_for_test()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            MediaEffect::StartLivestream(ls) => Some(ls),
            _ => None,
        })
        .expect("a StartLivestream effect")
}

/// The recording of the last `StartRecording` effect.
pub fn started_recording(b: &MemoryBackend) -> VtcRecording {
    b.media_effects_for_test()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            MediaEffect::StartRecording(r) => Some(r),
            _ => None,
        })
        .expect("a StartRecording effect")
}

pub fn last_effect(b: &MemoryBackend) -> Option<MediaEffect> {
    b.media_effects_for_test().pop()
}
