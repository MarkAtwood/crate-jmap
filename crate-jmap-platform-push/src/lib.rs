//! JMAP push delivery through the vendor push platforms: Google FCM, Apple
//! APNs, Amazon ADM, Huawei Push Kit, Xiaomi MiPush and Microsoft WNS.
//!
//! This is a support crate (workspace AGENTS.md §Support crates): it does
//! network I/O and holds platform credentials, and no `*-types` or
//! `*-server` crate depends on it. Decision 40 (docs/design/DECISIONS.md)
//! makes delivery to these platforms the deploying server's job; this
//! crate is an optional helper for it, as `jmap-web-push` is for Web Push.
//!
//! The JMAP push payload (`StateChange`, `ChatMessagePush`,
//! `VTCCallPush`, ...) is built elsewhere. A [`Message`] carries it with
//! its urgency, time to live and [`PushKind`]; each platform module wraps
//! it in that platform's envelope as jmap-push-platform-guide.md and
//! jmap-chat-push-platform-guide.md §Payload encoding describe, and sends
//! it to one device:
//!
//! | Module | Platform | Destination |
//! |---|---|---|
//! | [`fcm`] | Firebase Cloud Messaging HTTP v1 | registration token |
//! | [`apns`] | Apple Push Notification service | device token (hex) |
//! | [`adm`] | Amazon Device Messaging | registration ID |
//! | [`hpk`] | Huawei Push Kit | push token |
//! | [`mipush`] | Xiaomi MiPush | regId |
//! | [`wns`] | Windows Push Notification Services | channel URI |
//!
//! Every pusher implements [`PlatformPusher`], so the push layer can hold
//! them as `Box<dyn PlatformPusher>` and route by subscription. Each push
//! ends in an [`Outcome`]: [`Outcome::Gone`] only when the platform says the
//! device registration is gone (jmap-push-platform-guide §Credential errors
//! are not subscription errors: "Servers MUST only destroy the subscription
//! when the platform explicitly tells the server the token or endpoint is
//! gone").
//!
//! Only Web Push is end-to-end encrypted. Every platform here can read the
//! payload (jmap-push-platform-guide §Security, Vendor-visible payloads).

#![forbid(unsafe_code)]

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{HeaderMap, RETRY_AFTER};
use serde::Serialize;

mod auth;

pub mod adm;
pub mod apns;
pub mod fcm;
pub mod hpk;
pub mod mipush;

/// The push urgency (RFC 8030 §5.3), shared with every JMAP crate that
/// classifies a push. Each platform maps it to its own priority
/// (jmap-push-platform-guide §Urgency under each platform).
pub use jmap_types::Urgency;

pub use jmap_types;
pub use reqwest;
pub use url;

/// The per-request timeout of the clients the pushers' `new` constructors
/// build.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// The default key the payload goes under on platforms whose envelope is a
/// map: FCM and ADM `data`, the APNs payload. jmap-chat-push-platform-guide
/// §Payload encoding: "this guide uses `"jmap-push"`".
pub const DEFAULT_DATA_KEY: &str = "jmap-push";

/// How a push is presented on the device.
///
/// jmap-push-platform-guide §Use data-only messages: "Servers MUST use
/// data messages, not display messages, except for an APNs alert push that
/// sets `"mutable-content": 1`". Every kind here is a data message except
/// [`PushKind::Alert`] on APNs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PushKind {
    /// A data message: the app wakes and decides what to show. On APNs a
    /// background push (`apns-push-type: background`, `"content-available":
    /// 1`).
    #[default]
    Data,
    /// On APNs, an alert push with `"mutable-content": 1` and `body` as the
    /// placeholder a Notification Service Extension rewrites before display
    /// (jmap-push-platform-guide §Apple APNs, Delivery modes). Pushes of
    /// urgency `low` or `very-low` still go as background pushes (§Apple
    /// APNs, Urgency). Every other platform sends a data message.
    Alert {
        /// The placeholder `aps.alert.body`.
        body: String,
    },
    /// An incoming call ring (jmap-vtc-ring-push-guide.md). On APNs a VoIP
    /// push (`apns-push-type: voip`, topic with the `.voip` suffix, priority
    /// 10); elsewhere a data message at the platform's highest priority,
    /// whatever the urgency.
    Voip,
}

