//! `SceneStreamHandler` end to end over a real WebSocket: tokio-tungstenite
//! clients against `jmap_websocket::tungstenite::serve` on a loopback
//! socket, the handler registered next to a `Core/echo` dispatcher,
//! events published to a `SceneEventHub`.
//!
//! Oracle: draft-atwood-jmap-scene-wss-00 (jmap-chat-spec 813ad11)
//! §SceneStreamEnable, §SceneStreamDisable, §Submitting Interactions and
//! §Rate Limiting; each test quotes the sentence it checks. Records and
//! frames are hand-written JSON.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::*;
use futures::{SinkExt as _, StreamExt as _};
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::websocket::{SceneEventHub, SceneStreamHandler, SceneStreamLimits};
use jmap_scene_types::SceneEvent;
use jmap_server::{CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler};
use jmap_types::{Id, State};
use jmap_websocket::server::{Connection, DispatcherHandler};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// The region of the draft's examples.
const R1: &str = "01J5ABC0000000000000000001";
/// A non-interactable object in [`R1`].
const OBJ42: &str = "01J5OBJ0000000000000000042";
/// An interactable object in [`R1`].
const SCULPTURE: &str = "01J5OBJ0000000000000000001";
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

fn avatar(id: &str, user: &str) -> Value {
    json!({
        "id": id,
        "regionId": R1,
        "userId": user,
        "displayName": user,
        "position": [0, 0, 5],
        "orientation": [0, 0, 0, 1],
        "visualRef": null,
        "visualType": null,
        "joinedAt": "2026-06-06T10:00:00Z",
        "leftAt": null,
        "worldState": null
    })
}

fn object(id: &str, interactable: bool) -> Value {
    let mut o = stored_object(id, R1, json!([0, 0, 0]), "2026-06-02T10:00:00Z");
    o["interactable"] = json!(interactable);
    o
}

/// Public region [`R1`] with the two objects and active avatars for Alice
/// and Bob.
fn scene() -> Arc<MemoryBackend> {
    let backend = backend();
    seed_as(
        &backend,
        "SceneRegion",
        stored_region(R1, "Plaza", "2026-06-01T10:00:00Z"),
    );
    seed_as(&backend, "SceneObject", object(OBJ42, false));
    seed_as(&backend, "SceneObject", object(SCULPTURE, true));
    seed_as(
        &backend,
        "SceneAvatar",
        avatar("01J5AVT0000000000000000001", ALICE),
    );
    seed_as(
        &backend,
        "SceneAvatar",
        avatar("01J5AVT0000000000000000002", BOB),
    );
    Arc::new(backend)
}

fn event(raw: Value) -> SceneEvent {
    serde_json::from_value(raw).unwrap()
}

fn avatar_event(user: &str) -> SceneEvent {
    event(json!({
        "@type": "SceneAvatarEvent",
        "accountId": ACCOUNT,
        "regionId": R1,
        "avatarId": "01J5AVT0000000000000000009",
        "event": "entered",
        "userId": user,
        "displayName": user
    }))
}

fn object_update(x: f64) -> SceneEvent {
    event(json!({
        "@type": "SceneObjectEvent",
        "accountId": ACCOUNT,
        "regionId": R1,
        "objectId": OBJ42,
        "event": "updated",
        "updatedBy": null,
        "position": [x, 0, 0]
    }))
}

/// Starts a server. Connections are Alice's, then Bob's, in accept order.
async fn server(hub: SceneEventHub, limits: SceneStreamLimits) -> std::net::SocketAddr {
    let mut dispatcher = Dispatcher::new();
    dispatcher.register("Core/echo", Arc::new(Echo));
    let dispatcher = Arc::new(dispatcher);
    let backend = scene();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accepted = AtomicUsize::new(0);
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let user = [ALICE, BOB][accepted.fetch_add(1, Ordering::SeqCst) % 2];
            let handler = Arc::new(DispatcherHandler::new(Arc::clone(&dispatcher), (), || {
                State::from("s0")
            }));
            let scene = SceneStreamHandler::new(
                Arc::clone(&backend),
                Id::from(user),
                Id::from(ACCOUNT),
                &hub,
            )
            .with_limits(limits);
            tokio::spawn(async move {
                let Ok(ws) = jmap_websocket::tungstenite::accept(tcp).await else {
                    return;
                };
                let conn = Connection::new(handler).with_extension(scene);
                let _ = jmap_websocket::tungstenite::serve(conn, ws).await;
            });
        }
    });
    addr
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
    frame["@type"] = json!("SceneStreamEnable");
    send(ws, frame).await;
    sync(ws, "enabled").await;
}

fn interaction(object_id: &str) -> Value {
    json!({
        "@type": "SceneInteractionEvent",
        "id": "i1",
        "accountId": ACCOUNT,
        "regionId": R1,
        "objectId": object_id,
        "userId": "user:forged@example.com",
        "action": "com.example.game.cast-spell",
        "data": {"spellId": "fireball"}
    })
}

