//! Auth-agnostic base JMAP HTTP client (RFC 8620).
//!
//! Provides [`JmapClient`] for session fetch, API calls, blob transfer,
//! SSE event streaming, and [`extract_response`] for parsing method results.

use std::sync::Arc;

use futures::StreamExt;

use crate::auth::{AuthProvider, DefaultTransport, TransportConfig};
use crate::error::ClientError;
use crate::request::Session;
use crate::sse::{parse_sse_block, SseFrame};

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
    /// Maximum size in bytes of the JSON response body returned by the
    /// server in reply to `upload_blob()`. Does NOT cap the size of the
    /// blob being uploaded — the JMAP server enforces that via its
    /// `maxSizeUpload` capability. This field caps only the small JSON
    /// envelope the server returns describing the stored blob.
    /// Default: 1 MiB.
    pub max_upload_response_body: u64,
    /// Maximum byte length of a single SSE frame; applied independently to
    /// the raw incoming bytes (pre-UTF-8 decode) and the decoded text.
    /// Protects against memory exhaustion from a hostile or misbehaving
    /// server that sends a single very large frame. Must be > 0.
    /// Default: 1 MiB.
    ///
    /// **Memory residency note (bd:JMAP-6lsm.7):** because the raw byte
    /// buffer and the decoded text buffer are tracked separately, the
    /// in-flight footprint while parsing a single frame can momentarily
    /// reach ~2 × `max_sse_frame` (raw bytes accumulated up to the limit,
    /// plus decoded text not yet drained). If you tune this value for a
    /// tight memory budget, plan for that 2× peak. The independent
    /// tracking is correct for the streaming UTF-8 decoder (which needs
    /// the raw buffer to be at least one full frame to handle split
    /// multi-byte sequences across HTTP chunks); see `decode_utf8_chunk`
    /// for the rationale.
    pub max_sse_frame: usize,
    /// Maximum byte length of a single WebSocket message (and frame). Mirrors
    /// `max_sse_frame` for the WebSocket transport. Used by
    /// [`JmapClient::connect_ws_session`] and threaded through to
    /// `tokio_tungstenite::tungstenite::protocol::WebSocketConfig`'s
    /// `max_message_size` and `max_frame_size`. Must be > 0.
    /// Default: 1 MiB. (bd:JMAP-6lsm.5)
    pub max_ws_message: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            request_timeout: std::time::Duration::from_secs(30),
            max_session_body: 1024 * 1024,
            max_call_body: 8 * 1024 * 1024,
            max_download_body: 64 * 1024 * 1024,
            max_upload_response_body: 1024 * 1024,
            max_sse_frame: 1024 * 1024,
            max_ws_message: 1024 * 1024,
        }
    }
}

impl ClientConfig {
    /// Validate that all config fields satisfy their constraints.
    ///
    /// Called automatically by [`JmapClient::new`].  Callers may also call
    /// this directly to pre-validate a config before passing it to the
    /// constructor.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::InvalidArgument`] when any field is zero or
    /// out-of-range.
    pub fn validate(&self) -> Result<(), ClientError> {
        if self.max_session_body == 0 {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.max_session_body must be > 0".into(),
            ));
        }
        if self.max_call_body == 0 {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.max_call_body must be > 0".into(),
            ));
        }
        if self.max_download_body == 0 {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.max_download_body must be > 0".into(),
            ));
        }
        if self.max_upload_response_body == 0 {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.max_upload_response_body must be > 0".into(),
            ));
        }
        if self.request_timeout == std::time::Duration::ZERO {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.request_timeout must be > 0; use Duration::from_secs(30) or similar"
                    .into(),
            ));
        }
        if self.max_sse_frame == 0 {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.max_sse_frame must be > 0".into(),
            ));
        }
        if self.max_ws_message == 0 {
            return Err(ClientError::InvalidArgument(
                "ClientConfig.max_ws_message must be > 0".into(),
            ));
        }
        Ok(())
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
        // Parse-don't-validate: parse_base_url returns a fully-validated
        // url::Url so the constructor body can read as four facts (parse,
        // validate config, build transport, construct Self) rather than
        // a procedure (bd:JMAP-6lsm.26).
        let parsed = parse_base_url(base_url)?;
        config.validate()?;
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

    /// Apply the auth header (if any) to a request builder.
    ///
    /// Centralises the repeated `if let Some(...) = self.auth.auth_header()` pattern
    /// that every HTTP method uses. Callers: `fetch_session`, `call`,
    /// `subscribe_events`, `upload_blob`, `download_blob`.
    pub(crate) fn inject_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some((name, value)) = self.auth.auth_header() {
            builder.header(name, value)
        } else {
            builder
        }
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
}

