//! The RFC 8887 §4.3 WebSocket messages.
//!
//! Every message is one JSON object whose `@type` names it. Serialize
//! always writes `@type`; deserializing one message type directly rejects
//! any other `@type` value and accepts its absence (the jmap-types
//! `StateChange` convention). [`ClientMessage`] and [`ServerMessage`]
//! pick the type from `@type`, so there it is required.

use serde::de::{self, DeserializeOwned};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use jmap_types::problem::ProblemDetails;
use jmap_types::{JmapRequest, JmapResponse, StateChange};

const REQUEST: &str = "Request";
const RESPONSE: &str = "Response";
const REQUEST_ERROR: &str = "RequestError";
const STATE_CHANGE: &str = "StateChange";
const PUSH_ENABLE: &str = "WebSocketPushEnable";
const PUSH_DISABLE: &str = "WebSocketPushDisable";

/// Checks a deserialized `@type`: absent is accepted, any value other
/// than `expected` is an error.
fn check_type<E: de::Error>(found: Option<String>, expected: &'static str) -> Result<(), E> {
    match found {
        None => Ok(()),
        Some(t) if t == expected => Ok(()),
        Some(t) => Err(E::invalid_value(de::Unexpected::Str(&t), &expected)),
    }
}

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

/// A JMAP Request sent over the WebSocket (RFC 8887 §4.3.2): an RFC 8620
/// §3.3 Request plus `@type: "Request"` and an optional client `id`.
///
/// Unknown members land in the inner [`JmapRequest::extra`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct WsRequest {
    /// `id`: "A client-specified identifier for the request to be echoed
    /// back in the response to this request."
    pub id: Option<String>,
    /// The RFC 8620 Request (`using`, `methodCalls`, `createdIds`).
    pub request: JmapRequest,
}

impl WsRequest {
    /// A request frame for `request` with correlation id `id`.
    pub fn new(request: JmapRequest, id: Option<String>) -> Self {
        Self { id, request }
    }
}

#[derive(Serialize)]
struct RequestOut<'a> {
    #[serde(rename = "@type")]
    ty: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a str>,
    #[serde(flatten)]
    request: &'a JmapRequest,
}

#[derive(Deserialize)]
struct RequestIn {
    #[serde(rename = "@type", default)]
    ty: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(flatten)]
    request: JmapRequest,
}

impl Serialize for WsRequest {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RequestOut {
            ty: REQUEST,
            id: self.id.as_deref(),
            request: &self.request,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for WsRequest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = RequestIn::deserialize(d)?;
        check_type(r.ty, REQUEST)?;
        Ok(Self {
            id: r.id,
            request: r.request,
        })
    }
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

/// A JMAP Response sent over the WebSocket (RFC 8887 §4.3.3): an RFC 8620
/// §3.4 Response plus `@type: "Response"` and `requestId`.
///
/// `requestId` "MUST be returned if an identifier is included in the
/// request"; it is omitted on the wire when `None`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct WsResponse {
    /// `requestId`: the `id` of the [`WsRequest`] this answers.
    pub request_id: Option<String>,
    /// The RFC 8620 Response.
    pub response: JmapResponse,
}

impl WsResponse {
    /// A response frame answering the request with id `request_id`.
    pub fn new(response: JmapResponse, request_id: Option<String>) -> Self {
        Self {
            request_id,
            response,
        }
    }
}

#[derive(Serialize)]
struct ResponseOut<'a> {
    #[serde(rename = "@type")]
    ty: &'static str,
    #[serde(rename = "requestId", skip_serializing_if = "Option::is_none")]
    request_id: Option<&'a str>,
    #[serde(flatten)]
    response: &'a JmapResponse,
}

#[derive(Deserialize)]
struct ResponseIn {
    #[serde(rename = "@type", default)]
    ty: Option<String>,
    #[serde(rename = "requestId", default)]
    request_id: Option<String>,
    #[serde(flatten)]
    response: JmapResponse,
}

