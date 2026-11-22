//! Message size caps end to end: a tokio-tungstenite client against
//! `jmap_websocket::tungstenite::{accept, accept_with_config, serve}` on a
//! loopback socket.
//!
//! Oracles: RFC 6455 §7.4.1 (1009 "indicates that an endpoint is
//! terminating the connection because it has received a message that is
//! too big for it to process"), RFC 6455 §5.4 (a message is a first frame
//! and continuation frames up to one with FIN set), and RFC 8620 §3.6.1
//! (`limit` naming `maxSizeRequest`). The size of each message is counted
//! here from the bytes the test writes, not from anything this crate
//! computes.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use jmap_server::{CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler};
use jmap_types::State;
use jmap_websocket::server::{Connection, ConnectionConfig, DispatcherHandler};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::frame::Frame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::{Data, OpCode};
use tokio_tungstenite::tungstenite::{Bytes, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// The `maxSizeRequest` of these tests, in octets.
const LIMIT: usize = 64 * 1024;
/// The fragment size the client sends: LIMIT is 8 of them.
const FRAGMENT: usize = 8 * 1024;

/// RFC 8620 §4 `Core/echo`: "returns exactly the same arguments".
struct Echo;

impl JmapHandler<()> for Echo {
    fn call(&self, cx: CallContext<()>) -> HandlerFuture {
        let args = cx.args;
        Box::pin(async move { Ok(HandlerOutput::new(args)) })
    }
}

/// How the server accepts the WebSocket.
#[derive(Clone)]
enum Accept {
    /// `tungstenite::accept`: the default `ConnectionConfig`'s caps.
    Default,
    /// `tungstenite::accept_with_config` with this config.
    WithConfig(ConnectionConfig),
}

/// Starts a server whose connections run with `config` and accept with
/// `accept`; returns its address.
async fn server(config: ConnectionConfig, accept: Accept) -> std::net::SocketAddr {
    let mut dispatcher = Dispatcher::new();
    dispatcher.register("Core/echo", Arc::new(Echo));
    let dispatcher = Arc::new(dispatcher);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            let handler = Arc::new(DispatcherHandler::new(Arc::clone(&dispatcher), (), || {
                State::from("s0")
            }));
            let (config, accept) = (config.clone(), accept.clone());
            tokio::spawn(async move {
                let ws = match &accept {
                    Accept::Default => jmap_websocket::tungstenite::accept(tcp).await,
                    Accept::WithConfig(c) => {
                        jmap_websocket::tungstenite::accept_with_config(tcp, c).await
                    }
                };
                let Ok(ws) = ws else { return };
                let conn = Connection::new(handler).with_config(config);
                let _ = jmap_websocket::tungstenite::serve(conn, ws).await;
            });
        }
    });
    addr
}

/// A server capped at [`LIMIT`] both in tungstenite and in the driver.
async fn limited_server() -> std::net::SocketAddr {
    let config = ConnectionConfig::default().max_size_request(Some(LIMIT));
    server(config.clone(), Accept::WithConfig(config)).await
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

/// A `Core/echo` Request with id `id` whose serialized form is exactly
/// `size` octets (ASCII, so octets and chars agree).
fn echo_request_of_size(id: &str, size: usize) -> String {
    let with_pad = |pad: usize| {
        json!({
            "@type": "Request",
            "id": id,
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [["Core/echo", {"pad": "x".repeat(pad)}, "c0"]]
        })
        .to_string()
    };
    let overhead = with_pad(0).len();
    let text = with_pad(size - overhead);
    assert_eq!(text.len(), size);
    text
}

/// Sends `text` as one message cut into frames of at most [`FRAGMENT`]
/// octets (RFC 6455 §5.4).
async fn send_fragmented(ws: &mut Client, text: &str) {
    let chunks: Vec<&[u8]> = text.as_bytes().chunks(FRAGMENT).collect();
    let last = chunks.len() - 1;
    for (i, chunk) in chunks.into_iter().enumerate() {
        let opcode = if i == 0 {
            OpCode::Data(Data::Text)
        } else {
            OpCode::Data(Data::Continue)
        };
        let frame = Frame::message(Bytes::copy_from_slice(chunk), opcode, i == last);
        ws.send(Message::Frame(frame)).await.unwrap();
    }
}

/// The next message that is not a Ping or Pong, within 10 s.
async fn next_message(ws: &mut Client) -> Message {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("a message within 10 s")
            .expect("open")
            .unwrap();
        if !matches!(m, Message::Ping(_) | Message::Pong(_)) {
            return m;
        }
    }
}

async fn recv_json(ws: &mut Client) -> Value {
    match next_message(ws).await {
        Message::Text(t) => serde_json::from_str(&t).unwrap(),
        other => panic!("expected a text message, got {other:?}"),
    }
}