/// Read a response body into memory, capped at `limit` bytes.
///
/// Performs the Content-Length pre-check as a fast-path rejection for honest
/// oversized responses, then streams the body chunk-by-chunk and enforces the
/// cap before each accumulation. This prevents buffering a response that
/// exceeds the limit when Content-Length is absent (chunked Transfer-Encoding)
/// or under-reported by a hostile server — without per-chunk streaming, a
/// `.bytes().await` call would buffer the entire response before the
/// post-buffer length check could fire (bd:JMAP-6r7c.1).
///
/// Threat model: matters when connecting to untrusted servers (federation
/// peers, third-party JMAP services, DNS-hijacked endpoints, MITM scenarios).
/// On low-memory devices the peak allocation from naive buffering can OOM-kill
/// the client even if the post-buffer check would otherwise reject.
///
/// Returns `ClientError::ResponseTooLarge` when either the advertised
/// Content-Length or the accumulated chunk total exceeds `limit`.
pub(crate) async fn read_capped_body(
    resp: reqwest::Response,
    limit: u64,
) -> Result<Vec<u8>, ClientError> {
    if let Some(len) = resp.content_length() {
        if len > limit {
            return Err(ClientError::ResponseTooLarge { actual: len, limit });
        }
    }

    let mut stream = resp.bytes_stream();
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(ClientError::from_reqwest)?;
        let new_len = body.len() as u64 + chunk.len() as u64;
        if new_len > limit {
            return Err(ClientError::ResponseTooLarge {
                actual: new_len,
                limit,
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

impl JmapClient {
    /// Fetch the JMAP Session object from `{base_url}/.well-known/jmap` (RFC 8620 §2).
    ///
    /// The response body is capped at 1 MiB. Returns `ClientError::ResponseTooLarge`
    /// if the server sends more. Session URL fields (`apiUrl`, `uploadUrl`,
    /// `downloadUrl`, `eventSourceUrl`) are validated to have http/https scheme;
    /// a non-http scheme returns `ClientError::InvalidSession`.
    ///
    /// Returns `ClientError::AuthFailed` on HTTP 401 or 403.
    pub async fn fetch_session(&self) -> Result<Session, ClientError> {
        let limit = self.config.max_session_body;
        let url = self.base_url.join(".well-known/jmap").map_err(|e| {
            // base_url was pre-validated in JmapClient::new; this path is
            // unreachable in practice but must be handled for completeness.
            ClientError::InvalidSession(format!("cannot construct session URL: {e}"))
        })?;

        let req = self.inject_auth(self.http.get(url).timeout(self.config.request_timeout));

        let resp = {
            let raw_resp = req.send().await.map_err(ClientError::from_reqwest)?;
            Self::check_auth_status(raw_resp.status())?;
            raw_resp
                .error_for_status()
                .map_err(ClientError::from_reqwest)?
        };

        // Stream the body chunk-by-chunk with the cap enforced before each
        // accumulation. The Content-Length pre-check inside read_capped_body
        // is a fast-path rejection for honest oversized responses; the
        // streaming cap is the actual DOS guard against a hostile server
        // that under-reports Content-Length or omits it entirely under
        // chunked Transfer-Encoding (bd:JMAP-6r7c.1).
        let body = read_capped_body(resp, limit).await?;

        let session: Session = serde_json::from_slice(&body).map_err(ClientError::Parse)?;

        validate_session_url_schemes(&session)?;

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
        require_http_url(api_url)?;
        let limit = self.config.max_call_body;

        let builder = self.inject_auth(
            self.http
                .post(api_url)
                .json(req)
                .timeout(self.config.request_timeout),
        );

        let resp = {
            let raw_resp = builder.send().await.map_err(ClientError::from_reqwest)?;
            Self::check_auth_status(raw_resp.status())?;
            raw_resp
                .error_for_status()
                .map_err(ClientError::from_reqwest)?
        };

        // Stream the body chunk-by-chunk with the cap enforced before each
        // accumulation (bd:JMAP-6r7c.1). See `read_capped_body` for the DOS
        // rationale; without per-chunk streaming, a server that under-reports
        // or omits Content-Length can force unbounded allocation here.
        let body = read_capped_body(resp, limit).await?;

        let jmap_resp: jmap_types::JmapResponse =
            serde_json::from_slice(&body).map_err(ClientError::Parse)?;

        Ok(jmap_resp)
    }

    /// Open an SSE connection to `event_source_url` and return an async stream
    /// of parsed [`SseFrame`]s (RFC 8620 §7.3).
    ///
    /// # URI template expansion
    ///
    /// `Session.event_source_url` is a URI template (RFC 6570 Level-1) with
    /// variables `types`, `closeafter`, and `ping`. You **must** expand it
    /// before passing it to this function, or the server will receive the
    /// literal text `{types}` in the URL and return an error. Use
    /// [`expand_url_template`](crate::expand_url_template):
    ///
    /// ```rust,ignore
    /// let url = jmap_base_client::expand_url_template(
    ///     &session.event_source_url,
    ///     &[("types", "*"), ("closeafter", "no"), ("ping", "0")],
    /// )?;
    /// let stream = client.subscribe_events(&url, None).await?;
    /// ```
    ///
    /// If `last_event_id` is `Some`, sends a `Last-Event-ID` header so the
    /// server can resume from where the previous stream left off.
    ///
    /// Buffer growth is capped at [`ClientConfig::max_sse_frame`] bytes per
    /// frame (default: 1 MiB). If a single SSE frame exceeds this limit the
    /// stream yields `ClientError::SseFrameTooLarge` and terminates.
    ///
    /// No timeout is applied to this call or to the resulting stream.  The
    /// connect timeout (10 s, TCP only) is the only deadline enforced.  If the
    /// server stalls before sending HTTP response headers, or later goes silent
    /// on the open connection, this call or the stream will hang indefinitely.
    /// Wrap the entire call and/or stream iteration in [`tokio::time::timeout`]
    /// if you need to bound either phase.
    ///
    /// Returns `ClientError::AuthFailed` on HTTP 401 or 403 before the stream
    /// starts.
    pub async fn subscribe_events(
        &self,
        event_source_url: &str,
        last_event_id: Option<&str>,
    ) -> Result<futures::stream::BoxStream<'static, Result<SseFrame, ClientError>>, ClientError>
    {
        require_http_url(event_source_url)?;
        let mut req = self
            .http
            .get(event_source_url)
            .header("Accept", "text/event-stream");
        if let Some(id) = last_event_id {
            req = req.header("Last-Event-ID", id);
        }
        let req = self.inject_auth(req);

        let resp = req.send().await.map_err(ClientError::from_reqwest)?;
        Self::check_auth_status(resp.status())?;
        let resp = resp.error_for_status().map_err(ClientError::from_reqwest)?;

        // Verify Content-Type before streaming. A misconfigured server returning
        // application/json would silently produce no events (no SSE delimiter found).
        {
            // to_ascii_lowercase(): media types are case-insensitive per RFC 7231 §3.1.1.1.
            let ct = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();
            // RFC 7231 §3.1.1.1 / RFC 9110 §8.3: the media-type "essence" (type
            // + "/" + subtype) is bounded by ';', SP, HTAB, or end-of-string.
            // A naive starts_with("text/event-stream") would accept
            // "text/event-streamish" or "text/event-stream2" — exactly the bug
            // JMAP-6lsm.2 flagged. Split off the parameter list / trailing
            // whitespace and compare the essence exactly.
            let essence = ct
                .split(|c: char| c == ';' || c.is_whitespace())
                .next()
                .unwrap_or("");
            if essence != "text/event-stream" {
                return Err(ClientError::UnexpectedResponse(format!(
                    "subscribe_events: expected Content-Type text/event-stream, got: {ct:?}"
                )));
            }
        }

        let byte_stream = resp.bytes_stream();
        let sse_frame_limit = self.config.max_sse_frame;

        Ok(futures::stream::unfold(
            Some(SseStreamState {
                stream: byte_stream,
                raw_buf: Vec::new(),
                buf: String::new(),
                scan_from: 0, // invariant: valid UTF-8 char boundary of buf; 0 always satisfies this
            }),
            move |state| async move {
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
                    //   - `\n\r\n` (3 bytes): longest incomplete prefix is `\n\r` (2
                    //     bytes) — covered by the 3-byte overlap.
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
                        // Mixed: LF-terminated last field line followed by a
                        // CRLF-terminated blank line.  Not detected by \n\n (the
                        // LFs are separated by a CR) or \r\n\r\n (no leading \r\n).
                        buf[scan_from..]
                            .find("\n\r\n")
                            .map(|p| (scan_from + p, 3usize)),
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
                        buf.drain(..pos + delim_len);
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
                            return Some((Err(ClientError::from_reqwest(e)), None));
                        }
                        Some(Ok(bytes)) => {
                            // Accumulate raw bytes first. A multi-byte UTF-8 codepoint
                            // may be split across adjacent HTTP chunks; decode only the
                            // valid prefix and leave the remainder in raw_buf until the
                            // next chunk completes the sequence.
                            raw_buf.extend_from_slice(&bytes);
                            // Cap raw_buf to prevent OOM on persistent invalid UTF-8 input.
                            // Use the same limit as the decoded buf cap.
                            if raw_buf.len() > sse_frame_limit {
                                return Some((
                                    Err(ClientError::SseFrameTooLarge {
                                        limit: sse_frame_limit,
                                    }),
                                    None,
                                ));
                            }
                            let old_len = buf.len();
                            decode_utf8_chunk(&mut raw_buf, &mut buf);
                            scan_from = old_len.saturating_sub(3);
                            // Walk backward to a valid UTF-8 char boundary so that
                            // buf[scan_from..] never panics on multibyte characters.
                            while scan_from > 0 && !buf.is_char_boundary(scan_from) {
                                scan_from -= 1;
                            }
                            // Guard against unbounded buffer growth from a hostile server.
                            // Yield the error and terminate (state = None).
                            if buf.len() > sse_frame_limit {
                                return Some((
                                    Err(ClientError::SseFrameTooLarge {
                                        limit: sse_frame_limit,
                                    }),
                                    None,
                                ));
                            }
                        }
                    }
                }
            },
        )
        .boxed())
    }

    /// Open a WebSocket connection to `ws_url` using this client's
    /// configured `max_ws_message` byte cap.
    ///
    /// Convenience wrapper around [`crate::ws::connect_ws_with_limit`] that
    /// passes [`ClientConfig::max_ws_message`] as the per-message /
    /// per-frame byte cap. Mirrors the [`Self::subscribe_events`]
    /// pattern of "the JmapClient method uses ClientConfig; the free
    /// function takes an explicit value".
    ///
    /// `ws_url` must come from the session document's WebSocket capability
    /// URL. `auth_header` is an optional `(name, value)` pair for the
    /// upgrade request; the auth provider on this client is NOT used here
    /// because some servers attach WebSocket auth via cookie or session
    /// header rather than the same scheme as HTTP requests.
    ///
    /// # Security
    ///
    /// The `auth_header` value is a credential and must not be logged or
    /// echoed back to other systems. Treat it with the same care as a
    /// [`crate::auth::BearerAuth`] token. Transport errors raised by this
    /// method are constructed without the original credential bytes, but
    /// downstream code that inspects [`ClientError`] should still avoid
    /// printing or storing the `auth_header` itself.
    ///
    /// Returns [`ClientError::InvalidArgument`] for non-`ws://`/`wss://` URLs.
    /// See [`crate::ws::connect_ws_with_limit`] for full error semantics.
    pub async fn connect_ws_session(
        &self,
        ws_url: &str,
        auth_header: Option<(&str, &str)>,
    ) -> Result<crate::ws::WsSession, ClientError> {
        crate::ws::connect_ws_with_limit(ws_url, auth_header, self.config.max_ws_message).await
    }
}

/// Find the method response matching `call_id` in `resp` and deserialize its
/// arguments into `T`.
///
/// Returns [`ClientError::MethodNotFound`] if no invocation with the given
/// call_id exists. Returns [`ClientError::MethodError`] if any invocation
/// with the matching call_id is a JMAP `"error"` response (RFC 8620 §3.6.1).
///
/// # Multiple invocations sharing a call_id
///
/// Per RFC 8620 §3.2, a single method call may produce multiple invocations
/// in the response — for example, `Foo/copy` with `onSuccessDestroyOriginal:
/// true` produces both a `Foo/copy` and an implicit `Foo/set` invocation,
/// both stamped with the same call_id (RFC 8620 §5.8 example, lines 3158–
/// 3180). This function handles that case by:
///
/// 1. **Errors take precedence.** If any invocation matching `call_id`
///    has method name `"error"`, this function returns that error. A
///    success response cannot mask a sibling error response with the same
///    call_id — silently returning the success while the server reported
///    failure would be data loss for the caller.
/// 2. Otherwise, the **first** non-error invocation matching `call_id`
///    is deserialized into `T`. In the §5.8 implicit-method case both
///    invocations are successes; the first is the primary response and
///    is what the caller wants.
///
/// This function is `pub` so extension crates (`jmap-chat-client`,
/// `jmap-mail-client`) can use it to extract typed results from a
/// [`jmap_types::JmapResponse`] without depending on internal details.
pub fn extract_response<T: serde::de::DeserializeOwned>(
    resp: &jmap_types::JmapResponse,
    call_id: &str,
) -> Result<T, ClientError> {
    // Invocation is a type alias (String, Value, String) = (method, args, call_id).
    // Two-pass scan: errors take precedence per §3.6.1, so look for an error
    // invocation first; otherwise return the first non-error invocation.
    // The underlying iterator is slice::Iter, so .clone() is a cheap pointer
    // copy — no allocation.
    let mut candidates = resp.method_responses.iter().filter(|inv| inv.2 == call_id);

    if let Some(err_inv) = candidates.clone().find(|inv| inv.0 == "error") {
        let args = &err_inv.1;
        let err_type = args
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("serverError") // safe: fallback literal, not user input
            .to_owned();
        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        return Err(ClientError::MethodError {
            error_type: err_type,
            description,
        });
    }

    // The early return above already handled the error case; every remaining
    // candidate is a success invocation, so we just take the first one.
    let inv = candidates
        .next()
        .ok_or_else(|| ClientError::MethodNotFound(call_id.to_owned()))?;
    <T as serde::Deserialize>::deserialize(&inv.1).map_err(ClientError::Parse)
}

/// Decode as much valid UTF-8 as possible from `raw` into `buf`, draining
/// consumed (or definitively-invalid) bytes from `raw`.
///
/// Three cases:
/// - `raw` is fully valid UTF-8: pushed entirely to `buf`, `raw` cleared.
/// - `raw` ends with an **incomplete** multi-byte sequence (`error_len == None`):
///   the valid prefix is pushed to `buf` and drained from `raw`; the incomplete
///   head bytes stay in `raw` for the next chunk to complete them.
/// - `raw` contains a **definitively invalid** byte sequence (`error_len == Some(n)`):
///   the valid prefix is pushed to `buf`; the valid prefix AND the `n` invalid
///   bytes are drained from `raw` so they do not accumulate.
///
/// The caller is responsible for capping `raw.len()` before calling this
/// function; unbounded growth from a hostile server that never completes a
/// sequence is prevented by the 1 MiB `SSE_BUF_SIZE_LIMIT` check in
/// `subscribe_events`.
fn decode_utf8_chunk(raw: &mut Vec<u8>, buf: &mut String) {
    match std::str::from_utf8(raw) {
        Ok(s) => {
            buf.push_str(s);
            raw.clear();
        }
        Err(e) => {
            let valid_up_to = e.valid_up_to();
            // valid_up_to is always a char boundary by definition.
            buf.push_str(
                std::str::from_utf8(&raw[..valid_up_to])
                    .expect("valid_up_to is a valid UTF-8 boundary"),
            );
            match e.error_len() {
                Some(n) => {
                    // Definitively invalid: drain the valid prefix AND the
                    // invalid bytes so they do not accumulate in raw.
                    let drain_end = (valid_up_to + n).min(raw.len());
                    raw.drain(..drain_end);
                }
                None => {
                    // Incomplete multi-byte sequence: drain only the valid
                    // prefix.  The incomplete head stays in raw until the next
                    // chunk arrives with the missing continuation bytes.
                    raw.drain(..valid_up_to);
                }
            }
        }
    }
}

/// Extract the URL scheme as a borrowed slice of `url`.
///
/// Returns the prefix before `"://"`. The returned slice is in the
/// *original* case of the input (callers must use [`str::eq_ignore_ascii_case`]
/// for the comparison per RFC 3986 §3.1, which says only schemes are
/// case-insensitive). Returns `None` if `"://"` is not present in `url`.
/// URL templates containing `{variable}` syntax are handled correctly
/// because the extraction is a prefix scan, not a full URL parse.
///
/// Returning a borrowed slice rather than a `to_ascii_lowercase()` String
/// avoids allocating on every scheme check; previously each call to
/// `url_scheme` produced a fresh String the size of the URL prefix
/// (bd:JMAP-6lsm.10).
fn url_scheme(url: &str) -> Option<&str> {
    url.split_once("://").map(|(scheme, _)| scheme)
}

/// `true` if `url`'s scheme prefix is `http` or `https` (case-insensitive,
/// RFC 3986 §3.1).
fn is_http_or_https(url: &str) -> bool {
    url_scheme(url)
        .is_some_and(|s| s.eq_ignore_ascii_case("http") || s.eq_ignore_ascii_case("https"))
}

/// Parse and validate a JMAP base URL.
///
/// The input must be:
/// - non-empty
/// - syntactically valid (parses with [`url::Url::parse`])
/// - http or https scheme (case-insensitive per RFC 3986 §3.1)
/// - origin-only: no explicit path component, no query string, no fragment.
///   "Origin" here means scheme + host + optional port — the form a JMAP
///   server is identified by. Trailing slashes are accepted (the URL
///   parser normalizes them away).
///
/// Examples that PASS: `https://jmap.example.com`,
/// `https://jmap.example.com/`, `https://10.0.0.1:8008`,
/// `http://localhost:8080`.
///
/// Examples that FAIL: `""`, `https://example.com/api`,
/// `https://example.com?query=1`, `https://example.com#fragment`,
/// `ftp://example.com`.
///
/// Extracted from [`JmapClient::new`] so the constructor reads as
/// 'parse-don't-validate' rather than a six-step inline procedure
/// (bd:JMAP-6lsm.26).
fn parse_base_url(base_url: &str) -> Result<url::Url, ClientError> {
    if base_url.is_empty() {
        return Err(ClientError::InvalidArgument(
            "base_url may not be empty".into(),
        ));
    }
    let parsed = url::Url::parse(base_url)
        .map_err(|e| ClientError::InvalidArgument(format!("base_url is not a valid URL: {e}")))?;
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
    Ok(parsed)
}

/// Validate that `url` uses an http or https scheme.
///
/// Called at the top of each public method that accepts a URL parameter
/// (`call`, `subscribe_events`, `upload_blob`, `download_blob`).  This is a
/// defense-in-depth check: the primary protection is
/// [`validate_session_url_schemes`] which rejects bad URLs in the Session
/// document at fetch time.  This check makes each individual call site
/// self-defending against accidentally passing a non-http URL (e.g. from a
/// test fixture or a misused API).
///
/// Returns [`ClientError::InvalidArgument`] if the scheme is not http/https.
pub(crate) fn require_http_url(url: &str) -> Result<(), ClientError> {
    if !is_http_or_https(url) {
        return Err(ClientError::InvalidArgument(format!(
            "URL must have http or https scheme, got: {url:?}"
        )));
    }
    Ok(())
}

/// Validate the *schemes only* of the four session URL fields.
///
/// Three of the four (`upload_url`, `download_url`, `event_source_url`) are
/// RFC 6570 URI templates carrying `{accountId}`, `{blobId}`, `{types}`,
/// etc. They cannot be fully parsed as URLs without first being expanded
/// with the relevant variables, but the scheme prefix is always concrete
/// (templates put the scheme on the left of `://`). This function does the
/// minimal check that the scheme prefix is `http://` or `https://` — it
/// does NOT verify that the templates carry the required variables, that
/// the host is well-formed, or that the path is reachable.
///
/// Renamed from `validate_session_urls` for accuracy (bd:JMAP-6lsm.23):
/// the name implied stronger validation than the function actually
/// performs.
fn validate_session_url_schemes(session: &Session) -> Result<(), ClientError> {
    for url in [
        &session.api_url,
        &session.upload_url,
        &session.download_url,
        &session.event_source_url,
    ] {
        if !is_http_or_https(url) {
            return Err(ClientError::InvalidSession(format!(
                "session URL has non-http/https scheme: {url:?}"
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::decode_utf8_chunk;

    /// Oracle: all-ASCII bytes pushed to buf; raw cleared.
    #[test]
    fn decode_utf8_chunk_all_ascii() {
        let mut raw = b"hello".to_vec();
        let mut buf = String::new();
        decode_utf8_chunk(&mut raw, &mut buf);
        assert_eq!(buf, "hello");
        assert!(raw.is_empty());
    }

    /// Oracle: complete multi-byte codepoint (U+00E9 café = 0xC3 0xA9) pushed fully.
    #[test]
    fn decode_utf8_chunk_complete_multibyte() {
        let mut raw = "café".as_bytes().to_vec();
        let mut buf = String::new();
        decode_utf8_chunk(&mut raw, &mut buf);
        assert_eq!(buf, "café");
        assert!(raw.is_empty());
    }

    /// Oracle: first byte of a 2-byte sequence (U+00E9 = 0xC3 0xA9) arrives alone.
    /// error_len == None (incomplete) — the byte must be RETAINED in raw, not dropped.
    /// Regression test for the bug where valid_up_to.max(1) discarded this byte.
    #[test]
    fn decode_utf8_chunk_incomplete_head_retained() {
        let mut raw = vec![0xC3u8]; // first byte of 2-byte sequence
        let mut buf = String::new();
        decode_utf8_chunk(&mut raw, &mut buf);
        assert_eq!(buf, "", "no complete codepoints to push");
        assert_eq!(raw, vec![0xC3u8], "incomplete head must stay in raw");
    }

    /// Oracle: valid ASCII prefix then first byte of a 2-byte sequence.
    /// Valid prefix goes to buf; incomplete head stays in raw.
    #[test]
    fn decode_utf8_chunk_prefix_then_incomplete_head() {
        let mut raw = vec![b'a', b'b', 0xC3u8];
        let mut buf = String::new();
        decode_utf8_chunk(&mut raw, &mut buf);
        assert_eq!(buf, "ab");
        assert_eq!(raw, vec![0xC3u8], "incomplete head must stay in raw");
    }

    /// Oracle: two-call simulation — incomplete sequence completed on second call.
    /// This is the exact HTTP chunk-split scenario the fix targets.
    #[test]
    fn decode_utf8_chunk_split_sequence_completed() {
        // Chunk 1: only the first byte of U+00E9 (é = 0xC3 0xA9)
        let mut raw = vec![0xC3u8];
        let mut buf = String::new();
        decode_utf8_chunk(&mut raw, &mut buf);
        assert_eq!(raw, vec![0xC3u8], "incomplete head retained after chunk 1");

        // Chunk 2: completion byte
        raw.push(0xA9u8);
        decode_utf8_chunk(&mut raw, &mut buf);
        assert_eq!(buf, "é", "character fully decoded after chunk 2");
        assert!(raw.is_empty());
    }

    /// Oracle: definitively invalid byte (0xFF is never valid in UTF-8) is drained.
    #[test]
    fn decode_utf8_chunk_invalid_byte_drained() {
        let mut raw = vec![0xFFu8];
        let mut buf = String::new();
        decode_utf8_chunk(&mut raw, &mut buf);
        assert_eq!(buf, "");
        assert!(raw.is_empty(), "definitively invalid byte must be drained");
    }

    /// Oracle: valid prefix then definitively invalid byte — prefix pushed and both drained.
    #[test]
    fn decode_utf8_chunk_prefix_then_invalid_drained() {
        let mut raw = vec![b'a', b'b', 0xFFu8];
        let mut buf = String::new();
        decode_utf8_chunk(&mut raw, &mut buf);
        assert_eq!(buf, "ab");
        assert!(raw.is_empty(), "prefix and invalid byte must be drained");
    }
}
