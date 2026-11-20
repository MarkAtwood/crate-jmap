//! End to end over a real WebSocket: a tokio-tungstenite client against
//! `jmap_websocket::tungstenite::serve` on a loopback socket, in front of
//! a `jmap_server::Dispatcher` with the jmap-mail-server memory backend.
//!
//! Oracles: the RFC 8887 §4.4 examples (handshake, Core/echo request and
//! response, notJSON error) and the §4.3.x property rules; the expected
//! Mailbox state is the `newState` the dispatcher itself returned, not
//! anything this crate computes.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use jmap_server::{CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler};
use jmap_types::{Id, State};
use jmap_websocket::server::{Connection, DispatcherHandler, PublishingHandler, PushHub};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::{HeaderValue, StatusCode};
use tokio_tungstenite::tungstenite::{Bytes, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const ACCOUNT: &str = "a1";

/// RFC 8620 §4 `Core/echo`: "returns exactly the same arguments".
struct Echo;

impl JmapHandler<()> for Echo {
    fn call(&self, cx: CallContext<()>) -> HandlerFuture {
        let args = cx.args;
        Box::pin(async move { Ok(HandlerOutput::new(args)) })
    }
}

/// Starts the server; returns its address.
async fn server() -> std::net::SocketAddr {
    let backend = jmap_mail_server::memory::MemoryBackend::<()>::default();
    backend.register_account(&Id::from(ACCOUNT));
    let mut dispatcher = Dispatcher::new();
    dispatcher.register("Core/echo", Arc::new(Echo));
    jmap_mail_server::register_mail_handlers(&mut dispatcher, Arc::new(backend));
    let dispatcher = Arc::new(dispatcher);
    let hub = PushHub::new(64);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            let handler = Arc::new(PublishingHandler::new(
                DispatcherHandler::new(Arc::clone(&dispatcher), (), || State::from("s0"))
                    .with_known_capabilities([
                        "urn:ietf:params:jmap:core",
                        "urn:ietf:params:jmap:mail",
                    ]),
                hub.clone(),
            ));
            let hub = hub.clone();
            tokio::spawn(async move {
                let Ok(ws) = jmap_websocket::tungstenite::accept(tcp).await else {
                    return;
                };
                let conn = Connection::new(handler).with_push(hub);
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
    let (ws, response) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(response.headers()["Sec-WebSocket-Protocol"], "jmap");
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

fn mailbox_set(id: &str, name: &str) -> Value {
    json!({
        "@type": "Request",
        "id": id,
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "methodCalls": [["Mailbox/set", {
            "accountId": ACCOUNT,
            "create": {"k1": {"name": name}}
        }, "c0"]]
    })
}

/// RFC 8887 §4.4 Core/echo request and response, and §4.3.3: no
/// `requestId` for a request without `id`.
#[tokio::test]
async fn request_response_with_ids() {
    let mut ws = connect(server().await).await;
    let request: Value = serde_json::from_str(
        r#"{
     "@type": "Request",
     "id": "R1",
     "using": [ "urn:ietf:params:jmap:core" ],
     "methodCalls": [
       [
         "Core/echo", {
           "hello": true,
           "high": 5
         },
         "b3ff"
       ]
     ]
   }"#,
    )
    .unwrap();
    send(&mut ws, &request).await;
    let response = recv(&mut ws).await;
    assert_eq!(response["@type"], "Response");
    assert_eq!(response["requestId"], "R1");
    assert_eq!(
        response["methodResponses"],
        json!([["Core/echo", {"hello": true, "high": 5}, "b3ff"]])
    );
    assert_eq!(response["sessionState"], "s0");

    let mut no_id = request.clone();
    no_id.as_object_mut().unwrap().remove("id");
    send(&mut ws, &no_id).await;
    let response = recv(&mut ws).await;
    assert_eq!(response["@type"], "Response");
    assert!(response.get("requestId").is_none(), "{response}");

    // RFC 8620 §3.3: an unknown `using` URI is `unknownCapability`.
    let mut unknown = request;
    unknown["id"] = json!("R2");
    unknown["using"] = json!(["urn:example:nope"]);
    send(&mut ws, &unknown).await;
    let e = recv(&mut ws).await;
    assert_eq!(e["@type"], "RequestError");
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:unknownCapability");
    assert_eq!(e["requestId"], "R2");
}

/// §4.3.5.2: after WebSocketPushEnable a Mailbox/set produces a
/// StateChange carrying the new Mailbox state and a pushState;
/// §4.3.5.3: after WebSocketPushDisable it produces none.
#[tokio::test]
async fn push_enable_state_change_and_disable() {
    let mut ws = connect(server().await).await;
    send(
        &mut ws,
        &json!({"@type": "WebSocketPushEnable", "dataTypes": ["Mailbox", "Email"]}),
    )
    .await;
    send(&mut ws, &mailbox_set("S1", "Inbox2")).await;

    // The Response and the StateChange may come in either order.
    let (mut response, mut change) = (None, None);
    for _ in 0..2 {
        let m = recv(&mut ws).await;
        match m["@type"].as_str() {
            Some("Response") => response = Some(m),
            Some("StateChange") => change = Some(m),
            _ => panic!("unexpected {m}"),
        }
    }
    let response = response.unwrap();
    let change = change.unwrap();
    assert_eq!(response["requestId"], "S1");
    let set = &response["methodResponses"][0];
    assert_eq!(set[0], "Mailbox/set");
    assert!(set[1]["created"]["k1"]["id"].is_string(), "{set}");
    let new_state = set[1]["newState"].as_str().unwrap();
    assert_eq!(change["changed"], json!({ACCOUNT: {"Mailbox": new_state}}));
    let push_state = change["pushState"].as_str().unwrap().to_owned();
    assert!(!push_state.is_empty());

    send(&mut ws, &json!({"@type": "WebSocketPushDisable"})).await;
    send(&mut ws, &mailbox_set("S2", "Other")).await;
    let r = recv(&mut ws).await;
    assert_eq!(r["requestId"], "S2", "no StateChange after disable: {r}");
    let echo = json!({"@type": "Request", "id": "E", "using": ["urn:ietf:params:jmap:core"],
        "methodCalls": [["Core/echo", {}, "e"]]});
    send(&mut ws, &echo).await;
    let r = recv(&mut ws).await;
    assert_eq!(r["requestId"], "E", "no StateChange after disable: {r}");

    // Re-enabling with the earlier pushState replays the S2 change: "the
    // server SHOULD immediately send all changes since that state token".
    send(
        &mut ws,
        &json!({"@type": "WebSocketPushEnable", "dataTypes": null, "pushState": push_state}),
    )
    .await;
    let replay = recv(&mut ws).await;
    assert_eq!(replay["@type"], "StateChange", "{replay}");
    assert!(
        replay["changed"][ACCOUNT]["Mailbox"].is_string(),
        "{replay}"
    );
    assert_ne!(replay["changed"][ACCOUNT]["Mailbox"], json!(new_state));
}

/// §4.3.1 and the §4.4 notJSON example: invalid JSON gets a RequestError
/// with `requestId: null`; a message that is not a client message gets
/// `notRequest` with its id echoed; the connection stays usable; a binary
/// frame closes it with 1003.
#[tokio::test]
async fn malformed_frames() {
    let mut ws = connect(server().await).await;
    ws.send(Message::Text(
        "The quick brown fox jumps\n over the lazy dog.".into(),
    ))
    .await
    .unwrap();
    assert_eq!(
        recv(&mut ws).await,
        json!({
            "@type": "RequestError",
            "requestId": null,
            "type": "urn:ietf:params:jmap:error:notJSON",
            "status": 400,
            "detail": "The request did not parse as I-JSON."
        })
    );

    send(
        &mut ws,
        &json!({"@type": "StateChange", "id": "X1", "changed": {}}),
    )
    .await;
    let e = recv(&mut ws).await;
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:notRequest");
    assert_eq!(e["requestId"], "X1");

    send(
        &mut ws,
        &json!({"@type": "Request", "id": "X2", "using": []}),
    )
    .await;
    let e = recv(&mut ws).await;
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:notRequest");
    assert_eq!(e["requestId"], "X2");

    ws.send(Message::Binary(Bytes::from_static(b"\x00\x01")))
        .await
        .unwrap();
    let m = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let Message::Close(Some(frame)) = m else {
        panic!("expected Close, got {m:?}")
    };
    assert_eq!(u16::from(frame.code), 1003);
}

/// §4.2: a handshake that does not offer `jmap` is refused.
#[tokio::test]
async fn handshake_without_jmap_is_refused() {
    let addr = server().await;
    let request = format!("ws://{addr}/jmap/ws/")
        .into_client_request()
        .unwrap();
    let err = tokio_tungstenite::connect_async(request).await.unwrap_err();
    let tokio_tungstenite::tungstenite::Error::Http(resp) = err else {
        panic!("expected an HTTP refusal, got {err:?}")
    };
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
