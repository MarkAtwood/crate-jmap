//! Events API delivery: HTTP request verification, the outer envelopes,
//! and Socket Mode frames. The transport (an HTTP endpoint, or a WebSocket
//! to the URL from [`SlackClient::apps_connections_open`](crate::client::SlackClient::apps_connections_open))
//! is the caller's; hand each inner `event` to
//! [`SlackBridge::apply_event`](crate::SlackBridge::apply_event).
//!
//! Oracles:
//! - "Verifying requests from Slack"
//!   (<https://docs.slack.dev/authentication/verifying-requests-from-slack/>):
//!   basestring `v0:<X-Slack-Request-Timestamp>:<raw body>`, HMAC SHA-256
//!   keyed with the signing secret, hex, prefixed `v0=`, compared with
//!   `X-Slack-Signature`; reject timestamps more than five minutes from
//!   local time.
//! - Events API (<https://docs.slack.dev/apis/events-api/>) and
//!   `url_verification`
//!   (<https://docs.slack.dev/reference/events/url_verification/>):
//!   `type` is `url_verification` (answer with `challenge`) or
//!   `event_callback` (with `event`, `event_id`, `team_id`).
//! - "Using Socket Mode"
//!   (<https://docs.slack.dev/apis/events-api/using-socket-mode/>): frames of
//!   `type` `hello`, `disconnect` (`reason`), and envelopes with
//!   `envelope_id`, `type` (`events_api`, …) and `payload`; acknowledge
//!   each envelope by sending `{"envelope_id": …}`.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ring::hmac;
use serde_json::Value;

/// How far a request timestamp may be from local time.
pub const MAX_CLOCK_SKEW: Duration = Duration::from_secs(300);

/// An app's signing secret (Basic Information page).
///
/// The `Debug` impl never shows it.
#[derive(Clone)]
pub struct SigningSecret(String);

impl SigningSecret {
    /// Wrap `secret`.
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }
}

/// Never shows the secret.
impl fmt::Debug for SigningSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SigningSecret(<redacted>)")
    }
}

/// Why a request is not from Slack.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    /// The timestamp is not an integer.
    #[error("malformed X-Slack-Request-Timestamp")]
    MalformedTimestamp,
    /// The timestamp is more than [`MAX_CLOCK_SKEW`] from `now`.
    #[error("X-Slack-Request-Timestamp outside the allowed window")]
    Stale,
    /// The signature is not `v0=` and 64 hex digits.
    #[error("malformed X-Slack-Signature")]
    MalformedSignature,
    /// The signature does not match.
    #[error("X-Slack-Signature does not match")]
    Mismatch,
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Verify a signed Events API (or slash command, interactivity) request.
///
/// `timestamp` and `signature` are the `X-Slack-Request-Timestamp` and
/// `X-Slack-Signature` header values, `body` the raw body before any
/// parsing, `now` the local time. The comparison is constant-time
/// (`ring::hmac::verify`).
///
/// # Errors
///
/// [`SignatureError`] when the request must be rejected.
pub fn verify_signature(
    secret: &SigningSecret,
    timestamp: &str,
    body: &[u8],
    signature: &str,
    now: SystemTime,
) -> Result<(), SignatureError> {
    let ts: u64 = timestamp
        .trim()
        .parse()
        .map_err(|_| SignatureError::MalformedTimestamp)?;
    let now = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SignatureError::Stale)?
        .as_secs();
    if now.abs_diff(ts) > MAX_CLOCK_SKEW.as_secs() {
        return Err(SignatureError::Stale);
    }
    let tag = signature
        .strip_prefix("v0=")
        .and_then(hex_decode)
        .filter(|t| t.len() == 32)
        .ok_or(SignatureError::MalformedSignature)?;
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.0.as_bytes());
    let mut base = Vec::with_capacity(body.len() + timestamp.len() + 4);
    base.extend_from_slice(b"v0:");
    base.extend_from_slice(timestamp.trim().as_bytes());
    base.push(b':');
    base.extend_from_slice(body);
    hmac::verify(&key, &base, &tag).map_err(|_| SignatureError::Mismatch)
}

/// An Events API request body.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum HttpEvent {
    /// `url_verification`: answer the request with this `challenge`.
    UrlVerification {
        /// The value to echo.
        challenge: String,
    },
    /// `event_callback`: pass `event` to the bridge.
    Callback {
        /// The workspace.
        team_id: Option<String>,
        /// The delivery id; Slack retries a delivery with the same id.
        event_id: Option<String>,
        /// The inner event object.
        event: Value,
    },
    /// `app_rate_limited`: Slack is dropping this app's events for the
    /// minute; plan a resync (<https://docs.slack.dev/apis/events-api/>).
    AppRateLimited,
    /// Any other `type`.
    Other(Value),
}