/// Nothing before `SceneStreamEnable`; events of a region the user is in
/// afterwards; "The server MUST stop delivering Scene ephemeral events
/// after processing this message" (§SceneStreamDisable).
#[tokio::test]
async fn enable_event_disable_none() {
    let hub = SceneEventHub::new(64);
    let addr = server(hub.clone(), SceneStreamLimits::default()).await;
    let mut alice = connect(addr).await;

    hub.publish(avatar_event(BOB));
    nothing(&mut alice, "s1").await;

    enable(&mut alice, json!({"regionIds": [R1]})).await;
    hub.publish(avatar_event(BOB));
    let e = recv(&mut alice).await;
    assert_eq!(e["@type"], "SceneAvatarEvent");
    assert_eq!(e["accountId"], ACCOUNT);
    assert_eq!(e["userId"], BOB);

    send(&mut alice, json!({"@type": "SceneStreamDisable"})).await;
    sync(&mut alice, "s2").await;
    hub.publish(avatar_event(BOB));
    nothing(&mut alice, "s3").await;
}

/// §Submitting Interactions: "The server sets `userId` on the relayed
/// event to the authenticated user's userId; a `userId` supplied in the
/// frame is ignored"; an accepted interaction "is delivered as a
/// `SceneInteractionEvent` to the subscribers of that region"; a frame
/// whose object is not `interactable` is discarded and "returns no error
/// to the sender".
#[tokio::test]
async fn interaction_relay() {
    let hub = SceneEventHub::new(64);
    let addr = server(hub.clone(), SceneStreamLimits::default()).await;
    let mut alice = connect(addr).await;
    let mut bob = connect(addr).await;
    enable(&mut alice, json!({})).await;
    enable(&mut bob, json!({"eventTypes": ["SceneInteractionEvent"]})).await;

    send(&mut alice, interaction(OBJ42)).await; // not interactable
    send(&mut alice, interaction(SCULPTURE)).await;
    let expected = json!({
        "@type": "SceneInteractionEvent",
        "accountId": ACCOUNT,
        "regionId": R1,
        "objectId": SCULPTURE,
        "userId": ALICE,
        "action": "com.example.game.cast-spell",
        "data": {"spellId": "fireball"}
    });
    assert_eq!(recv(&mut bob).await, expected);
    assert_eq!(recv(&mut alice).await, expected);
    nothing(&mut alice, "s1").await;
    nothing(&mut bob, "s2").await;
}

/// §Rate Limiting: "`SceneInteractionEvent`: no more than five events per
/// user per region per second. Frames above this rate MAY be silently
/// dropped."
#[tokio::test]
async fn interactions_over_the_rate_are_dropped() {
    let hub = SceneEventHub::new(64);
    let addr = server(hub.clone(), SceneStreamLimits::default()).await;
    let mut alice = connect(addr).await;
    let mut bob = connect(addr).await;
    enable(&mut bob, json!({})).await;

    for _ in 0..6 {
        send(&mut alice, interaction(SCULPTURE)).await;
    }
    sync(&mut alice, "s1").await;
    hub.publish(avatar_event(ALICE)); // marker
    for _ in 0..5 {
        assert_eq!(recv(&mut bob).await["@type"], "SceneInteractionEvent");
    }
    assert_eq!(recv(&mut bob).await["@type"], "SceneAvatarEvent");
}

/// §Rate Limiting: "`SceneObjectEvent` with `event: "updated"`: no more
/// than two events per object per second. Events above this rate MAY be
/// silently dropped; the most recent state SHOULD be delivered when the
/// rate window reopens." Run with a shorter window.
#[tokio::test]
async fn object_updates_hold_the_latest_until_the_window_reopens() {
    let hub = SceneEventHub::new(64);
    let limits =
        SceneStreamLimits::default().object_update_window(Some(Duration::from_millis(400)));
    let addr = server(hub.clone(), limits).await;
    let mut alice = connect(addr).await;
    enable(&mut alice, json!({"eventTypes": ["SceneObjectEvent"]})).await;

    for x in [10.0, 10.1, 10.2, 10.3] {
        hub.publish(object_update(x));
    }
    assert_eq!(recv(&mut alice).await["position"][0], 10.0);
    assert_eq!(recv(&mut alice).await["position"][0], 10.1);
    sync(&mut alice, "s1").await;
    assert_eq!(recv(&mut alice).await["position"][0], 10.3);
    nothing(&mut alice, "s2").await;
}

/// §SceneStreamEnable: "If `eventTypes` is an empty array or contains only
/// unrecognized values, the server MUST respond with a `RequestError`
/// ... `invalidArguments` and MUST NOT update the current ephemeral
/// subscription state" (the draft's own example, `"SceneFooBarEvent"`).
#[tokio::test]
async fn invalid_enable_is_a_request_error_and_keeps_the_subscription() {
    let hub = SceneEventHub::new(64);
    let addr = server(hub.clone(), SceneStreamLimits::default()).await;
    let mut alice = connect(addr).await;
    enable(&mut alice, json!({})).await;
    send(
        &mut alice,
        json!({
            "@type": "SceneStreamEnable",
            "id": "e1",
            "regionIds": [R1],
            "eventTypes": ["SceneFooBarEvent"]
        }),
    )
    .await;
    let e = recv(&mut alice).await;
    assert_eq!(e["@type"], "RequestError");
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:invalidArguments");
    assert_eq!(e["requestId"], "e1");
    hub.publish(avatar_event(BOB));
    assert_eq!(recv(&mut alice).await["@type"], "SceneAvatarEvent");
}
