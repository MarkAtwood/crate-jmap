//! The union of the `ChatStreamEnable` `"vtc"` path and `VTCStreamEnable`
//! over a real WebSocket: `ChatStreamHandler` and `VtcStreamHandler`
//! joined by `link_vtc_stream` on one loopback connection.
//!
//! Oracle: draft-atwood-jmap-vtc-wss-00 (jmap-chat-spec 813ad11)
//! §Interoperability with JMAP Chat WebSocket; each test quotes the
//! sentence it checks. Records and events are hand-written JSON.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use jmap_chat_server::websocket::{ChatEventHub, ChatStreamHandler};
use jmap_chat_vtc_server::link_vtc_stream;
use jmap_server::{CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler};
use jmap_types::{Id, State};
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::websocket::{VtcEventHub, VtcStreamHandler};
use jmap_vtc_types::{VtcCapability, VtcEvent};
use jmap_websocket::server::{Connection, DispatcherHandler};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const ACCOUNT: &str = "A1";
const BOB: &str = "user:bob@example.com";
const ALICE: &str = "user:alice@example.com";
/// How long a negative check waits for a frame that must not come.
const QUIET: Duration = Duration::from_millis(150);

struct Echo;

impl JmapHandler<()> for Echo {
    fn call(&self, cx: CallContext<()>) -> HandlerFuture {
        let args = cx.args;
        Box::pin(async move { Ok(HandlerOutput::new(args)) })
    }
}

/// A stored active room call, hand-written from the VTCCall field list of
/// draft-atwood-jmap-vtc-00.
fn call(id: &str) -> Value {
    json!({
        "id": id,
        "accountId": ACCOUNT,
        "callType": "room",
        "state": "active",
        "createdAt": "2026-06-05T14:30:00Z",
        "endedAt": null,
        "endReason": null,
        "initiatorId": BOB,
        "subject": null,
        "joinUri": "https://meet.example.com/room/xkz",
        "joinPassword": null,
        "mediaTypes": ["audio"],
        "activeParticipantCount": 0,
        "lobbyEnabled": false,
        "parentCallId": null,
        "breakoutRoomIds": [],
        "scheduledStartAt": null,
        "gatewayPin": null,
        "e2eeEnabled": false
    })
}