impl Serialize for WsResponse {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ResponseOut {
            ty: RESPONSE,
            request_id: self.request_id.as_deref(),
            response: &self.response,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for WsResponse {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = ResponseIn::deserialize(d)?;
        check_type(r.ty, RESPONSE)?;
        Ok(Self {
            request_id: r.request_id,
            response: r.response,
        })
    }
}

// ---------------------------------------------------------------------------
// RequestError
// ---------------------------------------------------------------------------

/// A request-level error sent over the WebSocket (RFC 8887 §4.3.4): an
/// RFC 8620 §3.6.1 problem details object plus `@type: "RequestError"`
/// and `requestId`.
///
/// `requestId` is always written, as `null` when `None`, the way the
/// RFC 8887 §4.4 `notJSON` example writes it for a message the server
/// could not read an id from. It "MUST be returned if given in the
/// request".
///
/// ```
/// use jmap_websocket::WsRequestError;
/// use jmap_websocket::jmap_types::problem::ProblemDetails;
///
/// let e = WsRequestError::new(
///     ProblemDetails::new("urn:ietf:params:jmap:error:notJSON", 400),
///     None,
/// );
/// let v = serde_json::to_value(&e).unwrap();
/// assert_eq!(v["@type"], "RequestError");
/// assert!(v["requestId"].is_null());
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct WsRequestError {
    /// `requestId`: the `id` of the request this error answers.
    pub request_id: Option<String>,
    /// The problem details (`type`, `status`, `title`, `detail`, `limit`,
    /// extension members).
    pub problem: ProblemDetails,
}

impl WsRequestError {
    /// An error frame carrying `problem` for the request with id
    /// `request_id`.
    pub fn new(problem: ProblemDetails, request_id: Option<String>) -> Self {
        Self {
            request_id,
            problem,
        }
    }
}

#[derive(Serialize)]
struct RequestErrorOut<'a> {
    #[serde(rename = "@type")]
    ty: &'static str,
    #[serde(rename = "requestId")]
    request_id: Option<&'a str>,
    #[serde(flatten)]
    problem: &'a ProblemDetails,
}

#[derive(Deserialize)]
struct RequestErrorIn {
    #[serde(rename = "@type", default)]
    ty: Option<String>,
    #[serde(rename = "requestId", default)]
    request_id: Option<String>,
    #[serde(flatten)]
    problem: ProblemDetails,
}

impl Serialize for WsRequestError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RequestErrorOut {
            ty: REQUEST_ERROR,
            request_id: self.request_id.as_deref(),
            problem: &self.problem,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for WsRequestError {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = RequestErrorIn::deserialize(d)?;
        check_type(r.ty, REQUEST_ERROR)?;
        Ok(Self {
            request_id: r.request_id,
            problem: r.problem,
        })
    }
}

// ---------------------------------------------------------------------------
// StateChange
// ---------------------------------------------------------------------------

/// A push notification sent over the WebSocket (RFC 8887 §4.3.5.1): an
/// RFC 8620 §7.1 [`StateChange`] plus an optional `pushState`.
///
/// `pushState` "encodes the entire server state visible to the user"; a
/// client echoes the last one it got in [`WebSocketPushEnable`] after a
/// reconnect. Omitted on the wire when `None`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsStateChange {
    /// `pushState`.
    pub push_state: Option<String>,
    /// The RFC 8620 [`StateChange`] (`changed` and extension members).
    pub state_change: StateChange,
}

impl WsStateChange {
    /// A push frame for `state_change` with token `push_state`.
    pub fn new(state_change: StateChange, push_state: Option<String>) -> Self {
        Self {
            push_state,
            state_change,
        }
    }
}

#[derive(Serialize)]
struct StateChangeOut<'a> {
    // StateChange's own Serialize writes `@type` first.
    #[serde(flatten)]
    state_change: &'a StateChange,
    #[serde(rename = "pushState", skip_serializing_if = "Option::is_none")]
    push_state: Option<&'a str>,
}

#[derive(Deserialize)]
struct StateChangeIn {
    #[serde(rename = "pushState", default)]
    push_state: Option<String>,
    // StateChange's own Deserialize checks and drops `@type`.
    #[serde(flatten)]
    state_change: StateChange,
}

impl Serialize for WsStateChange {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        StateChangeOut {
            state_change: &self.state_change,
            push_state: self.push_state.as_deref(),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for WsStateChange {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = StateChangeIn::deserialize(d)?;
        Ok(Self {
            push_state: r.push_state,
            state_change: r.state_change,
        })
    }
}

// ---------------------------------------------------------------------------
// WebSocketPushEnable / WebSocketPushDisable
// ---------------------------------------------------------------------------

/// Turns on push notifications for the connection (RFC 8887 §4.3.5.2).
///
/// `dataTypes` is `String[]|null` and always written; `null` asks for
/// every supported data type. A missing `dataTypes` reads as `null`.
/// Sending it again replaces the earlier subscription.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WebSocketPushEnable {
    /// `dataTypes`: the data type names whose changes the client wants;
    /// "Other types are omitted from the `TypeState` object." `None` is
    /// every type.
    pub data_types: Option<Vec<String>>,
    /// `pushState`: the last token the client got; "the server SHOULD
    /// immediately send all changes since that state token".
    pub push_state: Option<String>,
    /// Vendor, site and future-spec members.
    pub extra: Map<String, Value>,
}

impl WebSocketPushEnable {
    /// A push-enable message for `data_types` (`None`: all types),
    /// resuming from `push_state`.
    pub fn new(data_types: Option<Vec<String>>, push_state: Option<String>) -> Self {
        Self {
            data_types,
            push_state,
            extra: Map::new(),
        }
    }
}

#[derive(Serialize)]
struct PushEnableOut<'a> {
    #[serde(rename = "@type")]
    ty: &'static str,
    #[serde(rename = "dataTypes")]
    data_types: Option<&'a [String]>,
    #[serde(rename = "pushState", skip_serializing_if = "Option::is_none")]
    push_state: Option<&'a str>,
    #[serde(flatten)]
    extra: &'a Map<String, Value>,
}

