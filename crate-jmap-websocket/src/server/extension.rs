//! Extension message types: [`ExtensionHandler`] and its parts.
//!
//! RFC 8887 §4.3 closes the set of message types. The JMAP Chat, VTC and
//! Scene WebSocket drafts update it: "A JMAP capability advertised in the
//! Session object ... MAY define additional WebSocket message types, each
//! identified by its `@type` value", and "A server MUST NOT send an
//! additional server-to-client message type unless the client has enabled
//! it using a control message defined by the same capability"
//! (draft-atwood-jmap-chat-wss-00 §Update to RFC 8887; the VTC and Scene
//! drafts restate it). This module is that hook, with no knowledge of any
//! one extension.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use futures::future::BoxFuture;
use jmap_types::JmapError;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use tokio::sync::mpsc;

/// The `@type` values RFC 8887 §4.3 defines. The driver handles the
/// client-to-server ones itself, and an extension may not send the
/// server-to-client ones.
const CORE_TYPES: [&str; 6] = [
    "Request",
    "WebSocketPushEnable",
    "WebSocketPushDisable",
    "Response",
    "RequestError",
    "StateChange",
];

/// Problem-details members the driver writes itself; an
/// [`ExtensionHandler`] error's `extra` members with these names are
/// dropped so they cannot appear twice.
const RESERVED_PROBLEM_MEMBERS: [&str; 8] = [
    "@type",
    "requestId",
    "type",
    "status",
    "title",
    "detail",
    "instance",
    "limit",
];

/// One client message whose `@type` an [`ExtensionHandler`] registered.
///
/// ```
/// use jmap_websocket::server::ExtensionMessage;
///
/// #[derive(serde::Deserialize)]
/// #[serde(rename_all = "camelCase")]
/// struct ToyEnable {
///     topics: Vec<String>,
/// }
///
/// let m = ExtensionMessage::new(
///     "ToyStreamEnable",
///     serde_json::from_str(r#"{"id":"e1","topics":["a"]}"#).unwrap(),
/// );
/// assert_eq!(m.id.as_deref(), Some("e1"));
/// let enable: ToyEnable = m.deserialize().unwrap();
/// assert_eq!(enable.topics, ["a"]);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionMessage {
    /// The message's `@type`.
    pub message_type: String,
    /// The message's `id` when it is a string: the `requestId` of a
    /// `RequestError` the message causes (RFC 8887 §4.3.4). It also stays
    /// in [`body`](Self::body).
    pub id: Option<String>,
    /// Every member of the message except `@type`.
    pub body: Map<String, Value>,
}

impl ExtensionMessage {
    /// A message of type `message_type` with the members `body` (without
    /// `@type`); [`id`](Self::id) is taken from `body`.
    pub fn new(message_type: impl Into<String>, body: Map<String, Value>) -> Self {
        let id = body.get("id").and_then(Value::as_str).map(str::to_owned);
        Self {
            message_type: message_type.into(),
            id,
            body,
        }
    }

    /// The body as a `T`, typically the extension's own frame type.
    ///
    /// # Errors
    ///
    /// A `notRequest` error when the body does not deserialize, as the
    /// driver answers a malformed `WebSocketPushEnable`. Return it from
    /// [`ExtensionHandler::handle`] to send it to the client.
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, JmapError> {
        serde_json::from_value(Value::Object(self.body.clone())).map_err(|e| {
            let mut err = JmapError::not_request();
            err.description = Some(format!("Malformed {}: {e}", self.message_type));
            err
        })
    }
}

/// Why an [`ExtensionSender`] did not queue a message.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ExtensionSendError {
    /// The queue is full ([`ExtensionSender::try_send`] only). The
    /// message was dropped.
    #[error("the connection's extension message queue is full")]
    Full,
    /// The connection has ended. The message was dropped.
    #[error("the connection has ended")]
    Closed,
    /// The message is not a JSON object with a string `@type`, or its
    /// `@type` is one RFC 8887 §4.3 defines (`Response`, `RequestError`,
    /// `StateChange`, and the client-to-server types).
    #[error("an extension message must be a JSON object whose @type is not an RFC 8887 type")]
    NotAnExtensionMessage,
    /// The message did not serialize.
    #[error("serializing an extension message failed")]
    Serialize(#[source] serde_json::Error),
}

