//! Integration smoke test for WebSocket `pushState` replay
//! (bd:JMAP-cf7p.12).
//!
//! Exercises the reconnect path:
//!
//! 1. Spawn the jig.
//! 2. Open `ws://<addr>/ws`, send `WebSocketPushEnable`, observe a
//!    `StateChange` frame on a `Space/set` mutation. Record the
//!    `pushState` field from that frame.
//! 3. Close the WS connection.
//! 4. POST one or more additional `Space/set` mutations through
//!    `POST /jmap` (no WS open during these — the producer-driven
//!    event log records them anyway).
//! 5. Reconnect to `ws://<addr>/ws`, send `WebSocketPushEnable`
//!    carrying the recorded `pushState`.
//! 6. Assert: the testjig immediately replays the post-pushState
//!    events so the client sees the missed mutations without
//!    issuing `Foo/changes`.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use jmap_testjig::{TestjigConfig, spawn_in_process};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;

const WS_WAIT_BUDGET: Duration = Duration::from_secs(5);

#[tokio::test]
async fn push_state_replays_missed_changes() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn testjig");
    let token = jig.token.clone();
    let addr = jig.addr;
    let http = reqwest::Client::new();
    let api_url = format!("http://{addr}/jmap");

    // -- Phase 1: open WS, enable push, mutate, capture pushState.
    let phase1_push_state = {
        let mut socket = open_ws_socket(addr, &token).await;
        send_push_enable(&mut socket, None).await;

        // Let the push task subscribe to the watch channel before
        // we mutate; otherwise the mutation could fire before the
        // task is awaiting the wake (the producer-side recording
        // happens regardless, but the live emit needs an active task).
        tokio::time::sleep(Duration::from_millis(50)).await;

        space_create(&http, &api_url, &token, "phase-1").await;

        let frame = read_state_change_for(&mut socket, "Space")
            .await
            .expect("phase-1 StateChange frame must arrive");
        let v: Value = serde_json::from_str(&frame).expect("StateChange JSON");
        let push_state = v["pushState"]
            .as_str()
            .expect("pushState must be a string per bd:JMAP-cf7p.12")
            .to_owned();
        assert!(!push_state.is_empty(), "pushState must be non-empty");
        // Verify it parses as u64 (the testjig's encoding).
        let _: u64 = push_state.parse().expect("pushState parses as u64");

        let _ = socket.close(None).await;
        push_state
    };

    // Pause so the previous push task has time to abort.
    tokio::time::sleep(Duration::from_millis(20)).await;

    // -- Phase 2: mutate while no WS is open. Producer-driven log
    //    records both regardless.
    space_create(&http, &api_url, &token, "phase-2a").await;
    space_create(&http, &api_url, &token, "phase-2b").await;

    // -- Phase 3: reconnect, send WebSocketPushEnable with pushState,
    //    expect immediate replay.
    let mut socket = open_ws_socket(addr, &token).await;
    send_push_enable(&mut socket, Some(&phase1_push_state)).await;

    let replay_frame = read_state_change_for(&mut socket, "Space")
        .await
        .expect("replay StateChange frame must arrive immediately on reconnect");
    let payload: Value = serde_json::from_str(&replay_frame).expect("replay JSON");
    assert_eq!(payload["@type"], "StateChange");
    let replay_push_state = payload["pushState"]
        .as_str()
        .expect("replay frame must carry pushState");
    let replay_id: u64 = replay_push_state.parse().expect("replay pushState parses");
    let phase1_id: u64 = phase1_push_state.parse().expect("phase1 pushState parses");
    assert!(
        replay_id > phase1_id,
        "replay pushState ({replay_id}) must be > phase1 pushState ({phase1_id})"
    );
    let space_state = payload["changed"]["testjig-account"]["Space"]
        .as_str()
        .expect("Space state in replay payload");
    assert!(!space_state.is_empty());

    let _ = socket.close(None).await;
    jig.shutdown().await;
}

/// The jig's per-type replay ring holds 1024 entries
/// (`replay::RING_CAPACITY`, crate-private).
const RING_CAPACITY: usize = 1024;

/// How long a reconnect is watched for a replay frame that must not
/// come. Replay is sent before the push task's first wait, so it would
/// arrive well inside this.
const NO_REPLAY_WINDOW: Duration = Duration::from_millis(300);

/// jmap-chat-wss-guide §State-change push: "When a client reconnects
/// with a `pushState` the server no longer has history for, the server
/// MUST resume delivering from the current state without replaying
/// history." The Space ring rolls over past the token while the
/// startup-recorded types keep lower ids retained, so the token looks
/// in range by the smallest retained id; nothing is replayed, and the
/// next change is delivered.
#[tokio::test]
async fn evicted_push_state_resumes_from_current() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn testjig");
    let token = jig.token.clone();
    let addr = jig.addr;
    let http = reqwest::Client::new();
    let api_url = format!("http://{addr}/jmap");

    let stale = {
        let mut socket = open_ws_socket(addr, &token).await;
        send_push_enable(&mut socket, None).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        space_create(&http, &api_url, &token, "before").await;
        let frame = read_state_change_for(&mut socket, "Space")
            .await
            .expect("StateChange frame must arrive");
        let v: Value = serde_json::from_str(&frame).expect("StateChange JSON");
        let push_state = v["pushState"].as_str().expect("pushState").to_owned();
        let _ = socket.close(None).await;
        push_state
    };
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Roll the Space ring past `stale`.
    for i in 0..=RING_CAPACITY {
        space_create(&http, &api_url, &token, &format!("churn-{i}")).await;
    }

    let mut socket = open_ws_socket(addr, &token).await;
    send_push_enable(&mut socket, Some(&stale)).await;
    assert_no_state_change(&mut socket).await;

    space_create(&http, &api_url, &token, "after").await;
    let frame = read_state_change_for(&mut socket, "Space")
        .await
        .expect("a change after the reconnect must be delivered");
    let v: Value = serde_json::from_str(&frame).expect("StateChange JSON");
    let after: u64 = v["pushState"].as_str().unwrap().parse().unwrap();
    let stale: u64 = stale.parse().unwrap();
    assert!(after > stale + RING_CAPACITY as u64);

    let _ = socket.close(None).await;
    jig.shutdown().await;
}

