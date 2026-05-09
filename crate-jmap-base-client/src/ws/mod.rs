//! WebSocket transport for JMAP (RFC 8887).
//!
//! Provides [`connect_ws`] which establishes a WebSocket connection and
//! returns a [`WsSession`] for sending and receiving frames.
//!
//! URL source: `Session::capabilities["urn:ietf:params:jmap:websocket"].url`
//! (the session document advertises the WebSocket endpoint).

use std::str::FromStr as _;

use futures::SinkExt as _;
use futures::StreamExt as _;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

use crate::push::StateChange;

/// Wire frame sent from the client to the server over WebSocket (RFC 8887 §4.3.2).
///
/// Wraps a [`jmap_types::JmapRequest`] and injects the mandatory `@type: "Request"`
/// field (and optional `id`) in a single `serde_json::to_string` pass, avoiding
/// the `to_value` + mutation + `to_string` double-serialization that the naive
/// approach requires.
#[derive(serde::Serialize)]
struct WsRequestFrame<'a> {
    /// RFC 8887 §4.3.2 — every JMAP request frame MUST carry "@type": "Request".
    #[serde(rename = "@type")]
    ws_type: &'static str,
    /// Optional correlation ID echoed back in the server's Response frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a str>,
    /// The JMAP request payload; flattened into the enclosing JSON object.
    #[serde(flatten)]
    inner: &'a jmap_types::JmapRequest,
}

/// Maximum WebSocket message size (1 MiB), consistent with the SSE frame limit.
/// Prevents a misbehaving or hostile server from forcing the client to buffer
/// large messages over the event connection.
/// Default per-message / per-frame byte cap for WebSocket connections opened
/// via [`connect_ws`] (which does not take a limit parameter). Callers that
/// need a different cap should use [`connect_ws_with_limit`] or the
/// [`crate::JmapClient::connect_ws_session`] convenience method which
/// reads the `max_ws_message` field from `ClientConfig`. Default: 1 MiB.
pub const DEFAULT_WS_MAX_MESSAGE_BYTES: usize = 1 << 20;

/// A parsed frame received from the JMAP WebSocket.
///
/// Marked `#[non_exhaustive]` because the spec may define additional
/// `@type` values in future revisions.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum WsFrame {
    /// RFC 8620 §7.1 StateChange — one or more object types have changed
    /// state; client must re-fetch the affected data types.
    StateChange(StateChange),
    /// RFC 8887 Response — reply to a JMAP request sent on this connection.
    Response(jmap_types::JmapResponse),
    /// Unrecognized `@type` — silently ignored per forward-compatibility rules
    /// (RFC 8887 §4.3.1: clients SHOULD ignore unknown message types).
    ///
    /// Also produced when a known type (`"Response"` or `"StateChange"`) fails
    /// to deserialize — `type_name` will be `"Response"` or `"StateChange"` in
    /// that case, which can signal server misbehavior or a schema version
    /// mismatch. Callers that log unknown frames should check for these names.
    Unknown {
        type_name: String,
        raw: serde_json::Value,
    },
}

type Inner =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// An established JMAP WebSocket session (RFC 8887).
///
/// Call [`next_frame`](WsSession::next_frame) in a loop to receive events.
/// Use [`send_request`](WsSession::send_request) to transmit JMAP requests.
///
/// The caller is responsible for reconnecting after the stream ends or returns
/// a transport error. Use exponential backoff.
pub struct WsSession {
    sink: futures::stream::SplitSink<Inner, Message>,
    stream: futures::stream::SplitStream<Inner>,
}

/// Maximum number of consecutive non-Text non-Close non-Binary frames
/// (Ping, Pong, Frame, etc.) `next_frame` will silently skip in a single call.
///
/// Tungstenite handles ping/pong at the protocol layer, so seeing them at the
/// `Message` layer is unusual but legal — we skip them. A misbehaving or
/// hostile server that floods the stream with no-op frames could otherwise
/// starve a caller of `next_frame` indefinitely; this cap surfaces an
/// `UnexpectedResponse` error before that can happen. 64 is high enough that
/// a normal connection never trips it (typical SSE/WS streams interleave at
/// most a handful of pings between data frames) and low enough that the
/// caller doesn't wait long if a bad server is talking nonsense.
///
/// `Binary` frames are NOT counted here — they violate RFC 8887 §4.1 and
/// surface as `UnexpectedResponse` immediately on the first occurrence.
const MAX_CONSECUTIVE_NON_TEXT_FRAMES: usize = 64;