/// Queues server-to-client messages of one connection's extensions.
///
/// The driver writes them in the order they were queued, interleaved
/// with Responses and `StateChange`s in no set order (the drafts guarantee
/// none: "No ordering relationship is guaranteed between ephemeral event
/// frames and `Response` or `StateChange` frames",
/// draft-atwood-jmap-chat-wss-00 §Ephemeral Chat Events). The queue is
/// bounded by [`ConnectionConfig::extension_queue_capacity`](super::ConnectionConfig::extension_queue_capacity).
///
/// Clone it freely, for example into a task that forwards events from a
/// broadcast channel. Once the connection ends every send fails with
/// [`ExtensionSendError::Closed`], and [`closed`](Self::closed)
/// resolves, so such a task can stop; messages still queued then are
/// dropped.
#[derive(Debug, Clone)]
pub struct ExtensionSender {
    tx: mpsc::Sender<String>,
}

impl ExtensionSender {
    pub(super) fn new(tx: mpsc::Sender<String>) -> Self {
        Self { tx }
    }

    /// Queue `message`, waiting while the queue is full.
    ///
    /// Awaiting it in [`ExtensionHandler::handle`] cannot deadlock (the
    /// driver keeps writing while `handle` runs), but with a full queue
    /// `handle`, and so the next client message, waits on a slow client.
    ///
    /// # Errors
    ///
    /// [`ExtensionSendError`] other than `Full`.
    pub async fn send<T: Serialize + ?Sized>(&self, message: &T) -> Result<(), ExtensionSendError> {
        let text = extension_text(message)?;
        self.tx
            .send(text)
            .await
            .map_err(|_| ExtensionSendError::Closed)
    }

    /// Queue `message` now, or drop it if the queue is full. Suits
    /// ephemeral events, which the drafts let a server drop under load
    /// ("events above this rate MAY be silently dropped").
    ///
    /// # Errors
    ///
    /// [`ExtensionSendError`].
    pub fn try_send<T: Serialize + ?Sized>(&self, message: &T) -> Result<(), ExtensionSendError> {
        let text = extension_text(message)?;
        self.tx.try_send(text).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => ExtensionSendError::Full,
            mpsc::error::TrySendError::Closed(_) => ExtensionSendError::Closed,
        })
    }

    /// Whether the connection has ended.
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    /// Resolves when the connection has ended.
    pub async fn closed(&self) {
        self.tx.closed().await;
    }
}

/// Serializes `message` after checking it is an extension message.
fn extension_text<T: Serialize + ?Sized>(message: &T) -> Result<String, ExtensionSendError> {
    let value = serde_json::to_value(message).map_err(ExtensionSendError::Serialize)?;
    match value.get("@type").and_then(Value::as_str) {
        Some(ty) if !CORE_TYPES.contains(&ty) => {}
        _ => return Err(ExtensionSendError::NotAnExtensionMessage),
    }
    serde_json::to_string(&value).map_err(ExtensionSendError::Serialize)
}

