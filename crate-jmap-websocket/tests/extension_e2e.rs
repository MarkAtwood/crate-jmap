//! Extension message types end to end over a real WebSocket: a
//! tokio-tungstenite client against `jmap_websocket::tungstenite::serve`
//! on a loopback socket, with a toy [`ExtensionHandler`] registered next
//! to a `Core/echo` dispatcher.
//!
//! Oracle: the rule the JMAP Chat, VTC and Scene WebSocket drafts add to
//! RFC 8887 (draft-atwood-jmap-chat-wss-00 §Update to RFC 8887): a
//! capability "MAY define additional WebSocket message types, each
//! identified by its `@type` value"; the server "MUST NOT send an
//! additional server-to-client message type unless the client has enabled
//! it using a control message"; an unrecognized client `@type` is answered
//! with `notRequest` and "The server SHOULD NOT close the WebSocket
//! connection"; the subscription "is bound to the WebSocket connection.
//! When the connection closes ... the subscription is immediately
//! cancelled" (§Subscription Lifecycle of the VTC and Scene drafts).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use jmap_server::{CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler};
use jmap_types::{JmapError, State};
use jmap_websocket::server::{
    Connection, DispatcherHandler, ExtensionHandler, ExtensionMessage, ExtensionSender,
};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// RFC 8620 §4 `Core/echo`.
struct Echo;

impl JmapHandler<()> for Echo {
    fn call(&self, cx: CallContext<()>) -> HandlerFuture {
        let args = cx.args;
        Box::pin(async move { Ok(HandlerOutput::new(args)) })
    }
}

/// The toy extension of one connection:
///
/// - `ToyEcho` is answered with a `ToyEchoed` carrying the same members;
/// - `ToyStreamEnable` / `ToyStreamDisable` turn `ToyEvent`s on and off;
///   the events come from a server-wide broadcast, forwarded by a task
///   that ends with the connection;
/// - `close` reports the connection's caller on `closed`.
struct Toy {
    caller: String,
    feed: broadcast::Sender<u64>,
    enabled: Arc<AtomicBool>,
    events: Option<ExtensionSender>,
    closed: mpsc::UnboundedSender<String>,
}

impl ExtensionHandler for Toy {
    fn message_types(&self) -> &[&str] {
        &["ToyEcho", "ToyStreamEnable", "ToyStreamDisable"]
    }

    fn start(&mut self, events: ExtensionSender) {
        let mut feed = self.feed.subscribe();
        let enabled = Arc::clone(&self.enabled);
        let forward = events.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = forward.closed() => return,
                    n = feed.recv() => {
                        let Ok(n) = n else { return };
                        if enabled.load(Ordering::SeqCst) {
                            // Ephemeral: drop rather than buffer.
                            let _ = forward.try_send(&json!({"@type": "ToyEvent", "n": n}));
                        }
                    }
                }
            }
        });
        self.events = Some(events);
    }

    async fn handle(&mut self, message: ExtensionMessage) -> Result<(), JmapError> {
        match message.message_type.as_str() {
            "ToyEcho" => {
                let mut echoed = message.body;
                echoed.insert("@type".to_owned(), json!("ToyEchoed"));
                if let Some(events) = &self.events {
                    events
                        .send(&echoed)
                        .await
                        .map_err(|e| JmapError::server_fail(e.to_string()))?;
                }
            }
            "ToyStreamEnable" => self.enabled.store(true, Ordering::SeqCst),
            _ => self.enabled.store(false, Ordering::SeqCst),
        }
        Ok(())
    }

    async fn close(&mut self) {
        self.enabled.store(false, Ordering::SeqCst);
        let _ = self.closed.send(self.caller.clone());
    }
}

struct Server {
    addr: std::net::SocketAddr,
    feed: broadcast::Sender<u64>,
    closed: mpsc::UnboundedReceiver<String>,
}