/// One JMAP push, ready for any platform.
#[derive(Debug, Clone)]
pub struct Message {
    value: serde_json::Value,
    json: String,
    data_key: String,
    urgency: Urgency,
    ttl: Duration,
    kind: PushKind,
}

impl Message {
    /// A [`PushKind::Data`] push of `payload` at urgency `normal` under
    /// [`DEFAULT_DATA_KEY`]. `ttl` is how long the platform may hold the
    /// push for an offline device (jmap-push-platform-guide §Apple APNs:
    /// "Shorter expiries suit higher urgency (e.g. 1 hour for `"high"`, 24
    /// hours for `"normal"`)").
    ///
    /// # Errors
    ///
    /// The [`serde_json::Error`] when `payload` does not serialize to JSON.
    pub fn new<T: Serialize + ?Sized>(
        payload: &T,
        ttl: Duration,
    ) -> Result<Self, serde_json::Error> {
        let value = serde_json::to_value(payload)?;
        let json = serde_json::to_string(&value)?;
        Ok(Self {
            value,
            json,
            data_key: DEFAULT_DATA_KEY.to_owned(),
            urgency: Urgency::Normal,
            ttl,
            kind: PushKind::Data,
        })
    }

    /// This message at `urgency`.
    pub fn with_urgency(self, urgency: Urgency) -> Self {
        Self { urgency, ..self }
    }

    /// This message as `kind`.
    pub fn with_kind(self, kind: PushKind) -> Self {
        Self { kind, ..self }
    }

    /// This message with the payload under `key` instead of
    /// [`DEFAULT_DATA_KEY`] (for example `"jmap-vtc-push"`,
    /// jmap-vtc-ring-push-guide §APNs request format).
    pub fn with_data_key(self, key: impl Into<String>) -> Self {
        Self {
            data_key: key.into(),
            ..self
        }
    }

    /// The payload as compact JSON text.
    pub fn json(&self) -> &str {
        &self.json
    }

    /// The payload as a JSON value.
    pub fn value(&self) -> &serde_json::Value {
        &self.value
    }

    /// The key the payload goes under in map envelopes.
    pub fn data_key(&self) -> &str {
        &self.data_key
    }

    /// The urgency.
    pub fn urgency(&self) -> Urgency {
        self.urgency
    }

    /// The time to live.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// The kind.
    pub fn kind(&self) -> &PushKind {
        &self.kind
    }

    /// Whether the platform's top priority applies: urgency `high`, or a
    /// VoIP ring.
    fn is_high_priority(&self) -> bool {
        self.urgency == Urgency::High || self.kind == PushKind::Voip
    }
}

/// What a platform said about a push it accepted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Receipt {
    /// The platform's identifier for the message, for logs: the FCM
    /// message `name`, the APNs `apns-id`, the Push Kit `requestId`, the
    /// MiPush `data.id`, the WNS `X-WNS-Msg-ID`.
    pub message_id: Option<String>,
    /// A replacement for the destination token. ADM §Response format: when
    /// the returned `registrationID` differs from the one sent, "your
    /// server must update its records to use this value."
    pub new_token: Option<String>,
}

/// A platform's answer to a push that it did not accept.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlatformError {
    /// The HTTP status.
    pub status: StatusCode,
    /// The platform's error code, verbatim: the FCM `errorCode` or
    /// `error.status`, the APNs and ADM `reason`, the Push Kit and MiPush
    /// `code`, the WNS `X-WNS-Status`.
    pub reason: Option<String>,
    /// The platform's human-readable detail, for logs.
    pub description: Option<String>,
}

impl PlatformError {
    fn new(status: StatusCode, reason: Option<String>, description: Option<String>) -> Self {
        Self {
            status,
            reason,
            description,
        }
    }
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "push platform answered {}", self.status)?;
        if let Some(reason) = &self.reason {
            write!(f, " ({reason})")?;
        }
        Ok(())
    }
}