/// Handles the client messages of one or more extension `@type`s on one
/// connection, and sends that extension's server-to-client messages.
///
/// Register it with
/// [`Connection::with_extension`](super::Connection::with_extension), one
/// value per connection, when the Session advertises the capability that
/// defines the types (for example `urn:ietf:params:jmap:chat:websocket`
/// for `ChatStreamEnable` and `ChatStreamDisable`). The drafts tie the
/// types to the advertised capability, not to a Request's `using`, so the
/// driver does no capability check of its own. A message whose `@type` no
/// handler registered is answered with `notRequest`, as before
/// ("A client-to-server message with an unrecognized `@type` value is a
/// request-level error", draft-atwood-jmap-chat-wss-00 §Handling Unknown
/// Message Types).
///
/// The value is the connection's extension state: build it with the
/// connection's authenticated caller, keep the subscription in it, and
/// use `&mut self` freely. The driver calls, in order:
///
/// 1. [`start`](Self::start) once, before reading any message, with the
///    [`ExtensionSender`] for server-to-client messages.
/// 2. [`handle`](Self::handle) for each registered message, one at a
///    time in the order the client sent them. Until `handle` returns the
///    driver reads no further client message, so a Request sent after a
///    `…StreamEnable` runs after the subscription is in place; Responses,
///    `StateChange`s and queued extension messages keep flowing meanwhile.
///    Keep it short: start long work on a task of your own.
/// 3. [`close`](Self::close) once when [`Connection::run`](super::Connection::run)
///    ends, however it ends, after the connection's queue is closed and
///    any running `handle` finished. Release per-connection resources
///    here (the drafts cancel the subscription "immediately" when the
///    connection closes; a Scene observer stops counting toward
///    `observerCount`). It is not called if the `run` future itself is
///    dropped; put cleanup that must run then in `Drop`.
///
/// To let one extension's control message turn on another's events (a
/// `ChatStreamEnable` with `"vtc"` in `dataTypes`), register one handler
/// for all the types involved.
///
/// ```
/// use jmap_websocket::jmap_types::JmapError;
/// use jmap_websocket::server::{ExtensionHandler, ExtensionMessage, ExtensionSender};
/// use serde_json::json;
///
/// /// `ToyStreamEnable` turns on `ToyEvent`s; `ToyStreamDisable` turns
/// /// them off.
/// #[derive(Default)]
/// struct Toy {
///     events: Option<ExtensionSender>,
///     enabled: bool,
/// }
///
/// impl ExtensionHandler for Toy {
///     fn message_types(&self) -> &[&str] {
///         &["ToyStreamEnable", "ToyStreamDisable"]
///     }
///
///     fn start(&mut self, events: ExtensionSender) {
///         self.events = Some(events);
///     }
///
///     async fn handle(&mut self, message: ExtensionMessage) -> Result<(), JmapError> {
///         match message.message_type.as_str() {
///             "ToyStreamEnable" => self.enabled = true,
///             _ => self.enabled = false,
///         }
///         if self.enabled
///             && let Some(events) = &self.events
///         {
///             // An ephemeral event: drop it rather than wait if the
///             // client is slow.
///             let _ = events.try_send(&json!({"@type": "ToyEvent", "hello": true}));
///         }
///         Ok(())
///     }
/// }
/// ```
pub trait ExtensionHandler: Send + 'static {
    /// The client-to-server `@type`s this handler takes. Read once, by
    /// [`Connection::with_extension`](super::Connection::with_extension).
    fn message_types(&self) -> &[&str];

    /// Called once before the first message, with the queue for this
    /// connection's server-to-client extension messages. The default
    /// drops it.
    fn start(&mut self, events: ExtensionSender) {
        let _ = events;
    }

    /// Process one message. Success sends nothing (the drafts' control
    /// messages are not acknowledged: "success is silent and failure
    /// produces a `RequestError`"). `Err` is sent as an RFC 8887 §4.3.4
    /// `RequestError` with the message's [`id`](ExtensionMessage::id) as
    /// `requestId`, through
    /// [`RequestHandler::prepare_problem`](super::RequestHandler::prepare_problem);
    /// its `extra` members become problem-details members (for example
    /// the Scene draft's `regionIds` on a `forbidden` error).
    fn handle(
        &mut self,
        message: ExtensionMessage,
    ) -> impl Future<Output = Result<(), JmapError>> + Send;

    /// Called once when the connection ends. The default does nothing.
    fn close(&mut self) -> impl Future<Output = ()> + Send {
        async {}
    }
}

/// [`ExtensionHandler`] made object safe, so a connection can hold
/// handlers of different types.
trait ErasedHandler: Send {
    fn start(&mut self, events: ExtensionSender);
    fn handle(&mut self, message: ExtensionMessage) -> BoxFuture<'_, Result<(), JmapError>>;
    fn close(&mut self) -> BoxFuture<'_, ()>;
}

impl<T: ExtensionHandler> ErasedHandler for T {
    fn start(&mut self, events: ExtensionSender) {
        ExtensionHandler::start(self, events);
    }

    fn handle(&mut self, message: ExtensionMessage) -> BoxFuture<'_, Result<(), JmapError>> {
        Box::pin(ExtensionHandler::handle(self, message))
    }

    fn close(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(ExtensionHandler::close(self))
    }
}

