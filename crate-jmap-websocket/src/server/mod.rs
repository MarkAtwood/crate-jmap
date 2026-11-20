//! The server side of RFC 8887: a per-connection session driver.
//!
//! [`Connection::run`] reads [`Inbound`] messages from any `Stream` and
//! writes [`Outbound`] messages to any `Sink`, so it is independent of
//! the WebSocket library. The deployment does the opening handshake
//! (authentication, RFC 8887 §4.1; the `jmap` subprotocol, §4.2, see
//! [`offers_jmap`](crate::offers_jmap)), maps its library's messages to
//! [`Inbound`] / [`Outbound`] (feature `tungstenite` does this for
//! tokio-tungstenite), and calls `run` once per connection.
//!
//! - Each text message goes through [`parse_client_message`]. A
//!   [`WsRequest`] is handed to the [`RequestHandler`]
//!   ([`DispatcherHandler`] wraps a [`jmap_server::Dispatcher`]); its
//!   [`WsResponse`] or
//!   [`WsRequestError`] carries the request's `id`
//!   back as `requestId` (§4.3.3, §4.3.4).
//! - Requests run concurrently, up to
//!   [`ConnectionConfig::max_concurrent_requests`], and answer in the
//!   order they finish: "JMAP over WebSocket allows the server to process
//!   requests out of order" (§4.3.2). A request over the limit gets a
//!   `limit` error naming `maxConcurrentRequests`, which "also applies to
//!   requests made on the WebSocket connection". [`Connection::with_shared_limit`]
//!   shares the limit with the deployment's HTTP API endpoint.
//! - A [`WebSocketPushEnable`] subscribes to the [`PushSource`]; each
//!   [`WsStateChange`] it yields is cut to the requested `dataTypes`
//!   ([`filter_state_change`]) and sent, in the order the source yields
//!   them. Enabling again replaces the subscription;
//!   [`WebSocketPushDisable`] drops it (§4.3.5). [`PushHub`] is a ready
//!   source.
//! - A binary message closes the connection with status 1003, or is
//!   ignored ([`BinaryFrames`], §4.3.1).

mod hub;

pub use hub::PushHub;

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use futures::stream::{BoxStream, FuturesUnordered};
use futures::{Sink, SinkExt as _, Stream, StreamExt as _};
use jmap_server::Dispatcher;
use jmap_types::problem::ProblemDetails;
use jmap_types::{JmapError, JmapRequest, JmapResponse, State};
use serde_json::Value;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::{
    ClientMessage, WebSocketPushDisable, WebSocketPushEnable, WsRequest, WsRequestError,
    WsResponse, WsStateChange,
};

/// The WebSocket close status for a binary message (RFC 8887 §4.3.1,
/// RFC 6455 §7.4.1: 1003 Unsupported Data).
pub const CLOSE_UNSUPPORTED_DATA: u16 = 1003;

// ---------------------------------------------------------------------------
// Messages in and out
// ---------------------------------------------------------------------------

/// A WebSocket message received from the client, as the driver needs it.
///
/// Fragmented messages must already be coalesced (RFC 8887 §4.3); every
/// WebSocket library does this. Map Ping and Pong to nothing: the
/// library answers Pings itself.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inbound {
    /// A text message (UTF-8, checked by the WebSocket library).
    Text(String),
    /// A binary message; its payload is not needed.
    Binary,
    /// A Close frame: the client is ending the connection.
    Close,
}

/// A WebSocket message the driver sends.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outbound {
    /// A text message: one serialized RFC 8887 server message.
    Text(String),
    /// A Close frame with a status code and reason.
    Close {
        /// The RFC 6455 §7.4 status code.
        code: u16,
        /// The reason text.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// What the driver does with a binary message (RFC 8887 §4.3.1: "the
/// endpoint can either ignore the frame or close the WebSocket
/// connection").
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BinaryFrames {
    /// Send a Close with [`CLOSE_UNSUPPORTED_DATA`] and end the connection.
    #[default]
    Close,
    /// Drop the message and keep reading.
    Ignore,
}

/// Per-connection limits and policies.
///
/// Set the limits to the values the Session's
/// `urn:ietf:params:jmap:core` capability advertises (RFC 8620 §2).
///
/// ```
/// use jmap_websocket::server::ConnectionConfig;
///
/// let config = ConnectionConfig::default()
///     .max_calls_in_request(32)
///     .max_size_request(Some(10_000_000))
///     .max_concurrent_requests(4);
/// assert_eq!(config.max_calls_in_request_value(), 32);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    max_calls_in_request: usize,
    max_size_request: Option<usize>,
    max_concurrent_requests: usize,
    binary_frames: BinaryFrames,
}

