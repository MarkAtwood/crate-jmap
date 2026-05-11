//! [`ClientError`] and the opaque wrapper types ([`HttpError`],
//! [`WebSocketError`], [`InvalidHeaderValueError`]) that hide
//! [`reqwest`] and [`tokio_tungstenite`] from this crate's public API.
//!
//! # SemVer policy
//!
//! `reqwest` and `tokio-tungstenite` are **private dependencies** of this
//! crate. Their types do not appear in any public function signature,
//! variant payload, or `From` impl. The wrapper types ([`HttpError`],
//! [`WebSocketError`], [`InvalidHeaderValueError`]) expose a curated set of
//! diagnostic accessors that return primitive types only, so this crate can
//! bump the underlying transport's major version without breaking
//! downstream callers.
//!
//! Internal construction goes through `pub(crate)` helpers on
//! [`ClientError`] (`from_reqwest`, `from_ws`, `from_invalid_header`) —
//! downstream consumers cannot construct the transport-error variants and
//! never need to.

use std::error::Error as StdError;
use std::fmt;

// ---------------------------------------------------------------------------
// HttpError — opaque wrapper around reqwest::Error
// ---------------------------------------------------------------------------

/// HTTP transport error reported by the underlying HTTP client.
///
/// The inner third-party error type is private; callers diagnose the failure
/// via the accessor methods, all of which return primitive types so this
/// crate can swap or bump the underlying HTTP client without breaking the
/// public API.
#[non_exhaustive]
pub struct HttpError(reqwest::Error);

impl HttpError {
    /// `true` if the request timed out before a response was received.
    pub fn is_timeout(&self) -> bool {
        self.0.is_timeout()
    }
    /// `true` if the underlying connection could not be established
    /// (DNS failure, TCP refused, TLS handshake failure, etc.).
    pub fn is_connect(&self) -> bool {
        self.0.is_connect()
    }
    /// `true` if the error originated in the request builder
    /// (URL parse failure, invalid header construction at build time, etc.).
    pub fn is_builder(&self) -> bool {
        self.0.is_builder()
    }
    /// `true` if the error is a redirect-loop or too-many-redirects failure.
    pub fn is_redirect(&self) -> bool {
        self.0.is_redirect()
    }
    /// `true` if the error originated from a non-success HTTP status.
    pub fn is_status(&self) -> bool {
        self.0.is_status()
    }
    /// `true` if the error happened while sending the request body.
    pub fn is_request(&self) -> bool {
        self.0.is_request()
    }
    /// `true` if the error happened while receiving / decoding the response body.
    pub fn is_body(&self) -> bool {
        self.0.is_body()
    }
    /// `true` if the response body could not be decoded as the requested
    /// representation (e.g. JSON parse failure inside the transport layer).
    pub fn is_decode(&self) -> bool {
        self.0.is_decode()
    }
    /// HTTP status code if the error came from a non-success response;
    /// `None` for transport-level failures (timeout, connection refused, etc.).
    pub fn status(&self) -> Option<u16> {
        self.0.status().map(|s| s.as_u16())
    }
    /// URL the request was sent to, if known. Returned as an owned `String`
    /// to avoid leaking the underlying transport's `Url` type into this
    /// crate's public API.
    pub fn url(&self) -> Option<String> {
        self.0.url().map(ToString::to_string)
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("HttpError").field(&self.0).finish()
    }
}

impl StdError for HttpError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.0)
    }
}

// ---------------------------------------------------------------------------
// WebSocketError — opaque wrapper around tokio_tungstenite::tungstenite::Error
// ---------------------------------------------------------------------------

/// WebSocket transport error reported by the underlying WebSocket client.
///
/// As with [`HttpError`], the inner third-party type is private and
/// diagnostics are exposed via accessor methods returning primitive types.
#[non_exhaustive]
pub struct WebSocketError(tokio_tungstenite::tungstenite::Error);