/// Classify a single tungstenite [`Message`] into a [`MessageDisposition`]
/// that tells the [`WsSession::next_frame`] loop what to do with it.
///
/// Extracted as a free function so the policy is unit-testable without a
/// real WebSocket: see the inline test module. Pure function over the
/// message variant.
fn classify_message(msg: &Message) -> MessageDisposition {
    match msg {
        Message::Text(_) => MessageDisposition::Text,
        Message::Close(_) => MessageDisposition::Close,
        Message::Binary(_) => MessageDisposition::Binary,
        // Ping, Pong, Frame, and any future variants: skip, but count.
        _ => MessageDisposition::Skip,
    }
}

/// Decision a `next_frame` loop iteration takes after looking at one
/// [`Message`]. See [`classify_message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageDisposition {
    /// Text frame: hand to `parse_ws_frame` and return its result.
    Text,
    /// Close frame: end the stream by returning `None`.
    Close,
    /// Binary frame: violates RFC 8887 §4.1; surface as
    /// `UnexpectedResponse` immediately on the first occurrence.
    Binary,
    /// Ping / Pong / Frame / future variants: silently skip and continue
    /// the loop, subject to [`MAX_CONSECUTIVE_NON_TEXT_FRAMES`].
    Skip,
}

impl WsSession {
    /// Receive the next parsed frame from the server.
    ///
    /// Returns `None` when the server has cleanly closed the connection.
    /// Returns `Some(Err(...))` on parse failure, transport error, RFC 8887
    /// §4.1 violation (Binary frame), or starvation cap (more than 64
    /// consecutive Ping/Pong/Frame messages — see the private
    /// `MAX_CONSECUTIVE_NON_TEXT_FRAMES` constant for the exact value).
    /// After a transport error the connection is broken and `next_frame`
    /// must not be called again. After an `UnexpectedResponse` error the
    /// underlying stream is still healthy — the caller may choose to
    /// ignore it and retry, or to disconnect.
    pub async fn next_frame(&mut self) -> Option<Result<WsFrame, crate::error::ClientError>> {
        let mut consecutive_skips = 0usize;
        loop {
            let msg = match self.stream.next().await? {
                Ok(m) => m,
                Err(e) => return Some(Err(crate::error::ClientError::from_ws(e))),
            };
            match classify_message(&msg) {
                MessageDisposition::Text => {
                    let Message::Text(text) = msg else {
                        // Unreachable: classify_message returned Text only for
                        // Message::Text. Defensive in case the variant grows.
                        return Some(Err(crate::error::ClientError::UnexpectedResponse(
                            "WebSocket: classify_message returned Text for non-Text variant".into(),
                        )));
                    };
                    return Some(parse_ws_frame(&text));
                }
                MessageDisposition::Close => return None,
                MessageDisposition::Binary => {
                    // RFC 8887 §4.1: JMAP only uses text frames. Surface the
                    // violation; underlying stream is still healthy so the
                    // caller can choose to retry next_frame if it wants.
                    return Some(Err(crate::error::ClientError::UnexpectedResponse(
                        "WebSocket: server sent Binary frame; RFC 8887 §4.1 mandates text frames"
                            .into(),
                    )));
                }
                MessageDisposition::Skip => {
                    consecutive_skips = consecutive_skips.saturating_add(1);
                    if consecutive_skips > MAX_CONSECUTIVE_NON_TEXT_FRAMES {
                        return Some(Err(crate::error::ClientError::UnexpectedResponse(
                            format!(
                                "WebSocket: exceeded {MAX_CONSECUTIVE_NON_TEXT_FRAMES} consecutive non-text frames; possible server misbehaviour"
                            ),
                        )));
                    }
                }
            }
        }
    }

