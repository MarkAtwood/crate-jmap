//! `next_frame` surfaces a server Close frame with status code 1008
//! (bd:JMAP-yuut.6).
//!
//! Oracle: draft-atwood-jmap-chat-wss-00 L135 (jmap-chat-spec 91672c9):
//! "If the authentication credentials expire during a WebSocket session,
//! the server MUST either treat subsequent requests as unauthenticated or
//! close the WebSocket connection with a Close frame status code of 1008
//! (Policy Violation). [...] servers MUST take one of these two actions and
//! MUST use status code 1008 if closing, so that clients can reliably detect
//! and recover from credential expiry on a long-lived chat connection."
//! RFC 6455 §7.4.1: "1008 indicates that an endpoint is terminating the
//! connection because it has received a message that violates its policy."

use futures::StreamExt as _;
use jmap_base_client::ClientError;
use jmap_base_client::{connect_ws_allow_plaintext, DEFAULT_WS_MAX_MESSAGE_BYTES};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;

/// Start a server that accepts one connection and closes it with `code`
/// and `reason`; return the client's first `next_frame` result.
async fn first_frame_after_server_close(
    code: CloseCode,
    reason: &'static str,
) -> Option<Result<jmap_base_client::WsFrame, ClientError>> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("must bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("must report local addr");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("must accept");
        let mut ws = tokio_tungstenite::accept_async(stream)
            .await
            .expect("must complete WS handshake");
        ws.close(Some(CloseFrame {
            code,
            reason: reason.into(),
        }))
        .await
        .expect("server close must succeed");
        // Keep the socket open until the client has read the Close frame.
        let _ = ws.next().await;
    });
    let mut session =
        connect_ws_allow_plaintext(&format!("ws://{addr}/"), None, DEFAULT_WS_MAX_MESSAGE_BYTES)
            .await
            .expect("client must complete WS handshake");
    let first = session.next_frame().await;
    drop(session);
    server.await.expect("server task must not panic");
    first
}

/// A 1008 close is a distinct error carrying the code, so the client can
/// "reliably detect" credential expiry (wss L135).
#[tokio::test]
async fn close_1008_surfaces_ws_closed_with_code() {
    match first_frame_after_server_close(CloseCode::Policy, "token expired").await {
        Some(Err(ClientError::WsClosed { code, reason })) => {
            assert_eq!(code, 1008);
            assert_eq!(reason, "token expired");
        }
        other => panic!("expected WsClosed {{ code: 1008, .. }}, got {other:?}"),
    }
}

/// RFC 6455 §7.4.1: "1000 indicates a normal closure". It is not a policy
/// violation, so the stream still ends cleanly with `None`.
#[tokio::test]
async fn close_1000_still_ends_stream_with_none() {
    let first = first_frame_after_server_close(CloseCode::Normal, "").await;
    assert!(
        first.is_none(),
        "normal close must yield None, got {first:?}"
    );
}