impl WebSocketError {
    /// `true` if the peer cleanly closed the connection.
    pub fn is_connection_closed(&self) -> bool {
        matches!(
            &self.0,
            tokio_tungstenite::tungstenite::Error::ConnectionClosed
        )
    }
    /// `true` if the connection was already closed when the operation was
    /// attempted (caller bug or race).
    pub fn is_already_closed(&self) -> bool {
        matches!(
            &self.0,
            tokio_tungstenite::tungstenite::Error::AlreadyClosed
        )
    }
    /// `true` if the error wraps an underlying `std::io::Error`.
    pub fn is_io(&self) -> bool {
        matches!(&self.0, tokio_tungstenite::tungstenite::Error::Io(_))
    }
    /// `true` if the error is a WebSocket protocol violation
    /// (malformed frame, invalid opcode, etc.).
    pub fn is_protocol(&self) -> bool {
        matches!(&self.0, tokio_tungstenite::tungstenite::Error::Protocol(_))
    }
    /// `true` if a frame or message exceeded a configured size limit.
    pub fn is_capacity(&self) -> bool {
        matches!(&self.0, tokio_tungstenite::tungstenite::Error::Capacity(_))
    }
    /// `true` if the WebSocket URL was invalid.
    pub fn is_url(&self) -> bool {
        matches!(&self.0, tokio_tungstenite::tungstenite::Error::Url(_))
    }
}

impl fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("WebSocketError").field(&self.0).finish()
    }
}

impl StdError for WebSocketError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.0)
    }
}

// ---------------------------------------------------------------------------
// InvalidHeaderValueError — string-only wrapper, no third-party leak
// ---------------------------------------------------------------------------

/// A header value (typically an authentication token) contained bytes that
/// are not valid for an HTTP header.
///
/// The inner type is just a string message; there is no actionable
/// diagnostic state beyond that, so this wrapper does not expose any
/// accessor beyond [`Display`](fmt::Display).
#[non_exhaustive]
pub struct InvalidHeaderValueError {
    message: String,
}

impl InvalidHeaderValueError {
    /// The human-readable description of the failure.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InvalidHeaderValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl fmt::Debug for InvalidHeaderValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InvalidHeaderValueError")
            .field("message", &self.message)
            .finish()
    }
}

impl StdError for InvalidHeaderValueError {}

// ---------------------------------------------------------------------------
// ClientError
// ---------------------------------------------------------------------------