    /// Send a raw text frame over the WebSocket connection.
    ///
    /// Used by extension crates to send non-JMAP frames (e.g., JMAP Chat
    /// ephemeral stream control messages).
    pub async fn send_text(&mut self, text: String) -> Result<(), crate::error::ClientError> {
        self.sink
            .send(Message::Text(text.into()))
            .await
            .map_err(crate::error::ClientError::from_ws)
    }

    /// Send a JMAP request over the WebSocket connection.
    ///
    /// Serializes `req` and injects `"@type": "Request"` into the outgoing
    /// JSON object as required by RFC 8887 §4.3.2.  The optional `id` is
    /// echoed back in the corresponding `Response` frame, enabling out-of-order
    /// correlation.
    ///
    /// # Errors
    ///
    /// Returns `ClientError::Serialize` if `req` cannot be serialized, or
    /// `ClientError::WebSocket` on a transport failure.
    pub async fn send_request(
        &mut self,
        req: &jmap_types::JmapRequest,
        id: Option<&str>,
    ) -> Result<(), crate::error::ClientError> {
        // Wrap req in WsRequestFrame to inject @type and optional id in one
        // serialization pass (no intermediate serde_json::Value allocation).
        let frame = WsRequestFrame {
            ws_type: "Request",
            id,
            inner: req,
        };
        let text = serde_json::to_string(&frame).map_err(crate::error::ClientError::Serialize)?;
        self.sink
            .send(Message::Text(text.into()))
            .await
            .map_err(crate::error::ClientError::from_ws)
    }
}

/// Parse a raw WebSocket text frame into a `WsFrame`.
///
/// Two passes over `text`:
///
/// 1. Parse to [`serde_json::Value`] to extract `@type` (and to keep a
///    structured fallback alive for the Unknown branch).
/// 2. For the typed branches (`StateChange`, `Response`), call
///    [`serde_json::from_str`] directly against the original `text`.
///
/// The previous shape `let raw = val.clone(); from_value::<T>(val)` paid a
/// deep Value clone on every successful frame even though `raw` was thrown
/// away. For 1-MiB-cap WS messages on a hot push path, the clone allocates
/// a HashMap per `Value::Object` and a `String` per `Value::String` and
/// dropped them moments later. Two text parses are cheaper for typical
/// payload shapes than one parse + one deep Value clone, and the borrow
/// checker no longer needs ownership tricks (bd:JMAP-6lsm.11).
fn parse_ws_frame(text: &str) -> Result<WsFrame, crate::error::ClientError> {
    let val: serde_json::Value =
        serde_json::from_str(text).map_err(crate::error::ClientError::Parse)?;

    let type_name = val
        .get("@type")
        .and_then(|v| v.as_str())
        .unwrap_or("<no @type>")
        .to_owned();

    match type_name.as_str() {
        // A malformed StateChange is degraded to Unknown rather than a
        // transport error. A single bad server frame must not kill the
        // entire WebSocket connection; only tungstenite transport errors
        // warrant a reconnect. The `val` we already parsed is the Unknown
        // payload — no clone needed.
        "StateChange" => match serde_json::from_str::<StateChange>(text) {
            Ok(sc) => Ok(WsFrame::StateChange(sc)),
            Err(_) => Ok(WsFrame::Unknown {
                type_name,
                raw: val,
            }),
        },
        // Same degradation policy for malformed Response frames.
        "Response" => match serde_json::from_str::<jmap_types::JmapResponse>(text) {
            Ok(r) => Ok(WsFrame::Response(r)),
            Err(_) => Ok(WsFrame::Unknown {
                type_name,
                raw: val,
            }),
        },
        _ => Ok(WsFrame::Unknown {
            type_name,
            raw: val,
        }),
    }
}

