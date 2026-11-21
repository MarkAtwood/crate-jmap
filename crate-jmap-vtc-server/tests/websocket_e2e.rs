//! `VtcStreamHandler` end to end over a real WebSocket: a
//! tokio-tungstenite client against `jmap_websocket::tungstenite::serve`
//! on a loopback socket, the handler registered next to a `Core/echo`
//! dispatcher, events published to a `VtcEventHub`.
//!
//! Oracle: draft-atwood-jmap-vtc-wss-00 (jmap-chat-spec 813ad11)
//! §VTCStreamEnable, §VTCStreamDisable, §Event Types and §Rate Limiting;
//! each test quotes the sentence it checks. Records and frames are
//! hand-written JSON.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use futures::{SinkExt as _, StreamExt as _};
use jmap_server::{CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler};
use jmap_types::{Id, State};
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::websocket::{VtcEventHub, VtcStreamHandler, VtcStreamLimits};
use jmap_vtc_types::VtcEvent;
use jmap_websocket::server::{Connection, DispatcherHandler};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Alice's own account, apart from the hosting account [`ACCOUNT`].
const ALICE_ACCOUNT: &str = "acct-alice";
/// How long a negative check waits for a frame that must not come.
const QUIET: Duration = Duration::from_millis(150);

/// RFC 8620 §4 `Core/echo`, used to sync with the server.
struct Echo;

impl JmapHandler<()> for Echo {
    fn call(&self, cx: CallContext<()>) -> HandlerFuture {
        let args = cx.args;
        Box::pin(async move { Ok(HandlerOutput::new(args)) })
    }
}

/// A VTCParticipant who joined and has not left, hand-written from the
/// field list of draft-atwood-jmap-vtc-00 §VTCParticipant.
fn participant(id: &str, call_id: &str, user: &str, role: &str) -> Value {
    json!({
        "id": id,
        "callId": call_id,
        "userId": user,
        "displayName": user,
        "role": role,
        "joinedAt": "2026-06-05T14:30:00Z",
        "leftAt": null,
        "callResponse": null,
        "joinMethod": "webrtc",
        "gatewayData": null,
        "lobbyState": null,
        "mediaState": { "audio": true, "video": false, "screen": false, "raisedHand": false },
        "speakerTimeMs": 0,
        "kickedBy": null,
        "e2eeFingerprint": null,
        "unmuteRequested": false
    })
}

/// Active room call `C1` in [`ACCOUNT`] with Bob (moderator), Alice and
/// Eve as current participants.
fn fixture() -> Arc<MemoryBackend> {
    let backend = backend();
    seed(
        &backend,
        stored_call("C1", "room", "active", "2026-06-05T14:30:00Z"),
    );
    for (id, user, role) in [
        ("P-BOB", BOB, "moderator"),
        ("P-ALICE", ALICE, "participant"),
        ("P-EVE", EVE, "participant"),
    ] {
        seed_as(
            &backend,
            "VTCParticipant",
            participant(id, "C1", user, role),
        );
    }
    Arc::new(backend)
}

fn event(mut raw: Value) -> VtcEvent {
    raw["accountId"] = json!(ACCOUNT);
    serde_json::from_value(raw).unwrap()
}

fn participant_event(participant_id: &str) -> VtcEvent {
    event(json!({
        "@type": "VTCParticipantEvent",
        "callId": "C1",
        "participantId": participant_id,
        "event": "joined",
        "displayName": "Eve",
        "role": "participant"
    }))
}

fn media_state(participant_id: &str, audio: bool, video: bool) -> VtcEvent {
    event(json!({
        "@type": "VTCMediaStateEvent",
        "callId": "C1",
        "participantId": participant_id,
        "mediaState": { "audio": audio, "video": video, "screen": false, "raisedHand": false }
    }))
}

fn active_speaker(participant_id: &str) -> VtcEvent {
    event(json!({
        "@type": "VTCActiveSpeakerEvent",
        "callId": "C1",
        "participantId": participant_id
    }))
}

fn ring() -> VtcEvent {
    event(json!({
        "@type": "VTCRingEvent",
        "callId": "C9",
        "initiatorId": BOB,
        "mediaTypes": ["audio"],
        "joinUri": "https://meet.example.com/room/xkz"
    }))
}