impl Default for ConnectionConfig {
    /// 16 calls per request, 10 000 000 octets per request, 4 concurrent
    /// requests (the RFC 8620 §2 suggested minimums), binary messages
    /// close the connection.
    fn default() -> Self {
        Self {
            max_calls_in_request: 16,
            max_size_request: Some(10_000_000),
            max_concurrent_requests: 4,
            binary_frames: BinaryFrames::Close,
        }
    }
}

impl ConnectionConfig {
    /// `maxCallsInRequest`: a Request with more method calls gets a
    /// `limit` error naming it.
    #[must_use]
    pub fn max_calls_in_request(mut self, max: usize) -> Self {
        self.max_calls_in_request = max;
        self
    }

    /// `maxSizeRequest` in octets, or `None` for no check here: a longer
    /// text message gets a `limit` error naming it, unparsed, so with
    /// `requestId: null`. Also cap the message size in the WebSocket
    /// library, which reads the whole message before the driver sees it.
    #[must_use]
    pub fn max_size_request(mut self, max: Option<usize>) -> Self {
        self.max_size_request = max;
        self
    }

    /// `maxConcurrentRequests` on this connection. Values below 1 are
    /// treated as 1.
    #[must_use]
    pub fn max_concurrent_requests(mut self, max: usize) -> Self {
        self.max_concurrent_requests = max.max(1);
        self
    }

    /// The binary-message policy.
    #[must_use]
    pub fn binary_frames(mut self, policy: BinaryFrames) -> Self {
        self.binary_frames = policy;
        self
    }

    /// The `maxCallsInRequest` limit.
    pub fn max_calls_in_request_value(&self) -> usize {
        self.max_calls_in_request
    }

    /// The `maxSizeRequest` limit.
    pub fn max_size_request_value(&self) -> Option<usize> {
        self.max_size_request
    }

    /// The per-connection `maxConcurrentRequests` limit.
    pub fn max_concurrent_requests_value(&self) -> usize {
        self.max_concurrent_requests
    }

    /// The binary-message policy.
    pub fn binary_frames_value(&self) -> BinaryFrames {
        self.binary_frames
    }
}

// ---------------------------------------------------------------------------
// Parsing client messages
// ---------------------------------------------------------------------------

/// The RFC 8887 §4.3.4 RequestError for a request-level `err`: its RFC
/// 8620 §3.6.1 problem details (status from [`jmap_server::error_status`])
/// with `requestId` set to `request_id`.
pub fn request_error(err: &JmapError, request_id: Option<String>) -> WsRequestError {
    let status = jmap_server::error_status(err).as_u16();
    WsRequestError::new(ProblemDetails::for_jmap_error(err, status), request_id)
}

