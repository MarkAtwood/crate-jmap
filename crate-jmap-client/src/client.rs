// JmapClient — auth-agnostic base JMAP HTTP client (RFC 8620).

use std::sync::Arc;

use futures::StreamExt;

use crate::auth::{AuthProvider, DefaultTransport, TransportConfig};
use crate::error::ClientError;
use crate::request::Session;
use crate::sse::{parse_sse_block, SseFrame};

/// Per-frame byte cap for the SSE streaming buffer (raw bytes and decoded text).
/// Mirrors `MAX_WS_MESSAGE_BYTES` in `ws/mod.rs`. 1 MiB.
const SSE_BUF_SIZE_LIMIT: usize = 1024 * 1024;

/// Internal state threaded through the `subscribe_events` unfold loop.
struct SseStreamState<S> {
    stream: S,
    /// Accumulates raw bytes from the HTTP stream before UTF-8 decoding.
    /// Incomplete multi-byte sequences remain here until the next chunk
    /// completes them, preventing stream termination when a codepoint is
    /// split across adjacent chunks.
    raw_buf: Vec<u8>,
    buf: String,
    /// Byte offset from which the next delimiter scan begins.
    /// Must always be a valid UTF-8 char boundary of `buf`.
    scan_from: usize,
}

/// Per-client configuration for timeouts and body size limits.
///
/// Use [`ClientConfig::default()`] for production defaults (30s timeout, RFC-safe caps).
///
/// This type is `#[non_exhaustive]`: callers outside this crate must use
/// `..ClientConfig::default()` when constructing it, allowing new fields to
/// be added in minor versions without breaking callers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    /// Timeout for HTTP request/response cycles (fetch_session, call, upload_blob, download_blob).
    /// Does NOT apply to SSE or WebSocket streams (which are indefinite by nature).
    /// Must be > 0. Use `Duration::from_secs(30)` for a 30-second timeout.
    /// Default: 30 seconds.
    pub request_timeout: std::time::Duration,
    /// Maximum response body for fetch_session. Default: 1 MiB.
    pub max_session_body: u64,
    /// Maximum response body for call(). Default: 8 MiB.
    pub max_call_body: u64,
    /// Maximum response body for download_blob(). Default: 64 MiB.
    pub max_download_body: u64,
    /// Maximum response body for upload_blob() response parsing. Default: 1 MiB.
    pub max_upload_body: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            request_timeout: std::time::Duration::from_secs(30),
            max_session_body: 1024 * 1024,
            max_call_body: 8 * 1024 * 1024,
            max_download_body: 64 * 1024 * 1024,
            max_upload_body: 1024 * 1024,
        }
    }
}

/// Auth-agnostic JMAP base HTTP client.
///
/// Construct with [`JmapClient::new`] or [`JmapClient::new_plain`].
/// Extension-specific clients (`jmap-chat-client`, `jmap-mail-client`) depend
/// on this crate and add their method implementations via `impl JmapClient`.
#[derive(Clone)]
pub struct JmapClient {
    pub(crate) base_url: url::Url,
    pub(crate) auth: Arc<dyn AuthProvider>,
    pub(crate) http: reqwest::Client,
    pub(crate) config: ClientConfig,
}

