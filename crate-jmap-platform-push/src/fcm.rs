//! Firebase Cloud Messaging HTTP v1.
//!
//! Oracles:
//! - jmap-push-platform-guide.md §Google FCM.
//! - <https://firebase.google.com/docs/reference/fcm/rest/v1/projects.messages>
//!   (Message, AndroidConfig, ApnsConfig).
//! - <https://firebase.google.com/docs/reference/fcm/rest/v1/ErrorCode> and
//!   <https://firebase.google.com/docs/cloud-messaging/error-codes>
//!   §REST error codes for the HTTP v1 API.
//! - <https://developers.google.com/identity/protocols/oauth2/service-account>
//!   §Make a delegated API call, HTTP/REST (the JWT bearer grant).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap};
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::apns::{self, Delivery};
use crate::auth::{self, TokenCache};
use crate::{
    CredentialError, KeyError, Message, Outcome, PlatformError, PlatformPusher, PushError,
    PushFuture, PushKind, Receipt,
};

/// The OAuth scope for sending (jmap-push-platform-guide §Google FCM,
/// Auth).
pub const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

/// The default service endpoint.
pub const ENDPOINT: &str = "https://fcm.googleapis.com";

/// The JWT lifetime. Service-account guide, Construct the JWT claim set:
/// `exp` "has a maximum of 1 hour after the issued time."
const ASSERTION_LIFETIME: u64 = 3600;

/// A Google service account: the `client_email`, the RS256 private key and
/// the `token_uri` of its JSON key file.
pub struct ServiceAccount {
    project_id: String,
    client_email: String,
    key_id: Option<String>,
    token_uri: Url,
    key: RsaKeyPair,
}

#[derive(Deserialize)]
struct ServiceAccountFile {
    project_id: String,
    client_email: String,
    private_key: String,
    private_key_id: Option<String>,
    token_uri: String,
}

impl ServiceAccount {
    /// The account in a service-account JSON key file (fields
    /// `project_id`, `client_email`, `private_key`, `private_key_id`,
    /// `token_uri`).
    ///
    /// # Errors
    ///
    /// [`KeyError::ServiceAccount`] when the JSON is malformed or lacks a
    /// field, or `token_uri` is not a URL; [`KeyError::Pem`] or
    /// [`KeyError::Invalid`] when `private_key` is not a PEM PKCS #8 RSA
    /// key.
    pub fn from_json(json: &str) -> Result<Self, KeyError> {
        let file: ServiceAccountFile =
            serde_json::from_str(json).map_err(|e| KeyError::ServiceAccount(e.to_string()))?;
        let token_uri = Url::parse(&file.token_uri)
            .map_err(|_| KeyError::ServiceAccount("token_uri is not a URL".to_owned()))?;
        let der = auth::pkcs8_der(&file.private_key)?;
        let key = RsaKeyPair::from_pkcs8(&der).map_err(|_| KeyError::Invalid("RSA"))?;
        Ok(Self {
            project_id: file.project_id,
            client_email: file.client_email,
            key_id: file.private_key_id,
            token_uri,
            key,
        })
    }

    /// The Firebase project the account sends for.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// The signed JWT assertion for the token request, issued at `iat`.
    fn assertion(&self, iat: u64) -> Result<String, CredentialError> {
        let mut header = json!({ "alg": "RS256", "typ": "JWT" });
        if let Some(kid) = &self.key_id {
            header["kid"] = json!(kid);
        }
        let claims = json!({
            "iss": self.client_email,
            "scope": SCOPE,
            "aud": self.token_uri.as_str(),
            "iat": iat,
            "exp": iat + ASSERTION_LIFETIME,
        });
        let input = auth::signing_input(&header, &claims);
        let mut signature = vec![0u8; self.key.public().modulus_len()];
        self.key
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                input.as_bytes(),
                &mut signature,
            )
            .map_err(|_| CredentialError::Sign)?;
        Ok(format!("{input}.{}", auth::b64url(signature)))
    }
}

/// The `Debug` impl never shows the private key.
impl fmt::Debug for ServiceAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceAccount")
            .field("project_id", &self.project_id)
            .field("client_email", &self.client_email)
            .field("key_id", &self.key_id)
            .field("token_uri", &self.token_uri.as_str())
            .field("key", &"<redacted>")
            .finish()
    }
}

/// Which device family the tokens of an [`FcmPusher`] belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DeviceFamily {
    /// Android: an `android` block with `priority` and `ttl`.
    #[default]
    Android,
    /// Apple devices through FCM's APNs proxy. jmap-push-platform-guide
    /// §Google FCM: "servers MUST replace the `"android"` block with an
    /// `"apns"` block."
    Apple,
}