/// Errors produced by the base JMAP client.
///
/// Variants cover transport failures (`Http`, `WebSocket`), authentication
/// (`AuthFailed`), JMAP protocol errors (`MethodError`, `UnexpectedResponse`),
/// caller bugs (`InvalidArgument`, `InvalidHeaderValue`, `Serialize`), and
/// resource-exhaustion guards (`ResponseTooLarge`, `SseFrameTooLarge`).
///
/// Marked `#[non_exhaustive]` so additional variants may be introduced in
/// minor releases. See per-variant documentation for retriability guidance.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// Network or TLS error from the HTTP layer. May be retriable (transient
    /// network failure) or permanent (TLS configuration error). Indicates a
    /// network or transport problem, not a JMAP protocol error.
    ///
    /// The payload is an opaque [`HttpError`] that does not expose any
    /// third-party error type — this crate's HTTP transport can be swapped
    /// or its major version bumped without affecting downstream callers.
    /// Use [`HttpError::is_timeout`], [`HttpError::status`], etc. to diagnose.
    #[error("HTTP error: {0}")]
    Http(HttpError),

    /// A header value could not be encoded. Indicates a caller bug — the
    /// credential string contains characters that are not valid HTTP header
    /// value characters. Not retriable.
    #[error("invalid header value: {0}")]
    InvalidHeaderValue(InvalidHeaderValueError),

    /// The server returned HTTP 401 (authentication failure) or 403
    /// (authorization failure — credentials present but insufficient). Not
    /// retriable without correcting credentials.
    #[error("authentication or authorization failure: HTTP {0}")]
    AuthFailed(u16),

    /// A server response could not be parsed or did not match the expected
    /// shape. Indicates the server sent a malformed response. Not retriable
    /// without a server fix.
    ///
    /// Construct explicitly: `.map_err(ClientError::Parse)`.
    #[error("parse error: {0}")]
    Parse(serde_json::Error),

    /// Downloaded blob SHA-256 does not match the expected digest. Indicates
    /// in-transit corruption or a misbehaving server. Not retriable without
    /// re-fetching metadata.
    #[error("blob integrity check failed: expected {expected}, got {actual}")]
    BlobIntegrityMismatch {
        /// Hex-encoded SHA-256 digest the caller asked the client to verify against.
        expected: String,
        /// Hex-encoded SHA-256 digest actually computed over the downloaded bytes.
        actual: String,
    },

    /// A caller-supplied argument violates a precondition (e.g. empty token,
    /// colon in BasicAuth username, missing required filter field).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// The JMAP Session object from the server was missing a required field.
    /// Indicates a server-side bug or incompatible server. Not retriable.
    #[error("invalid session: {0}")]
    InvalidSession(String),

    /// The JMAP API response did not contain the expected method call ID.
    /// Indicates a server-side bug or unexpected response shape.
    #[error("method not found in response: {0}")]
    MethodNotFound(String),

    /// The JMAP server returned a method-level error object (RFC 8620 §3.6).
    /// Retriability depends on `error_type` (e.g. `serverFail` may be
    /// retried; `invalidArguments` is not retriable).
    ///
    /// `description` is `None` when the server omits the optional description field.
    #[error("JMAP method error: {error_type}")]
    MethodError {
        /// The `type` field of the JMAP method-level error object (RFC 8620 §3.6.2),
        /// e.g. `"invalidArguments"`, `"serverFail"`, `"accountNotFound"`.
        error_type: String,
        /// Optional human-readable error description (RFC 8620 §3.6.2);
        /// `None` when the server omits this field.
        description: Option<String>,
    },

    /// A JMAP request could not be serialized to JSON when sending over
    /// WebSocket. Indicates a caller bug — the data structure contains
    /// non-serializable values. Not retriable.
    ///
    /// This error is only returned by [`WsSession::send_request`](crate::WsSession::send_request); the HTTP
    /// `call()` path delegates serialization to reqwest, which surfaces
    /// serialization failures as [`ClientError::Http`].
    ///
    /// Construct explicitly: `.map_err(ClientError::Serialize)`.
    #[error("serialization error: {0}")]
    Serialize(serde_json::Error),

    /// An SSE frame exceeded the configured buffer limit
    /// ([`ClientConfig::max_sse_frame`](crate::ClientConfig::max_sse_frame)). The stream is terminated after this
    /// error. Indicates a misbehaving or hostile server.
    #[error("SSE frame too large (limit: {limit} bytes)")]
    SseFrameTooLarge {
        /// The configured per-frame buffer cap (in bytes) that was exceeded.
        limit: usize,
    },

    /// A server response body exceeded the enforced size limit. Protects
    /// against unbounded memory allocation from malicious or buggy servers.
    /// `actual` is in bytes (from Content-Length or actual read size).
    #[error("response too large: {actual} bytes exceeds limit of {limit} bytes")]
    ResponseTooLarge {
        /// Observed response size in bytes (from `Content-Length` or the
        /// running total of bytes read so far when streaming).
        actual: u64,
        /// Configured maximum response size in bytes.
        limit: u64,
    },

    /// A WebSocket transport error (connection, framing, or TLS). May be
    /// retriable (transient network failure) or permanent (TLS config error).
    ///
    /// The payload is an opaque [`WebSocketError`] that does not expose any
    /// third-party error type — see [`HttpError`] for the same SemVer
    /// rationale. Use [`WebSocketError::is_io`],
    /// [`WebSocketError::is_protocol`], etc. to diagnose.
    #[error("WebSocket error: {0}")]
    WebSocket(WebSocketError),

    /// The server returned a response that violates the JMAP protocol (outside
    /// the Session fetch path). Examples: wrong `Content-Type` on an SSE
    /// connection, unexpected response shape on a non-session endpoint.
    ///
    /// Distinct from [`ClientError::InvalidSession`], which indicates a
    /// problem with the Session document itself. Not retriable without a
    /// server fix.
    #[error("unexpected server response: {0}")]
    UnexpectedResponse(String),

    /// Server rate-limited the request. `retry_after` indicates when to retry.
    ///
    /// **Note (bd:JMAP-6lsm.3): this base crate does not currently produce
    /// this variant.** HTTP 429 responses fall through reqwest's
    /// `error_for_status()` and surface as [`ClientError::Http`] instead.
    /// The variant is part of the public contract so:
    ///
    /// 1. Extension crates that wrap or replace this crate's transport may
    ///    detect 429 + parse `Retry-After` themselves and produce
    ///    `RateLimited` from their own error-conversion code.
    /// 2. Callers that want to handle rate limiting via this typed variant
    ///    have a stable target to match on, even before the conversion
    ///    logic lands here (tracked under `bd:JMAP-6lsm.3`).
    ///
    /// If you encounter a 429 today, match on `ClientError::Http` and call
    /// [`HttpError::status`] to confirm `Some(429)`. The base crate may
    /// gain native 429 → `RateLimited` conversion in a future minor
    /// release; the variant shape will not change in a backward-incompatible
    /// way (it is `#[non_exhaustive]` via the enum-level annotation, so
    /// extra fields can be added without a SemVer break).
    #[error("rate limited; retry after {retry_after}")]
    RateLimited {
        /// Absolute UTC instant the client should wait until before retrying,
        /// parsed from the `Retry-After` HTTP header (RFC 9110 §10.2.3).
        retry_after: jmap_types::UTCDate,
    },
}

