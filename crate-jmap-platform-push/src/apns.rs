//! Apple Push Notification service, token-based (ES256 JWT) over HTTP/2.
//!
//! Oracles:
//! - jmap-push-platform-guide.md §Apple APNs; jmap-chat-push-platform-guide.md
//!   §Payload encoding, APNs; jmap-vtc-ring-push-guide.md §APNs request
//!   format.
//! - <https://developer.apple.com/documentation/usernotifications/sending-notification-requests-to-apns>
//!   (request headers, `apns-push-type` values, payload sizes).
//! - <https://developer.apple.com/documentation/usernotifications/handling-notification-responses-from-apns>
//!   (status codes, `reason` strings).
//! - <https://developer.apple.com/documentation/usernotifications/establishing-a-token-based-connection-to-apns>
//!   (the provider token and its refresh window).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::DecodePrivateKey as _;
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::auth::{self, TokenCache};
use crate::{
    CredentialError, KeyError, Message, Outcome, PlatformError, PlatformPusher, PushError,
    PushFuture, PushKind, Receipt, Urgency,
};

/// The production server.
pub const PRODUCTION: &str = "https://api.push.apple.com";

/// The development (sandbox) server.
pub const SANDBOX: &str = "https://api.sandbox.push.apple.com";

/// The largest payload of a non-VoIP push. Sending notification requests,
/// Send a POST request to APNs: "it's limited to a maximum size of 4 KB
/// (4096 bytes)."
pub const MAX_PAYLOAD: usize = 4096;

/// The largest VoIP payload: "The maxiumum size for a Voice over Internet
/// Protocol (VoIP) notification is 5 KB (5120 bytes)."
pub const MAX_VOIP_PAYLOAD: usize = 5120;

/// How long a provider token is reused. Establishing a token-based
/// connection, Refresh your token regularly: "Refresh your token no more
/// than once every 20 minutes and no less than once every 60 minutes."
const TOKEN_REUSE: Duration = Duration::from_secs(40 * 60);

/// An APNs token signing key: the `.p8` key, its 10-character Key ID and
/// the 10-character Team ID that issues tokens with it.
#[derive(Clone)]
pub struct ApnsKey {
    key: SigningKey,
    key_id: String,
    team_id: String,
}

impl ApnsKey {
    /// The key in the PEM text of a `.p8` file (a PKCS #8 P-256 key).
    ///
    /// # Errors
    ///
    /// [`KeyError::Pem`] when `p8` has no `PRIVATE KEY` block;
    /// [`KeyError::Invalid`] when it is not a P-256 key.
    pub fn from_p8(
        p8: &str,
        key_id: impl Into<String>,
        team_id: impl Into<String>,
    ) -> Result<Self, KeyError> {
        let der = auth::pkcs8_der(p8)?;
        let key = SigningKey::from_pkcs8_der(&der).map_err(|_| KeyError::Invalid("P-256"))?;
        Ok(Self {
            key,
            key_id: key_id.into(),
            team_id: team_id.into(),
        })
    }

    /// The provider token issued at `iat`: `{"alg":"ES256","kid":...}` over
    /// `{"iss":<team>,"iat":...}`.
    fn token(&self, iat: u64) -> String {
        let input = auth::signing_input(
            &json!({ "alg": "ES256", "kid": self.key_id }),
            &json!({ "iss": self.team_id, "iat": iat }),
        );
        let signature: Signature = self.key.sign(input.as_bytes());
        format!("{input}.{}", auth::b64url(signature.to_bytes()))
    }
}

/// The `Debug` impl never shows the private key.
impl fmt::Debug for ApnsKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApnsKey")
            .field("key_id", &self.key_id)
            .field("team_id", &self.team_id)
            .field("key", &"<redacted>")
            .finish()
    }
}

/// Which APNs server a pusher talks to. Device tokens belong to one
/// environment: a development build's token is a `BadDeviceToken` in
/// production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Environment {
    /// [`PRODUCTION`].
    Production,
    /// [`SANDBOX`].
    Sandbox,
}

/// How one push goes out: the `apns-push-type`, the priority and the
/// `aps` dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Delivery {
    Background,
    Alert { priority: u8 },
    Voip,
}

impl Delivery {
    /// jmap-push-platform-guide §Apple APNs, Urgency: in alert mode "high"
    /// is priority 10, "normal" priority 5, and "`"low"` and `"very-low"`
    /// remain background regardless of mode."
    pub(crate) fn of(message: &Message) -> Self {
        match message.kind() {
            PushKind::Voip => Self::Voip,
            PushKind::Alert { .. } => match message.urgency() {
                Urgency::High => Self::Alert { priority: 10 },
                Urgency::Normal => Self::Alert { priority: 5 },
                _ => Self::Background,
            },
            _ => Self::Background,
        }
    }

