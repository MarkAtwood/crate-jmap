#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// Network or TLS error from the HTTP layer. May be retriable (transient
    /// network failure) or permanent (TLS configuration error). Indicates a
    /// network or transport problem, not a JMAP protocol error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// A header value could not be encoded. Indicates a caller bug — the
    /// credential string contains characters that are not valid HTTP header
    /// value characters. Not retriable.
    #[error("invalid header value: {0}")]
    InvalidHeaderValue(#[from] reqwest::header::InvalidHeaderValue),

    /// The server returned HTTP 401 or 403 during authentication. Not
    /// retriable without correcting credentials. Indicates authentication
    /// failure.
    #[error("authentication failed: HTTP {0}")]
    AuthFailed(u16),

    /// A server response could not be parsed or did not match the expected
    /// shape. Indicates the server sent a malformed response. Not retriable
    /// without a server fix.
    #[error("parse error: {0}")]
    Parse(String),

    /// Downloaded blob SHA-256 does not match the expected digest. Indicates
    /// in-transit corruption or a misbehaving server. Not retriable without
    /// re-fetching metadata.
    #[error("blob integrity check failed: expected {expected}, got {actual}")]
    BlobIntegrityMismatch { expected: String, actual: String },

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
    #[error("JMAP method error: {error_type}: {description}")]
    MethodError {
        error_type: String,
        description: String,
    },

    /// A request could not be serialized to JSON. Indicates a caller bug —
    /// the data structure contains non-serializable values. Not retriable.
    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),

    /// An SSE frame exceeded the 1 MiB buffer limit. The stream is terminated
    /// after this error. Indicates a misbehaving or hostile server.
    #[error("SSE frame too large (limit: 1 MiB)")]
    SseFrameTooLarge,

    /// A server response body exceeded the enforced size limit. Protects
    /// against unbounded memory allocation from malicious or buggy servers.
    /// `actual` is in bytes (from Content-Length or actual read size).
    #[error("response too large: {actual} bytes exceeds limit of {limit} bytes")]
    ResponseTooLarge { actual: u64, limit: u64 },

    /// A WebSocket transport error (connection, framing, or TLS). May be
    /// retriable (transient network failure) or permanent (TLS config error).
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify ClientError does not have a RateLimited variant by exhaustive match.
    /// This match will fail to compile if RateLimited is ever reintroduced.
    #[test]
    fn client_error_no_rate_limited_variant() {
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
            ClientError::SseFrameTooLarge => {}
            ClientError::ResponseTooLarge { .. } => {}
            ClientError::WebSocket(_) => {}
        }
    }
}