/// Open a JMAP WebSocket connection (RFC 8887).
///
/// `ws_url` must come from the session document's WebSocket capability URL
/// (a `wss://` endpoint in production; `ws://` is accepted in tests).
///
/// `auth_header` is an optional `(header-name, header-value)` pair injected
/// into the WebSocket upgrade request. Pass `None` when the server does not
/// require authentication headers on the WebSocket handshake.
///
/// Returns `ClientError::InvalidArgument` if the URL scheme is not
/// `ws://` or `wss://`, preventing accidental use with untrusted URLs.
///
/// The returned [`WsSession`] provides [`WsSession::next_frame`] for receiving
/// events. The caller is responsible for reconnecting after disconnect with
/// exponential backoff.
///
/// Uses [`DEFAULT_WS_MAX_MESSAGE_BYTES`] as the per-message / per-frame cap.
/// Callers that need a different cap should use [`connect_ws_with_limit`] or
/// [`crate::JmapClient::connect_ws_session`] (which reads `ClientConfig::max_ws_message`).
pub async fn connect_ws(
    ws_url: &str,
    auth_header: Option<(&str, &str)>,
) -> Result<WsSession, crate::error::ClientError> {
    connect_ws_with_limit(ws_url, auth_header, DEFAULT_WS_MAX_MESSAGE_BYTES).await
}

/// Establish a WebSocket connection with an explicit per-message / per-frame
/// byte cap.
///
/// Same contract as [`connect_ws`] but lets the caller pin the
/// `max_message_size` / `max_frame_size` config passed to tungstenite.
/// Useful when the JMAP server is known to send larger pushes than the
/// 1 MiB default (e.g. some Mailbox/changes push payloads on accounts with
/// many mailboxes can exceed 1 MiB).
///
/// `max_message_bytes` MUST be > 0; tungstenite treats `Some(0)` as
/// "no message of any size is acceptable" which is a misconfiguration trap.
/// We surface `ClientError::InvalidArgument` instead.
pub async fn connect_ws_with_limit(
    ws_url: &str,
    auth_header: Option<(&str, &str)>,
    max_message_bytes: usize,
) -> Result<WsSession, crate::error::ClientError> {
    if max_message_bytes == 0 {
        return Err(crate::error::ClientError::InvalidArgument(
            "connect_ws_with_limit: max_message_bytes must be > 0".to_owned(),
        ));
    }
    // Validate scheme to prevent SSRF via a compromised or MITM'd session.
    // Case-insensitive check per RFC 3986 §3.1: lowercase the URL before
    // comparing so that `WS://` and `wss://` are both accepted.  The
    // original (unmodified) URL is passed to tungstenite and kept in error
    // messages for diagnostics.
    let ws_url_lc = ws_url.to_ascii_lowercase();
    if !ws_url_lc.starts_with("ws://") && !ws_url_lc.starts_with("wss://") {
        return Err(crate::error::ClientError::InvalidArgument(format!(
            "WebSocket URL must start with ws:// or wss://, got: {ws_url:?}"
        )));
    }

    let mut request = ws_url
        .into_client_request()
        .map_err(crate::error::ClientError::from_ws)?;

    if let Some((name, value)) = auth_header {
        let hdr_name = http::HeaderName::from_str(name).map_err(|e| {
            crate::error::ClientError::InvalidArgument(format!("invalid auth header name: {e}"))
        })?;
        let hdr_value = http::HeaderValue::from_str(value).map_err(|_| {
            crate::error::ClientError::InvalidArgument("invalid auth header value".to_owned())
        })?;
        request.headers_mut().insert(hdr_name, hdr_value);
    }

    // WebSocketConfig is #[non_exhaustive] in tungstenite; use Default + field assignment.
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(max_message_bytes);
    config.max_frame_size = Some(max_message_bytes);

    // Apply a 10-second connect timeout, consistent with the HTTP transport's
    // connect_timeout in DefaultTransport/CustomCaTransport.  tungstenite does
    // not expose a connect timeout parameter, so we wrap at the Future level.
    // A stalled TCP or TLS handshake would otherwise block indefinitely.
    let connect_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio_tungstenite::connect_async_with_config(request, Some(config), false),
    )
    .await
    .map_err(|_elapsed| {
        // Synthesize an Io-kind transport error to surface the timeout
        // through the public WebSocketError accessors (is_io() will be
        // true). The third-party error type is constructed locally and
        // immediately wrapped, so it does not leak to callers.
        crate::error::ClientError::from_ws(tokio_tungstenite::tungstenite::Error::Io(
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "WebSocket connect timed out after 10 seconds",
            ),
        ))
    })?;
    let (ws_stream, _response) = connect_result.map_err(crate::error::ClientError::from_ws)?;

    let (sink, stream) = ws_stream.split();
    Ok(WsSession { sink, stream })
}

