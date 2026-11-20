//! Running a [`Connection`] over tokio-tungstenite.
//!
//! The opening handshake stays the deployment's, because it carries the
//! authentication (RFC 8887 §4.1). [`negotiate_jmap`] does the §4.2 part:
//! call it from the `accept_hdr_async` callback after authenticating, or
//! use [`accept`] when the transport authenticated already. Then hand the
//! stream to [`serve`]. Cap the message size in the `WebSocketConfig` too.

use futures::{SinkExt as _, StreamExt as _, future};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::{HeaderValue, StatusCode};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Error, Message};

use crate::server::{Connection, ConnectionError, Inbound, Outbound, PushSource, RequestHandler};

/// The RFC 8887 §4.2 subprotocol negotiation, as an `accept_hdr_async`
/// callback: refuse (400) a handshake whose `Sec-WebSocket-Protocol` does
/// not offer `jmap`, otherwise answer with `Sec-WebSocket-Protocol: jmap`.
///
/// # Errors
///
/// The 400 response, when `jmap` is not offered.
#[allow(clippy::result_large_err)] // the callback signature is tungstenite's
pub fn negotiate_jmap(
    request: &Request,
    mut response: Response,
) -> Result<Response, ErrorResponse> {
    let offered = request
        .headers()
        .get_all("Sec-WebSocket-Protocol")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(crate::offers_jmap);
    if !offered {
        let mut err = ErrorResponse::new(Some("the jmap subprotocol is required".to_owned()));
        *err.status_mut() = StatusCode::BAD_REQUEST;
        return Err(err);
    }
    response.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static(crate::SUBPROTOCOL),
    );
    Ok(response)
}

/// Accept a WebSocket on `stream` with [`negotiate_jmap`] and no other
/// checks: for a transport that authenticated the client already.
///
/// # Errors
///
/// tungstenite's handshake error, including the refusal when `jmap` is not
/// offered.
pub async fn accept<S>(stream: S) -> Result<WebSocketStream<S>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    tokio_tungstenite::accept_hdr_async(stream, negotiate_jmap).await
}

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