/// Asserts the next message is a Close with status 1009.
async fn expect_close_1009(ws: &mut Client) {
    let m = next_message(ws).await;
    let Message::Close(Some(frame)) = m else {
        panic!("expected Close 1009, got {m:?}")
    };
    assert_eq!(u16::from(frame.code), 1009, "{frame:?}");
}

/// A message that grows past `maxSizeRequest` is refused while it
/// arrives: the client sends fragments up to one fragment past the limit
/// and never sends the final fragment, so the message is never complete,
/// and the server still closes with 1009. A server that buffered the
/// whole message before checking would wait forever instead.
#[tokio::test]
async fn growing_message_is_closed_with_1009_before_it_completes() {
    let mut ws = connect(limited_server().await).await;
    let fragment = "x".repeat(FRAGMENT);
    // The client means to send 4 * LIMIT, but stops one fragment past
    // LIMIT (none of these frames has FIN set).
    let planned = 4 * LIMIT / FRAGMENT;
    let sent_fragments = LIMIT / FRAGMENT + 1;
    assert!(sent_fragments < planned);
    for i in 0..sent_fragments {
        let opcode = if i == 0 {
            OpCode::Data(Data::Text)
        } else {
            OpCode::Data(Data::Continue)
        };
        let frame = Frame::message(Bytes::from(fragment.clone()), opcode, false);
        ws.send(Message::Frame(frame)).await.unwrap();
    }
    assert!(sent_fragments * FRAGMENT > LIMIT);
    expect_close_1009(&mut ws).await;
}

/// A single frame over the limit is refused from its header (the frame
/// cap equals the message cap).
#[tokio::test]
async fn single_frame_over_the_limit_is_closed_with_1009() {
    let mut ws = connect(limited_server().await).await;
    let text = echo_request_of_size("big", LIMIT + 1);
    ws.send(Message::Text(text.into())).await.unwrap();
    expect_close_1009(&mut ws).await;
}

/// A message of exactly `maxSizeRequest` octets is still answered, sent
/// as one frame and as fragments, on the same connection.
#[tokio::test]
async fn message_at_the_limit_is_answered() {
    let mut ws = connect(limited_server().await).await;

    let text = echo_request_of_size("whole", LIMIT);
    ws.send(Message::Text(text.into())).await.unwrap();
    let r = recv_json(&mut ws).await;
    assert_eq!(r["@type"], "Response", "{r}");
    assert_eq!(r["requestId"], "whole");

    let text = echo_request_of_size("fragmented", LIMIT);
    send_fragmented(&mut ws, &text).await;
    let r = recv_json(&mut ws).await;
    assert_eq!(r["@type"], "Response", "{r}");
    assert_eq!(r["requestId"], "fragmented");
    let sent: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        r["methodResponses"][0][1]["pad"], sent["methodCalls"][0][1]["pad"],
        "the whole fragmented message arrived"
    );
}

/// `tungstenite::accept` caps at the default `maxSizeRequest`,
/// 10 000 000 octets, not at tungstenite's 64 MiB: 10 000 001 octets
/// in fragments, without a final one, get 1009.
#[tokio::test]
async fn default_accept_caps_at_ten_million_octets() {
    let mut ws = connect(server(ConnectionConfig::default(), Accept::Default).await).await;
    let megabyte = Bytes::from("x".repeat(1_000_000));
    for i in 0..10 {
        let opcode = if i == 0 {
            OpCode::Data(Data::Text)
        } else {
            OpCode::Data(Data::Continue)
        };
        ws.send(Message::Frame(Frame::message(
            megabyte.clone(),
            opcode,
            false,
        )))
        .await
        .unwrap();
    }
    let one_more = Frame::message(
        Bytes::from_static(b"x"),
        OpCode::Data(Data::Continue),
        false,
    );
    ws.send(Message::Frame(one_more)).await.unwrap();
    expect_close_1009(&mut ws).await;
}

/// With a transport cap above the driver's `maxSizeRequest`, a message
/// between the two gets the RFC 8620 `limit` RequestError naming
/// `maxSizeRequest`, and the connection stays open.
#[tokio::test]
async fn message_under_the_transport_cap_and_over_max_size_request_is_a_request_error() {
    let driver = ConnectionConfig::default().max_size_request(Some(LIMIT));
    let transport = ConnectionConfig::default().max_size_request(Some(2 * LIMIT));
    let mut ws = connect(server(driver, Accept::WithConfig(transport)).await).await;

    let text = echo_request_of_size("over", LIMIT + 1);
    send_fragmented(&mut ws, &text).await;
    let e = recv_json(&mut ws).await;
    assert_eq!(e["@type"], "RequestError", "{e}");
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:limit");
    assert_eq!(e["limit"], "maxSizeRequest");
    assert_eq!(e["requestId"], Value::Null, "unparsed, so no requestId");

    ws.send(Message::Text(echo_request_of_size("after", 1000).into()))
        .await
        .unwrap();
    let r = recv_json(&mut ws).await;
    assert_eq!(r["requestId"], "after", "{r}");
}