impl std::fmt::Debug for WsSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsSession").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify WsFrame does not contain ChatTyping or ChatPresence variants.
    /// This exhaustive match will fail to compile if either variant is reintroduced.
    #[test]
    fn ws_frame_has_no_chat_variants() {
        let frame = WsFrame::Unknown {
            type_name: "test".to_owned(),
            raw: serde_json::Value::Null,
        };
        match frame {
            WsFrame::StateChange(_) => {}
            WsFrame::Response(_) => {}
            WsFrame::Unknown { .. } => {}
        }
    }

    /// Oracle: parse_ws_frame dispatches on @type field and produces a typed StateChange.
    /// Wire format from RFC 8620 §7.1.1 example.
    #[test]
    fn parse_state_change() {
        let json = r#"{"@type":"StateChange","changed":{"account1":{"Mail":"s2"}}}"#;
        let frame = parse_ws_frame(json).expect("must parse");
        match frame {
            WsFrame::StateChange(sc) => {
                let account = sc
                    .changed
                    .get("account1")
                    .expect("account1 must be present");
                assert_eq!(account.get("Mail").map(|s| s.as_ref()), Some("s2"));
            }
            other => panic!("expected StateChange, got {other:?}"),
        }
    }

    /// Oracle: a StateChange with missing `changed` field degrades to Unknown.
    #[test]
    fn parse_malformed_state_change_degrades_to_unknown() {
        let json = r#"{"@type":"StateChange","unexpected_field":42}"#;
        let frame = parse_ws_frame(json).expect("must not error");
        match frame {
            WsFrame::Unknown { type_name, .. } => assert_eq!(type_name, "StateChange"),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    /// Oracle: parse_ws_frame returns Unknown for unrecognized @type.
    /// Derived from parse_unknown_type test in source ws/mod.rs.
    #[test]
    fn parse_unknown_type() {
        let json = r#"{"@type":"FutureEvent","foo":"bar"}"#;
        let frame = parse_ws_frame(json).expect("must parse");
        match frame {
            WsFrame::Unknown { type_name, .. } => assert_eq!(type_name, "FutureEvent"),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    /// Oracle: parse_ws_frame returns Unknown for missing @type.
    /// Derived from parse_missing_type_field test in source ws/mod.rs.
    #[test]
    fn parse_missing_type_field() {
        let json = r#"{"foo":"bar"}"#;
        let frame = parse_ws_frame(json).expect("must parse");
        assert!(matches!(frame, WsFrame::Unknown { .. }));
    }

    /// Oracle: parse_ws_frame returns Err(Parse) for invalid JSON.
    /// Derived from parse_invalid_json_returns_parse_error test in source ws/mod.rs.
    #[test]
    fn parse_invalid_json_returns_parse_error() {
        let err = parse_ws_frame("not json").expect_err("must fail");
        assert!(matches!(err, crate::error::ClientError::Parse(_)));
    }

    /// Oracle: RFC 8887 §4.3.2 — every JMAP request sent over WebSocket MUST
    /// include "@type": "Request".  Tests WsRequestFrame serde directly to
    /// verify the #[serde(rename = "@type")] attribute and flatten are correct.
    #[test]
    fn send_request_includes_at_type_request() {
        let req = jmap_types::JmapRequest::new(
            vec!["urn:ietf:params:jmap:core".to_owned()],
            vec![],
            None,
        );
        let frame = WsRequestFrame {
            ws_type: "Request",
            id: None,
            inner: &req,
        };
        let serialized = serde_json::to_string(&frame).expect("WsRequestFrame must serialize");
        assert!(
            serialized.contains("\"@type\":\"Request\""),
            "RFC 8887 §4.3.2 requires @type:Request in outgoing WS frames; got: {serialized}"
        );
    }

    /// Oracle: RFC 8887 §4.3.2 — optional `id` field is echoed in the response.
    /// When an id is supplied, WsRequestFrame must include it in the serialized frame.
    #[test]
    fn send_request_includes_id_when_provided() {
        let req = jmap_types::JmapRequest::new(
            vec!["urn:ietf:params:jmap:core".to_owned()],
            vec![],
            None,
        );
        let frame = WsRequestFrame {
            ws_type: "Request",
            id: Some("req-42"),
            inner: &req,
        };
        let serialized = serde_json::to_string(&frame).expect("WsRequestFrame must serialize");
        assert!(
            serialized.contains("\"id\":\"req-42\""),
            "RFC 8887 §4.3.2 optional id must be present when provided; got: {serialized}"
        );
    }

    /// Oracle: RFC 8887 §4.3.2 — when id is None, no `id` field appears in the frame.
    /// WsRequestFrame uses skip_serializing_if to omit the field entirely.
    #[test]
    fn send_request_omits_id_when_none() {
        let req = jmap_types::JmapRequest::new(
            vec!["urn:ietf:params:jmap:core".to_owned()],
            vec![],
            None,
        );
        let frame = WsRequestFrame {
            ws_type: "Request",
            id: None,
            inner: &req,
        };
        let serialized = serde_json::to_string(&frame).expect("WsRequestFrame must serialize");
        assert!(
            !serialized.contains("\"id\":"),
            "RFC 8887 §4.3.2: no id field must appear when id is None; got: {serialized}"
        );
    }

    /// Oracle: connect_ws must reject http:// and https:// URLs with InvalidArgument.
    ///
    /// This is the documented SSRF prevention guard: a compromised or MITM'd session
    /// could send an http:// URL; we must not follow it as a WebSocket URL.
    /// The scheme check runs before any network I/O.
    /// Derived from connect_ws_rejects_non_ws_schemes test in source ws/mod.rs.
    #[tokio::test]
    async fn connect_ws_rejects_non_ws_schemes() {
        for bad_url in &["http://host/", "https://host/", "ftp://host/"] {
            let result = connect_ws(bad_url, None).await.map(|_| ());
            match result {
                Err(crate::error::ClientError::InvalidArgument(_)) => {}
                other => panic!("expected InvalidArgument for {bad_url:?}, got {other:?}"),
            }
        }
    }

    // -----------------------------------------------------------------------
    // classify_message — bd:JMAP-6lsm.6
    // -----------------------------------------------------------------------

    /// Oracle: Text frames classify as Text. The independent oracle is
    /// the next_frame contract in the docstring above.
    #[test]
    fn classify_text_message() {
        let m = Message::Text("hi".into());
        assert_eq!(classify_message(&m), MessageDisposition::Text);
    }

    /// Oracle: Close frames classify as Close, ending the stream.
    #[test]
    fn classify_close_message() {
        let m = Message::Close(None);
        assert_eq!(classify_message(&m), MessageDisposition::Close);
    }

    /// Oracle: Binary frames violate RFC 8887 §4.1 and must classify as
    /// Binary so the next_frame loop surfaces UnexpectedResponse rather
    /// than silently skipping (the bug JMAP-6lsm.6 fixes). The independent
    /// oracle is RFC 8887 §4.1.
    #[test]
    fn classify_binary_message_is_not_skipped() {
        let m = Message::Binary(vec![1, 2, 3].into());
        assert_eq!(classify_message(&m), MessageDisposition::Binary);
        assert_ne!(
            classify_message(&m),
            MessageDisposition::Skip,
            "Binary must NOT be silently skipped (RFC 8887 §4.1)"
        );
    }

    /// Oracle: Ping/Pong frames classify as Skip. Tungstenite handles
    /// them at the protocol layer, so seeing them at the Message layer
    /// is unusual but legal — skip and continue.
    #[test]
    fn classify_ping_pong_messages_are_skipped() {
        let ping = Message::Ping(vec![].into());
        let pong = Message::Pong(vec![].into());
        assert_eq!(classify_message(&ping), MessageDisposition::Skip);
        assert_eq!(classify_message(&pong), MessageDisposition::Skip);
    }

    /// Tripwire: the consecutive-skip cap is the documented value.
    /// A future retune will fail this test loudly so the change is
    /// visible in CI. Documented value is 64 (see the const docstring).
    #[test]
    fn consecutive_skip_cap_matches_documented_value() {
        assert_eq!(MAX_CONSECUTIVE_NON_TEXT_FRAMES, 64);
    }
}
