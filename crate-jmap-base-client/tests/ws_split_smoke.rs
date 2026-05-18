//! Integration test for [`WsSession::split`] (bd:JMAP-6r7c.31).
//!
//! Verifies that the WsSender / WsReceiver halves can drive concurrent
//! send-while-receiving from two distinct tokio tasks — the topology that
//! the unified [`WsSession`] handle cannot service because all three of
//! its methods take `&mut self` and would serialise.
//!
//! The test spins up a real tokio_tungstenite WebSocket server on
//! 127.0.0.1:0 so the connection traverses the same code path as a
//! production connect (no in-process bypass). The server holds the
//! receive side open while it interleaves an outbound StateChange frame
//! and a synchronous read of whatever the client sends, so a regression
//! that re-introduces &mut-self coupling between send and receive would
//! deadlock here.

use futures::SinkExt as _;
use futures::StreamExt as _;
use jmap_base_client::{ws::connect_ws, WsFrame};
use tokio::net::TcpListener;

/// Oracle: bd:JMAP-6r7c.31 — `WsSession::split()` returns owning halves
/// that can be moved to separate tokio tasks. One task drives the
/// `WsReceiver::next_frame` loop, another drives `WsSender::send_text`.
/// The server interleaves an inbound text read with an outbound
/// StateChange send to exercise both directions concurrently.
///
/// A regression that bundled send and receive back onto a single
/// `&mut WsSession` would either fail to compile (because two `&mut`
/// borrows cannot be moved to two tasks) or deadlock at runtime
/// (because one task's `.await` would block the other).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn split_enables_concurrent_send_and_receive() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("must bind 127.0.0.1:0");
    let server_addr = listener.local_addr().expect("must report local addr");

    // Server: accept exactly one connection. The server-side task:
    //   1. Reads one text frame from the client (the synchronization point;
    //      proves the client sender task ran).
    //   2. Sends one StateChange JSON frame back (the receive task on the
    //      client side proves it.)
    //   3. Initiates a graceful close so the client's next_frame loop
    //      terminates with None.
    let server_handle = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("server must accept one connection");
        let mut ws = tokio_tungstenite::accept_async(stream)
            .await
            .expect("server must complete WS handshake");

        let inbound = ws
            .next()
            .await
            .expect("server must observe a client message")
            .expect("server message must be Ok");
        let inbound_text = match inbound {
            tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
            other => panic!("server expected Text frame from client, got {other:?}"),
        };

        let state_change =
            r#"{"@type":"StateChange","changed":{"account1":{"Mail":"s-from-server"}}}"#;
        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            state_change.to_owned().into(),
        ))
        .await
        .expect("server must send StateChange");

        ws.close(None).await.expect("server close must succeed");

        inbound_text
    });

    let ws_url = format!("ws://{server_addr}/");
    let session = connect_ws(&ws_url, None)
        .await
        .expect("client must complete WS handshake");

    let (mut sender, mut receiver) = session.split();

    // Spawn the receive loop in its own task — exactly the topology the
    // unified WsSession could not service. Receives one StateChange and
    // then None (the close handshake).
    let recv_task = tokio::spawn(async move {
        let first = receiver
            .next_frame()
            .await
            .expect("must receive one frame before close");
        let frame = first.expect("first frame must parse");
        // Drain to the close — must observe None for clean shutdown.
        let after_close = receiver.next_frame().await;
        (frame, after_close.is_none())
    });

    // Send a single text frame concurrently with the receive loop. If
    // `split` re-bundled the halves into a single `&mut`-borrowed
    // WsSession, this call would not compile.
    sender
        .send_text("client-hello".to_owned())
        .await
        .expect("client send_text must succeed");

    // The sender goes out of scope here so the close-handshake can
    // proceed when the receive task drops its half.

    let inbound_text = server_handle.await.expect("server task must not panic");
    assert_eq!(
        inbound_text, "client-hello",
        "server must observe the client's exact text frame"
    );

    let (frame, observed_close) = recv_task.await.expect("recv task must not panic");
    match frame {
        WsFrame::StateChange(sc) => {
            let acct = sc
                .changed
                .get("account1")
                .expect("server StateChange must carry account1");
            assert_eq!(
                acct.get("Mail").map(|s| s.as_ref()),
                Some("s-from-server"),
                "server StateChange must carry the expected mail state"
            );
        }
        other => panic!("expected StateChange frame, got {other:?}"),
    }
    assert!(
        observed_close,
        "client must observe None after server close"
    );
}