/// Sends pushes through FCM HTTP v1 for one Firebase project.
#[derive(Debug, Clone)]
pub struct FcmPusher {
    client: reqwest::Client,
    account: Arc<ServiceAccount>,
    endpoint: String,
    family: DeviceFamily,
    token: Arc<TokenCache>,
}

impl FcmPusher {
    /// A pusher for Android tokens over a client that follows no
    /// redirects and times out after [`SEND_TIMEOUT`](crate::SEND_TIMEOUT).
    ///
    /// # Errors
    ///
    /// The [`reqwest::Error`] when the client cannot be built.
    pub fn new(account: ServiceAccount) -> Result<Self, reqwest::Error> {
        crate::default_client().map(|client| Self::from_client(client, account))
    }

    /// A pusher over `client`, as configured.
    pub fn from_client(client: reqwest::Client, account: ServiceAccount) -> Self {
        Self {
            client,
            account: Arc::new(account),
            endpoint: ENDPOINT.to_owned(),
            family: DeviceFamily::Android,
            token: Arc::default(),
        }
    }

    /// This pusher for tokens of `family`.
    pub fn for_family(self, family: DeviceFamily) -> Self {
        Self { family, ..self }
    }

    /// This pusher sending to `endpoint` instead of [`ENDPOINT`].
    pub fn with_endpoint(self, endpoint: &Url) -> Self {
        Self {
            endpoint: endpoint.as_str().trim_end_matches('/').to_owned(),
            ..self
        }
    }

    async fn access_token(&self) -> Result<String, CredentialError> {
        self.token
            .get(|| async {
                let assertion = self.account.assertion(auth::unix_now())?;
                auth::request_token(
                    &self.client,
                    &self.account.token_uri,
                    &[
                        ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                        ("assertion", &assertion),
                    ],
                )
                .await
            })
            .await
    }

    fn body(&self, token: &str, message: &Message) -> Result<serde_json::Value, PushError> {
        let key = message.data_key();
        // Message.data: keys "cannot be `from` or `message_type`, and cannot
        // start with `google.` or `gcm.notification.`".
        if key.is_empty()
            || key == "from"
            || key == "message_type"
            || key.starts_with("google.")
            || key.starts_with("gcm.notification.")
        {
            return Err(PushError::InvalidMessage("reserved FCM data key"));
        }
        // jmap-push-platform-guide §Google FCM, Request: "`data` values MUST
        // be strings ... The `"notification"` key MUST be omitted".
        let mut msg = json!({
            "token": token,
            "data": { key: message.json() },
        });
        match self.family {
            DeviceFamily::Android => {
                // §Google FCM, Urgency: "servers MUST map `"low"` and
                // `"very-low"` to `"NORMAL"`".
                let priority = if message.is_high_priority() {
                    "HIGH"
                } else {
                    "NORMAL"
                };
                msg["android"] = json!({
                    "priority": priority,
                    "ttl": crate::duration_string(message.ttl()),
                });
            }
            DeviceFamily::Apple => {
                if *message.kind() == PushKind::Voip {
                    return Err(PushError::InvalidMessage(
                        "FCM cannot send APNs VoIP pushes; use apns::ApnsPusher",
                    ));
                }
                let delivery = Delivery::of(message);
                // FCM delivers the `data` entries as custom keys beside `aps`.
                let aps = delivery.aps(message);
                msg["apns"] = json!({
                    "headers": {
                        "apns-push-type": delivery.push_type(),
                        "apns-priority": delivery.priority().to_string(),
                        "apns-expiration": apns::expiration(message.ttl()).to_string(),
                    },
                    "payload": { "aps": aps },
                });
            }
        }
        Ok(json!({ "message": msg }))
    }

    async fn send(&self, token: &str, message: &Message) -> Outcome {
        let body = match self.body(token, message) {
            Ok(body) => body,
            Err(e) => return Outcome::Failed(e),
        };
        let url = format!(
            "{}/v1/projects/{}/messages:send",
            self.endpoint, self.account.project_id
        );

        let mut refreshed = false;
        loop {
            let bearer = match self.access_token().await {
                Ok(t) => t,
                Err(e) => return Outcome::from_credential(e),
            };
            let response = match self
                .client
                .post(&url)
                .header(AUTHORIZATION, format!("Bearer {bearer}"))
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_string())
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
                Err(e) => return Outcome::retry(None, PushError::Transport(e)),
            };
            match classify(status, &headers, &text) {
                Classified::Refresh if !refreshed => {
                    refreshed = true;
                    self.token.invalidate(&bearer).await;
                }
                Classified::Refresh => {
                    return Outcome::Failed(PushError::Platform(error_of(status, &text)));
                }
                Classified::Done(outcome) => return outcome,
            }
        }
    }
}