/// What a finished [`ExtensionHandler::handle`] gives back: the handler's
/// slot, the handler, the message id, the result.
type Handled = (
    usize,
    Box<dyn ErasedHandler>,
    Option<String>,
    Result<(), JmapError>,
);

/// The extension handlers of one connection, and the one message being
/// handled.
#[derive(Default)]
pub(super) struct Extensions {
    /// `None` while that handler's `handle` runs.
    handlers: Vec<Option<Box<dyn ErasedHandler>>>,
    by_type: HashMap<String, usize>,
    in_flight: Option<Pin<Box<dyn Future<Output = Handled> + Send>>>,
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut types: Vec<&str> = self.by_type.keys().map(String::as_str).collect();
        types.sort_unstable();
        f.debug_list().entries(types).finish()
    }
}

impl Extensions {
    /// Adds `handler` under its message types.
    ///
    /// # Panics
    ///
    /// If a type is an RFC 8887 type or another handler's.
    pub(super) fn register<X: ExtensionHandler>(&mut self, handler: X) {
        let index = self.handlers.len();
        for &ty in handler.message_types() {
            assert!(
                !CORE_TYPES.contains(&ty),
                "extension message type {ty:?} is an RFC 8887 type"
            );
            let previous = self.by_type.insert(ty.to_owned(), index);
            assert!(
                previous.is_none_or(|p| p == index),
                "extension message type {ty:?} is registered twice"
            );
        }
        self.handlers.push(Some(Box::new(handler)));
    }

    /// Whether a handler takes messages of type `ty`.
    pub(super) fn handles(&self, ty: &str) -> bool {
        self.by_type.contains_key(ty)
    }

    /// Whether a message is being handled.
    pub(super) fn is_busy(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Calls every handler's `start` with a sender on a new queue of
    /// `capacity` messages, and returns its receiver; `None` when there
    /// are no handlers.
    pub(super) fn start(&mut self, capacity: usize) -> Option<mpsc::Receiver<String>> {
        if self.handlers.is_empty() {
            return None;
        }
        let (tx, rx) = mpsc::channel(capacity);
        for handler in self.handlers.iter_mut().flatten() {
            handler.start(ExtensionSender::new(tx.clone()));
        }
        Some(rx)
    }

    /// Starts handling `message`. The driver calls this only when not
    /// [`is_busy`](Self::is_busy) and for a type [`handles`](Self::handles)
    /// accepts.
    pub(super) fn dispatch(&mut self, message: ExtensionMessage) {
        let index = self.by_type[&message.message_type];
        let mut handler = self.handlers[index]
            .take()
            .expect("handlers run one message at a time");
        let id = message.id.clone();
        self.in_flight = Some(Box::pin(async move {
            let result = handler.handle(message).await;
            (index, handler, id, result)
        }));
    }

    /// The outcome of the message being handled, `(id, result)`; pending
    /// forever when none is. Cancel safe: dropping it leaves the message
    /// running.
    pub(super) async fn next_done(&mut self) -> (Option<String>, Result<(), JmapError>) {
        let Some(in_flight) = self.in_flight.as_mut() else {
            return std::future::pending().await;
        };
        let (index, handler, id, result) = in_flight.await;
        self.in_flight = None;
        self.handlers[index] = Some(handler);
        (id, result)
    }

    /// Finishes the message being handled, then calls every handler's
    /// `close`. Close the queue first, so a `send` waiting in it returns.
    pub(super) async fn close(&mut self) {
        if self.in_flight.is_some() {
            let _ = self.next_done().await;
        }
        for handler in self.handlers.iter_mut().flatten() {
            handler.close().await;
        }
    }
}

/// The problem details for an [`ExtensionHandler`] error: as for a
/// Request ([`request_error`](super::request_error)), plus the error's
/// `extra` members.
pub(super) fn extension_error(
    err: &JmapError,
    request_id: Option<String>,
) -> crate::WsRequestError {
    let mut error = super::request_error(err, request_id);
    for (name, value) in &err.extra {
        if !RESERVED_PROBLEM_MEMBERS.contains(&name.as_str()) {
            error.problem.extra.insert(name.clone(), value.clone());
        }
    }
    error
}
