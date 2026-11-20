//! Toy consumer: a Slack bridge.
//!
//! Builds the jmap-chat-slack types from public API (`SlackToken`,
//! `SlackClient`, `BridgeConfig`, `SlackBridge`), connects against a
//! loopback stand-in for the Slack Web API (no network), verifies and
//! parses a signed Events API request, feeds its events to `apply_event`,
//! and reads the result back through JMAP `Message/query`.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jmap_chat_slack::client::SlackClient;
use jmap_chat_slack::events::{self, HttpEvent, SigningSecret};
use jmap_chat_slack::{BridgeConfig, EventOutcome, SlackBridge, SlackToken};
use jmap_server::Dispatcher;
use jmap_types::{Id, Invocation, JmapRequest, State};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

const OWNER: &str = "W12345678";
const TEAM: &str = "T12345678";

/// The Slack Web API answer for `method`.
fn slack_answer(method: &str) -> Value {
    let body = match method {
        "auth.test" => json!({ "url": "https://toy.slack.com/", "team": "Toy",
                               "user": "grace", "team_id": TEAM, "user_id": OWNER }),
        "users.info" => json!({ "user": { "id": "U1", "team_id": TEAM, "name": "alice",
                                          "profile": { "display_name": "Alice" } } }),
        "conversations.info" => json!({ "channel": { "id": "C1", "name": "general",
                                                     "is_channel": true, "is_member": true,
                                                     "created": 1_700_000_000 } }),
        "conversations.members" => json!({ "members": [OWNER, "U1"] }),
        "conversations.history" | "conversations.replies" => json!({ "messages": [] }),
        _ => return json!({ "ok": false, "error": "unknown_method" }),
    };
    let mut body = body;
    body["ok"] = json!(true);
    body
}

/// A minimal HTTP/1.1 server answering `/api/<method>`.
async fn serve(listener: TcpListener) {
    loop {
        let Ok((mut sock, _)) = listener.accept().await else {
            return;
        };
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let (head_end, len) = loop {
                let n = sock.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                buf.extend_from_slice(&chunk[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .map_or(0, |v| v.trim().parse::<usize>().unwrap());
                    break (i + 4, len);
                }
            };
            while buf.len() < head_end + len {
                let n = sock.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
            }
            let line = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let path = line.split_whitespace().nth(1).unwrap_or("/");
            let method = path
                .trim_start_matches("/api/")
                .split('?')
                .next()
                .unwrap_or("");
            let body = serde_json::to_vec(&slack_answer(method)).unwrap();
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            sock.write_all(head.as_bytes()).await.unwrap();
            sock.write_all(&body).await.unwrap();
        });
    }
}

/// `v0=<hex HMAC-SHA256>` of `v0:<ts>:<body>`, as Slack signs a request.
fn sign(secret: &str, ts: &str, body: &[u8]) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    let mut base = format!("v0:{ts}:").into_bytes();
    base.extend_from_slice(body);
    let tag = ring::hmac::sign(&key, &base);
    let hex: String = tag.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    format!("v0={hex}")
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    // `with_base_url` takes a `url::Url`; jmap-chat-slack re-exports `url`
    // (finding d1).
    let base = jmap_chat_slack::url::Url::parse(&format!(
        "http://{}/api/",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    tokio::spawn(serve(listener));

    let token = SlackToken::new("xoxb-toy-not-a-real-token").expect("token");
    let client = SlackClient::new(token)
        .expect("client")
        .with_base_url(&base);
    let account = Id::from("a1");
    let bridge = SlackBridge::connect(
        client,
        BridgeConfig::new(account.clone()).with_space_name("Toy workspace"),
    )
    .await
    .expect("connect");
    assert_eq!(bridge.team_id(), TEAM);
    assert_eq!(bridge.owner_user(), OWNER);

    // A signed Events API request carrying a channel_created event.
    let secret = "toy-signing-secret";
    let now = SystemTime::now();
    let ts = now
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let deliver = |event: Value| {
        let body = json!({ "type": "event_callback", "team_id": TEAM, "event": event,
                           "event_id": "Ev1", "event_time": 1_763_600_000 })
        .to_string()
        .into_bytes();
        let sig = sign(secret, &ts, &body);
        events::verify_signature(&SigningSecret::new(secret), &ts, &body, &sig, now)
            .expect("signature");
        match events::parse_http_event(&body).expect("parse") {
            HttpEvent::Callback { event, .. } => event,
            other => panic!("not a callback: {other:?}"),
        }
    };

    let created = deliver(json!({ "type": "channel_created", "channel": {
        "id": "C1", "name": "general", "created": 1_700_000_000, "creator": OWNER } }));
    let outcome = bridge.apply_event(&created).await.expect("channel_created");
    println!("channel_created: {outcome:?}");

    let message = deliver(json!({ "type": "message", "channel": "C1", "user": "U1",
                                  "text": "Hello *world*", "ts": "1763600000.000100" }));
    let outcome = bridge.apply_event(&message).await.expect("message");
    assert_eq!(outcome, EventOutcome::Applied, "message");

    // Read it back through JMAP.
    let mut d: Dispatcher<()> = Dispatcher::new();
    jmap_chat_server::register_chat_handlers(&mut d, Arc::new(bridge.clone()));
    let using = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"]
        .map(str::to_owned)
        .to_vec();
    let r = d
        .dispatch(
            JmapRequest::new(
                using,
                vec![
                    Invocation::new(
                        "Chat/get",
                        json!({ "accountId": account, "ids": null, "properties": ["name", "kind"] }),
                        "0",
                    ),
                    Invocation::new(
                        "Message/get",
                        json!({ "accountId": account, "ids": null, "properties": ["body", "chatId"] }),
                        "1",
                    ),
                ],
                None,
            ),
            (),
            State::from("s0"),
        )
        .await;
    for inv in &r.method_responses {
        println!("{} {}", inv.name, inv.arguments);
    }
    let chats = &r.method_responses[0].arguments["list"];
    assert!(
        chats
            .as_array()
            .is_some_and(|l| l.iter().any(|c| c["name"] == "general")),
        "{chats}"
    );
    let msgs = &r.method_responses[1].arguments["list"];
    assert!(
        msgs.as_array()
            .is_some_and(|l| l.iter().any(|m| m["body"].to_string().contains("world"))),
        "{msgs}"
    );

    println!("toy-slack-bridge: ok");
}