impl std::fmt::Debug for JmapClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JmapClient")
            .field("base_url", &self.base_url)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl JmapClient {
    /// Create a new client.
    ///
    /// `transport` configures the underlying HTTP client (TLS trust roots,
    /// client certificates, timeouts). `auth` injects per-request credentials
    /// (Bearer token, Basic credentials, or none). The two are independent so
    /// any transport can be paired with any credential scheme — for example,
    /// `CustomCaTransport` with `BearerAuth`. `base_url` must be the server
    /// origin (scheme, host, optional port) with no path, query, or fragment
    /// — e.g. `"https://100.64.1.1:8008"`. Trailing slashes are normalized
    /// away by the URL parser and are therefore accepted.
    pub fn new(
        transport: impl TransportConfig,
        auth: impl AuthProvider + 'static,
        base_url: &str,
        config: ClientConfig,
    ) -> Result<Self, ClientError> {
        if base_url.is_empty() {
            return Err(ClientError::InvalidArgument(
                "base_url may not be empty".into(),
            ));
        }
        let parsed = url::Url::parse(base_url).map_err(|e| {
            ClientError::InvalidArgument(format!("base_url is not a valid URL: {e}"))
        })?;
        let scheme = parsed.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(ClientError::InvalidArgument(format!(
                "base_url scheme must be http or https, got: {scheme:?}"
            )));
        }
        let path = parsed.path();
        // url::Url::path() returns "/" for root-only URLs (no path segments);
        // any value other than "/" means the URL contains an explicit path component.
        if path != "/" {
            return Err(ClientError::InvalidArgument(format!(
                "base_url must not have a path component, got: {path:?}"
            )));
        }
        if parsed.query().is_some() {
            return Err(ClientError::InvalidArgument(
                "base_url must not have a query string".into(),
            ));
        }
        if parsed.fragment().is_some() {
            return Err(ClientError::InvalidArgument(
                "base_url must not have a fragment".into(),
            ));
        }
        if config.max_session_body == 0 {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.max_session_body must be > 0".into(),
            ));
        }
        if config.max_call_body == 0 {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.max_call_body must be > 0".into(),
            ));
        }
        if config.max_download_body == 0 {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.max_download_body must be > 0".into(),
            ));
        }
        if config.max_upload_body == 0 {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.max_upload_body must be > 0".into(),
            ));
        }
        if config.request_timeout == std::time::Duration::ZERO {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.request_timeout must be > 0; use Duration::from_secs(30) or similar".into(),
            ));
        }
        let http = transport.build_client()?;
        Ok(Self {
            base_url: parsed,
            auth: Arc::new(auth),
            http,
            config,
        })
    }

    /// Convenience constructor for servers with publicly-trusted TLS.
    ///
    /// Equivalent to `JmapClient::new(DefaultTransport, auth, base_url, config)`.
    /// Use [`JmapClient::new`] when you need a custom transport (e.g.
    /// `CustomCaTransport` for a private-CA server).
    pub fn new_plain(
        auth: impl AuthProvider + 'static,
        base_url: &str,
        config: ClientConfig,
    ) -> Result<Self, ClientError> {
        Self::new(DefaultTransport, auth, base_url, config)
    }

    /// Returns `Err(ClientError::AuthFailed)` when the HTTP status indicates an
    /// authentication or authorization failure.
    ///
    /// Specifically handles:
    /// - 401 Unauthorized (RFC 7235 §3.1) — missing or invalid credentials
    /// - 403 Forbidden (RFC 7235 §3.2) — credentials present but insufficient
    ///
    /// Called before reading the response body so callers can distinguish
    /// permanent auth failures from transient errors without consuming the body.
    pub(crate) fn check_auth_status(status: reqwest::StatusCode) -> Result<(), ClientError> {
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            Err(ClientError::AuthFailed(status.as_u16()))
        } else {
            Ok(())
        }
    }

    /// Fetch the JMAP Session object from `{base_url}/.well-known/jmap` (RFC 8620 §2).
    ///
    /// The response body is capped at 1 MiB. Returns `ClientError::ResponseTooLarge`
    /// if the server sends more. Session URL fields (`apiUrl`, `uploadUrl`,
    /// `downloadUrl`, `eventSourceUrl`) are validated to have http/https scheme;
    /// a non-http scheme returns `ClientError::InvalidArgument`.
    ///
    /// Returns `ClientError::AuthFailed` on HTTP 401 or 403.
    pub async fn fetch_session(&self) -> Result<Session, ClientError> {
        let limit = self.config.max_session_body;
        let url = self
            .base_url
            .join(".well-known/jmap")
            .map_err(|e| ClientError::InvalidArgument(format!("cannot construct session URL: {e}")))?
            .to_string();

        let mut req = self
            .http
            .get(&url)
            .timeout(self.config.request_timeout);
        if let Some((name, value)) = self.auth.auth_header() {
            req = req.header(name, value.as_str());
        }

        let resp = {
            let raw_resp = req.send().await.map_err(ClientError::Http)?;
            Self::check_auth_status(raw_resp.status())?;
            raw_resp.error_for_status().map_err(ClientError::Http)?
        };

        // Enforce size cap before reading. Content-Length can lie, so we check
        // both the header and the actual read size.
        if let Some(len) = resp.content_length() {
            if len > limit {
                return Err(ClientError::ResponseTooLarge {
                    actual: len,
                    limit,
                });
            }
        }
        let bytes = resp.bytes().await.map_err(ClientError::Http)?;
        if bytes.len() as u64 > limit {
            return Err(ClientError::ResponseTooLarge {
                actual: bytes.len() as u64,
                limit,
            });
        }

        let session: Session = serde_json::from_slice(&bytes)
            .map_err(|e| ClientError::Parse(e.to_string()))?;

        validate_session_urls(&session)?;

        Ok(session)
    }

    /// POST a [`jmap_types::JmapRequest`] to `api_url` and return the parsed [`jmap_types::JmapResponse`]
    /// (RFC 8620 §3.3).
    ///
    /// `api_url` is taken as an explicit parameter (not from `self`) because the
    /// caller holds a [`Session`] and selects the correct URL from it.
    ///
    /// The response body is capped at 8 MiB. Returns `ClientError::ResponseTooLarge`
    /// if the server sends more.
    ///
    /// Returns `ClientError::AuthFailed` on HTTP 401 or 403.
    pub async fn call(
        &self,
        api_url: &str,
        req: &jmap_types::JmapRequest,
    ) -> Result<jmap_types::JmapResponse, ClientError> {
        let limit = self.config.max_call_body;

        let mut builder = self
            .http
            .post(api_url)
            .json(req)
            .timeout(self.config.request_timeout);
        if let Some((name, value)) = self.auth.auth_header() {
            builder = builder.header(name, value.as_str());
        }

        let resp = {
            let raw_resp = builder.send().await.map_err(ClientError::Http)?;
            Self::check_auth_status(raw_resp.status())?;
            raw_resp.error_for_status().map_err(ClientError::Http)?
        };

        // Enforce size cap before reading.
        if let Some(len) = resp.content_length() {
            if len > limit {
                return Err(ClientError::ResponseTooLarge {
                    actual: len,
                    limit,
                });
            }
        }
        let bytes = resp.bytes().await.map_err(ClientError::Http)?;
        if bytes.len() as u64 > limit {
            return Err(ClientError::ResponseTooLarge {
                actual: bytes.len() as u64,
                limit,
            });
        }

        let jmap_resp: jmap_types::JmapResponse = serde_json::from_slice(&bytes)
            .map_err(|e| ClientError::Parse(e.to_string()))?;

        Ok(jmap_resp)
    }

    /// Open an SSE connection to `event_source_url` and return an async stream
    /// of parsed [`SseFrame`]s (RFC 8620 §7.3).
    ///
    /// If `last_event_id` is `Some`, sends a `Last-Event-ID` header so the
    /// server can resume from where the previous stream left off.
    ///
    /// Buffer growth is capped at 1 MiB per frame. If a single SSE frame
    /// exceeds this limit the stream yields `ClientError::SseFrameTooLarge`
    /// and terminates.
    ///
    /// No idle timeout is applied to the stream (unlike point requests).
    /// Wrap in [`tokio::time::timeout`] if you need to detect server silence
    /// and reconnect after a quiet period.
    ///
    /// Returns `ClientError::AuthFailed` on HTTP 401 or 403 before the stream
    /// starts.
    pub async fn subscribe_events(
        &self,
        event_source_url: &str,
        last_event_id: Option<&str>,
    ) -> Result<futures::stream::BoxStream<'static, Result<SseFrame, ClientError>>, ClientError>
    {
        let mut req = self
            .http
            .get(event_source_url)
            .header("Accept", "text/event-stream");
        if let Some(id) = last_event_id {
            req = req.header("Last-Event-ID", id);
        }
        if let Some((name, value)) = self.auth.auth_header() {
            req = req.header(name, value.as_str());
        }

        let resp = req.send().await.map_err(ClientError::Http)?;
        Self::check_auth_status(resp.status())?;
        let resp = resp.error_for_status().map_err(ClientError::Http)?;

        let byte_stream = resp.bytes_stream();

        Ok(futures::stream::unfold(
            Some(SseStreamState {
                stream: byte_stream,
                raw_buf: Vec::new(),
                buf: String::new(),
                scan_from: 0usize, // invariant: valid UTF-8 char boundary of buf; 0 always satisfies this
            }),
            |state| async move {
                let SseStreamState {
                    mut stream,
                    mut raw_buf,
                    mut buf,
                    mut scan_from,
                } = state?;
                loop {
                    // Search for any double-newline delimiter (LF/CRLF/CR variants).
                    // scan_from is set to old_len.saturating_sub(3) after each append
                    // so we only re-scan the overlap region.  3 bytes back is the
                    // minimum that covers all delimiter prefixes that can straddle a
                    // chunk boundary:
                    //   - `\r\n\r\n` (4 bytes): longest prefix that fits in one chunk
                    //     but is incomplete is `\r\n\r` (3 bytes) — exactly covered.
                    //   - `\n\n` and `\r\r` (2 bytes each): longest incomplete prefix
                    //     is 1 byte — covered by the 3-byte overlap.
                    // Since \r and \n are single-byte UTF-8 codepoints, 3 bytes back
                    // is always a valid char boundary — no adjustment needed.
                    let frame_end = [
                        buf[scan_from..]
                            .find("\r\n\r\n")
                            .map(|p| (scan_from + p, 4usize)),
                        buf[scan_from..]
                            .find("\n\n")
                            .map(|p| (scan_from + p, 2usize)),
                        buf[scan_from..]
                            .find("\r\r")
                            .map(|p| (scan_from + p, 2usize)),
                    ]
                    .into_iter()
                    .flatten()
                    .min_by_key(|&(pos, _)| pos);

                    if let Some((pos, delim_len)) = frame_end {
                        let frame = {
                            let slice = &buf[..pos];
                            if slice.contains('\r') {
                                slice.replace("\r\n", "\n").replace('\r', "\n")
                            } else {
                                slice.to_owned()
                            }
                        };
                        let suffix = buf.split_off(pos + delim_len);
                        buf = suffix;
                        scan_from = 0; // 0 satisfies the UTF-8 char boundary invariant
                        let sse_frame = parse_sse_block(&frame);
                        return Some((
                            Ok(sse_frame),
                            Some(SseStreamState {
                                stream,
                                raw_buf,
                                buf,
                                scan_from,
                            }),
                        ));
                    }

                    match stream.next().await {
                        None => return None,
                        Some(Err(e)) => {
                            return Some((Err(ClientError::Http(e)), None));
                        }
                        Some(Ok(bytes)) => {
                            // Accumulate raw bytes first. A multi-byte UTF-8 codepoint
                            // may be split across adjacent HTTP chunks; decode only the
                            // valid prefix and leave the remainder in raw_buf until the
                            // next chunk completes the sequence.
                            raw_buf.extend_from_slice(&bytes);
                            // Cap raw_buf to prevent OOM on persistent invalid UTF-8 input.
                            // Use the same limit as the decoded buf cap (1 MiB per frame).
                            if raw_buf.len() > SSE_BUF_SIZE_LIMIT {
                                return Some((Err(ClientError::SseFrameTooLarge), None));
                            }
                            let old_len = buf.len();
                            match std::str::from_utf8(&raw_buf) {
                                Ok(s) => {
                                    buf.push_str(s);
                                    raw_buf.clear();
                                }
                                Err(e) => {
                                    let valid_up_to = e.valid_up_to();
                                    // valid_up_to is always a char boundary by definition.
                                    buf.push_str(
                                        std::str::from_utf8(&raw_buf[..valid_up_to])
                                            .expect("valid_up_to is a valid UTF-8 boundary"),
                                    );
                                    // Drain valid prefix plus at least one byte so that an
                                    // invalid-sequence head (valid_up_to == 0) is not stuck
                                    // in raw_buf forever, which would cause it to grow until
                                    // the 1 MiB cap fires even if valid data follows.
                                    let drain_end = valid_up_to.max(1);
                                    raw_buf.drain(..drain_end.min(raw_buf.len()));
                                }
                            }
                            scan_from = old_len.saturating_sub(3);
                            // Walk backward to a valid UTF-8 char boundary so that
                            // buf[scan_from..] never panics on multibyte characters.
                            while scan_from > 0 && !buf.is_char_boundary(scan_from) {
                                scan_from -= 1;
                            }
                            // Guard against unbounded buffer growth from a hostile server.
                            // Yield the error and terminate (state = None).
                            if buf.len() > SSE_BUF_SIZE_LIMIT {
                                return Some((Err(ClientError::SseFrameTooLarge), None));
                            }
                        }
                    }
                }
            },
        )
        .boxed())
    }
}

