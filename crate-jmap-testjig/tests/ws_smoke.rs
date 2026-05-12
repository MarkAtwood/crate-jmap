//! Integration smoke test for the WebSocket endpoint (bd:JMAP-cf7p.5).
//!
//! Drives a full WebSocket conversation against an in-process testjig:
//!
//! 1. Spawn the jig on an OS-assigned port.
//! 2. Open `ws://<addr>/ws` with the `jmap` subprotocol and bearer
//!    auth via `?token=...` (the WS handshake cannot easily set
//!    arbitrary headers via tokio-tungstenite without re-deriving a
//!    request, and the testjig's auth layer accepts the query
//!    fallback).
//! 3. Send a `Request` frame (Core/echo), receive a matching
//!    `Response` frame on the same socket.
//! 4. Send a `WebSocketPushEnable` frame, POST `Space/set` over
//!    HTTP, read the inbound `StateChange` frame and verify its
//!    shape.
//! 5. Send a `WebSocketPushDisable`, perform another mutation, and
//!    verify no further `StateChange` frame arrives in a tight
//!    window.
//!
//! These five steps map onto the bead's acceptance criteria:
//! > - A WebSocket client can open ws://localhost:<port>/ws with 'jmap' subprotocol
//! > - Send a Request frame, receive a Response frame on the same socket
//! > - After WebSocketPushEnable, server pushes StateChange frames matching SSE behavior
//! > - WebSocketPushDisable stops the push

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use jmap_testjig::{spawn_in_process, TestjigConfig};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;

/// Cap on how long the test waits for any single inbound WS message
/// before declaring the slice broken. The testjig's push polling
/// interval is 200 ms; 5 seconds is comfortably long enough to absorb
/// CI scheduling jitter without producing false negatives.
const WS_WAIT_BUDGET: Duration = Duration::from_secs(5);

