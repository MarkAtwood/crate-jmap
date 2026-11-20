//! Huawei Push Kit.
//!
//! Oracles:
//! - jmap-push-platform-guide.md §Huawei Push Kit (HPK);
//!   jmap-chat-push-platform-guide.md §Payload encoding, HPK.
//! - <https://developer.huawei.com/consumer/en/doc/HMSCore-References/https-send-api-0000001050986197>
//!   "Sending Downlink Messages": §Prototype, §Request Parameters, §Response
//!   Parameters, §Result Codes.
//! - <https://developer.huawei.com/consumer/en/doc/HMS-References/push-sendapi>
//!   (AndroidConfig `urgency`, `HIGH` or `NORMAL`).
//! - <https://developer.huawei.com/consumer/en/doc/HMSCore-Guides/open-platform-oauth-0000001053629189>
//!   "OAuth 2.0-based Authentication", client credentials mode.

use std::fmt;
use std::sync::Arc;

use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::auth::{self, TokenCache};
use crate::{
    CredentialError, Message, Outcome, PlatformError, PlatformPusher, PushFuture, Receipt,
};

/// The default token endpoint.
pub const TOKEN_URL: &str = "https://oauth-login.cloud.huawei.com/oauth2/v3/token";

/// The default messaging endpoint.
pub const ENDPOINT: &str = "https://push-api.cloud.huawei.com";

/// Result code: "Success."
const SUCCESS: &str = "80000000";

/// An app's OAuth 2.0 client ID (the App ID) and client secret from
/// AppGallery Connect.
#[derive(Clone)]
pub struct HpkCredentials {
    client_id: String,
    client_secret: String,
}

impl HpkCredentials {
    /// Credentials from AppGallery Connect.
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
        }
    }
}

/// The `Debug` impl never shows the client secret.
impl fmt::Debug for HpkCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HpkCredentials")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

/// Sends pushes through Push Kit for one app.
#[derive(Debug, Clone)]
pub struct HpkPusher {
    client: reqwest::Client,
    credentials: Arc<HpkCredentials>,
    endpoint: String,
    token_url: String,
    token: Arc<TokenCache>,
}

impl HpkPusher {
    /// A pusher over a client that follows no redirects and times out
    /// after [`SEND_TIMEOUT`](crate::SEND_TIMEOUT).
    ///
    /// # Errors
    ///
    /// The [`reqwest::Error`] when the client cannot be built.
    pub fn new(credentials: HpkCredentials) -> Result<Self, reqwest::Error> {
        crate::default_client().map(|client| Self::from_client(client, credentials))
    }

    /// A pusher over `client`, as configured.
    pub fn from_client(client: reqwest::Client, credentials: HpkCredentials) -> Self {
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
                        ("client_id", &self.credentials.client_id),
                        ("client_secret", &self.credentials.client_secret),
                    ],
                )
                .await
            })
            .await
    }

    async fn send(&self, token: &str, message: &Message) -> Outcome {
        // §Prototype: "https://push-api.cloud.huawei.com/v1/[clientid]/messages:send".
        let url = format!(
            "{}/v1/{}/messages:send",
            self.endpoint, self.credentials.client_id
        );
        // jmap-push-platform-guide §Huawei Push Kit, Request: "`"data"` is a
        // single string (not a map); servers MUST serialize the entire
        // payload JSON into it. The `"notification"` key MUST be omitted."
        // §Urgency: "Servers MUST map `"low"` and `"very-low"` to
        // `"NORMAL"`."
        let body = json!({
            "validate_only": false,
            "message": {
                "data": message.json(),
                "token": [token],
                "android": {
                    "urgency": if message.is_high_priority() { "HIGH" } else { "NORMAL" },
                    "ttl": crate::duration_string(message.ttl()),
                },
            },
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
                .post(&url)
                .header(AUTHORIZATION, format!("Bearer {bearer}"))
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
            let text = match response.bytes().await {
                Ok(b) => b,
                Err(e) => return crate::transport_failure(e),
            };
            // "HPK returns a JSON body with a `code` field on every response
            // regardless of HTTP status. ... Servers MUST parse the `code`
            // field to classify errors".
            let parsed = serde_json::from_slice::<ResponseBody>(&text).ok();
            let code = parsed.as_ref().and_then(|b| b.code.as_ref()).map(code_text);
            if status.is_success() && code.as_deref() == Some(SUCCESS) {
                return Outcome::Delivered(Receipt {
                    message_id: parsed.and_then(|b| b.request_id).filter(|s| !s.is_empty()),
                    new_token: None,
                });
            }
            let msg = parsed.and_then(|b| b.msg);
            let error = PlatformError::new(status, code, msg);
            let expired_token = match error.reason.as_deref() {
                // 80200001 "OAuth authentication error", 80200003 "OAuth token
                // expired. ... Obtain a new access token."
                Some(code) => code == "80200001" || code == "80200003",
                None => status == StatusCode::UNAUTHORIZED,
            };
            match error.reason.as_deref() {
                // 80300007 "All tokens are invalid." One token is sent, so it
                // is the one. 80100000 "Tokens identified by illegal_tokens are
                // those to which the message failed to be sent."
                Some("80300007") => return Outcome::Gone(error),
                Some("80100000") if illegal_token(error.description.as_deref(), token) => {
                    return Outcome::Gone(error);
                }
                _ if expired_token && !refreshed => {
                    refreshed = true;
                    self.token.invalidate(&bearer).await;
                }
                _ if expired_token => return Outcome::Failed(error.into()),
                // 81000001 "An internal error of the system occurs."
                Some("81000001") => {
                    return Outcome::retry(crate::retry_after(&headers), error);
                }
                // HTTP 502 "the network is unstable", 503 "Traffic control":
                // retried. 80300002 (no permission), 80300008 (body over 4096
                // bytes), 8010000x (bad parameters) fail.
                _ => return crate::by_status(error, &headers),
            }
        }
    }
}

impl PlatformPusher for HpkPusher {
    fn push<'a>(&'a self, destination: &'a str, message: &'a Message) -> PushFuture<'a> {
        Box::pin(self.send(destination, message))
    }
}

#[derive(Deserialize)]
struct ResponseBody {
    code: Option<serde_json::Value>,
    msg: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
}

/// The `code` as text; the documentation shows a string, some responses
/// carry a number.
fn code_text(code: &serde_json::Value) -> String {
    match code {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Whether the 80100000 `msg` (a JSON string such as
/// `{"success":3,"failure":1,"illegal_tokens":["xxx"]}`) lists `token`.
fn illegal_token(msg: Option<&str>, token: &str) -> bool {
    #[derive(Deserialize)]
    struct Partial {
        #[serde(default)]
        illegal_tokens: Vec<String>,
    }
    msg.and_then(|m| serde_json::from_str::<Partial>(m).ok())
        .is_some_and(|p| p.illegal_tokens.iter().any(|t| t == token))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §Result Codes, the 80100000 response example.
    #[test]
    fn partial_success_lists_illegal_tokens() {
        let msg = r#"{"success":3,"failure":1,"illegal_tokens":["xxx"]}"#;
        assert!(illegal_token(Some(msg), "xxx"));
        assert!(!illegal_token(Some(msg), "yyy"));
        assert!(!illegal_token(None, "xxx"));
    }

    #[test]
    fn numeric_code_reads_as_text() {
        assert_eq!(code_text(&json!(80000000)), "80000000");
        assert_eq!(code_text(&json!("80000000")), "80000000");
    }
}
