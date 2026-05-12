//! Consume a synthetic JMAP Server-Sent Events stream.
//!
//! Spins up an in-process tokio `TcpListener` that accepts one HTTP
//! request and replies with a `Content-Type: text/event-stream` body
//! containing two `event: state` frames per RFC 8620 §7.3 / JMAP Chat
//! draft §StateChange (draft-atwood-jmap-chat-00 lines 1334-1337).
//!
//! The client side calls
//! [`jmap_base_client::JmapClient::subscribe_events`] against that URL
//! and prints each parsed `SseFrame`. The stream ends cleanly when the
//! stub server closes the connection after emitting both frames.
//!
//! `parse_chat_sse_block` (the chat-aware promoter that turns
//! `event: typing` / `event: presence` blocks into
//! [`jmap_chat_client::ChatSseEvent::Typing`] / `Presence`) is not
//! exercised here because the base-client `subscribe_events` stream
//! pre-parses each block into the base-client `SseFrame` type. To
//! exercise the chat-aware path, hand-roll the chunked-body read loop
//! and call `parse_chat_sse_block` per block — see the unit tests in
//! `src/sse.rs` for the input shapes.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example sse_listen -p jmap-chat-client
//! ```
//!
//! NOT FOR PRODUCTION — single-shot, no retry, no auth, no TLS.

use futures::StreamExt;
use jmap_base_client::{ClientConfig, JmapClient, NoneAuth};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Two SSE blocks the stub server emits, one per JMAP Chat draft
/// §StateChange wire shape (lines 1334-1337). The first carries the
/// `@type` discriminator; the second omits it (the base-client parser
/// accepts both forms).
const SSE_BODY: &str = "\
event: state\n\
data: {\"@type\":\"StateChange\",\"changed\":{\"u1\":{\"Message\":\"d35ecb040aab\"}}}\n\
\n\
event: state\n\
data: {\"changed\":{\"u1\":{\"Chat\":\"7fa019b3c200\"}}}\n\
\n\
";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Bind on an OS-assigned port so the example never collides with
    // another running instance.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr: SocketAddr = listener.local_addr()?;
    let event_source_url = format!("http://{addr}/sse");
    eprintln!("stub event source: {event_source_url}");

    // Spawn the stub server. It accepts exactly one connection, drains
    // the request headers, sends the SSE response, then closes.
    let stub = tokio::spawn(async move {
        let (mut sock, _peer) = listener.accept().await?;
        // Drain the HTTP request headers (we don't inspect them).
        let mut buf = [0u8; 1024];
        let mut total = 0usize;
        loop {
            let n = sock.read(&mut buf[total..]).await?;
            if n == 0 {
                break;
            }
            total += n;
            if buf[..total].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
            if total == buf.len() {
                break;
            }
        }
        let resp = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/event-stream\r\n\
             Cache-Control: no-cache\r\n\
             Connection: close\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {}",
            SSE_BODY.len(),
            SSE_BODY
        );
        sock.write_all(resp.as_bytes()).await?;
        sock.shutdown().await?;
        Ok::<_, std::io::Error>(())
    });

    // Client side: connect to the stub and consume the SSE stream.
    let client =
        JmapClient::new_plain(NoneAuth, &format!("http://{addr}"), ClientConfig::default())?;
    let mut stream = client.subscribe_events(&event_source_url, None).await?;

    let mut count = 0usize;
    while let Some(frame) = stream.next().await {
        let frame = frame?;
        count += 1;
        println!("frame #{count}: event={:?} id={:?}", frame.event, frame.id);
    }
    println!("stream closed after {count} frame(s)");

    stub.await??;
    Ok(())
}
