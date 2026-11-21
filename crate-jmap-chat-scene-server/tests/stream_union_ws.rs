//! The union of the `ChatStreamEnable` `"scene"` path and
//! `SceneStreamEnable` over a real WebSocket: `ChatStreamHandler` and
//! `SceneStreamHandler` joined by `link_scene_stream` on one loopback
//! connection.
//!
//! Oracle: draft-atwood-jmap-scene-wss-00 (jmap-chat-spec 813ad11)
//! §Interoperability with JMAP Chat WebSocket; each test quotes the
//! sentence it checks. Records and events are hand-written JSON.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use jmap_chat_scene_server::link_scene_stream;
use jmap_chat_server::websocket::{ChatEventHub, ChatStreamHandler};
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::websocket::{SceneEventHub, SceneStreamHandler};
use jmap_scene_types::{SceneCapability, SceneEvent};
use jmap_server::{CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler};
use jmap_types::{Id, State};
use jmap_websocket::server::{Connection, DispatcherHandler};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const ACCOUNT: &str = "acct-001";
const ALICE: &str = "user:alice@example.com";
const BOB: &str = "user:bob@example.com";
/// Alice and Bob both have an active avatar here.
const R1: &str = "01J5ABC0000000000000000001";
/// Alice and Bob both have an active avatar here.
const R2: &str = "01J5ABC0000000000000000002";
/// Only Bob has an active avatar here; Alice's has left.
const R3: &str = "01J5ABC0000000000000000003";
/// How long a negative check waits for a frame that must not come.
const QUIET: Duration = Duration::from_millis(150);

struct Echo;

impl JmapHandler<()> for Echo {
    fn call(&self, cx: CallContext<()>) -> HandlerFuture {
        let args = cx.args;
        Box::pin(async move { Ok(HandlerOutput::new(args)) })
    }
}

/// A public region, hand-written from the SceneRegion field list of
/// draft-atwood-jmap-scene-00.
fn region(id: &str) -> Value {
    json!({
        "id": id,
        "accountId": ACCOUNT,
        "ownerId": ALICE,
        "name": id,
        "description": null,
        "bounds": { "min": [-20, 0, -20], "max": [20, 8, 20] },
        "environment": null,
        "simulationUri": null,
        "spawnPosition": [0, 0, 5],
        "spawnOrientation": [0, 0, 0, 1],
        "activeAvatarCount": 0,
        "viewHint": "3d",
        "accessPolicy": "public",
        "createdAt": "2026-06-01T10:00:00Z",
        "updatedAt": "2026-06-01T10:00:00Z"
    })
}

fn avatar(id: &str, region: &str, user: &str, left_at: Value) -> Value {
    json!({
        "id": id,
        "regionId": region,
        "userId": user,
        "displayName": user,
        "position": [0, 0, 5],
        "orientation": [0, 0, 0, 1],
        "visualRef": null,
        "visualType": null,
        "joinedAt": "2026-06-06T10:00:00Z",
        "leftAt": left_at,
        "worldState": null
    })
}

fn scene_backend() -> Arc<MemoryBackend> {
    let backend = MemoryBackend::new();
    let cap: SceneCapability = serde_json::from_value(json!({
        "mayCreateRegion": true,
        "mayCreateObject": true,
        "supportedVisualTypes": ["model/gltf-binary"],
        "maxRegionsPerAccount": null,
        "maxObjectsPerRegion": null,
        "maxAvatarsPerRegion": null,
        "maxAssetSizeBytes": null
    }))
    .expect("capability fixture");
    let account = Id::from(ACCOUNT);
    backend.register_account(&account, cap);
    for id in [R1, R2, R3] {
        backend.insert_object_for_test("SceneRegion", &account, id, region(id));
    }
    for (id, region, user, left_at) in [
        ("AV-ALICE-1", R1, ALICE, json!(null)),
        ("AV-ALICE-2", R2, ALICE, json!(null)),
        ("AV-ALICE-3", R3, ALICE, json!("2026-06-06T11:00:00Z")),
        ("AV-BOB-1", R1, BOB, json!(null)),
        ("AV-BOB-2", R2, BOB, json!(null)),
        ("AV-BOB-3", R3, BOB, json!(null)),
    ] {
        backend.insert_object_for_test(
            "SceneAvatar",
            &account,
            id,
            avatar(id, region, user, left_at),
        );
    }
    Arc::new(backend)
}

