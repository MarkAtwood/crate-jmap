//! Consume a synthetic JMAP WebSocket stream via `ChatWsExt`.
//!
//! Spins up an in-process tokio `TcpListener` that performs one
//! WebSocket handshake via `tokio_tungstenite::accept_async`, sends
//! one `@type:"Response"` frame and one `@type:"StateChange"` push
//! frame (RFC 8887 §4.3.5 / draft-atwood-jmap-chat-wss-00 §3.5–3.7),
//! then closes.
//!
//! The client side calls [`jmap_base_client::connect_ws`] to open the
//! session, then drives the read loop through
//! [`jmap_chat_client::ChatWsExt::next_chat_frame`] — the chat-aware
//! extension trait that promotes typed `WsFrame` variants into
//! [`jmap_chat_client::ChatWsFrame`] (adds `ChatTyping` / `ChatPresence`
//! over the base set).
//!
//! Run with:
//!
//! ```sh
//! cargo run --example ws_loop -p jmap-chat-client
//! ```
//!
//! NOT FOR PRODUCTION — single connection, no retry, no auth, no TLS.

use futures::SinkExt;
use jmap_base_client::connect_ws;
use jmap_chat_client::{ChatWsExt, ChatWsFrame};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// JMAP-WSS §3.5 `Response` frame, minimal valid shape: empty
/// `methodResponses` and a session-state token. The `@type` discriminator
/// routes the frame into the typed [`jmap_base_client::WsFrame::Response`]
/// variant.
const RESPONSE_FRAME: &str =
    r#"{"@type":"Response","requestId":"r1","methodResponses":[],"sessionState":"s1"}"#;

/// JMAP-WSS §3.7 `StateChange` push frame. The `pushState` field is
/// RFC 8887 §4.3.5; it lands in `StateChange.extra` per the workspace
/// extras-preservation policy and round-trips losslessly.
const STATE_CHANGE_FRAME: &str = concat!(
    r#"{"@type":"StateChange","#,
    r#""changed":{"u1":{"Message":"d35ecb040aab"}},"#,
    r#""pushState":"bbb"}"#
);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // OS-assigned port so the example never collides with another run.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr: SocketAddr = listener.local_addr()?;
    let ws_url = format!("ws://{addr}/ws");
    eprintln!("stub WebSocket: {ws_url}");

    // Spawn the stub server. Accepts one TCP connection, performs the
    // WS handshake, sends the two frames, then closes.
    let stub = tokio::spawn(async move {
        let (stream, _peer) = listener.accept().await?;
        let mut ws = tokio_tungstenite::accept_async(stream).await?;
        ws.send(Message::Text(RESPONSE_FRAME.into())).await?;
        ws.send(Message::Text(STATE_CHANGE_FRAME.into())).await?;
        ws.close(None).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });

    // Client side: open the WS session and drive the chat-aware read
    // loop until the stub closes (next_chat_frame returns None).
    //
    // Production: after `next_chat_frame` returns `None` (clean close)
    // or `Some(Err(...))` (transport error), reconnect with
    // exponential backoff. JMAP base-client's `connect_ws` doc
    // (ws/mod.rs in crate-jmap-base-client) is explicit that the
    // caller is responsible for reconnecting — the library does not
    // retry transparently. After reconnect, resend any
    // `WebSocketPushEnable` and (when implemented) re-issue the prior
    // `pushState` to resume push delivery from the right point.
    let mut session = connect_ws(&ws_url, None).await?;

    let mut count = 0usize;
    while let Some(frame) = session.next_chat_frame().await {
        let frame = frame?;
        count += 1;
        match &frame {
            ChatWsFrame::Response(r) => println!(
                "frame #{count}: Response — {} method responses, session_state={:?}",
                r.method_responses.len(),
                r.session_state
            ),
            ChatWsFrame::StateChange(sc) => println!(
                "frame #{count}: StateChange — {} account(s) changed",
                sc.changed.len()
            ),
            other => println!("frame #{count}: other — {other:?}"),
        }
    }
    println!("session closed after {count} frame(s)");

    stub.await??;
    Ok(())
}