#[tokio::test]
async fn ws_request_response_and_push_lifecycle() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn testjig");

    let token = jig.token.clone();
    let addr = jig.addr;

    // Connect with the `jmap` subprotocol. Bearer auth rides on the
    // `?token=` query fallback so the WS upgrade request does not
    // require a custom Authorization header (tokio-tungstenite would
    // accept one, but the query fallback keeps the test simple).
    let url = format!("ws://{addr}/ws?token={token}");
    let mut request = url.into_client_request().expect("build WS client request");
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "jmap".parse().expect("static header"),
    );
    let (mut socket, response) = tokio_tungstenite::connect_async(request)
        .await
        .expect("WS connect");

    // RFC 8887 §4.2: the server MUST echo the `jmap` subprotocol in
    // its handshake response. axum's WebSocketUpgrade::protocols
    // sets the response header only when the client offered the
    // matching subprotocol; absence is a slice bug.
    let chosen = response
        .headers()
        .get("Sec-WebSocket-Protocol")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    assert_eq!(
        chosen.as_deref(),
        Some("jmap"),
        "server must echo 'jmap' subprotocol per RFC 8887 §4.2"
    );

    // -- Step 1: Request frame round-trip.
    //
    // Core/echo (RFC 8620 §4) is the cheapest method to exercise
    // dispatch — it requires no backend state and echoes its
    // arguments verbatim.
    let req = json!({
        "@type": "Request",
        "id": "wsrq1",
        "using": ["urn:ietf:params:jmap:core"],
        "methodCalls": [
            ["Core/echo", {"hello": "ws", "n": 42}, "c1"]
        ]
    });
    socket
        .send(Message::Text(req.to_string().into()))
        .await
        .expect("send Request frame");

    let response_frame = recv_text(&mut socket)
        .await
        .expect("Response frame must arrive");
    let parsed: Value = serde_json::from_str(&response_frame).expect("Response frame must be JSON");
    assert_eq!(
        parsed["@type"], "Response",
        "RFC 8887 §4.3.3: WS responses carry @type='Response'"
    );
    assert_eq!(
        parsed["requestId"], "wsrq1",
        "RFC 8887 §4.3.3: requestId must echo the request's id"
    );
    let method_response = &parsed["methodResponses"][0];
    assert_eq!(method_response[0], "Core/echo");
    assert_eq!(method_response[1], json!({"hello": "ws", "n": 42}));
    assert_eq!(method_response[2], "c1");

    // -- Step 2: PushEnable + mutation triggers StateChange.
    let enable = json!({
        "@type": "WebSocketPushEnable",
        "dataTypes": null,        // "all types" per §4.3.5.2
        "pushState": null
    });
    socket
        .send(Message::Text(enable.to_string().into()))
        .await
        .expect("send PushEnable");

    // Give the push poll task a tick to capture its baseline snapshot
    // before we mutate state. 50 ms is well below the 200 ms poll
    // interval but long enough that the polling task is running.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let http = reqwest::Client::new();
    let api_url = format!("http://{addr}/jmap");
    let post_resp = http
        .post(&api_url)
        .bearer_auth(&token)
        .json(&json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"],
            "methodCalls": [
                ["Space/set", {
                    "accountId": "testjig-account",
                    "create": {"new-1": {"name": "ws push test"}}
                }, "c1"]
            ]
        }))
        .send()
        .await
        .expect("Space/set HTTP request");
    assert_eq!(
        post_resp.status().as_u16(),
        200,
        "Space/set must dispatch successfully"
    );

    let state_change = read_state_change_for(&mut socket, "Space")
        .await
        .expect("StateChange frame for Space must arrive after PushEnable");
    let payload: Value = serde_json::from_str(&state_change).expect("StateChange data is JSON");
    assert_eq!(payload["@type"], "StateChange");
    let space_state = &payload["changed"]["testjig-account"]["Space"];
    assert!(
        space_state.is_string(),
        "StateChange must report Space state token, got {payload}"
    );
    assert!(
        !space_state.as_str().unwrap().is_empty(),
        "Space state token must be non-empty"
    );

    // -- Step 3: PushDisable suppresses subsequent state changes.
    let disable = json!({"@type": "WebSocketPushDisable"});
    socket
        .send(Message::Text(disable.to_string().into()))
        .await
        .expect("send PushDisable");

    // Trigger another mutation. With push disabled, the server MUST
    // NOT push another StateChange.
    let post2 = http
        .post(&api_url)
        .bearer_auth(&token)
        .json(&json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"],
            "methodCalls": [
                ["Space/set", {
                    "accountId": "testjig-account",
                    "create": {"new-2": {"name": "post-disable mutation"}}
                }, "c1"]
            ]
        }))
        .send()
        .await
        .expect("Space/set HTTP request (post-disable)");
    assert_eq!(post2.status().as_u16(), 200);

    // Wait two poll cycles (400 ms is one full + ~50 ms slack on each
    // side). If a StateChange arrives in this window, push was not
    // actually disabled.
    match tokio::time::timeout(Duration::from_millis(600), recv_text(&mut socket)).await {
        Ok(Some(unexpected)) => panic!(
            "PushDisable must suppress further state changes; got unexpected frame: {unexpected}"
        ),
        Ok(None) => panic!("WS stream ended unexpectedly during PushDisable check"),
        Err(_) => {} // timeout — expected; no frame arrived
    }

    // -- Step 4: Sanity check that Request/Response still works after
    // PushDisable. The control plane is independent of push state.
    socket
        .send(Message::Text(
            json!({
                "@type": "Request",
                "id": "wsrq2",
                "using": ["urn:ietf:params:jmap:core"],
                "methodCalls": [["Core/echo", {}, "c1"]]
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send second Request");
    let frame = recv_text(&mut socket)
        .await
        .expect("second Response must arrive");
    let parsed: Value = serde_json::from_str(&frame).expect("Response is JSON");
    assert_eq!(parsed["@type"], "Response");
    assert_eq!(parsed["requestId"], "wsrq2");

    let _ = socket.close(None).await;
    jig.shutdown().await;
}

/// Oracle: RFC 8887 §4.3.4 — a malformed frame (not a valid envelope)
/// produces a RequestError envelope, not a connection close, so the
/// client can recover and continue using the socket.
#[tokio::test]
async fn malformed_frame_produces_request_error_envelope() {
    let jig = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn testjig");

    let addr = jig.addr;
    let token = jig.token.clone();
    let url = format!("ws://{addr}/ws?token={token}");
    let mut request = url.into_client_request().expect("build WS client request");
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "jmap".parse().expect("static header"),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("WS connect");

    // Send malformed JSON; expect a RequestError envelope (notJSON).
    socket
        .send(Message::Text("{not json}".into()))
        .await
        .expect("send malformed frame");

    let frame = recv_text(&mut socket)
        .await
        .expect("RequestError frame must arrive");
    let parsed: Value = serde_json::from_str(&frame).expect("RequestError is JSON");
    assert_eq!(parsed["@type"], "RequestError");
    assert_eq!(parsed["type"], "urn:ietf:params:jmap:error:notJSON");

    // The socket MUST still be usable.
    socket
        .send(Message::Text(
            json!({
                "@type": "Request",
                "id": "after-error",
                "using": ["urn:ietf:params:jmap:core"],
                "methodCalls": [["Core/echo", {}, "c1"]]
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send Request after error");
    let recovery = recv_text(&mut socket)
        .await
        .expect("Response after RequestError must arrive — socket stays open");
    let parsed: Value = serde_json::from_str(&recovery).expect("Response JSON");
    assert_eq!(parsed["@type"], "Response");
    assert_eq!(parsed["requestId"], "after-error");

    let _ = socket.close(None).await;
    jig.shutdown().await;
}

/// Read the next text frame from `socket`, returning its body as a
/// `String`. Returns `None` if the stream ends before a text frame
/// arrives (close, error, or timeout).
async fn recv_text<S>(socket: &mut S) -> Option<String>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + WS_WAIT_BUDGET;
    loop {
        let remaining = deadline.checked_duration_since(tokio::time::Instant::now())?;
        let next = tokio::time::timeout(remaining, socket.next()).await.ok()?;
        match next {
            Some(Ok(Message::Text(t))) => return Some(t.to_string()),
            // Other frame types (binary, ping, pong, close) are
            // either auto-handled by tokio-tungstenite or
            // not-applicable to JMAP-over-WS; keep reading.
            Some(Ok(Message::Close(_))) => return None,
            Some(Ok(_)) => continue,
            Some(Err(_)) | None => return None,
        }
    }
}

/// Read text frames from `socket` until a `StateChange` frame whose
/// `changed.<account>.<type_name>` field is populated arrives, or
/// until [`WS_WAIT_BUDGET`] elapses.
async fn read_state_change_for<S>(socket: &mut S, type_name: &str) -> Option<String>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + WS_WAIT_BUDGET;
    loop {
        let remaining = deadline.checked_duration_since(tokio::time::Instant::now())?;
        let next = tokio::time::timeout(remaining, recv_text(socket))
            .await
            .ok()?;
        let body = next?;
        let v: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("@type").and_then(Value::as_str) == Some("StateChange") {
            // Only return when the specific type_name is present in
            // the changed map; otherwise it was an unrelated push.
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