/// Bob entered `region` (the draft's `SceneAvatarEvent` example).
fn bob_entered(region: &str) -> SceneEvent {
    serde_json::from_value(json!({
        "@type": "SceneAvatarEvent",
        "accountId": ACCOUNT,
        "regionId": region,
        "avatarId": "AV-BOB",
        "event": "entered",
        "userId": BOB,
        "displayName": "Bob"
    }))
    .expect("SceneAvatarEvent")
}

/// A server whose connections are Alice's, with both handlers linked.
async fn server() -> (std::net::SocketAddr, SceneEventHub) {
    let mut dispatcher = Dispatcher::new();
    dispatcher.register("Core/echo", Arc::new(Echo));
    let dispatcher = Arc::new(dispatcher);
    let chat_hub = ChatEventHub::new(16);
    let scene_hub = SceneEventHub::new(64);
    let chat_backend = Arc::new(jmap_chat_server::memory::MemoryBackend::<()>::default());
    let scene = scene_backend();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hub = scene_hub.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let handler = Arc::new(DispatcherHandler::new(Arc::clone(&dispatcher), (), || {
                State::from("s0")
            }));
            let scene_handler = SceneStreamHandler::new(
                Arc::clone(&scene),
                Id::from(ALICE),
                Id::from(ACCOUNT),
                &hub,
            );
            let chat_handler = link_scene_stream(
                ChatStreamHandler::new(
                    Arc::clone(&chat_backend),
                    (),
                    [Id::from("acct-alice")],
                    &chat_hub,
                ),
                &scene_handler,
            );
            tokio::spawn(async move {
                let Ok(ws) = jmap_websocket::tungstenite::accept(tcp).await else {
                    return;
                };
                let conn = Connection::new(handler)
                    .with_extension(chat_handler)
                    .with_extension(scene_handler);
                let _ = jmap_websocket::tungstenite::serve(conn, ws).await;
            });
        }
    });
    (addr, scene_hub)
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

/// The next frame is Bob entering `region`.
async fn next_is(ws: &mut Client, region: &str) {
    let e = recv(ws).await;
    assert_eq!(e["@type"], "SceneAvatarEvent");
    assert_eq!(e["regionId"], region);
}

/// "`ChatStreamEnable` with `"scene"` in `dataTypes`: Subscribes to all
/// Scene event types for all regions in which the user has an active
/// avatar"; "If `"scene"` was included in the `ChatStreamEnable`
/// `dataTypes`, Scene events delivered through that path stop"
/// (§Subscription Independence).
#[tokio::test]
async fn chat_path_alone() {
    let (addr, hub) = server().await;
    let mut ws = connect(addr).await;
    control(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["scene"]}),
    )
    .await;

    hub.publish(bob_entered(R3)); // Alice's avatar there has left
    hub.publish(bob_entered(R1));
    hub.publish(bob_entered(R2));
    next_is(&mut ws, R1).await;
    next_is(&mut ws, R2).await;

    control(&mut ws, json!({"@type": "ChatStreamDisable"})).await;
    hub.publish(bob_entered(R1));
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
        json!({"@type": "SceneStreamEnable", "regionIds": [R1]}),
    )
    .await;
    control(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["presence", "scene"]}),
    )
    .await;

    hub.publish(bob_entered(R1)); // both paths
    hub.publish(bob_entered(R2)); // the "scene" path only
    next_is(&mut ws, R1).await;
    next_is(&mut ws, R2).await;
    nothing(&mut ws).await;
}

/// §Subscription Independence and §Coexistence Rules: each disable leaves
/// the other path, and "An error in one mechanism (e.g., a
/// `SceneStreamEnable` whose `eventTypes` contains no recognized values)
/// MUST NOT affect the other's active subscription."
#[tokio::test]
async fn each_path_survives_the_other() {
    let (addr, hub) = server().await;
    let mut ws = connect(addr).await;
    control(
        &mut ws,
        json!({"@type": "SceneStreamEnable", "regionIds": [R1]}),
    )
    .await;
    control(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["scene"]}),
    )
    .await;

    control(&mut ws, json!({"@type": "ChatStreamDisable"})).await;
    hub.publish(bob_entered(R2)); // only the "scene" path covered it
    hub.publish(bob_entered(R1));
    next_is(&mut ws, R1).await;

    control(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["scene"]}),
    )
    .await;
    send(
        &mut ws,
        json!({"@type": "SceneStreamEnable", "id": "e1", "eventTypes": ["SceneFooBarEvent"]}),
    )
    .await;
    let e = recv(&mut ws).await;
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:invalidArguments");
    control(&mut ws, json!({"@type": "SceneStreamDisable"})).await;
    hub.publish(bob_entered(R2));
    next_is(&mut ws, R2).await;
    nothing(&mut ws).await;
}
