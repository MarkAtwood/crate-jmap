//! `ChatStreamHandler` end to end over a real WebSocket: a
//! tokio-tungstenite client against `jmap_websocket::tungstenite::serve`
//! on a loopback socket, the handler registered next to a `Core/echo`
//! dispatcher, events published to a `ChatEventHub`.
//!
//! Oracle: draft-atwood-jmap-chat-wss-00 (jmap-chat-spec 813ad11)
//! §Enabling Ephemeral Push, §Disabling Ephemeral Push, §ChatTypingEvent
//! and §ChatPresenceEvent; each test quotes the sentence it checks.
//! Records and frames are hand-written JSON.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use jmap_chat_server::memory::MemoryBackend;
use jmap_chat_server::websocket::{ChatEventHub, ChatStreamHandler, ChatStreamLimits};
use jmap_chat_types::{ChatPresenceEvent, ChatTypingEvent};
use jmap_server::{CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler};
use jmap_types::{Id, State};
use jmap_websocket::server::{Connection, DispatcherHandler};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const OWNER: &str = "acct-owner";
const OTHER: &str = "acct-other";
const BOB: &str = "contact-bob";
const CAROL: &str = "contact-carol";
const CHAT: &str = "chat-1";
const CHAT2: &str = "chat-2";

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

/// A group Chat with the owner, Bob and Carol as members.
fn group(id: &str) -> Value {
    let members: Vec<Value> = [OWNER, BOB, CAROL]
        .iter()
        .map(|m| json!({"id": m, "role": "member", "joinedAt": "2026-10-01T00:00:00Z"}))
        .collect();
    json!({
        "id": id,
        "kind": "group",
        "name": "Team",
        "members": members,
        "createdAt": "2026-10-01T00:00:00Z",
        "unreadCount": 0,
    })
}

fn contact(id: &str, blocked: bool) -> Value {
    json!({
        "id": id,
        "identity": format!("{id}@example.net"),
        "login": id,
        "displayName": id,
        "blocked": blocked,
        "firstSeenAt": "2026-10-01T00:00:00Z",
        "lastSeenAt": "2026-10-19T00:00:00Z",
    })
}

fn backend() -> Arc<MemoryBackend<()>> {
    let backend = MemoryBackend::<()>::default();
    backend.register_account(&Id::from(OWNER));
    backend.insert_object_for_test("Chat", OWNER, CHAT, group(CHAT));
    backend.insert_object_for_test("Chat", OWNER, CHAT2, group(CHAT2));
    backend.insert_object_for_test("ChatContact", OWNER, BOB, contact(BOB, false));
    backend.insert_object_for_test("ChatContact", OWNER, CAROL, contact(CAROL, true));
    Arc::new(backend)
}

fn typing(account: &str, chat: &str, sender: &str, on: bool) -> ChatTypingEvent {
    serde_json::from_value(json!({
        "accountId": account, "chatId": chat, "senderId": sender, "typing": on
    }))
    .unwrap()
}

fn presence(contact: &str, presence: &str) -> ChatPresenceEvent {
    serde_json::from_value(json!({
        "accountId": OWNER, "contactId": contact, "presence": presence
    }))
    .unwrap()
}

/// Starts a server whose connections all belong to the owner.
async fn server(limits: ChatStreamLimits) -> (std::net::SocketAddr, ChatEventHub) {
    let mut dispatcher = Dispatcher::new();
    dispatcher.register("Core/echo", Arc::new(Echo));
    let dispatcher = Arc::new(dispatcher);
    let hub = ChatEventHub::new(64);
    let backend = backend();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_hub = hub.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let handler = Arc::new(DispatcherHandler::new(Arc::clone(&dispatcher), (), || {
                State::from("s0")
            }));
            let chat =
                ChatStreamHandler::new(Arc::clone(&backend), (), [Id::from(OWNER)], &server_hub)
                    .with_limits(limits);
            tokio::spawn(async move {
                let Ok(ws) = jmap_websocket::tungstenite::accept(tcp).await else {
                    return;
                };
                let conn = Connection::new(handler).with_extension(chat);
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

/// §Enabling Ephemeral Push: "A server MUST NOT send an additional
/// server-to-client message type unless the client has enabled it"
/// (§Update to RFC 8887); after ChatStreamEnable typing events flow;
/// "The server MUST stop delivering ephemeral events after processing
/// this message" (§Disabling Ephemeral Push).
#[tokio::test]
async fn enable_event_disable_none() {
    let (addr, hub) = server(ChatStreamLimits::default().typing_window(None)).await;
    let mut ws = connect(addr).await;

    hub.publish_typing(typing(OWNER, CHAT, BOB, true));
    nothing(&mut ws, "s1").await;

    send(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["typing"]}),
    )
    .await;
    sync(&mut ws, "s2").await;
    hub.publish_typing(typing(OWNER, CHAT, BOB, true));
    assert_eq!(
        recv(&mut ws).await,
        json!({
            "@type": "ChatTypingEvent",
            "accountId": OWNER, "chatId": CHAT, "senderId": BOB, "typing": true
        })
    );

    send(&mut ws, json!({"@type": "ChatStreamDisable"})).await;
    sync(&mut ws, "s3").await;
    hub.publish_typing(typing(OWNER, CHAT, BOB, false));
    nothing(&mut ws, "s4").await;
}

/// Delivery-time filters: the `chatIds` scope; another account's event
/// ("accountId: The id of the account that contains the Chat"); and "a
/// `ChatTypingEvent` whose `senderId` corresponds to a ChatContact that
/// is `blocked` ... is not delivered" (§ChatTypingEvent).
#[tokio::test]
async fn scope_account_and_block_filter_typing() {
    let (addr, hub) = server(ChatStreamLimits::default().typing_window(None)).await;
    let mut ws = connect(addr).await;
    send(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["typing"], "chatIds": [CHAT2]}),
    )
    .await;
    sync(&mut ws, "s1").await;

    hub.publish_typing(typing(OWNER, CHAT, BOB, true)); // out of scope
    hub.publish_typing(typing(OTHER, CHAT2, BOB, true)); // not this connection's account
    hub.publish_typing(typing(OWNER, CHAT2, CAROL, true)); // blocked
    hub.publish_typing(typing(OWNER, CHAT2, BOB, true)); // delivered
    let e = recv(&mut ws).await;
    assert_eq!(
        (e["chatId"].clone(), e["senderId"].clone()),
        (json!(CHAT2), json!(BOB))
    );
    nothing(&mut ws, "s2").await;
}