/// Reads one text message from the client (RFC 8887 §4.3).
///
/// In order:
///
/// 1. Longer than `maxSizeRequest`: `limit` (`maxSizeRequest`).
/// 2. Not JSON: `notJSON`, with the detail text of the RFC 8887 §4.4
///    example.
/// 3. Not an object, or `@type` missing or not one of `Request`,
///    `WebSocketPushEnable`, `WebSocketPushDisable`: `notRequest`.
/// 4. `Request`: `id`, when present, must be a string (§4.3.2), else
///    `notRequest`; the rest must be an RFC 8620 §3.3 Request
///    ([`jmap_server::parse_request`]: `notRequest`, or `limit` for
///    `maxCallsInRequest`).
/// 5. `WebSocketPushEnable` / `WebSocketPushDisable` with members of the
///    wrong type: `notRequest`.
///
/// Every error carries the message's `id` as `requestId` when the message
/// is an object with a string `id`, and `null` otherwise, so the client
/// can correlate it (§4.3.4: "MUST be returned if given in the request").
///
/// ```
/// use jmap_websocket::ClientMessage;
/// use jmap_websocket::server::{ConnectionConfig, parse_client_message};
///
/// let config = ConnectionConfig::default();
/// let m = parse_client_message(r#"{"@type":"WebSocketPushDisable"}"#, &config).unwrap();
/// assert!(matches!(m, ClientMessage::PushDisable(_)));
///
/// let e = parse_client_message("The quick brown fox", &config).unwrap_err();
/// assert_eq!(e.problem.problem_type, "urn:ietf:params:jmap:error:notJSON");
/// assert_eq!(e.request_id, None);
/// ```
///
/// # Errors
///
/// The [`WsRequestError`] to send back, as listed above.
// The error is the message to send back, built once per bad message;
// boxing it would only add an allocation and a deref for the caller.
#[allow(clippy::result_large_err)]
pub fn parse_client_message(
    text: &str,
    config: &ConnectionConfig,
) -> Result<ClientMessage, WsRequestError> {
    if let Some(max) = config.max_size_request
        && text.len() > max
    {
        let mut err = JmapError::limit("maxSizeRequest");
        err.description = Some(format!(
            "The request is {} octets, more than maxSizeRequest {max}.",
            text.len()
        ));
        return Err(request_error(&err, None));
    }
    let value: Value = serde_json::from_str(text).map_err(|_| {
        let mut err = JmapError::not_json();
        err.description = Some("The request did not parse as I-JSON.".to_owned());
        request_error(&err, None)
    })?;
    let Value::Object(mut obj) = value else {
        return Err(not_request("The message is not a JSON object.", None));
    };
    let request_id = obj.get("id").and_then(Value::as_str).map(str::to_owned);
    let Some(Value::String(ty)) = obj.remove("@type") else {
        return Err(not_request("The message has no string @type.", request_id));
    };
    match ty.as_str() {
        "Request" => {
            let id = match obj.remove("id") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s),
                Some(_) => return Err(not_request("The request id is not a String.", None)),
            };
            match jmap_server::parse_request(Value::Object(obj), config.max_calls_in_request) {
                Ok(request) => Ok(ClientMessage::Request(WsRequest::new(request, id))),
                Err(err) => Err(request_error(&err, id)),
            }
        }
        "WebSocketPushEnable" => serde_json::from_value::<WebSocketPushEnable>(Value::Object(obj))
            .map(ClientMessage::PushEnable)
            .map_err(|_| not_request("Malformed WebSocketPushEnable.", request_id)),
        "WebSocketPushDisable" => {
            serde_json::from_value::<WebSocketPushDisable>(Value::Object(obj))
                .map(ClientMessage::PushDisable)
                .map_err(|_| not_request("Malformed WebSocketPushDisable.", request_id))
        }
        _ => Err(not_request(
            "The @type is not one a client may send.",
            request_id,
        )),
    }
}

fn not_request(detail: &str, request_id: Option<String>) -> WsRequestError {
    let mut err = JmapError::not_request();
    err.description = Some(detail.to_owned());
    request_error(&err, request_id)
}

// ---------------------------------------------------------------------------
// Request handling
// ---------------------------------------------------------------------------

/// Answers the Requests of one connection.
///
/// Implement it to add work around each dispatch (a `using` check, state
/// change notification, metrics), or use [`DispatcherHandler`].
pub trait RequestHandler: Send + Sync + 'static {
    /// Process one parsed Request. `Err` is a request-level error (RFC
    /// 8620 §3.6.1, e.g. `unknownCapability`) and is sent as a
    /// RequestError; method-level errors belong in the Response.
    fn handle(
        &self,
        request: JmapRequest,
    ) -> impl Future<Output = Result<JmapResponse, JmapError>> + Send;

    /// Called on every RequestError before it is sent, including those
    /// the driver makes itself. Set a localized `title` here (RFC 9457
    /// §3.1.3); the default leaves the problem as it is.
    fn prepare_problem(&self, problem: &mut ProblemDetails) {
        let _ = problem;
    }
}

type SessionStateFn = dyn Fn() -> State + Send + Sync;