    pub(crate) fn push_type(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Alert { .. } => "alert",
            Self::Voip => "voip",
        }
    }

    /// Background: "Always use priority `5`. Using priority `10` is an
    /// error." VoIP: priority 10 (jmap-vtc-ring-push-guide §APNs request
    /// format, "immediate delivery; required for voip").
    pub(crate) fn priority(self) -> u8 {
        match self {
            Self::Background => 5,
            Self::Alert { priority } => priority,
            Self::Voip => 10,
        }
    }

    /// The `aps` dictionary (jmap-chat-push-platform-guide §Payload
    /// encoding, APNs; jmap-vtc-ring-push-guide: `"aps": {}`).
    pub(crate) fn aps(self, message: &Message) -> serde_json::Value {
        match (self, message.kind()) {
            (Self::Alert { .. }, PushKind::Alert { body }) => json!({
                "alert": { "body": body },
                "mutable-content": 1,
            }),
            (Self::Voip, _) => json!({}),
            _ => json!({ "content-available": 1 }),
        }
    }
}

/// The `apns-expiration` for `ttl`: "a UNIX epoch expressed in seconds
/// (UTC)", or `0` ("APNs attempts to deliver the notification only once
/// and doesn't store it") for a zero `ttl`.
pub(crate) fn expiration(ttl: Duration) -> u64 {
    if ttl.is_zero() {
        0
    } else {
        auth::unix_now().saturating_add(ttl.as_secs())
    }
}

/// Sends pushes to APNs for one app (one bundle ID).
#[derive(Debug, Clone)]
pub struct ApnsPusher {
    client: reqwest::Client,
    key: Arc<ApnsKey>,
    bundle_id: String,
    endpoint: String,
    token: Arc<TokenCache>,
}

impl ApnsPusher {
    /// A pusher over an HTTP/2-only client ("APNs requires HTTP/2",
    /// jmap-push-platform-guide §Apple APNs) that follows no redirects and
    /// times out after [`SEND_TIMEOUT`](crate::SEND_TIMEOUT). `bundle_id`
    /// is the `apns-topic`; VoIP pushes add `.voip`.
    ///
    /// # Errors
    ///
    /// The [`reqwest::Error`] when the client cannot be built.
    pub fn new(
        key: ApnsKey,
        bundle_id: impl Into<String>,
        environment: Environment,
    ) -> Result<Self, reqwest::Error> {
        reqwest::Client::builder()
            .http2_prior_knowledge()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(crate::SEND_TIMEOUT)
            .build()
            .map(|client| Self::from_client(client, key, bundle_id, environment))
    }

    /// A pusher over `client`, as configured. APNs refuses HTTP/1.1, so
    /// the client should speak HTTP/2.
    pub fn from_client(
        client: reqwest::Client,
        key: ApnsKey,
        bundle_id: impl Into<String>,
        environment: Environment,
    ) -> Self {
        let endpoint = match environment {
            Environment::Production => PRODUCTION,
            Environment::Sandbox => SANDBOX,
        };
        Self {
            client,
            key: Arc::new(key),
            bundle_id: bundle_id.into(),
            endpoint: endpoint.to_owned(),
            token: Arc::default(),
        }
    }

    /// This pusher sending to `endpoint` instead of the environment's
    /// server (for example port 2197, which "you can also use ... on either
    /// server").
    pub fn with_endpoint(self, endpoint: &Url) -> Self {
        Self {
            endpoint: endpoint.as_str().trim_end_matches('/').to_owned(),
            ..self
        }
    }

    async fn provider_token(&self) -> Result<String, CredentialError> {
        self.token
            .get(|| async { Ok((self.key.token(auth::unix_now()), TOKEN_REUSE)) })
            .await
    }

    async fn send(&self, device_token: &str, message: &Message) -> Outcome {
        // ":path ... `/3/device/<device_token>`, where `<device_token>` is the
        // hexadecimal bytes that identify the user's device." Checked so the
        // token cannot change the path.
        if device_token.is_empty() || !device_token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Outcome::Failed(PushError::InvalidDestination);
        }
        let delivery = Delivery::of(message);
        let mut body = serde_json::Map::new();
        body.insert(message.data_key().to_owned(), message.value().clone());
        // `aps` last so a payload key named "aps" cannot replace it.
        body.insert("aps".to_owned(), delivery.aps(message));
        let body = serde_json::Value::Object(body).to_string();
        let max = if delivery == Delivery::Voip {
            MAX_VOIP_PAYLOAD
        } else {
            MAX_PAYLOAD
        };
        if body.len() > max {
            return Outcome::Failed(PushError::TooLarge {
                len: body.len(),
                max,
            });
        }
        // "If you set this push type, the `apns-topic` header field must use
        // your app's bundle ID with `.voip` appended to the end."
        let topic = if delivery == Delivery::Voip {
            format!("{}.voip", self.bundle_id)
        } else {
            self.bundle_id.clone()
        };
        let url = format!("{}/3/device/{device_token}", self.endpoint);