/// Starts a server whose connections are all Alice's.
async fn server(limits: VtcStreamLimits) -> (std::net::SocketAddr, VtcEventHub) {
    let mut dispatcher = Dispatcher::new();
    dispatcher.register("Core/echo", Arc::new(Echo));
    let dispatcher = Arc::new(dispatcher);
    let hub = VtcEventHub::new(64);
    let backend = fixture();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_hub = hub.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let handler = Arc::new(DispatcherHandler::new(Arc::clone(&dispatcher), (), || {
                State::from("s0")
            }));
            let vtc = VtcStreamHandler::new(
                Arc::clone(&backend),
                Id::from(ALICE),
                Id::from(ALICE_ACCOUNT),
                &server_hub,
            )
            .with_limits(limits);
            tokio::spawn(async move {
                let Ok(ws) = jmap_websocket::tungstenite::accept(tcp).await else {
                    return;
                };
                let conn = Connection::new(handler).with_extension(vtc);
                let _ = jmap_websocket::tungstenite::serve(conn, ws).await;
            });
        }
    });
    (addr, hub)
}

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn connect(addr: std::net::SocketAddr) -> Client {
    let mut request = format!("ws://{addr}/jmap/ws/")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", HeaderValue::from_static("jmap"));
    let (ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    ws
}

async fn send(ws: &mut Client, v: Value) {
    ws.send(Message::Text(v.to_string().into())).await.unwrap();
}

async fn recv(ws: &mut Client) -> Value {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("a message within 5 s")
            .expect("open")
            .unwrap();
        if let Message::Text(t) = m {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

/// A `Core/echo` round trip: every message sent before it has been
/// handled when its Response arrives, and no other frame came first.
async fn sync(ws: &mut Client, id: &str) {
    send(
        ws,
        json!({
            "@type": "Request",
            "id": id,
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [["Core/echo", {}, "c"]]
        }),
    )
    .await;
    let r = recv(ws).await;
    assert_eq!(
        r["@type"], "Response",
        "expected only the Response, got {r}"
    );
    assert_eq!(r["requestId"], id);
}

/// Waits a little, then checks no frame arrived.
async fn nothing(ws: &mut Client, id: &str) {
    tokio::time::sleep(QUIET).await;
    sync(ws, id).await;
}

async fn enable(ws: &mut Client, body: Value) {
    let mut frame = body;
    frame["@type"] = json!("VTCStreamEnable");
    send(ws, frame).await;
    sync(ws, "enabled").await;
}

/// Nothing before `VTCStreamEnable`; per-call events of a call the user
/// is in afterwards, each carrying "the id of the call's hosting account"
/// as `accountId` (§Event Types); "The server MUST stop delivering VTC
/// ephemeral events after processing this message" (§VTCStreamDisable).
#[tokio::test]
async fn enable_event_disable_none() {
    let (addr, hub) = server(VtcStreamLimits::default()).await;
    let mut ws = connect(addr).await;

    hub.publish(participant_event("P-EVE"));
    nothing(&mut ws, "s1").await;

    enable(&mut ws, json!({})).await;
    hub.publish(participant_event("P-EVE"));
    let e = recv(&mut ws).await;
    assert_eq!(e["@type"], "VTCParticipantEvent");
    assert_eq!(e["accountId"], ACCOUNT);
    assert_eq!(e["participantId"], "P-EVE");

    send(&mut ws, json!({"@type": "VTCStreamDisable"})).await;
    sync(&mut ws, "s2").await;
    hub.publish(participant_event("P-EVE"));
    nothing(&mut ws, "s3").await;
}

/// "Per-user events (`VTCRingEvent`, `VTCCallEndEvent`) are delivered
/// based on the authenticated user's identity and are not filtered by
/// `callIds` ... including when `callIds` is an empty array" (§Event
/// Types). A ring for another user, or for no one, is not Alice's.
#[tokio::test]
async fn ring_goes_to_its_user_whatever_call_ids() {
    let (addr, hub) = server(VtcStreamLimits::default()).await;
    let mut ws = connect(addr).await;
    enable(&mut ws, json!({"callIds": []})).await;

    hub.publish(ring());
    hub.publish_to_user(BOB, ring());
    hub.publish(participant_event("P-EVE")); // per-call, callIds []: none
    hub.publish_to_user(ALICE, ring());
    let e = recv(&mut ws).await;
    assert_eq!(e["@type"], "VTCRingEvent");
    assert_eq!(e["callId"], "C9");
    nothing(&mut ws, "s1").await;
}

/// "Per-call events are delivered only for calls that match the active
/// subscription's `callIds` filter" (§Event Types), and `eventTypes`
/// selects types (§VTCStreamEnable).
#[tokio::test]
async fn call_ids_and_event_types_filter() {
    let (addr, hub) = server(VtcStreamLimits::default()).await;
    let mut ws = connect(addr).await;
    enable(&mut ws, json!({"callIds": ["C2"]})).await;
    hub.publish(participant_event("P-EVE"));
    nothing(&mut ws, "s1").await;

    enable(
        &mut ws,
        json!({"callIds": ["C1"], "eventTypes": ["VTCActiveSpeakerEvent"]}),
    )
    .await;
    hub.publish(participant_event("P-EVE"));
    hub.publish(active_speaker("P-EVE"));
    assert_eq!(recv(&mut ws).await["@type"], "VTCActiveSpeakerEvent");
    nothing(&mut ws, "s2").await;
}

/// §Rate Limiting: "`VTCMediaStateEvent`: no more than one event per
/// participant per call per second. Events above this rate MAY be
/// silently dropped; the most recent state SHOULD be delivered when the
/// rate window reopens." Run with a shorter window.
#[tokio::test]
async fn media_state_holds_the_latest_until_the_window_reopens() {
    let window = Duration::from_millis(400);
    let limits = VtcStreamLimits::default().media_state_window(Some(window));
    let (addr, hub) = server(limits).await;
    let mut ws = connect(addr).await;
    enable(&mut ws, json!({})).await;

    hub.publish(media_state("P-EVE", true, false));
    hub.publish(media_state("P-EVE", false, false)); // superseded
    hub.publish(media_state("P-EVE", false, true)); // held
    hub.publish(media_state("P-BOB", false, false)); // another participant
    let first = recv(&mut ws).await;
    assert_eq!(
        (
            first["participantId"].clone(),
            first["mediaState"]["audio"].clone()
        ),
        (json!("P-EVE"), json!(true))
    );
    assert_eq!(recv(&mut ws).await["participantId"], "P-BOB");
    sync(&mut ws, "s1").await;
    let held = recv(&mut ws).await;
    assert_eq!(
        (
            held["participantId"].clone(),
            held["mediaState"]["video"].clone()
        ),
        (json!("P-EVE"), json!(true))
    );
    nothing(&mut ws, "s2").await;
}

/// §Rate Limiting: "`VTCActiveSpeakerEvent`: no more than two events per
/// call per second. Events above this rate MAY be silently dropped."
#[tokio::test]
async fn active_speaker_drops_the_third_in_a_window() {
    let (addr, hub) = server(VtcStreamLimits::default()).await;
    let mut ws = connect(addr).await;
    enable(&mut ws, json!({})).await;

    hub.publish(active_speaker("P-EVE"));
    hub.publish(active_speaker("P-BOB"));
    hub.publish(active_speaker("P-ALICE")); // dropped
    hub.publish(participant_event("P-EVE")); // marker
    assert_eq!(recv(&mut ws).await["participantId"], "P-EVE");
    assert_eq!(recv(&mut ws).await["participantId"], "P-BOB");
    assert_eq!(recv(&mut ws).await["@type"], "VTCParticipantEvent");
}

/// §VTCStreamEnable: "If `eventTypes` is an empty array or contains only
/// unrecognized values, the server MUST respond with a `RequestError`
/// ... `invalidArguments` and MUST NOT update the current ephemeral
/// subscription state."
#[tokio::test]
async fn invalid_enable_is_a_request_error_and_keeps_the_subscription() {
    let (addr, hub) = server(VtcStreamLimits::default()).await;
    let mut ws = connect(addr).await;
    enable(&mut ws, json!({})).await;
    send(
        &mut ws,
        json!({"@type": "VTCStreamEnable", "id": "e1", "eventTypes": []}),
    )
    .await;
    let e = recv(&mut ws).await;
    assert_eq!(e["@type"], "RequestError");
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:invalidArguments");
    assert_eq!(e["requestId"], "e1");
    hub.publish(participant_event("P-EVE"));
    assert_eq!(recv(&mut ws).await["@type"], "VTCParticipantEvent");
}