/// A [`RequestHandler`] that dispatches every Request on a
/// [`Dispatcher`] for one caller.
///
/// ```
/// use std::sync::Arc;
/// use jmap_websocket::server::DispatcherHandler;
/// use jmap_websocket::jmap_server::Dispatcher;
/// use jmap_websocket::jmap_types::State;
///
/// let dispatcher: Arc<Dispatcher<()>> = Arc::new(Dispatcher::new());
/// let handler = DispatcherHandler::new(dispatcher, (), || State::from("s0"))
///     .with_known_capabilities(["urn:ietf:params:jmap:core"]);
/// # let _ = handler;
/// ```
pub struct DispatcherHandler<C> {
    dispatcher: Arc<Dispatcher<C>>,
    caller: C,
    session_state: Arc<SessionStateFn>,
    known_capabilities: Option<Vec<String>>,
}

impl<C: Clone + Send + Sync + 'static> DispatcherHandler<C> {
    /// Dispatch on `dispatcher` as `caller` (the authenticated principal
    /// of the opening handshake, RFC 8887 §4.1). `session_state` gives
    /// the Response's `sessionState` (RFC 8620 §3.4) at each dispatch.
    pub fn new(
        dispatcher: Arc<Dispatcher<C>>,
        caller: C,
        session_state: impl Fn() -> State + Send + Sync + 'static,
    ) -> Self {
        Self {
            dispatcher,
            caller,
            session_state: Arc::new(session_state),
            known_capabilities: None,
        }
    }

    /// Answer a Request whose `using` names a capability not in
    /// `capabilities` with `unknownCapability` (RFC 8620 §3.3) through
    /// [`jmap_server::check_known_capabilities`]. Without this every
    /// `using` is accepted.
    #[must_use]
    pub fn with_known_capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.known_capabilities = Some(capabilities.into_iter().map(Into::into).collect());
        self
    }
}

impl<C> fmt::Debug for DispatcherHandler<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DispatcherHandler")
            .field("known_capabilities", &self.known_capabilities)
            .finish_non_exhaustive()
    }
}

impl<C: Clone + Send + Sync + 'static> RequestHandler for DispatcherHandler<C> {
    async fn handle(&self, request: JmapRequest) -> Result<JmapResponse, JmapError> {
        if let Some(known) = &self.known_capabilities {
            jmap_server::check_known_capabilities(&request, known)?;
        }
        let state = (self.session_state)();
        Ok(self
            .dispatcher
            .dispatch(request, self.caller.clone(), state)
            .await)
    }
}

// ---------------------------------------------------------------------------
// Push
// ---------------------------------------------------------------------------

/// Where a connection's push notifications come from (RFC 8887 §4.3.5).
///
/// One value serves one connection, so it may be scoped to the
/// connection's caller: it must yield only changes that caller may see.
pub trait PushSource: Send + 'static {
    /// Start a subscription for `enable`: yield every later state change,
    /// and first, when `enable.push_state` is a token this source issued,
    /// the changes since it ("the server SHOULD immediately send all
    /// changes since that state token", §4.3.5.2).
    ///
    /// The source may skip types outside `enable.data_types`; the driver
    /// cuts every notification to them anyway. A stream that ends turns
    /// push off for the connection.
    fn subscribe(&mut self, enable: &WebSocketPushEnable) -> BoxStream<'static, WsStateChange>;
}

/// A [`PushSource`] that never pushes, for a server whose capability says
/// `supportsPush: false`. A [`WebSocketPushEnable`] is accepted and
/// yields nothing.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPush;

impl PushSource for NoPush {
    fn subscribe(&mut self, _enable: &WebSocketPushEnable) -> BoxStream<'static, WsStateChange> {
        futures::stream::empty().boxed()
    }
}

/// Cuts `change` to `data_types` (RFC 8887 §4.3.5.2: "A StateChange
/// notification will only be sent if the data for one of these types
/// changes. Other types are omitted from the TypeState object.").
///
/// `None` keeps every type. Accounts left with no types are dropped;
/// `None` is returned when nothing is left. An empty list therefore
/// matches nothing.
pub fn filter_state_change(
    mut change: WsStateChange,
    data_types: Option<&[String]>,
) -> Option<WsStateChange> {
    if let Some(types) = data_types {
        change.state_change.changed.retain(|_, by_type| {
            by_type.retain(|name, _| types.iter().any(|t| t == name));
            !by_type.is_empty()
        });
    }
    if change.state_change.changed.is_empty() {
        None
    } else {
        Some(change)
    }
}