/// Parse an Events API request body (or a Socket Mode `events_api`
/// payload, which has the same shape).
///
/// # Errors
///
/// The [`serde_json::Error`] when `body` is not JSON.
pub fn parse_http_event(body: &[u8]) -> Result<HttpEvent, serde_json::Error> {
    let v: Value = serde_json::from_slice(body)?;
    Ok(classify(v))
}

fn classify(mut v: Value) -> HttpEvent {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    match v.get("type").and_then(Value::as_str) {
        Some("url_verification") => HttpEvent::UrlVerification {
            challenge: s(&v, "challenge").unwrap_or_default(),
        },
        Some("event_callback") => HttpEvent::Callback {
            team_id: s(&v, "team_id"),
            event_id: s(&v, "event_id"),
            event: v.get_mut("event").map(Value::take).unwrap_or(Value::Null),
        },
        Some("app_rate_limited") => HttpEvent::AppRateLimited,
        _ => HttpEvent::Other(v),
    }
}

/// A Socket Mode frame.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum SocketFrame {
    /// `hello`: the connection is ready.
    Hello,
    /// `disconnect`: Slack will close this connection; open a new one
    /// (`reason` is `warning`, `refresh_requested`, `link_disabled`, …).
    Disconnect {
        /// Why.
        reason: String,
    },
    /// An `events_api` envelope. Acknowledge it with [`ack`] first, then
    /// apply the payload.
    Event {
        /// The id to acknowledge.
        envelope_id: String,
        /// The payload, an `event_callback` like an HTTP delivery.
        payload: HttpEvent,
    },
    /// Another envelope (`slash_commands`, `interactive`, …): acknowledge
    /// it; the bridge has no use for it.
    Other {
        /// The id to acknowledge, when the frame has one.
        envelope_id: Option<String>,
    },
}

/// Parse a Socket Mode text frame.
///
/// # Errors
///
/// The [`serde_json::Error`] when `frame` is not JSON.
pub fn parse_socket_frame(frame: &str) -> Result<SocketFrame, serde_json::Error> {
    let mut v: Value = serde_json::from_str(frame)?;
    let envelope_id = v
        .get("envelope_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(match (v.get("type").and_then(Value::as_str), envelope_id) {
        (Some("hello"), _) => SocketFrame::Hello,
        (Some("disconnect"), _) => SocketFrame::Disconnect {
            reason: v
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        },
        (Some("events_api"), Some(envelope_id)) => SocketFrame::Event {
            envelope_id,
            payload: classify(v.get_mut("payload").map(Value::take).unwrap_or(Value::Null)),
        },
        (_, envelope_id) => SocketFrame::Other { envelope_id },
    })
}

