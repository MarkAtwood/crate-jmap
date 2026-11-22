//! Running a [`Connection`] over tokio-tungstenite.
//!
//! The opening handshake stays the deployment's, because it carries the
//! authentication (RFC 8887 §4.1). [`negotiate_jmap`] does the §4.2 part:
//! call it from the `accept_hdr_async_with_config` callback after
//! authenticating, or use [`accept`] / [`accept_with_config`] when the
//! transport authenticated already. Then hand the stream to [`serve`].
//!
//! # Message size
//!
//! tungstenite reads a whole message before [`serve`] sees it, so the
//! `maxSizeRequest` check of [`ConnectionConfig`] alone lets a client make
//! the server buffer up to tungstenite's own default cap (64 MiB) per
//! connection. [`websocket_config`] sets tungstenite's caps from
//! `maxSizeRequest`, and [`accept`] and [`accept_with_config`] apply them.
//! tungstenite then refuses an oversized message while it arrives, frame
//! by frame, and [`serve`] closes the connection with status 1009
//! (Message Too Big, RFC 6455 §7.4.1).
//!
//! ```no_run
//! use jmap_websocket::server::ConnectionConfig;
//! use jmap_websocket::tokio_tungstenite::accept_hdr_async_with_config;
//! use jmap_websocket::tungstenite::{negotiate_jmap, websocket_config};
//!
//! # async fn run(tcp: tokio::net::TcpStream) -> Result<(), Box<dyn std::error::Error>> {
//! let config = ConnectionConfig::default().max_size_request(Some(1_000_000));
//! // Authenticate in the callback, then negotiate `jmap` (RFC 8887 §4.1, §4.2).
//! let ws = accept_hdr_async_with_config(tcp, negotiate_jmap, Some(websocket_config(&config)))
//!     .await?;
//! // ... Connection::new(handler).with_config(config), then serve(connection, ws).
//! # let _ = ws; Ok(()) }
//! ```

use futures::{SinkExt as _, StreamExt as _, future};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::error::CapacityError;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::{HeaderValue, StatusCode};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::{Error, Message};

use crate::server::{
    Connection, ConnectionConfig, ConnectionError, Inbound, Outbound, PushSource, RequestHandler,
};

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

/// tungstenite's configuration for a connection served with `config`: the
/// default [`WebSocketConfig`] with its message and frame size caps set to
/// `config`'s `maxSizeRequest`
/// ([`ConnectionConfig::max_size_request_value`]).
///
/// The frame cap equals the message cap because most clients send a
/// message as one frame, so a smaller frame cap would refuse messages that
/// `maxSizeRequest` allows. With `maxSizeRequest` `None`, tungstenite's
/// own caps stay (64 MiB per message, 16 MiB per frame).
///
/// Pass it to `accept_hdr_async_with_config` when the handshake callback
/// authenticates; [`accept_with_config`] uses it.
///
/// ```
/// use jmap_websocket::server::ConnectionConfig;
/// use jmap_websocket::tungstenite::websocket_config;
///
/// let ws = websocket_config(&ConnectionConfig::default().max_size_request(Some(1_000_000)));
/// assert_eq!(ws.max_message_size, Some(1_000_000));
/// assert_eq!(ws.max_frame_size, Some(1_000_000));
/// ```
pub fn websocket_config(config: &ConnectionConfig) -> WebSocketConfig {
    let ws = WebSocketConfig::default();
    match config.max_size_request_value() {
        Some(max) => ws.max_message_size(Some(max)).max_frame_size(Some(max)),
        None => ws,
    }
}

/// Accept a WebSocket on `stream` with [`negotiate_jmap`] and no other
/// checks, for a transport that authenticated the client already. The
/// size caps are those of the default [`ConnectionConfig`]
/// (`maxSizeRequest` 10 000 000 octets; see [`websocket_config`]).
///
/// When the [`Connection`] runs with another `maxSizeRequest`, use
/// [`accept_with_config`] with its [`ConnectionConfig`].
///
/// # Errors
///
/// tungstenite's handshake error, including the refusal when `jmap` is not
/// offered.
pub async fn accept<S>(stream: S) -> Result<WebSocketStream<S>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    accept_with_config(stream, &ConnectionConfig::default()).await
}

/// [`accept`], with tungstenite's message and frame size caps taken from
/// `config` ([`websocket_config`]). Pass the [`ConnectionConfig`] the
/// [`Connection`] runs with.
///
/// # Errors
///
/// tungstenite's handshake error, including the refusal when `jmap` is not
/// offered.
pub async fn accept_with_config<S>(
    stream: S,
    config: &ConnectionConfig,
) -> Result<WebSocketStream<S>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    tokio_tungstenite::accept_hdr_async_with_config(
        stream,
        negotiate_jmap,
        Some(websocket_config(config)),
    )
    .await
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
/// A message over tungstenite's size cap ([`websocket_config`]) reaches the
/// connection as [`Inbound::TooBig`]: it sends a Close with status 1009
/// (Message Too Big) and `serve` returns `Ok(())`, without waiting for
/// the client's Close or reading the rest of the message.
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
    let incoming = stream.filter_map(|m| {
        future::ready(match m {
            Ok(m) => inbound(m).map(Ok),
            Err(Error::Capacity(CapacityError::MessageTooLong { .. })) => Some(Ok(Inbound::TooBig)),
            Err(e) => Some(Err(e)),
        })
    });
    let outgoing = sink.with(|m: Outbound| future::ready(Ok::<_, Error>(outbound(m))));
    futures::pin_mut!(incoming, outgoing);
    connection.run(incoming, outgoing).await
}
