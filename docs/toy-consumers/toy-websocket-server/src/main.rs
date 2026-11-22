//! A deployment's view of jmap-websocket: one `Dispatcher<Id>` with the
//! Tasks handlers behind RFC 8887, push included, and a plain
//! tokio-tungstenite client that enables push, creates a TaskList and
//! waits for the StateChange. The connection also carries
//! jmap-chat-server's `ChatStreamHandler` extension: the client enables
//! presence events and receives one the server publishes. A second
//! connection sends a message over `maxSizeRequest` and gets Close 1009.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use jmap_chat_server::websocket::{ChatEventHub, ChatStreamHandler};
use jmap_chat_types::ChatPresenceEvent;
use jmap_tasks_server::memory::MemoryBackend;
use jmap_types::{Id, State};
use jmap_websocket::jmap_server::Dispatcher;
use jmap_websocket::server::{
    CLOSE_MESSAGE_TOO_BIG, Connection, ConnectionConfig, DispatcherHandler, PublishingHandler,
    PushHub,
};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::HeaderValue;

const ACCOUNT: &str = "alice";
/// The `maxSizeRequest` the toy serves with, in octets.
const MAX_SIZE_REQUEST: usize = 1_000_000;

#[tokio::main]
async fn main() {
    // Server: one dispatcher, one push hub for the principal.
    let alice = Id::from(ACCOUNT);
    let backend = MemoryBackend::new().with_account(&alice);
    let mut dispatcher = Dispatcher::<Id>::new();
    jmap_tasks_server::register_tasks_handlers(&mut dispatcher, Arc::new(backend));
    let dispatcher = Arc::new(dispatcher);
    let hub = PushHub::new(64);
    let chat_backend =
        Arc::new(jmap_chat_server::memory::MemoryBackend::<Id>::default().with_account(&alice));
    let chat_hub = ChatEventHub::new(64);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (server_dispatcher, server_hub) = (Arc::clone(&dispatcher), hub.clone());
    let chat = ChatStreamHandler::new(chat_backend, Id::from(ACCOUNT), [alice.clone()], &chat_hub);
    // The same maxSizeRequest caps tungstenite (so an oversized message
    // is refused while it arrives, with Close 1009) and the driver.
    let config = ConnectionConfig::default().max_size_request(Some(MAX_SIZE_REQUEST));
    tokio::spawn(async move {
        // The first connection carries the Tasks and Chat checks.
        let (tcp, _) = listener.accept().await.expect("accept");
        // A real deployment authenticates here; this toy trusts loopback.
        let ws = jmap_websocket::tungstenite::accept_with_config(tcp, &config)
            .await
            .expect("handshake");
        let handler = PublishingHandler::new(
            DispatcherHandler::new(
                Arc::clone(&server_dispatcher),
                Id::from(ACCOUNT),
                || State::from("s0"),
            ),
            server_hub.clone(),
        );
        let connection = Connection::new(Arc::new(handler))
            .with_config(config.clone())
            .with_push(server_hub)
            .with_extension(chat);
        tokio::spawn(async move {
            jmap_websocket::tungstenite::serve(connection, ws).await.expect("serve");
        });

        // The second one gets a message over maxSizeRequest.
        let (tcp, _) = listener.accept().await.expect("accept");
        let ws = jmap_websocket::tungstenite::accept_with_config(tcp, &config)
            .await
            .expect("handshake");
        let handler =
            DispatcherHandler::new(server_dispatcher, Id::from(ACCOUNT), || State::from("s0"));
        let connection = Connection::new(Arc::new(handler)).with_config(config);
        jmap_websocket::tungstenite::serve(connection, ws).await.expect("serve");
    });

    // Client.
    let connect = move || async move {
        let mut request = format!("ws://{addr}/jmap/ws").into_client_request().expect("request");
        request
            .headers_mut()
            .insert("Sec-WebSocket-Protocol", HeaderValue::from_static("jmap"));
        let (ws, response) = tokio_tungstenite::connect_async(request).await.expect("connect");
        assert_eq!(response.headers()["Sec-WebSocket-Protocol"], "jmap");
        ws
    };
    let mut ws = connect().await;

    let send = |v: Value| Message::Text(v.to_string().into());
    ws.send(send(json!({ "@type": "WebSocketPushEnable", "dataTypes": ["TaskList"] })))
        .await
        .expect("push enable");
    ws.send(send(json!({
        "@type": "Request",
        "id": "r1",
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:tasks"],
        "methodCalls": [["TaskList/set",
            { "accountId": ACCOUNT, "create": { "l": { "name": "Chores" } } }, "0"]]
    })))
    .await
    .expect("request");

    let (mut new_state, mut pushed) = (None, None);
    while new_state.is_none() || pushed.is_none() {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timed out")
            .expect("closed")
            .expect("ws error");
        let Message::Text(text) = msg else { continue };
        let v: Value = serde_json::from_str(&text).expect("json");
        match v["@type"].as_str() {
            Some("Response") => {
                assert_eq!(v["requestId"], "r1", "{v}");
                let args = &v["methodResponses"][0][1];
                assert!(args["created"]["l"]["id"].is_string(), "{v}");
                new_state = Some(args["newState"].clone());
            }
            Some("StateChange") => pushed = Some(v["changed"][ACCOUNT]["TaskList"].clone()),
            other => panic!("unexpected {other:?}: {v}"),
        }
    }
    assert_eq!(new_state, pushed, "push carries the /set newState");
    println!("toy-websocket-server: response and StateChange agree ({})", pushed.unwrap());

    // Ephemeral Chat presence: enable, then a Request whose Response
    // proves the enable was handled, then publish.
    ws.send(send(json!({ "@type": "ChatStreamEnable", "dataTypes": ["presence"] })))
        .await
        .expect("chat stream enable");
    ws.send(send(json!({
        "@type": "Request",
        "id": "r2",
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:tasks"],
        "methodCalls": [["TaskList/get", { "accountId": ACCOUNT, "ids": [] }, "0"]]
    })))
    .await
    .expect("request");
    let mut published = false;
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timed out")
            .expect("closed")
            .expect("ws error");
        let Message::Text(text) = msg else { continue };
        let v: Value = serde_json::from_str(&text).expect("json");
        match v["@type"].as_str() {
            Some("Response") if !published => {
                assert_eq!(v["requestId"], "r2", "{v}");
                let event: ChatPresenceEvent = serde_json::from_value(json!({
                    "accountId": ACCOUNT, "contactId": "c1", "presence": "invisible"
                }))
                .expect("presence event");
                chat_hub.publish_presence(event);
                published = true;
            }
            Some("ChatPresenceEvent") => {
                // "invisible" is reported as "offline".
                assert_eq!(v["presence"], "offline", "{v}");
                break;
            }
            other => panic!("unexpected {other:?}: {v}"),
        }
    }
    println!("toy-websocket-server: ChatPresenceEvent delivered");

    // A message one octet over maxSizeRequest: Close 1009 (RFC 6455
    // §7.4.1), not a RequestError, because tungstenite refuses it.
    let mut ws = connect().await;
    ws.send(Message::Text("x".repeat(MAX_SIZE_REQUEST + 1).into()))
        .await
        .expect("oversized message");
    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out")
        .expect("closed")
        .expect("ws error");
    let Message::Close(Some(frame)) = msg else { panic!("expected Close, got {msg:?}") };
    assert_eq!(u16::from(frame.code), CLOSE_MESSAGE_TOO_BIG, "{frame:?}");
    println!("toy-websocket-server: oversized message closed with {}", frame.code);
}
