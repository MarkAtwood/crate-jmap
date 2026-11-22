//! `GET /ws` caps each message at the Session's `maxSizeRequest`.
//!
//! Oracles: the `maxSizeRequest` the Session advertises (RFC 8620 §2),
//! RFC 6455 §5.4 (a message is a first frame and continuation frames up to
//! one with FIN set) and §7.4.1 (1009: "a message that is too big for it
//! to process"). Message sizes are counted from the bytes the test sends.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use jmap_testjig::{TestjigConfig, TestjigHandle, spawn_in_process};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::tungstenite::protocol::frame::Frame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::{Data, OpCode};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The fragment size the client sends.
const FRAGMENT: usize = 1024 * 1024;

/// The Session's `urn:ietf:params:jmap:core` `maxSizeRequest`.
async fn advertised_max_size_request(jig: &TestjigHandle) -> usize {
    let session: Value = reqwest::Client::new()
        .get(format!("http://{}/.well-known/jmap", jig.addr))
        .bearer_auth(&jig.token)
        .send()
        .await
        .expect("session request")
        .json()
        .await
        .expect("session JSON");
    let max = session["capabilities"]["urn:ietf:params:jmap:core"]["maxSizeRequest"]
        .as_u64()
        .expect("maxSizeRequest is a number");
    usize::try_from(max).expect("fits usize")
}

async fn connect(jig: &TestjigHandle) -> Socket {
    let mut request = format!("ws://{}/ws?token={}", jig.addr, jig.token)
        .into_client_request()
        .expect("request");
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", "jmap".parse().expect("header"));
    let (socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("WS connect");
    socket
}

/// Sends `payload` as frames of at most [`FRAGMENT`] octets of one text
/// message, with FIN on the last frame only when `finish`.
async fn send_fragments(socket: &mut Socket, payload: &[u8], finish: bool) {
    let chunks: Vec<&[u8]> = payload.chunks(FRAGMENT).collect();
    let last = chunks.len() - 1;
    for (i, chunk) in chunks.into_iter().enumerate() {
        let opcode = if i == 0 {
            OpCode::Data(Data::Text)
        } else {
            OpCode::Data(Data::Continue)
        };
        let frame = Frame::message(chunk.to_vec(), opcode, finish && i == last);
        socket
            .send(Message::Frame(frame))
            .await
            .expect("send frame");
    }
}

/// The next message that is not a Ping or Pong, within 30 s.
async fn next_message(socket: &mut Socket) -> Message {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(30), socket.next())
            .await
            .expect("a message within 30 s")
            .expect("open")
            .expect("no transport error");
        if !matches!(m, Message::Ping(_) | Message::Pong(_)) {
            return m;
        }
    }
}

/// A message growing one octet past `maxSizeRequest`, whose final
/// fragment is never sent, is closed with 1009: the jig refused it while
/// it arrived rather than buffering a whole message first.
#[tokio::test]
async fn ws_message_over_max_size_request_is_closed_with_1009() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn testjig");
    let max = advertised_max_size_request(&jig).await;
    let mut socket = connect(&jig).await;

    send_fragments(&mut socket, &vec![b'x'; max + 1], false).await;
    let m = next_message(&mut socket).await;
    let Message::Close(Some(frame)) = m else {
        panic!("expected Close 1009, got {m:?}")
    };
    assert_eq!(u16::from(frame.code), 1009, "{frame:?}");
    jig.shutdown().await;
}

/// A Request of exactly `maxSizeRequest` octets, sent in fragments, is
/// still answered.
#[tokio::test]
async fn ws_message_at_max_size_request_is_answered() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn testjig");
    let max = advertised_max_size_request(&jig).await;
    let mut socket = connect(&jig).await;

    let with_pad = |pad: usize| {
        json!({
            "@type": "Request",
            "id": "big",
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [["Core/echo", {"pad": "x".repeat(pad)}, "c0"]]
        })
        .to_string()
    };
    let text = with_pad(max - with_pad(0).len());
    assert_eq!(text.len(), max);
    send_fragments(&mut socket, text.as_bytes(), true).await;

    let m = next_message(&mut socket).await;
    let Message::Text(body) = m else {
        panic!("expected a Response, got {m:?}")
    };
    let response: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(response["@type"], "Response");
    assert_eq!(response["requestId"], "big");
    let _ = socket.close(None).await;
    jig.shutdown().await;
}
