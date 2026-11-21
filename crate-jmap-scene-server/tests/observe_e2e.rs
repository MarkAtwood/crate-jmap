//! Observer subscriptions end to end over a real WebSocket:
//! tokio-tungstenite clients against `jmap_websocket::tungstenite::serve`
//! on a loopback socket, `SceneStreamHandler` over a `MemoryBackend` that
//! advertises `supportsObserve`.
//!
//! Oracle: draft-atwood-jmap-scene-wss-00 at jmap-chat-spec 93f2258,
//! §SceneStreamEnable (Semantics) and draft-atwood-jmap-scene-00
//! §SceneRegion (`allowObservers`, `observerCount`); each test quotes the
//! sentence it checks. Records and frames are hand-written JSON.

mod common;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use futures::{SinkExt as _, StreamExt as _};
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::websocket::{SceneEventHub, SceneStreamHandler};
use jmap_scene_types::{SceneEvent, SceneRegion};
use jmap_server::{CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler, JmapReads};
use jmap_types::{Id, State};
use jmap_websocket::server::{Connection, DispatcherHandler};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// A public region owned by [`ALICE`], open to observers.
const R1: &str = "01J5ABC0000000000000000001";
/// A public region owned by [`ALICE`] with `allowObservers: false`.
const CLOSED: &str = "01J5ABC0000000000000000002";
/// An `"invite"` region [`CAROL`] cannot access.
const VAULT: &str = "01J5ABC0000000000000000003";
/// A spectator with no avatar anywhere.
const CAROL: &str = "user:carol@example.com";
/// How long a negative check waits for a frame that must not come.
const QUIET: Duration = Duration::from_millis(150);

struct Echo;

impl JmapHandler<()> for Echo {
    fn call(&self, cx: CallContext<()>) -> HandlerFuture {
        let args = cx.args;
        Box::pin(async move { Ok(HandlerOutput::new(args)) })
    }
}

fn region(id: &str, policy: &str, allow: bool) -> Value {
    let mut r = stored_region(id, id, "2026-06-01T10:00:00Z");
    r["accessPolicy"] = json!(policy);
    r["allowObservers"] = json!(allow);
    r["observerCount"] = json!(0);
    r
}

fn scene() -> Arc<MemoryBackend> {
    let backend = backend();
    backend.set_supports_observe(true);
    seed_as(&backend, "SceneRegion", region(R1, "public", true));
    seed_as(&backend, "SceneRegion", region(CLOSED, "public", false));
    seed_as(&backend, "SceneRegion", region(VAULT, "invite", true));
    Arc::new(backend)
}

/// The users of the next connections, in accept order.
type Users = Arc<Mutex<VecDeque<&'static str>>>;

