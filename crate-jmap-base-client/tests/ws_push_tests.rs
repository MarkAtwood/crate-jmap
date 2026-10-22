//! Integration tests for the RFC 8887 push-control helpers and the
//! client-side WebSocket keepalive, against a real local tungstenite server.

use std::time::Duration;

use futures::StreamExt as _;
use jmap_base_client::ClientError;
use jmap_base_client::{connect_ws_allow_plaintext, DEFAULT_WS_MAX_MESSAGE_BYTES};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

type ServerWs = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

/// Bind 127.0.0.1:0 and return the listener with its `ws://` URL.
async fn listen() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("must bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("must report local addr");
    (listener, format!("ws://{addr}/"))
}

async fn accept(listener: TcpListener) -> ServerWs {
    let (stream, _) = listener.accept().await.expect("server must accept");
    tokio_tungstenite::accept_async(stream)
        .await
        .expect("server must complete WS handshake")
}

/// Oracle: RFC 8887 §4.3.5.2 / §4.3.5.3, with the enable object copied
/// from the §4.4 example: `{"@type": "WebSocketPushEnable", "dataTypes":
/// ["Mailbox", "Email"], "pushState": "aaa"}`. `dataTypes` is
/// "String[]|null" ("If null, changes will be pushed for all supported data
/// types"), so it must be sent as `null`, not omitted; `pushState` is
/// optional. The disable object "has the following property: @type ...
/// MUST be the string "WebSocketPushDisable"".
#[tokio::test]
async fn push_enable_and_disable_wire_format() {
    let (listener, url) = listen().await;
    let server = tokio::spawn(async move {
        let mut ws = accept(listener).await;
        let mut got = Vec::new();
        for _ in 0..3 {
            match ws.next().await.expect("message").expect("ok message") {
                Message::Text(t) => got
                    .push(serde_json::from_str::<serde_json::Value>(&t).expect("client sent JSON")),
                other => panic!("expected Text, got {other:?}"),
            }
        }
        got
    });

    let mut session = connect_ws_allow_plaintext(&url, None, DEFAULT_WS_MAX_MESSAGE_BYTES)
        .await
        .expect("connect");
    session
        .send_push_enable(Some(&["Mailbox", "Email"]), Some("aaa"))
        .await
        .expect("send enable");
    session
        .send_push_enable(None, None)
        .await
        .expect("send enable(all)");
    session.send_push_disable().await.expect("send disable");

    let got = server.await.expect("server task");
    assert_eq!(
        got[0],
        serde_json::json!({
            "@type": "WebSocketPushEnable",
            "dataTypes": ["Mailbox", "Email"],
            "pushState": "aaa"
        })
    );
    assert_eq!(
        got[1],
        serde_json::json!({"@type": "WebSocketPushEnable", "dataTypes": null})
    );
    assert_eq!(got[2], serde_json::json!({"@type": "WebSocketPushDisable"}));
}

/// Oracle: RFC 6455 §5.5.2 — "A Ping frame MAY serve either as a keepalive
/// or as a means to verify that the remote endpoint is still responsive",
/// and §5.5.3 — the peer "MUST send a Pong frame in response". With a 5 ms
/// interval on an otherwise silent connection, the server must observe
/// Pings, and a responsive server (tungstenite answers Pings as it reads)
/// must never be reported dead. 500 ms is ~100 Ping/Pong rounds, well past
/// the 64-frame non-text flood cap, so solicited Pongs must not count
/// against it.
#[tokio::test]
async fn keepalive_pings_idle_connection_and_tolerates_pongs() {
    let (listener, url) = listen().await;
    let server = tokio::spawn(async move {
        let mut ws = accept(listener).await;
        let mut pings = 0usize;
        // Read (and thereby auto-answer Pings) until the client goes away.
        while let Some(Ok(msg)) = ws.next().await {
            if matches!(msg, Message::Ping(_)) {
                pings += 1;
            }
        }
        pings
    });

    let mut session = connect_ws_allow_plaintext(&url, None, DEFAULT_WS_MAX_MESSAGE_BYTES)
        .await
        .expect("connect");
    session
        .enable_keepalive(Duration::from_millis(5), Duration::from_secs(2))
        .expect("valid keepalive");
    let res = tokio::time::timeout(Duration::from_millis(500), session.next_frame()).await;
    assert!(
        res.is_err(),
        "a responsive idle peer must yield nothing, got {res:?}"
    );
    drop(session);

    let pings = server.await.expect("server task");
    assert!(pings >= 65, "expected >= 65 keepalive Pings, got {pings}");
}

