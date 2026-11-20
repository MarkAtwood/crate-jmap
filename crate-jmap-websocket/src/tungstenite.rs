//! Running a [`Connection`] over tokio-tungstenite.
//!
//! The opening handshake stays the deployment's: accept with
//! `tokio_tungstenite::accept_hdr_async`, authenticate the request
//! (RFC 8887 §4.1), check [`offers_jmap`](crate::offers_jmap) on its
//! `Sec-WebSocket-Protocol` and set `Sec-WebSocket-Protocol: jmap` on the
//! response (§4.2), then hand the stream to [`serve`]. Cap the message
//! size in the `WebSocketConfig` too.

use futures::{SinkExt as _, StreamExt as _, future};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Error, Message};

use crate::server::{Connection, ConnectionError, Inbound, Outbound, PushSource, RequestHandler};

/// The [`Inbound`] for a tungstenite message; `None` for Ping, Pong and
/// raw frames, which tungstenite handles itself.
pub fn inbound(message: Message) -> Option<Inbound> {
    match message {
        Message::Text(text) => Some(Inbound::Text(text.as_str().to_owned())),
        Message::Binary(_) => Some(Inbound::Binary),
        Message::Close(_) => Some(Inbound::Close),
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => None,
    }
}

/// The tungstenite message for an [`Outbound`].
pub fn outbound(message: Outbound) -> Message {
    match message {
        Outbound::Text(text) => Message::Text(text.into()),
        Outbound::Close { code, reason } => Message::Close(Some(CloseFrame {
            code: CloseCode::from(code),
            reason: reason.into(),
        })),
    }
}

/// Serve `connection` on an accepted `stream` until it closes.
///
/// # Errors
///
/// As [`Connection::run`]; transport errors are tungstenite's.
pub async fn serve<H, P, S>(
    connection: Connection<H, P>,
    stream: WebSocketStream<S>,
) -> Result<(), ConnectionError>
where
    H: RequestHandler,
    P: PushSource,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (sink, stream) = stream.split();
    let incoming = stream.filter_map(|m| future::ready(m.map(inbound).transpose()));
    let outgoing = sink.with(|m: Outbound| future::ready(Ok::<_, Error>(outbound(m))));
    futures::pin_mut!(incoming, outgoing);
    connection.run(incoming, outgoing).await
}