fn participant(id: &str, call_id: &str, user: &str) -> Value {
    json!({
        "id": id,
        "callId": call_id,
        "userId": user,
        "displayName": user,
        "role": "participant",
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

/// Calls `C1` and `C2` with Alice and Bob in both, and `C3` with only Bob.
fn vtc_backend() -> Arc<MemoryBackend> {
    let backend = MemoryBackend::new();
    let cap: VtcCapability = serde_json::from_value(json!({
        "mayCreateCall": true,
        "supportsRingCalls": true,
        "supportsRoomCalls": true,
        "supportsScheduledCalls": true,
        "supportsRecording": false,
        "supportsLivestream": false,
        "livestreamModes": [],
        "livestreamPushSchemes": [],
        "layouts": [],
        "supportsLobby": false,
        "supportsBreakoutRooms": false,
        "supportedMediaTypes": ["audio", "video"],
        "maxParticipantsPerCall": null,
        "maxConcurrentCalls": null,
        "maxBreakoutRooms": null
    }))
    .expect("capability fixture");
    let account = Id::from(ACCOUNT);
    backend.register_account(&account, cap);
    for id in ["C1", "C2", "C3"] {
        backend.insert_object_for_test("VTCCall", &account, id, call(id));
    }
    for (id, call_id, user) in [
        ("P-ALICE-1", "C1", ALICE),
        ("P-BOB-1", "C1", BOB),
        ("P-ALICE-2", "C2", ALICE),
        ("P-BOB-2", "C2", BOB),
        ("P-BOB-3", "C3", BOB),
    ] {
        backend.insert_object_for_test(
            "VTCParticipant",
            &account,
            id,
            participant(id, call_id, user),
        );
    }
    Arc::new(backend)
}

/// A `VTCParticipantEvent` for Bob's participant in `call_id`.
fn joined(call_id: &str) -> VtcEvent {
    let participant_id = format!("P-BOB-{}", &call_id[1..]);
    serde_json::from_value(json!({
        "@type": "VTCParticipantEvent",
        "accountId": ACCOUNT,
        "callId": call_id,
        "participantId": participant_id,
        "event": "joined",
        "displayName": "Bob",
        "role": "participant"
    }))
    .expect("VTCParticipantEvent")
}

/// A server whose connections are Alice's, with both handlers linked.
async fn server() -> (std::net::SocketAddr, VtcEventHub) {
    let mut dispatcher = Dispatcher::new();
    dispatcher.register("Core/echo", Arc::new(Echo));
    let dispatcher = Arc::new(dispatcher);
    let chat_hub = ChatEventHub::new(16);
    let vtc_hub = VtcEventHub::new(64);
    let chat_backend = Arc::new(jmap_chat_server::memory::MemoryBackend::<()>::default());
    let vtc = vtc_backend();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hub = vtc_hub.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let handler = Arc::new(DispatcherHandler::new(Arc::clone(&dispatcher), (), || {
                State::from("s0")
            }));
            let vtc_handler =
                VtcStreamHandler::new(Arc::clone(&vtc), Id::from(ALICE), Id::from(ACCOUNT), &hub);
            let chat_handler = link_vtc_stream(
                ChatStreamHandler::new(
                    Arc::clone(&chat_backend),
                    (),
                    [Id::from("acct-alice")],
                    &chat_hub,
                ),
                &vtc_handler,
            );
            tokio::spawn(async move {
                let Ok(ws) = jmap_websocket::tungstenite::accept(tcp).await else {
                    return;
                };
                let conn = Connection::new(handler)
                    .with_extension(chat_handler)
                    .with_extension(vtc_handler);
                let _ = jmap_websocket::tungstenite::serve(conn, ws).await;
            });
        }
    });
    (addr, vtc_hub)
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

/// Sends `frame`, then syncs.
async fn control(ws: &mut Client, frame: Value) {
    send(ws, frame).await;
    sync(ws).await;
}

/// A `Core/echo` round trip: every message sent before it has been
/// handled when its Response arrives, and no other frame came first.
async fn sync(ws: &mut Client) {
    send(
        ws,
        json!({
            "@type": "Request",
            "id": "sync",
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
}

/// Waits a little, then checks no frame is waiting.
async fn nothing(ws: &mut Client) {
    tokio::time::sleep(QUIET).await;
    sync(ws).await;
}

/// The next frame is the event for `call_id`.
async fn next_is(ws: &mut Client, call_id: &str) {
    let e = recv(ws).await;
    assert_eq!(e["@type"], "VTCParticipantEvent");
    assert_eq!(e["callId"], call_id);
}

/// "`ChatStreamEnable` with `"vtc"` in `dataTypes`: Subscribes to all VTC
/// event types: per-user events for the user, and per-call events for all
/// calls in which the user is a current participant"; "`ChatStreamDisable`
/// cancels only the Chat ephemeral subscription. If `"vtc"` was included
/// ... VTC events delivered through that path stop" (§Subscription
/// Independence).
#[tokio::test]
async fn chat_path_alone() {
    let (addr, hub) = server().await;
    let mut ws = connect(addr).await;
    control(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["vtc"]}),
    )
    .await;

    hub.publish(joined("C3")); // Alice is not in C3
    hub.publish(joined("C1"));
    hub.publish(joined("C2"));
    next_is(&mut ws, "C1").await;
    next_is(&mut ws, "C2").await;

    control(&mut ws, json!({"@type": "ChatStreamDisable"})).await;
    hub.publish(joined("C1"));
    nothing(&mut ws).await;
}

/// §Subscription Union: "the server MUST deliver the union of both
/// subscriptions. The server SHOULD NOT deliver duplicate events; if an
/// event matches both subscriptions, it is delivered exactly once."
#[tokio::test]
async fn union_without_duplicates() {
    let (addr, hub) = server().await;
    let mut ws = connect(addr).await;
    control(
        &mut ws,
        json!({"@type": "VTCStreamEnable", "callIds": ["C1"], "eventTypes": null}),
    )
    .await;
    control(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["typing", "vtc"]}),
    )
    .await;

    hub.publish(joined("C1")); // both paths
    hub.publish(joined("C2")); // the "vtc" path only
    next_is(&mut ws, "C1").await;
    next_is(&mut ws, "C2").await;
    nothing(&mut ws).await;
}

/// §Subscription Independence: "`VTCStreamDisable` cancels only the
/// VTC-specific subscription ... It does not affect VTC events delivered
/// through `ChatStreamEnable`"; after `ChatStreamDisable`, "events
/// delivered through an active `VTCStreamEnable` subscription are
/// unaffected".
#[tokio::test]
async fn each_disable_leaves_the_other_path() {
    let (addr, hub) = server().await;
    let mut ws = connect(addr).await;
    control(
        &mut ws,
        json!({"@type": "VTCStreamEnable", "callIds": ["C1"]}),
    )
    .await;
    control(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["vtc"]}),
    )
    .await;

    control(&mut ws, json!({"@type": "ChatStreamDisable"})).await;
    hub.publish(joined("C2")); // only the "vtc" path covered it
    hub.publish(joined("C1"));
    next_is(&mut ws, "C1").await;

    control(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["vtc"]}),
    )
    .await;
    control(&mut ws, json!({"@type": "VTCStreamDisable"})).await;
    hub.publish(joined("C2"));
    next_is(&mut ws, "C2").await;
    nothing(&mut ws).await;
}