async fn server(backend: Arc<MemoryBackend>, hub: SceneEventHub) -> (std::net::SocketAddr, Users) {
    let mut dispatcher = Dispatcher::new();
    dispatcher.register("Core/echo", Arc::new(Echo));
    let dispatcher = Arc::new(dispatcher);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let users: Users = Arc::default();
    let queue = Arc::clone(&users);
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let user = queue.lock().unwrap().pop_front().expect("a queued user");
            let handler = Arc::new(DispatcherHandler::new(Arc::clone(&dispatcher), (), || {
                State::from("s0")
            }));
            let scene = SceneStreamHandler::new(
                Arc::clone(&backend),
                Id::from(user),
                Id::from(ACCOUNT),
                &hub,
            );
            tokio::spawn(async move {
                let Ok(ws) = jmap_websocket::tungstenite::accept(tcp).await else {
                    return;
                };
                let conn = Connection::new(handler).with_extension(scene);
                let _ = jmap_websocket::tungstenite::serve(conn, ws).await;
            });
        }
    });
    (addr, users)
}

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn connect(addr: std::net::SocketAddr, users: &Users, user: &'static str) -> Client {
    users.lock().unwrap().push_back(user);
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

fn round_start(region: &str) -> (SceneEvent, Value) {
    let raw = json!({
        "@type": "SceneRegionEvent",
        "accountId": ACCOUNT,
        "regionId": region,
        "event": "com.example.game.round-start",
        "data": {"round": 1}
    });
    (serde_json::from_value(raw.clone()).unwrap(), raw)
}

async fn observer_count(backend: &MemoryBackend, region: &str) -> Option<u64> {
    let (list, _) = backend
        .get_objects::<SceneRegion>(
            &Id::from(ALICE),
            &Id::from(ACCOUNT),
            Some(&[Id::from(region)]),
            None,
        )
        .await
        .unwrap();
    list[0].observer_count
}

/// Waits until `region`'s `observerCount` is `want`, for changes made
/// when a connection closes, which the client cannot sync with.
async fn count_becomes(backend: &MemoryBackend, region: &str, want: u64) {
    for _ in 0..100 {
        if observer_count(backend, region).await == Some(want) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "observerCount stayed {:?}, want {want}",
        observer_count(backend, region).await
    );
}

/// "An observer subscription delivers the events of each region in
/// `regionIds` that the user has access to ... and whose `allowObservers`
/// property is `true`, with or without an active avatar"; the observer
/// "counts toward the region's `observerCount`", "A user with several
/// observing connections counts once", and "The count drops when the
/// subscription stops covering the region, through a new
/// `SceneStreamEnable`, `SceneStreamDisable`, the connection closing".
#[tokio::test]
async fn observer_receives_events_and_is_counted_until_close() {
    let backend = scene();
    let hub = SceneEventHub::new(64);
    let (addr, users) = server(Arc::clone(&backend), hub.clone()).await;
    let mut carol = connect(addr, &users, CAROL).await;
    let (event, raw) = round_start(R1);

    enable(&mut carol, json!({"regionIds": [R1]})).await;
    hub.publish(event.clone());
    nothing(&mut carol, "no-avatar").await;
    assert_eq!(observer_count(&backend, R1).await, Some(0));

    enable(&mut carol, json!({"regionIds": [R1], "observe": true})).await;
    assert_eq!(observer_count(&backend, R1).await, Some(1));
    hub.publish(event.clone());
    assert_eq!(recv(&mut carol).await, raw);

    let mut carol2 = connect(addr, &users, CAROL).await;
    enable(&mut carol2, json!({"regionIds": [R1], "observe": true})).await;
    assert_eq!(observer_count(&backend, R1).await, Some(1));
    hub.publish(event.clone());
    assert_eq!(recv(&mut carol2).await, raw);
    assert_eq!(recv(&mut carol).await, raw);

    // A new SceneStreamEnable without `observe` on the second connection.
    enable(&mut carol2, json!({"regionIds": [R1]})).await;
    assert_eq!(observer_count(&backend, R1).await, Some(1));
    enable(&mut carol2, json!({"regionIds": [R1], "observe": true})).await;

    // SceneStreamDisable on the first.
    send(&mut carol, json!({"@type": "SceneStreamDisable"})).await;
    sync(&mut carol, "disabled").await;
    assert_eq!(observer_count(&backend, R1).await, Some(1));

    // Closing the second.
    carol2.close(None).await.unwrap();
    count_becomes(&backend, R1, 0).await;
}

/// "When `observe` is `true` and a listed region that the user has access
/// to has `allowObservers` set to `false` ... the server MUST respond with
/// a `RequestError` frame ... `forbidden` and a `regionIds` member listing
/// the refused regions, and MUST NOT update the current ephemeral
/// subscription state." "A listed region that the user has no access to,
/// or that does not exist, is not refused".
#[tokio::test]
async fn refused_observe_is_forbidden_with_region_ids() {
    let backend = scene();
    let hub = SceneEventHub::new(64);
    let (addr, users) = server(Arc::clone(&backend), hub.clone()).await;
    let mut carol = connect(addr, &users, CAROL).await;
    enable(&mut carol, json!({"regionIds": [R1], "observe": true})).await;

    send(
        &mut carol,
        json!({
            "@type": "SceneStreamEnable",
            "id": "obs2",
            "regionIds": [VAULT, CLOSED, "01J5ABC0000000000000000099", R1],
            "observe": true
        }),
    )
    .await;
    let e = recv(&mut carol).await;
    assert_eq!(e["@type"], "RequestError");
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:forbidden");
    assert_eq!(e["requestId"], "obs2");
    assert_eq!(e["regionIds"], json!([CLOSED]));

    // The previous subscription stands and is still counted.
    let (event, raw) = round_start(R1);
    hub.publish(event);
    assert_eq!(recv(&mut carol).await, raw);
    assert_eq!(observer_count(&backend, R1).await, Some(1));
    assert_eq!(observer_count(&backend, CLOSED).await, Some(0));

    // Inaccessible and missing regions alone: accepted, silent, uncounted.
    enable(
        &mut carol,
        json!({"regionIds": [VAULT, "01J5ABC0000000000000000099"], "observe": true}),
    )
    .await;
    assert_eq!(observer_count(&backend, VAULT).await, Some(0));
    assert_eq!(observer_count(&backend, R1).await, Some(0));
    hub.publish(round_start(VAULT).0);
    nothing(&mut carol, "vault").await;
}

/// "If `observe` is `true` and `regionIds` is `null`, the server MUST
/// respond with a `RequestError` of type ... `invalidArguments`".
#[tokio::test]
async fn observe_without_region_ids_is_invalid_arguments() {
    let backend = scene();
    let hub = SceneEventHub::new(64);
    let (addr, users) = server(backend, hub).await;
    let mut carol = connect(addr, &users, CAROL).await;
    send(
        &mut carol,
        json!({"@type": "SceneStreamEnable", "id": "o", "regionIds": null, "observe": true}),
    )
    .await;
    let e = recv(&mut carol).await;
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:invalidArguments");
    assert_eq!(e["requestId"], "o");
}

/// "If the region owner later sets `allowObservers` to `false` ... events
/// for that region stop", and the observer no longer counts.
#[tokio::test]
async fn closing_the_region_stops_delivery() {
    let backend = scene();
    let hub = SceneEventHub::new(64);
    let (addr, users) = server(Arc::clone(&backend), hub.clone()).await;
    let mut carol = connect(addr, &users, CAROL).await;
    enable(&mut carol, json!({"regionIds": [R1], "observe": true})).await;
    assert_eq!(observer_count(&backend, R1).await, Some(1));

    let resp = jmap_scene_server::handle_scene_region_set(
        &*backend,
        &Id::from(ALICE),
        &std::collections::BTreeMap::new(),
        json!({"accountId": ACCOUNT, "update": {(R1): {"allowObservers": false}}}),
    )
    .await
    .unwrap()
    .primary;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(observer_count(&backend, R1).await, Some(0));
    hub.publish(round_start(R1).0);
    nothing(&mut carol, "closed").await;
}

/// Without `supportsObserve`, `observe` is "Recognized only when the
/// server advertises `supportsObserve`": ignored, no error, no count.
#[tokio::test]
async fn observe_is_ignored_without_supports_observe() {
    let backend = scene();
    backend.set_supports_observe(false);
    let hub = SceneEventHub::new(64);
    let (addr, users) = server(Arc::clone(&backend), hub.clone()).await;
    let mut carol = connect(addr, &users, CAROL).await;
    enable(&mut carol, json!({"regionIds": null, "observe": true})).await;
    enable(&mut carol, json!({"regionIds": [CLOSED], "observe": true})).await;
    hub.publish(round_start(CLOSED).0);
    nothing(&mut carol, "ignored").await;
    assert_eq!(observer_count(&backend, CLOSED).await, Some(0));
}