impl PlatformPusher for FcmPusher {
    fn push<'a>(&'a self, destination: &'a str, message: &'a Message) -> PushFuture<'a> {
        Box::pin(self.send(destination, message))
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    error: Option<RpcStatus>,
}

/// A `google.rpc.Status` (the error body of every Google REST API).
#[derive(Deserialize)]
struct RpcStatus {
    message: Option<String>,
    status: Option<String>,
    #[serde(default)]
    details: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct SuccessBody {
    name: Option<String>,
}

const FCM_ERROR_TYPE: &str = "type.googleapis.com/google.firebase.fcm.v1.FcmError";

/// The FCM error of a response: the `errorCode` of the `FcmError` detail,
/// else the canonical `error.status`.
fn error_of(status: StatusCode, body: &[u8]) -> PlatformError {
    let rpc = serde_json::from_slice::<ErrorBody>(body)
        .ok()
        .and_then(|b| b.error);
    let Some(rpc) = rpc else {
        return PlatformError::new(status, None, None);
    };
    let code = rpc
        .details
        .iter()
        .find(|d| d.get("@type").and_then(|t| t.as_str()) == Some(FCM_ERROR_TYPE))
        .and_then(|d| d.get("errorCode"))
        .and_then(|c| c.as_str())
        .map(str::to_owned)
        .or(rpc.status);
    PlatformError::new(status, code, rpc.message)
}

enum Classified {
    Done(Outcome),
    Refresh,
}

fn classify(status: StatusCode, headers: &HeaderMap, body: &[u8]) -> Classified {
    // jmap-push-platform-guide §Google FCM, Error responses: "FCM returns
    // `200 OK` even when delivery fails — servers MUST always check the
    // response body".
    let has_error = serde_json::from_slice::<ErrorBody>(body)
        .ok()
        .is_some_and(|b| b.error.is_some());
    if status.is_success() && !has_error {
        let message_id = serde_json::from_slice::<SuccessBody>(body)
            .ok()
            .and_then(|b| b.name);
        return Classified::Done(Outcome::Delivered(Receipt {
            message_id,
            new_token: None,
        }));
    }
    let error = error_of(status, body);
    let outcome = match error.reason.as_deref() {
        // "`UNREGISTERED` or `SENDER_ID_MISMATCH` — servers MUST destroy the
        // `PushSubscription`."
        Some("UNREGISTERED" | "SENDER_ID_MISMATCH") => Outcome::Gone(error),
        // ErrorCode THIRD_PARTY_AUTH_ERROR (401): "APNs certificate or web
        // push auth key was invalid or missing." A new OAuth token will not
        // help.
        Some("THIRD_PARTY_AUTH_ERROR") => Outcome::Failed(error.into()),
        // §Delivery failures: "401 ... Server MUST refresh credential and
        // retry".
        _ if status == StatusCode::UNAUTHORIZED => return Classified::Refresh,
        // error-codes QUOTA_EXCEEDED: "Use exponential backoff with a
        // minimum initial delay of 1 minute".
        Some("QUOTA_EXCEEDED") => {
            let after = crate::retry_after(headers)
                .unwrap_or_default()
                .max(Duration::from_secs(60));
            Outcome::retry(Some(after), error)
        }
        Some("UNAVAILABLE" | "INTERNAL") => Outcome::retry(crate::retry_after(headers), error),
        // INVALID_ARGUMENT: "MUST NOT retry". It can also mean a bad token,
        // but only "if the payload is valid", so it never destroys.
        _ => crate::by_status(error, headers),
    };
    Classified::Done(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Example 2 of the FCM error-codes page: an invalid registration
    /// token.
    #[test]
    fn error_code_comes_from_the_fcm_error_detail() {
        let body = br#"{"error":{"code":400,"message":"The registration token is not a valid FCM registration token","status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.firebase.fcm.v1.FcmError","errorCode":"INVALID_ARGUMENT"}]}}"#;
        let e = error_of(StatusCode::BAD_REQUEST, body);
        assert_eq!(e.reason.as_deref(), Some("INVALID_ARGUMENT"));
        assert_eq!(
            e.description.as_deref(),
            Some("The registration token is not a valid FCM registration token")
        );
    }

    /// Example 1: a `BadRequest` detail only, so the canonical status.
    #[test]
    fn error_code_falls_back_to_the_status() {
        let body = br#"{"error":{"code":400,"message":"Invalid value at 'message.data[0].value' (TYPE_STRING), 12","status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.rpc.BadRequest","fieldViolations":[{"field":"message.data[0].value","description":"Invalid value at 'message.data[0].value' (TYPE_STRING), 12"}]}]}}"#;
        let e = error_of(StatusCode::BAD_REQUEST, body);
        assert_eq!(e.reason.as_deref(), Some("INVALID_ARGUMENT"));
    }
}