/// §ChatTypingEvent: "no more than one `ChatTypingEvent` per sender per
/// chat per 3 seconds SHOULD be delivered; events above this rate MAY be
/// silently dropped." A `typing: false` is exempt: always delivered and
/// not counted (decision 66). Run with a shorter window.
#[tokio::test]
async fn typing_rate_limit_drops_per_sender_per_chat() {
    let window = Duration::from_millis(400);
    let (addr, hub) = server(ChatStreamLimits::default().typing_window(Some(window))).await;
    let mut ws = connect(addr).await;
    send(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["typing"]}),
    )
    .await;
    sync(&mut ws, "s1").await;

    hub.publish_typing(typing(OWNER, CHAT, BOB, true));
    hub.publish_typing(typing(OWNER, CHAT, BOB, true)); // over the limit: dropped
    hub.publish_typing(typing(OWNER, CHAT, BOB, false)); // a stop: always delivered
    hub.publish_typing(typing(OWNER, CHAT2, BOB, true)); // another chat: its own window
    let first = recv(&mut ws).await;
    assert_eq!(
        (first["chatId"].clone(), first["typing"].clone()),
        (json!(CHAT), json!(true))
    );
    let stop = recv(&mut ws).await;
    assert_eq!(
        (stop["chatId"].clone(), stop["typing"].clone()),
        (json!(CHAT), json!(false)),
        "the second start must be dropped and the stop delivered"
    );
    assert_eq!(recv(&mut ws).await["chatId"], CHAT2);

    // The stop did not count: a start right after it is still over the
    // limit, and one after the window is delivered.
    hub.publish_typing(typing(OWNER, CHAT, BOB, true));
    nothing(&mut ws, "s2").await;
    tokio::time::sleep(window).await;
    hub.publish_typing(typing(OWNER, CHAT, BOB, true));
    let again = recv(&mut ws).await;
    assert_eq!(
        (again["chatId"].clone(), again["typing"].clone()),
        (json!(CHAT), json!(true))
    );
}

/// §ChatPresenceEvent: "no more than one `ChatPresenceEvent` per contact
/// per 30 seconds SHOULD be delivered; events above this rate MAY be
/// silently dropped, but the most recent presence state SHOULD be
/// delivered when the rate window reopens"; "invisible" is reported as
/// "offline". Run with a shorter window.
#[tokio::test]
async fn presence_rate_limit_delivers_the_latest_when_the_window_reopens() {
    let window = Duration::from_millis(400);
    let (addr, hub) = server(ChatStreamLimits::default().presence_window(Some(window))).await;
    let mut ws = connect(addr).await;
    send(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["presence"], "contactIds": null}),
    )
    .await;
    sync(&mut ws, "s1").await;

    hub.publish_presence(presence(BOB, "online"));
    hub.publish_presence(presence(BOB, "away"));
    hub.publish_presence(presence(BOB, "invisible"));
    hub.publish_presence(presence(CAROL, "online")); // blocked: never delivered
    assert_eq!(
        recv(&mut ws).await,
        json!({"@type": "ChatPresenceEvent", "accountId": OWNER, "contactId": BOB, "presence": "online"})
    );
    sync(&mut ws, "s2").await;
    let held = recv(&mut ws).await;
    assert_eq!(
        held,
        json!({"@type": "ChatPresenceEvent", "accountId": OWNER, "contactId": BOB, "presence": "offline"})
    );
    nothing(&mut ws, "s3").await;
}

/// §Enabling Ephemeral Push: "If `dataTypes` is empty, or contains only
/// unrecognized values ... the server MUST respond with a `RequestError`
/// ... `invalidArguments` and MUST NOT update the current ephemeral
/// subscription state"; "A `RequestError` caused by this message carries
/// this value as its `requestId`". `"vtc"` is unrecognized without the
/// VTC WebSocket capability.
#[tokio::test]
async fn invalid_enable_is_a_request_error_and_keeps_the_subscription() {
    let (addr, hub) = server(ChatStreamLimits::default().typing_window(None)).await;
    let mut ws = connect(addr).await;
    send(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "dataTypes": ["typing"]}),
    )
    .await;
    send(
        &mut ws,
        json!({"@type": "ChatStreamEnable", "id": "e1", "dataTypes": ["vtc", "bogus"]}),
    )
    .await;
    let e = recv(&mut ws).await;
    assert_eq!(e["@type"], "RequestError");
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:invalidArguments");
    assert_eq!(e["requestId"], "e1");

    hub.publish_typing(typing(OWNER, CHAT, BOB, true));
    assert_eq!(recv(&mut ws).await["@type"], "ChatTypingEvent");
}