/// Starts the server. Each connection's caller is `c<n>`, numbered from 1.
async fn server() -> Server {
    let mut dispatcher = Dispatcher::new();
    dispatcher.register("Core/echo", Arc::new(Echo));
    let dispatcher = Arc::new(dispatcher);
    let (feed, _) = broadcast::channel(16);
    let (closed_tx, closed) = mpsc::unbounded_channel();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_feed = feed.clone();
    tokio::spawn(async move {
        for n in 1.. {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            let handler = Arc::new(DispatcherHandler::new(Arc::clone(&dispatcher), (), || {
                State::from("s0")
            }));
            let toy = Toy {
                caller: format!("c{n}"),
                feed: server_feed.clone(),
                enabled: Arc::default(),
                events: None,
                closed: closed_tx.clone(),
            };
            tokio::spawn(async move {
                let Ok(ws) = jmap_websocket::tungstenite::accept(tcp).await else {
                    return;
                };
                let conn = Connection::new(handler).with_extension(toy);
                let _ = jmap_websocket::tungstenite::serve(conn, ws).await;
            });
        }
    });
    Server { addr, feed, closed }
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

async fn send(ws: &mut Client, v: &Value) {
    ws.send(Message::Text(v.to_string().into())).await.unwrap();
}

async fn recv(ws: &mut Client) -> Value {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("a message within 5 s")
            .expect("open")
            .unwrap();
        match m {
            Message::Text(t) => return serde_json::from_str(&t).unwrap(),
            Message::Ping(_) | Message::Pong(_) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// A `Core/echo` round trip: everything sent before it has been read and
/// handled by the time its Response arrives.
async fn sync(ws: &mut Client, id: &str) {
    send(
        ws,
        &json!({
            "@type": "Request",
            "id": id,
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [["Core/echo", {}, "c"]]
        }),
    )
    .await;
    let r = recv(ws).await;
    assert_eq!(r["@type"], "Response");
    assert_eq!(r["requestId"], id);
}

/// Publishes `n` and waits until the connection's forwarder took it, so
/// the test knows whether it was sent.
async fn publish(server: &Server, n: u64) {
    server.feed.send(n).unwrap();
    while !server.feed.is_empty() {
        tokio::task::yield_now().await;
    }
}

async fn closed_caller(server: &mut Server) -> String {
    tokio::time::timeout(Duration::from_secs(5), server.closed.recv())
        .await
        .expect("close hook within 5 s")
        .unwrap()
}

/// Echo, server-initiated events only while enabled, `notRequest` for an
/// unregistered type, and the close hook on a clean close.
#[tokio::test]
async fn toy_extension_over_a_real_socket() {
    let mut server = server().await;
    let mut ws = connect(server.addr).await;

    // A client message of a registered type reaches the handler, which
    // answers through its sender.
    send(
        &mut ws,
        &json!({"@type": "ToyEcho", "id": "e1", "say": "hi"}),
    )
    .await;
    assert_eq!(
        recv(&mut ws).await,
        json!({"@type": "ToyEchoed", "id": "e1", "say": "hi"})
    );

    // Not enabled yet: no ToyEvent may be sent.
    publish(&server, 1).await;
    sync(&mut ws, "s1").await;

    // Enabled: the server pushes on its own.
    send(&mut ws, &json!({"@type": "ToyStreamEnable"})).await;
    sync(&mut ws, "s2").await;
    server.feed.send(2).unwrap();
    assert_eq!(recv(&mut ws).await, json!({"@type": "ToyEvent", "n": 2}));

    // A type no capability defines on this server.
    send(
        &mut ws,
        &json!({"@type": "SceneStreamEnable", "id": "x1", "regionIds": null}),
    )
    .await;
    let e = recv(&mut ws).await;
    assert_eq!(e["@type"], "RequestError");
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:notRequest");
    assert_eq!(e["requestId"], "x1");

    // Disabled: events stop; the connection stays open.
    send(&mut ws, &json!({"@type": "ToyStreamDisable"})).await;
    sync(&mut ws, "s3").await;
    publish(&server, 3).await;
    sync(&mut ws, "s4").await;

    ws.close(None).await.unwrap();
    assert_eq!(closed_caller(&mut server).await, "c1");
}

/// "network failure": the TCP connection drops without a Close frame, and
/// the close hook still runs.
#[tokio::test]
async fn close_hook_runs_when_the_socket_drops() {
    let mut server = server().await;
    let mut ws = connect(server.addr).await;
    send(&mut ws, &json!({"@type": "ToyStreamEnable"})).await;
    sync(&mut ws, "s1").await;
    drop(ws);
    assert_eq!(closed_caller(&mut server).await, "c1");
}