/// A `pushState` newer than any event the jig has assigned (a token
/// from before a restart) is one the server has no history for, so it
/// resumes from current (jmap-chat-wss-guide §State-change push):
/// nothing is replayed, and every change after the connect is
/// delivered rather than skipped up to the token's value.
#[tokio::test]
async fn future_push_state_resumes_from_current() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn testjig");
    let token = jig.token.clone();
    let addr = jig.addr;
    let http = reqwest::Client::new();
    let api_url = format!("http://{addr}/jmap");

    // History the reconnect must not replay.
    space_create(&http, &api_url, &token, "before").await;

    let future = "1000000";
    let mut socket = open_ws_socket(addr, &token).await;
    send_push_enable(&mut socket, Some(future)).await;
    assert_no_state_change(&mut socket).await;

    for name in ["after-1", "after-2"] {
        space_create(&http, &api_url, &token, name).await;
        let frame = read_state_change_for(&mut socket, "Space")
            .await
            .expect("each change after the connect must be delivered");
        let v: Value = serde_json::from_str(&frame).expect("StateChange JSON");
        let id: u64 = v["pushState"].as_str().unwrap().parse().unwrap();
        assert!(id < 1_000_000, "pushState {id} is the jig's own id");
    }

    let _ = socket.close(None).await;
    jig.shutdown().await;
}

/// Assert that no `StateChange` frame arrives within
/// [`NO_REPLAY_WINDOW`].
async fn assert_no_state_change<S>(socket: &mut S)
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + NO_REPLAY_WINDOW;
    while let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) {
        let Ok(next) = tokio::time::timeout(remaining, socket.next()).await else {
            return;
        };
        if let Some(Ok(Message::Text(t))) = next {
            let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
            assert_ne!(
                v.get("@type").and_then(Value::as_str),
                Some("StateChange"),
                "no history may be replayed: {t}"
            );
        } else {
            panic!("socket ended while waiting: {next:?}");
        }
    }
}

/// Open a WebSocket connection to the testjig, negotiating the `jmap`
/// subprotocol and authenticating via the `?token=` query fallback.
async fn open_ws_socket(
    addr: std::net::SocketAddr,
    token: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://{addr}/ws?token={token}");
    let mut request = url.into_client_request().expect("build WS client request");
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "jmap".parse().expect("static header"),
    );
    let (socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("WS connect");
    socket
}

/// Send a `WebSocketPushEnable` envelope, optionally carrying a
/// `pushState` token to request replay-from-position per RFC 8887
/// §4.3.5.2.
async fn send_push_enable<S>(socket: &mut S, push_state: Option<&str>)
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let mut body = json!({
        "@type": "WebSocketPushEnable",
        "dataTypes": null,
    });
    if let Some(ps) = push_state {
        body["pushState"] = Value::String(ps.to_owned());
    }
    socket
        .send(Message::Text(body.to_string().into()))
        .await
        .expect("send PushEnable");
}

/// POST a `Space/set` create with a unique name.
async fn space_create(client: &reqwest::Client, url: &str, token: &str, name: &str) {
    let body = json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"],
        "methodCalls": [
            ["Space/set", {
                "accountId": "testjig-account",
                "create": {"new-1": {"name": name}}
            }, "c1"]
        ]
    });
    let resp = client
        .post(url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .expect("Space/set request");
    assert_eq!(resp.status().as_u16(), 200);
    let _: Value = resp.json().await.expect("Space/set body");
}

/// Read frames from `socket` until a `StateChange` frame referencing
/// the named type arrives, or [`WS_WAIT_BUDGET`] elapses.
async fn read_state_change_for<S>(socket: &mut S, type_name: &str) -> Option<String>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + WS_WAIT_BUDGET;
    loop {
        let remaining = deadline.checked_duration_since(tokio::time::Instant::now())?;
        let next = tokio::time::timeout(remaining, socket.next()).await.ok()?;
        let msg = match next {
            Some(Ok(m)) => m,
            Some(Err(_)) | None => return None,
        };
        let body = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => return None,
            _ => continue,
        };
        let v: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("@type").and_then(Value::as_str) == Some("StateChange") {
            let changed = v.get("changed").and_then(Value::as_object);
            let has_type = changed
                .and_then(|m| m.values().next())
                .and_then(Value::as_object)
                .map(|inner| inner.contains_key(type_name))
                .unwrap_or(false);
            if has_type {
                return Some(body);
            }
        }
    }
}