// ---------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------

/// Why [`Connection::run`] stopped early.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    /// The incoming stream returned an error.
    #[error("receiving a WebSocket message failed")]
    Receive(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// The outgoing sink returned an error.
    #[error("sending a WebSocket message failed")]
    Send(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// A server message did not serialize.
    #[error("serializing a server message failed")]
    Serialize(#[source] serde_json::Error),
}

/// One JMAP-over-WebSocket connection: a [`RequestHandler`], a
/// [`PushSource`] and limits. See the [module docs](self).
pub struct Connection<H, P = NoPush> {
    handler: Arc<H>,
    push: P,
    config: ConnectionConfig,
    shared_limit: Option<Arc<Semaphore>>,
}

impl<H, P> fmt::Debug for Connection<H, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("config", &self.config)
            .field("shared_limit", &self.shared_limit.is_some())
            .finish_non_exhaustive()
    }
}

impl<H: RequestHandler> Connection<H, NoPush> {
    /// A connection answering with `handler`, with no push
    /// ([`NoPush`]) and the default [`ConnectionConfig`].
    pub fn new(handler: Arc<H>) -> Self {
        Self {
            handler,
            push: NoPush,
            config: ConnectionConfig::default(),
            shared_limit: None,
        }
    }
}

type InFlight = std::pin::Pin<
    Box<
        dyn Future<
                Output = (
                    Option<String>,
                    Result<JmapResponse, JmapError>,
                    Option<OwnedSemaphorePermit>,
                ),
            > + Send,
    >,
>;

impl<H: RequestHandler, P: PushSource> Connection<H, P> {
    /// Push notifications from `push`.
    pub fn with_push<Q: PushSource>(self, push: Q) -> Connection<H, Q> {
        Connection {
            handler: self.handler,
            push,
            config: self.config,
            shared_limit: self.shared_limit,
        }
    }

    /// Use `config`.
    #[must_use]
    pub fn with_config(mut self, config: ConnectionConfig) -> Self {
        self.config = config;
        self
    }

    /// Also hold a permit of `limit` while each Request runs, failing the
    /// Request with `maxConcurrentRequests` when none is free. Share one
    /// semaphore with the HTTP API endpoint to apply the limit to "the sum
    /// of requests made on both" (RFC 8887 §4.3.2).
    #[must_use]
    pub fn with_shared_limit(mut self, limit: Arc<Semaphore>) -> Self {
        self.shared_limit = Some(limit);
        self
    }