/// Find the method response matching `call_id` in `resp` and deserialize its
/// arguments into `T`.
///
/// Returns [`ClientError::MethodNotFound`] if no invocation with the given
/// call_id exists. Returns [`ClientError::MethodError`] if the matched
/// invocation is a JMAP `"error"` response (RFC 8620 §3.6.1).
///
/// This function is `pub` so extension crates (`jmap-chat-client`,
/// `jmap-mail-client`) can use it to extract typed results from a
/// [`jmap_types::JmapResponse`] without depending on internal details.
pub fn extract_response<T: serde::de::DeserializeOwned>(
    resp: jmap_types::JmapResponse,
    call_id: &str,
) -> Result<T, ClientError> {
    // Invocation is a type alias (String, Value, String) = (method, args, call_id)
    let inv = resp
        .method_responses
        .into_iter()
        .find(|inv| inv.2 == call_id)
        .ok_or_else(|| ClientError::MethodNotFound(call_id.to_owned()))?;
    let (method_name, args, _) = inv;

    // RFC 8620 §3.6.1: a method name of "error" signals a protocol-level error.
    if method_name == "error" {
        let err_type = args
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("serverError") // safe: fallback literal, not user input
            .to_owned();
        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown") // safe: fallback literal, not user input
            .to_owned();
        return Err(ClientError::MethodError {
            error_type: err_type,
            description,
        });
    }

    serde_json::from_value(args).map_err(|e| ClientError::Parse(e.to_string()))
}

/// Validate that all URL fields in `session` use an http or https scheme.
///
/// Returns `ClientError::InvalidArgument` if any URL has a non-http/https scheme.
/// This prevents a malicious server from injecting non-HTTP URLs into subsequent
/// requests (e.g. `file://`, `ftp://`).
fn validate_session_urls(session: &Session) -> Result<(), ClientError> {
    for url in [
        &session.api_url,
        &session.upload_url,
        &session.download_url,
        &session.event_source_url,
    ] {
        let has_http_scheme = url.starts_with("http://") || url.starts_with("https://");
        if !has_http_scheme {
            return Err(ClientError::InvalidArgument(format!(
                "session URL has non-http/https scheme: {:?}",
                url
            )));
        }
    }
    Ok(())
}