impl ClientError {
    /// Convert a [`reqwest::Error`] into a [`ClientError::Http`] variant.
    ///
    /// `pub(crate)` so downstream callers cannot construct transport-error
    /// variants — that responsibility belongs to this crate's transport
    /// layer alone. This is the only conversion path from the third-party
    /// type into `ClientError`, and is the reason this crate's public API
    /// no longer mentions `reqwest::Error`.
    pub(crate) fn from_reqwest(e: reqwest::Error) -> Self {
        Self::Http(HttpError(e))
    }

    /// Convert a [`tokio_tungstenite::tungstenite::Error`] into a
    /// [`ClientError::WebSocket`] variant. See
    /// [`from_reqwest`](Self::from_reqwest) for the SemVer rationale.
    pub(crate) fn from_ws(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::WebSocket(WebSocketError(e))
    }

    /// Convert a [`reqwest::header::InvalidHeaderValue`] into a
    /// [`ClientError::InvalidHeaderValue`] variant. The inner third-party
    /// type carries no actionable diagnostic state, so we keep only the
    /// `Display` representation as a `String`.
    pub(crate) fn from_invalid_header(e: reqwest::header::InvalidHeaderValue) -> Self {
        Self::InvalidHeaderValue(InvalidHeaderValueError {
            message: e.to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify ClientError variants by exhaustive match. Variant names are
    /// part of the public API; this catches accidental rename / removal.
    #[test]
    fn client_error_exhaustive_match() {
        let e = ClientError::InvalidArgument("test".into());
        match e {
            ClientError::Http(_) => {}
            ClientError::InvalidHeaderValue(_) => {}
            ClientError::AuthFailed(_) => {}
            ClientError::Parse(_) => {}
            ClientError::BlobIntegrityMismatch { .. } => {}
            ClientError::InvalidArgument(_) => {}
            ClientError::InvalidSession(_) => {}
            ClientError::MethodNotFound(_) => {}
            ClientError::MethodError { .. } => {}
            ClientError::Serialize(_) => {}
            ClientError::SseFrameTooLarge { .. } => {}
            ClientError::ResponseTooLarge { .. } => {}
            ClientError::WebSocket(_) => {}
            ClientError::UnexpectedResponse(_) => {}
            ClientError::RateLimited { .. } => {}
        }
    }

    /// InvalidHeaderValueError preserves the underlying message so the
    /// Display output matches the third-party error's Display verbatim —
    /// callers that previously logged the `ClientError::InvalidHeaderValue`
    /// variant see the same diagnostic text after the wrapper rename.
    ///
    /// Independent oracle: reqwest::header::InvalidHeaderValue is produced
    /// by HeaderValue::from_str on bytes that are not valid header values
    /// (e.g. embedded newline). The wrapper's Display is just the inner
    /// type's Display.
    #[test]
    fn invalid_header_value_preserves_message() {
        let inner_err = reqwest::header::HeaderValue::from_str("bad\nvalue")
            .expect_err("newline must be rejected as a header value");
        let inner_display = inner_err.to_string();

        let ce = ClientError::from_invalid_header(inner_err);
        let ClientError::InvalidHeaderValue(ihve) = &ce else {
            panic!("must be InvalidHeaderValue variant, got {ce:?}");
        };
        assert_eq!(
            ihve.message(),
            inner_display,
            "wrapper message must equal inner Display"
        );
        // Outer ClientError Display includes the prefix.
        assert!(
            ce.to_string().starts_with("invalid header value: "),
            "ClientError Display must use the variant's #[error] prefix: {ce}"
        );
    }

    /// An HttpError constructed from a reqwest builder failure exposes the
    /// expected diagnostic accessor values: is_builder=true, status=None,
    /// and a non-empty Display. Independent oracle: reqwest's documented
    /// behaviour for invalid URLs (builder error, no status code).
    #[test]
    fn http_error_from_invalid_url_is_builder_error() {
        // reqwest::Client::new().get("not a url") produces a builder error
        // when the URL fails to parse. Building the request and calling
        // .send() requires async context; .build() is synchronous and
        // suffices to provoke a parse failure.
        let client = reqwest::Client::new();
        let build_err = client
            .request(reqwest::Method::GET, "://not-a-url")
            .build()
            .expect_err("malformed URL must produce a build error");

        let ce = ClientError::from_reqwest(build_err);
        let ClientError::Http(http_err) = &ce else {
            panic!("must be Http variant, got {ce:?}");
        };
        assert!(
            http_err.is_builder(),
            "malformed URL must be classified as a builder error"
        );
        assert!(
            http_err.status().is_none(),
            "builder errors carry no HTTP status"
        );
        assert!(
            !http_err.is_timeout(),
            "builder error must not classify as timeout"
        );
        assert!(
            !http_err.is_connect(),
            "builder error must not classify as connect"
        );
        assert!(
            !http_err.to_string().is_empty(),
            "Display must produce a non-empty diagnostic"
        );
    }

    /// A WebSocketError wrapping ConnectionClosed correctly classifies via
    /// its accessor methods. Independent oracle: tungstenite's documented
    /// Error variants are matched directly via the matches! macro.
    #[test]
    fn websocket_error_classifies_connection_closed() {
        let inner = tokio_tungstenite::tungstenite::Error::ConnectionClosed;
        let ce = ClientError::from_ws(inner);
        let ClientError::WebSocket(ws_err) = &ce else {
            panic!("must be WebSocket variant, got {ce:?}");
        };
        assert!(ws_err.is_connection_closed());
        assert!(!ws_err.is_already_closed());
        assert!(!ws_err.is_io());
        assert!(!ws_err.is_protocol());
        assert!(!ws_err.is_capacity());
    }

    /// A WebSocketError wrapping an Io variant correctly classifies via
    /// is_io. Independent oracle: tungstenite::Error::Io is the documented
    /// wrapper for std::io::Error sources.
    #[test]
    fn websocket_error_classifies_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "test");
        let inner = tokio_tungstenite::tungstenite::Error::Io(io_err);
        let ce = ClientError::from_ws(inner);
        let ClientError::WebSocket(ws_err) = &ce else {
            panic!("must be WebSocket variant, got {ce:?}");
        };
        assert!(ws_err.is_io());
        assert!(!ws_err.is_connection_closed());
        assert!(!ws_err.is_already_closed());
    }

    /// HttpError, WebSocketError, and InvalidHeaderValueError all implement
    /// std::error::Error. This is a regression guard against any future
    /// refactor that drops one of these impls (which would silently break
    /// downstream code that iterates the source chain via Error::source).
    #[test]
    fn wrapper_types_implement_std_error() {
        fn assert_error<E: StdError>() {}
        assert_error::<HttpError>();
        assert_error::<WebSocketError>();
        assert_error::<InvalidHeaderValueError>();
    }
}