/// Oracle: RFC 6455 §5.5.3 — a live endpoint "MUST send a Pong frame in
/// response" to a Ping. A server that holds the TCP connection open but
/// never reads (so never answers) is a dead peer: `next_frame` must report
/// a timed-out transport error instead of hanging.
#[tokio::test]
async fn keepalive_reports_dead_peer() {
    let (listener, url) = listen().await;
    let (release_tx, release_rx) = futures::channel::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let ws = accept(listener).await;
        // Hold the socket without polling it until the test finishes.
        let _ = release_rx.await;
        drop(ws);
    });

    let mut session = connect_ws_allow_plaintext(&url, None, DEFAULT_WS_MAX_MESSAGE_BYTES)
        .await
        .expect("connect");
    session
        .enable_keepalive(Duration::from_millis(20), Duration::from_millis(50))
        .expect("valid keepalive");
    let res = tokio::time::timeout(Duration::from_secs(5), session.next_frame())
        .await
        .expect("dead peer must be detected well before 5 s");
    match res {
        Some(Err(ClientError::WebSocket(e))) => {
            assert!(e.is_io(), "dead peer must surface as an I/O error: {e}");
        }
        other => panic!("expected WebSocket I/O error, got {other:?}"),
    }
    let _ = release_tx.send(());
    server.await.expect("server task");
}

/// Oracle: RFC 6455 §5.5.3 (as above). Here the peer is dead while the app
/// is writing to it: a sender task blocks inside `send_text` on the full
/// TCP send buffer (the server never reads). The receive loop's Ping then
/// cannot be sent either, and `next_frame` must still report a timed-out
/// transport error instead of waiting forever behind the blocked sender.
#[tokio::test]
async fn keepalive_reports_dead_peer_while_sender_blocked() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let (listener, url) = listen().await;
    let (release_tx, release_rx) = futures::channel::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let ws = accept(listener).await;
        let _ = release_rx.await;
        drop(ws);
    });

    let mut session = connect_ws_allow_plaintext(&url, None, DEFAULT_WS_MAX_MESSAGE_BYTES)
        .await
        .expect("connect");
    session
        .enable_keepalive(Duration::from_millis(20), Duration::from_millis(200))
        .expect("valid keepalive");
    let (mut sender, mut receiver) = session.split();
    let sent = Arc::new(AtomicUsize::new(0));
    let sent_in_task = Arc::clone(&sent);
    let writer = tokio::spawn(async move {
        let chunk = "x".repeat(1 << 20);
        while sender.send_text(chunk.clone()).await.is_ok() {
            sent_in_task.fetch_add(1, Ordering::SeqCst);
        }
    });

    // Wait until the writer has stopped making progress, i.e. it is parked
    // inside send_text holding the sink.
    let mut last = usize::MAX;
    let mut stalled = 0;
    while stalled < 4 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let now = sent.load(Ordering::SeqCst);
        stalled = if now == last { stalled + 1 } else { 0 };
        last = now;
    }

    let res = tokio::time::timeout(Duration::from_secs(5), receiver.next_frame())
        .await
        .expect("dead peer must be detected even while the sender is blocked");
    match res {
        Some(Err(ClientError::WebSocket(e))) => {
            assert!(e.is_io(), "dead peer must surface as an I/O error: {e}");
        }
        other => panic!("expected WebSocket I/O error, got {other:?}"),
    }
    writer.abort();
    let _ = release_tx.send(());
    server.await.expect("server task");
}

/// Oracle: `enable_keepalive` docs — a zero interval or timeout is rejected
/// with `InvalidArgument` (a zero interval would ping in a busy loop).
#[tokio::test]
async fn enable_keepalive_rejects_zero_durations() {
    let (listener, url) = listen().await;
    let server = tokio::spawn(accept(listener));
    let mut session = connect_ws_allow_plaintext(&url, None, DEFAULT_WS_MAX_MESSAGE_BYTES)
        .await
        .expect("connect");
    for (interval, timeout) in [
        (Duration::ZERO, Duration::from_secs(1)),
        (Duration::from_secs(1), Duration::ZERO),
    ] {
        assert!(matches!(
            session.enable_keepalive(interval, timeout),
            Err(ClientError::InvalidArgument(_))
        ));
    }
    drop(server.await.expect("server task"));
}