        let mut refreshed = false;
        loop {
            let bearer = match self.provider_token().await {
                Ok(t) => t,
                Err(e) => return Outcome::from_credential(e),
            };
            let response = match self
                .client
                .post(&url)
                .header(AUTHORIZATION, format!("bearer {bearer}"))
                .header("apns-topic", &topic)
                .header("apns-push-type", delivery.push_type())
                .header("apns-priority", delivery.priority().to_string())
                .header("apns-expiration", expiration(message.ttl()).to_string())
                .header(CONTENT_TYPE, "application/json")
                .body(body.clone())
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => return crate::transport_failure(e),
            };
            let status = response.status();
            let headers = response.headers().clone();
            if status.is_success() {
                let message_id = headers
                    .get("apns-id")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                return Outcome::Delivered(Receipt {
                    message_id,
                    new_token: None,
                });
            }
            let text = match response.bytes().await {
                Ok(b) => b,
                Err(e) => return crate::transport_failure(e),
            };
            let reason = serde_json::from_slice::<ErrorBody>(&text)
                .ok()
                .and_then(|b| b.reason);
            let error = PlatformError::new(status, reason, None);
            match error.reason.as_deref() {
                // "If the value in the `iat` field is more than one hour old,
                // APNs rejects any notifications containing the token,
                // returning an `ExpiredProviderToken` `(403)` error."
                Some("ExpiredProviderToken") if !refreshed => {
                    refreshed = true;
                    self.token.invalidate(&bearer).await;
                }
                _ => return classify(error, &headers),
            }
        }
    }
}

impl PlatformPusher for ApnsPusher {
    fn push<'a>(&'a self, destination: &'a str, message: &'a Message) -> PushFuture<'a> {
        Box::pin(self.send(destination, message))
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    reason: Option<String>,
}

fn classify(error: PlatformError, headers: &reqwest::header::HeaderMap) -> Outcome {
    match error.reason.as_deref() {
        // jmap-push-platform-guide §Apple APNs, Error responses:
        // "`BadDeviceToken` / `DeviceTokenNotForTopic` — servers MUST destroy
        // the `PushSubscription`. `Unregistered` / `ExpiredToken` — servers
        // MUST destroy the `PushSubscription`."
        Some("BadDeviceToken" | "DeviceTokenNotForTopic" | "Unregistered" | "ExpiredToken") => {
            Outcome::Gone(error)
        }
        // 410: "The device token is no longer active for the topic."
        _ if error.status == StatusCode::GONE => Outcome::Gone(error),
        // Credential and configuration errors (`BadTopic`,
        // `InvalidProviderToken`, `ExpiredProviderToken` after a refresh,
        // `TopicDisallowed`, ...) never destroy: "Servers MUST NOT destroy the
        // `PushSubscription` in this case". `TooManyRequests` and
        // `TooManyProviderTokenUpdates` are 429, `InternalServerError`,
        // `ServiceUnavailable` and `Shutdown` are 5xx: retried.
        _ => crate::by_status(error, headers),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(kind: PushKind, urgency: Urgency) -> Message {
        Message::new(&json!({}), Duration::ZERO)
            .unwrap()
            .with_kind(kind)
            .with_urgency(urgency)
    }

    /// jmap-push-platform-guide §Apple APNs, Urgency table.
    #[test]
    fn urgency_table() {
        let alert = PushKind::Alert {
            body: "New message".into(),
        };
        let rows = [
            (PushKind::Data, Urgency::High, "background", 5),
            (PushKind::Data, Urgency::VeryLow, "background", 5),
            (alert.clone(), Urgency::High, "alert", 10),
            (alert.clone(), Urgency::Normal, "alert", 5),
            (alert.clone(), Urgency::Low, "background", 5),
            (alert, Urgency::VeryLow, "background", 5),
            (PushKind::Voip, Urgency::Low, "voip", 10),
        ];
        for (kind, urgency, push_type, priority) in rows {
            let d = Delivery::of(&message(kind.clone(), urgency));
            assert_eq!(
                (d.push_type(), d.priority()),
                (push_type, priority),
                "{kind:?} {urgency:?}"
            );
        }
    }

    #[test]
    fn zero_ttl_is_expiration_zero() {
        assert_eq!(expiration(Duration::ZERO), 0);
        assert!(expiration(Duration::from_secs(60)) > 1_700_000_000);
    }
}