impl std::error::Error for PlatformError {}

/// Why a push was not delivered.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PushError {
    /// The platform refused the push.
    #[error(transparent)]
    Platform(PlatformError),
    /// No access token or provider token: the credentials were refused or
    /// the token endpoint was unreachable. This says nothing about the
    /// subscription (jmap-push-platform-guide §Credential errors are not
    /// subscription errors).
    #[error("push platform credential failed")]
    Credential(#[from] CredentialError),
    /// The request failed (connect, TLS, timeout, reading the response).
    #[error("push request failed")]
    Transport(#[source] reqwest::Error),
    /// The platform's envelope would exceed its documented size limit.
    #[error("push body is {len} octets, more than the platform's {max}")]
    TooLarge {
        /// The envelope size in octets.
        len: usize,
        /// The platform's limit in octets.
        max: usize,
    },
    /// The destination is not a token or URI this platform accepts (for
    /// example an APNs device token that is not hexadecimal, or a WNS
    /// channel URI outside `notify.windows.com`). Checked before sending;
    /// the platform has not judged it, so the subscription is not
    /// destroyed.
    #[error("push destination is not valid for this platform")]
    InvalidDestination,
    /// The message cannot be sent on this platform.
    #[error("push message is not valid for this platform: {0}")]
    InvalidMessage(&'static str),
}

/// Why a credential could not be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CredentialError {
    /// The token endpoint refused the request.
    #[error("token endpoint answered {status}")]
    Rejected {
        /// The response status.
        status: StatusCode,
        /// The OAuth `error` (RFC 6749 §5.2) or platform `reason`, when the
        /// body gave one.
        error: Option<String>,
    },
    /// The token endpoint's answer had no usable `access_token`.
    #[error("token endpoint response has no access_token")]
    Response,
    /// The token request failed (connect, TLS, timeout).
    #[error("token request failed")]
    Transport(#[source] reqwest::Error),
    /// Signing the JWT failed.
    #[error("signing the token failed")]
    Sign,
}

impl CredentialError {
    /// Whether a later attempt may succeed without a configuration change.
    fn is_transient(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Rejected { status, .. } => {
                *status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
            }
            Self::Response | Self::Sign => false,
        }
    }
}

/// Why a key or credential file could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyError {
    /// The text is not a PEM `PRIVATE KEY` block (PKCS #8, RFC 5958).
    #[error("key is not a PEM PRIVATE KEY block")]
    Pem,
    /// The PKCS #8 key is not of the expected type or does not parse.
    #[error("key is not a valid {0} PKCS #8 private key")]
    Invalid(&'static str),
    /// The service-account JSON is malformed or misses a field.
    #[error("service account JSON is invalid: {0}")]
    ServiceAccount(String),
}

/// What became of one push.
///
/// | `jmap-web-push` | this crate |
/// |---|---|
/// | `Ok(Outcome::Accepted)` | [`Outcome::Delivered`] |
/// | `Ok(Outcome::Gone)` | [`Outcome::Gone`] |
/// | `Err(SendError::Unavailable { retry_after, .. })`, `Err(SendError::Transport(_))` | [`Outcome::Retry`] |
/// | any other `Err` | [`Outcome::Failed`] |
///
/// Unlike `jmap-web-push::Sender`, a pusher makes one delivery attempt
/// (plus one after refreshing a credential the platform reported expired)
/// and leaves backoff to the caller.
#[must_use]
#[derive(Debug)]
#[non_exhaustive]
pub enum Outcome {
    /// The platform accepted the push. Acceptance is not delivery: the
    /// platform delivers when it can.
    Delivered(Receipt),
    /// The platform says the destination is gone (FCM `UNREGISTERED` or
    /// `SENDER_ID_MISMATCH`, APNs `BadDeviceToken`, `Unregistered`, ...,
    /// WNS `404`/`410`). Destroy the `PushSubscription`; nothing sent to it
    /// will arrive.
    Gone(PlatformError),
    /// A transient failure: rate limit, platform outage, transport error,
    /// token endpoint unavailable. Send again later, no sooner than
    /// `after` when it is set.
    Retry {
        /// The platform's `Retry-After`, or the minimum it documents.
        after: Option<Duration>,
        /// What failed.
        error: PushError,
    },
    /// The push will not succeed as it is: bad payload, configuration or
    /// credentials. Do not resend it unchanged, and do not destroy the
    /// subscription.
    Failed(PushError),
}

impl Outcome {
    fn retry(after: Option<Duration>, error: impl Into<PushError>) -> Self {
        Self::Retry {
            after,
            error: error.into(),
        }
    }

    fn from_credential(error: CredentialError) -> Self {
        if error.is_transient() {
            Self::retry(None, error)
        } else {
            Self::Failed(error.into())
        }
    }
}

impl From<PlatformError> for PushError {
    fn from(error: PlatformError) -> Self {
        Self::Platform(error)
    }
}

/// The future a [`PlatformPusher`] returns.
pub type PushFuture<'a> = Pin<Box<dyn Future<Output = Outcome> + Send + 'a>>;

/// Delivery of one push to one device on one platform.
///
/// Object safe, so a push layer can keep one `Box<dyn PlatformPusher>` per
/// configured platform and app.
pub trait PlatformPusher: Send + Sync {
    /// Sends `message` to `destination`: the platform's device token,
    /// registration ID or (WNS) channel URI, as the device registered it.
    fn push<'a>(&'a self, destination: &'a str, message: &'a Message) -> PushFuture<'a>;
}

