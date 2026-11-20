//! Amazon Device Messaging.
//!
//! Oracles:
//! - jmap-push-platform-guide.md §Amazon ADM; jmap-chat-push-platform-guide.md
//!   §Payload encoding, FCM and ADM.
//! - <https://developer.amazon.com/docs/adm/request-access-token.html>
//!   §Request format, §Response format.
//! - <https://developer.amazon.com/docs/adm/send-message.html> §Request
//!   format, §Response format.

use std::fmt;
use std::sync::Arc;

use reqwest::StatusCode;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::auth::{self, TokenCache};
use crate::{
    CredentialError, Message, Outcome, PlatformError, PlatformPusher, PushError, PushFuture,
    Receipt,
};

/// The default token endpoint.
pub const TOKEN_URL: &str = "https://api.amazon.com/auth/O2/token";

/// The default messaging endpoint.
pub const ENDPOINT: &str = "https://api.amazon.com";

/// `expiresAfter` bounds: "Allowed range is 60 to 2678400 inclusive."
const EXPIRES_AFTER: (u64, u64) = (60, 2_678_400);

/// A security profile's client ID and client secret.
#[derive(Clone)]
pub struct AdmCredentials {
    client_id: String,
    client_secret: String,
}

impl AdmCredentials {
    /// Credentials from the Amazon Developer Console.
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
        }
    }
}

/// The `Debug` impl never shows the client secret.
impl fmt::Debug for AdmCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdmCredentials")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

/// Sends pushes through ADM for one security profile.
#[derive(Debug, Clone)]
pub struct AdmPusher {
    client: reqwest::Client,
    credentials: Arc<AdmCredentials>,
    endpoint: String,
    token_url: String,
    token: Arc<TokenCache>,
}

impl AdmPusher {
    /// A pusher over a client that follows no redirects and times out
    /// after [`SEND_TIMEOUT`](crate::SEND_TIMEOUT).
    ///
    /// # Errors
    ///
    /// The [`reqwest::Error`] when the client cannot be built.
    pub fn new(credentials: AdmCredentials) -> Result<Self, reqwest::Error> {
        crate::default_client().map(|client| Self::from_client(client, credentials))
    }

    /// A pusher over `client`, as configured.
    pub fn from_client(client: reqwest::Client, credentials: AdmCredentials) -> Self {
        Self {
            client,
            credentials: Arc::new(credentials),
            endpoint: ENDPOINT.to_owned(),
            token_url: TOKEN_URL.to_owned(),
            token: Arc::default(),
        }
    }

    /// This pusher sending to `endpoint` instead of [`ENDPOINT`].
    pub fn with_endpoint(self, endpoint: &Url) -> Self {
        Self {
            endpoint: endpoint.as_str().trim_end_matches('/').to_owned(),
            ..self
        }
    }

    /// This pusher requesting tokens from `token_url` instead of
    /// [`TOKEN_URL`].
    pub fn with_token_url(self, token_url: &Url) -> Self {
        Self {
            token_url: token_url.as_str().to_owned(),
            ..self
        }
    }

    async fn access_token(&self) -> Result<String, CredentialError> {
        self.token
            .get(|| async {
                let url = Url::parse(&self.token_url).map_err(|_| CredentialError::Response)?;
                auth::request_token(
                    &self.client,
                    &url,
                    &[
                        ("grant_type", "client_credentials"),
                        ("scope", "messaging:push"),
                        ("client_id", &self.credentials.client_id),
                        ("client_secret", &self.credentials.client_secret),
                    ],
                )
                .await
            })
            .await
    }

    async fn send(&self, registration_id: &str, message: &Message) -> Outcome {
        let Ok(mut url) = Url::parse(&self.endpoint) else {
            return Outcome::Failed(PushError::InvalidDestination);
        };
        match url.path_segments_mut() {
            Ok(mut segments) => {
                segments.pop_if_empty().extend([
                    "messaging",
                    "registrations",
                    registration_id,
                    "messages",
                ]);
            }
            Err(()) => return Outcome::Failed(PushError::InvalidDestination),
        }
        // jmap-push-platform-guide §Amazon ADM, Request; the payload as a
        // JSON string under the data key ("`data` values MUST be strings").
        // `priority`: "high" wakes the device, so low and very-low map to
        // "normal" (§Urgency).
        let ttl = message
            .ttl()
            .as_secs()
            .clamp(EXPIRES_AFTER.0, EXPIRES_AFTER.1);
        let body = json!({
            "data": { message.data_key(): message.json() },
            "priority": if message.is_high_priority() { "high" } else { "normal" },
            "expiresAfter": ttl,
        })
        .to_string();

        let mut refreshed = false;
        loop {
            let bearer = match self.access_token().await {
                Ok(t) => t,
                Err(e) => return Outcome::from_credential(e),
            };
            let response = match self
                .client
                .post(url.clone())
                .header(AUTHORIZATION, format!("Bearer {bearer}"))
                .header(CONTENT_TYPE, "application/json")
                .header(
                    "X-Amzn-Type-Version",
                    "com.amazon.device.messaging.ADMMessage@1.0",
                )
                .header(ACCEPT, "application/json")
                .header(
                    "X-Amzn-Accept-Type",
                    "com.amazon.device.messaging.ADMSendResult@1.0",
                )
                .body(body.clone())
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => return crate::transport_failure(e),
            };
            let status = response.status();
            let headers = response.headers().clone();
            let text = match response.bytes().await {
                Ok(b) => b,
                Err(e) => return crate::transport_failure(e),
            };
            if status.is_success() {
                // "If this value differs from the one your server sent, your
                // server must update its records to use this value."
                let new_token = serde_json::from_slice::<SuccessBody>(&text)
                    .ok()
                    .and_then(|b| b.registration_id)
                    .filter(|id| id != registration_id);
                return Outcome::Delivered(Receipt {
                    message_id: None,
                    new_token,
                });
            }
            // "ADM returns a JSON body with a `reason` field. Servers MUST
            // parse it — HTTP status alone is not sufficient".
            let reason = serde_json::from_slice::<ErrorBody>(&text)
                .ok()
                .and_then(|b| b.reason);
            let error = PlatformError::new(status, reason, None);
            match error.reason.as_deref() {
                // "`InvalidRegistrationId` / `Unregistered` — servers MUST
                // destroy the `PushSubscription`."
                Some("InvalidRegistrationId" | "Unregistered") => return Outcome::Gone(error),
                // 401 `AccessTokenExpired`: refresh the token and retry.
                _ if status == StatusCode::UNAUTHORIZED && !refreshed => {
                    refreshed = true;
                    self.token.invalidate(&bearer).await;
                }
                // `MaxRateExceeded` (429), 500 and 503 carry `Retry-After`;
                // `MessageTooLarge` (413) and the other 400 reasons fail.
                _ => return crate::by_status(error, &headers),
            }
        }
    }
}

impl PlatformPusher for AdmPusher {
    fn push<'a>(&'a self, destination: &'a str, message: &'a Message) -> PushFuture<'a> {
        Box::pin(self.send(destination, message))
    }
}

#[derive(Deserialize)]
struct SuccessBody {
    #[serde(rename = "registrationID")]
    registration_id: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    reason: Option<String>,
}