#[derive(Deserialize)]
struct PushEnableIn {
    #[serde(rename = "@type", default)]
    ty: Option<String>,
    #[serde(rename = "dataTypes", default)]
    data_types: Option<Vec<String>>,
    #[serde(rename = "pushState", default)]
    push_state: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl Serialize for WebSocketPushEnable {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        PushEnableOut {
            ty: PUSH_ENABLE,
            data_types: self.data_types.as_deref(),
            push_state: self.push_state.as_deref(),
            extra: &self.extra,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for WebSocketPushEnable {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = PushEnableIn::deserialize(d)?;
        check_type(r.ty, PUSH_ENABLE)?;
        Ok(Self {
            data_types: r.data_types,
            push_state: r.push_state,
            extra: r.extra,
        })
    }
}

/// Turns off push notifications for the connection (RFC 8887 §4.3.5.3).
/// Its only RFC property is `@type`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WebSocketPushDisable {
    /// Vendor, site and future-spec members.
    pub extra: Map<String, Value>,
}

impl WebSocketPushDisable {
    /// A push-disable message with no extension members.
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Serialize)]
struct PushDisableOut<'a> {
    #[serde(rename = "@type")]
    ty: &'static str,
    #[serde(flatten)]
    extra: &'a Map<String, Value>,
}

#[derive(Deserialize)]
struct PushDisableIn {
    #[serde(rename = "@type", default)]
    ty: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl Serialize for WebSocketPushDisable {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        PushDisableOut {
            ty: PUSH_DISABLE,
            extra: &self.extra,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for WebSocketPushDisable {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = PushDisableIn::deserialize(d)?;
        check_type(r.ty, PUSH_DISABLE)?;
        Ok(Self { extra: r.extra })
    }
}

// ---------------------------------------------------------------------------
// Message enums
// ---------------------------------------------------------------------------

/// Reads `@type` from a message object and deserializes the whole object
/// as `T`.
fn from_object<T: DeserializeOwned, E: de::Error>(obj: Map<String, Value>) -> Result<T, E> {
    serde_json::from_value(Value::Object(obj)).map_err(E::custom)
}

fn take_tag<E: de::Error>(obj: &Map<String, Value>) -> Result<String, E> {
    match obj.get("@type") {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(E::custom("`@type` is not a string")),
        None => Err(E::missing_field("@type")),
    }
}

/// A message a client sends (RFC 8887 §4.3): "a single JMAP Request
/// object ..., JMAP [`WebSocketPushEnable`] object ..., or JMAP
/// [`WebSocketPushDisable`] object".
///
/// Deserialize requires `@type` and fails on any other value; a server
/// that must answer bad input with the right problem type uses
/// `server::parse_client_message` (feature `server`) instead.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    /// `@type: "Request"`.
    Request(WsRequest),
    /// `@type: "WebSocketPushEnable"`.
    PushEnable(WebSocketPushEnable),
    /// `@type: "WebSocketPushDisable"`.
    PushDisable(WebSocketPushDisable),
}

impl Serialize for ClientMessage {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Request(m) => m.serialize(s),
            Self::PushEnable(m) => m.serialize(s),
            Self::PushDisable(m) => m.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for ClientMessage {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let obj = Map::deserialize(d)?;
        match take_tag::<D::Error>(&obj)?.as_str() {
            REQUEST => from_object(obj).map(Self::Request),
            PUSH_ENABLE => from_object(obj).map(Self::PushEnable),
            PUSH_DISABLE => from_object(obj).map(Self::PushDisable),
            other => Err(de::Error::unknown_variant(
                other,
                &[REQUEST, PUSH_ENABLE, PUSH_DISABLE],
            )),
        }
    }
}

/// A message a server sends (RFC 8887 §4.3): "a single JMAP Response
/// object, JSON Problem Details object, or JMAP [`StateChange`] object".
///
/// Deserialize requires `@type` and fails on any other value; RFC 8887
/// §4.3.1 lets a client ignore such a message or close the connection.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    /// `@type: "Response"`.
    Response(WsResponse),
    /// `@type: "RequestError"`.
    RequestError(WsRequestError),
    /// `@type: "StateChange"`.
    StateChange(WsStateChange),
}

impl Serialize for ServerMessage {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Response(m) => m.serialize(s),
            Self::RequestError(m) => m.serialize(s),
            Self::StateChange(m) => m.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for ServerMessage {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let obj = Map::deserialize(d)?;
        match take_tag::<D::Error>(&obj)?.as_str() {
            RESPONSE => from_object(obj).map(Self::Response),
            REQUEST_ERROR => from_object(obj).map(Self::RequestError),
            STATE_CHANGE => from_object(obj).map(Self::StateChange),
            other => Err(de::Error::unknown_variant(
                other,
                &[RESPONSE, REQUEST_ERROR, STATE_CHANGE],
            )),
        }
    }
}

#[cfg(test)]
mod tests;