/// A client that does not follow redirects and times out after
/// [`SEND_TIMEOUT`]. Redirects are off so a bearer token never follows
/// one.
fn default_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(SEND_TIMEOUT)
        .build()
}

/// The outcome of a request that failed before a response: a request
/// that could not be built (a bad endpoint or destination) fails, any
/// other error (connect, TLS, timeout) is retried.
fn transport_failure(error: reqwest::Error) -> Outcome {
    if error.is_builder() {
        Outcome::Failed(PushError::Transport(error))
    } else {
        Outcome::retry(None, PushError::Transport(error))
    }
}

// ponytail: only the delay-seconds form of Retry-After (RFC 9110 §10.2.3);
// an HTTP-date is ignored, as in jmap-web-push.
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
        .map(Duration::from_secs)
}

/// The `Retry` or `Failed` outcome of a platform error that is neither a
/// gone destination nor an expired credential: `429` and `5xx` are
/// transient, any other status is not.
fn by_status(error: PlatformError, headers: &HeaderMap) -> Outcome {
    if error.status == StatusCode::TOO_MANY_REQUESTS || error.status.is_server_error() {
        Outcome::retry(retry_after(headers), error)
    } else {
        Outcome::Failed(error.into())
    }
}

/// Seconds as a JSON duration string, `"86400s"` (protobuf `Duration`
/// JSON form, used by FCM and Push Kit `ttl`).
fn duration_string(d: Duration) -> String {
    format!("{}s", d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn retry_after_reads_delay_seconds_only() {
        let mut headers = HeaderMap::new();
        assert_eq!(retry_after(&headers), None);
        headers.insert(RETRY_AFTER, HeaderValue::from_static("120"));
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(120)));
        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT"),
        );
        assert_eq!(retry_after(&headers), None);
    }

    #[test]
    fn voip_is_high_priority_at_any_urgency() {
        let m = Message::new(&serde_json::json!({}), Duration::ZERO)
            .unwrap()
            .with_urgency(Urgency::VeryLow);
        assert!(!m.is_high_priority());
        assert!(m.with_kind(PushKind::Voip).is_high_priority());
    }

    #[test]
    fn platform_error_display_names_status_and_reason() {
        let e = PlatformError::new(
            StatusCode::GONE,
            Some("Unregistered".into()),
            Some("detail".into()),
        );
        assert_eq!(
            e.to_string(),
            "push platform answered 410 Gone (Unregistered)"
        );
    }

    #[test]
    fn pushers_are_object_safe() {
        fn takes(_: &[Box<dyn PlatformPusher>]) {}
        takes(&[]);
    }
}