/// The acknowledgement frame for envelope `envelope_id`.
pub fn ack(envelope_id: &str) -> String {
    serde_json::json!({ "envelope_id": envelope_id }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example of "Verifying requests from Slack" (signing
    /// secret, timestamp, slash-command body, expected signature),
    /// independently confirmed with `openssl dgst -sha256 -hmac`.
    const SECRET: &str = "8f742231b10e8888abcd99yyyzzz85a5";
    const TS: &str = "1531420618";
    const BODY: &str = "token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J&team_domain=testteamnow&channel_id=G8PSS9T3V&channel_name=foobar&user_id=U2CERLKJA&user_name=roadrunner&command=%2Fwebhook-collect&text=&response_url=https%3A%2F%2Fhooks.slack.com%2Fcommands%2FT1DC2JH3J%2F397700885554%2F96rGlfmibIGlgcZRskXaIFfN&trigger_id=398738663015.47445629121.803a0bc887a14d10d2c447fce8b6703c";
    const SIG: &str = "v0=a2114d57b48eac39b9ad189dd8316235a7b4a8d21a10bd27519666489c69b503";

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn slack_worked_example_verifies() {
        let secret = SigningSecret::new(SECRET);
        assert_eq!(
            verify_signature(&secret, TS, BODY.as_bytes(), SIG, at(1_531_420_618 + 10)),
            Ok(())
        );
    }

    #[test]
    fn tampered_body_or_signature_fails() {
        let secret = SigningSecret::new(SECRET);
        let now = at(1_531_420_618);
        let mut body = BODY.as_bytes().to_vec();
        body[0] = b'T';
        assert_eq!(
            verify_signature(&secret, TS, &body, SIG, now),
            Err(SignatureError::Mismatch)
        );
        assert_eq!(
            verify_signature(&SigningSecret::new("other"), TS, BODY.as_bytes(), SIG, now),
            Err(SignatureError::Mismatch)
        );
        assert_eq!(
            verify_signature(&secret, TS, BODY.as_bytes(), "v1=abcd", now),
            Err(SignatureError::MalformedSignature)
        );
        assert_eq!(
            verify_signature(&secret, "soon", BODY.as_bytes(), SIG, now),
            Err(SignatureError::MalformedTimestamp)
        );
    }

    /// "If the request timestamp is more than five minutes from local
    /// time, it could be a replay attack".
    #[test]
    fn stale_timestamps_fail() {
        let secret = SigningSecret::new(SECRET);
        assert_eq!(
            verify_signature(&secret, TS, BODY.as_bytes(), SIG, at(1_531_420_618 + 301)),
            Err(SignatureError::Stale)
        );
        assert_eq!(
            verify_signature(&secret, TS, BODY.as_bytes(), SIG, at(1_531_420_618 - 301)),
            Err(SignatureError::Stale)
        );
    }

    #[test]
    fn signing_secret_debug_is_redacted() {
        let shown = format!("{:?}", SigningSecret::new("canary-signing-secret"));
        assert!(!shown.contains("canary"), "{shown}");
    }

    /// url_verification example from its reference page.
    #[test]
    fn url_verification() {
        let body = br#"{"token":"Jhj5dZrVaK7ZwHHjRyZWjbDl","challenge":"3eZbrw1aBm2rZgRNFdxV2595E9CY3gmdALWMmHkvFXO7tYXAYM8P","type":"url_verification"}"#;
        assert_eq!(
            parse_http_event(body).expect("json"),
            HttpEvent::UrlVerification {
                challenge: "3eZbrw1aBm2rZgRNFdxV2595E9CY3gmdALWMmHkvFXO7tYXAYM8P".to_owned()
            }
        );
    }

    #[test]
    fn event_callback() {
        let body = br#"{"token":"XXYYZZ","team_id":"T123ABC456","api_app_id":"A123ABC456","event":{"type":"channel_deleted","channel":"C123ABC456"},"type":"event_callback","event_id":"Ev123ABC456","event_time":123456789}"#;
        let HttpEvent::Callback {
            team_id,
            event_id,
            event,
        } = parse_http_event(body).expect("json")
        else {
            panic!("not a callback");
        };
        assert_eq!(team_id.as_deref(), Some("T123ABC456"));
        assert_eq!(event_id.as_deref(), Some("Ev123ABC456"));
        assert_eq!(event["type"], "channel_deleted");
    }

    /// Frames from "Using Socket Mode".
    #[test]
    fn socket_frames() {
        assert_eq!(
            parse_socket_frame(r#"{"type":"hello","num_connections":1}"#).expect("json"),
            SocketFrame::Hello
        );
        assert_eq!(
            parse_socket_frame(r#"{"type":"disconnect","reason":"refresh_requested","debug_info":{"host":"wss-111.slack.com"}}"#)
                .expect("json"),
            SocketFrame::Disconnect {
                reason: "refresh_requested".to_owned()
            }
        );
        let frame = r#"{"payload":{"type":"event_callback","event":{"type":"app_home_opened"}},"envelope_id":"dbdd0ef3-1543-4f94-bfb4-133d0e6c1545","type":"events_api","accepts_response_payload":false}"#;
        let SocketFrame::Event {
            envelope_id,
            payload,
        } = parse_socket_frame(frame).expect("json")
        else {
            panic!("not an event");
        };
        assert_eq!(envelope_id, "dbdd0ef3-1543-4f94-bfb4-133d0e6c1545");
        assert!(matches!(payload, HttpEvent::Callback { .. }));
        assert_eq!(
            ack(&envelope_id),
            r#"{"envelope_id":"dbdd0ef3-1543-4f94-bfb4-133d0e6c1545"}"#
        );
        assert_eq!(
            parse_socket_frame(r#"{"envelope_id":"e1","type":"slash_commands","payload":{}}"#)
                .expect("json"),
            SocketFrame::Other {
                envelope_id: Some("e1".to_owned())
            }
        );
    }
}