    /// Serve the connection until the client closes it or the incoming
    /// stream ends.
    ///
    /// Returns `Ok` after a Close, the end of `incoming`, or a binary
    /// message under [`BinaryFrames::Close`] (after sending the Close).
    /// Requests still running then are dropped; method calls the
    /// [`Dispatcher`] already started still finish (see
    /// [`Dispatcher::dispatch`], Cancellation).
    ///
    /// # Errors
    ///
    /// [`ConnectionError`] if `incoming` or `outgoing` fails, or a message
    /// does not serialize.
    pub async fn run<S, K, E>(self, mut incoming: S, mut outgoing: K) -> Result<(), ConnectionError>
    where
        S: Stream<Item = Result<Inbound, E>> + Unpin,
        E: std::error::Error + Send + Sync + 'static,
        K: Sink<Outbound> + Unpin,
        K::Error: std::error::Error + Send + Sync + 'static,
    {
        let Self {
            handler,
            mut push,
            config,
            shared_limit,
        } = self;
        let mut in_flight: FuturesUnordered<InFlight> = FuturesUnordered::new();
        let mut subscription: Option<(BoxStream<'static, WsStateChange>, Option<Vec<String>>)> =
            None;

        loop {
            tokio::select! {
                done = in_flight.next(), if !in_flight.is_empty() => {
                    let Some((request_id, result, permit)) = done else { continue };
                    drop(permit);
                    let text = match result {
                        Ok(response) => to_text(&WsResponse::new(response, request_id))?,
                        Err(err) => error_text(&*handler, request_error(&err, request_id))?,
                    };
                    send(&mut outgoing, Outbound::Text(text)).await?;
                }
                change = next_change(&mut subscription) => {
                    match change {
                        Some(change) => {
                            let types = subscription.as_ref().and_then(|(_, t)| t.as_deref());
                            if let Some(change) = filter_state_change(change, types) {
                                send(&mut outgoing, Outbound::Text(to_text(&change)?)).await?;
                            }
                        }
                        None => subscription = None,
                    }
                }
                message = incoming.next() => {
                    let message = match message {
                        None | Some(Ok(Inbound::Close)) => return Ok(()),
                        Some(Err(e)) => return Err(ConnectionError::Receive(Box::new(e))),
                        Some(Ok(m)) => m,
                    };
                    let text = match message {
                        Inbound::Text(text) => text,
                        Inbound::Binary => match config.binary_frames {
                            BinaryFrames::Ignore => continue,
                            BinaryFrames::Close => {
                                let close = Outbound::Close {
                                    code: CLOSE_UNSUPPORTED_DATA,
                                    reason: "binary messages are not supported".to_owned(),
                                };
                                send(&mut outgoing, close).await?;
                                return Ok(());
                            }
                        },
                        Inbound::Close => return Ok(()),
                    };
                    match parse_client_message(&text, &config) {
                        Err(e) => {
                            send(&mut outgoing, Outbound::Text(error_text(&*handler, e)?)).await?;
                        }
                        Ok(ClientMessage::Request(WsRequest { id, request })) => {
                            match admit(&in_flight, &config, shared_limit.as_ref()) {
                                Ok(permit) => {
                                    let h = Arc::clone(&handler);
                                    in_flight.push(Box::pin(async move {
                                        let result = h.handle(request).await;
                                        (id, result, permit)
                                    }));
                                }
                                Err(()) => {
                                    let err = JmapError::limit("maxConcurrentRequests");
                                    let text = error_text(&*handler, request_error(&err, id))?;
                                    send(&mut outgoing, Outbound::Text(text)).await?;
                                }
                            }
                        }
                        Ok(ClientMessage::PushEnable(enable)) => {
                            let stream = push.subscribe(&enable);
                            subscription = Some((stream, enable.data_types));
                        }
                        Ok(ClientMessage::PushDisable(_)) => subscription = None,
                    }
                }
            }
        }
    }
}

/// A slot for one more Request, or `Err` at the limit.
fn admit(
    in_flight: &FuturesUnordered<InFlight>,
    config: &ConnectionConfig,
    shared: Option<&Arc<Semaphore>>,
) -> Result<Option<OwnedSemaphorePermit>, ()> {
    if in_flight.len() >= config.max_concurrent_requests {
        return Err(());
    }
    match shared {
        None => Ok(None),
        Some(s) => Arc::clone(s).try_acquire_owned().map(Some).map_err(|_| ()),
    }
}

/// The next push from the subscription; pending forever without one.
async fn next_change(
    subscription: &mut Option<(BoxStream<'static, WsStateChange>, Option<Vec<String>>)>,
) -> Option<WsStateChange> {
    match subscription {
        Some((stream, _)) => stream.next().await,
        None => std::future::pending().await,
    }
}

fn to_text<T: serde::Serialize>(message: &T) -> Result<String, ConnectionError> {
    serde_json::to_string(message).map_err(ConnectionError::Serialize)
}

fn error_text<H: RequestHandler>(
    handler: &H,
    mut error: WsRequestError,
) -> Result<String, ConnectionError> {
    handler.prepare_problem(&mut error.problem);
    to_text(&error)
}

async fn send<K>(outgoing: &mut K, message: Outbound) -> Result<(), ConnectionError>
where
    K: Sink<Outbound> + Unpin,
    K::Error: std::error::Error + Send + Sync + 'static,
{
    outgoing
        .send(message)
        .await
        .map_err(|e| ConnectionError::Send(Box::new(e)))
}

#[cfg(test)]
mod tests;
