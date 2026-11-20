//! Xiaomi MiPush, pass-through (data) messages to a regId.
//!
//! Oracles:
//! - jmap-push-platform-guide.md §Xiaomi Push (MiPush);
//!   jmap-chat-push-platform-guide.md §Payload encoding, MiPush.
//! - <https://dev.mi.com/xiaomihyperos/documentation/detail?pId=1696>
//!   "Server API Address" (domains, `Authorization: key=`, Table 1-1 request
//!   parameters, response format).
//! - <https://dev.mi.com/xiaomihyperos/documentation/detail?pId=1695>
//!   "Guide for Mi Push Server Side SDK (Java)" §4.7 Message receipt (the
//!   `callback`, `callback.type` 16 invalid targets, `errorCode` 1).
//!
//! The send call does not say whether a regId is still valid: MiPush
//! reports invalid regIds in a type 16 message receipt, sent to the
//! [`MiPusher::with_callback`] URL. [`invalid_reg_ids`] reads one.

use std::fmt;
use std::sync::Arc;

use reqwest::header::AUTHORIZATION;
use serde::Deserialize;
use url::Url;

use crate::{Message, Outcome, PlatformError, PlatformPusher, PushFuture, Receipt};

/// The mainland China endpoint (jmap-push-platform-guide §Xiaomi Push).
pub const ENDPOINT: &str = "https://api.xmpush.xiaomi.com";

/// The endpoint for devices outside mainland China, Russia, India and the
/// EEA and UK ("Server API Address": "Other countries/regions"). The others
/// are `https://ru-api.xmpush.global.xiaomi.com`,
/// `https://idmb-api.xmpush.global.xiaomi.com` and
/// `https://fr-api.xmpush.global.xiaomi.com`.
pub const GLOBAL_ENDPOINT: &str = "https://sgp-api.xmpush.global.xiaomi.com";

/// The receipt type for invalid target devices (§4.7, `callback.type` 16).
const INVALID_TARGETS: u64 = 16;

/// Sends pass-through pushes through MiPush for one app.
#[derive(Clone)]
pub struct MiPusher {
    client: reqwest::Client,
    app_secret: Arc<str>,
    package_name: String,
    endpoint: String,
    callback: Option<Url>,
}

/// The `Debug` impl never shows the App Secret. jmap-push-platform-guide
/// §Credential security: static keys "MUST NOT be logged".
impl fmt::Debug for MiPusher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MiPusher")
            .field("client", &self.client)
            .field("app_secret", &"<redacted>")
            .field("package_name", &self.package_name)
            .field("endpoint", &self.endpoint)
            .field("callback", &self.callback.as_ref().map(Url::as_str))
            .finish()
    }
}

impl MiPusher {
    /// A pusher for the app `package_name` (the `restricted_package_name`)
    /// with its App Secret, over a client that follows no redirects and
    /// times out after [`SEND_TIMEOUT`](crate::SEND_TIMEOUT).
    ///
    /// # Errors
    ///
    /// The [`reqwest::Error`] when the client cannot be built.
    pub fn new(
        app_secret: impl Into<String>,
        package_name: impl Into<String>,
    ) -> Result<Self, reqwest::Error> {
        crate::default_client().map(|client| Self::from_client(client, app_secret, package_name))
    }

    /// A pusher over `client`, as configured.
    pub fn from_client(
        client: reqwest::Client,
        app_secret: impl Into<String>,
        package_name: impl Into<String>,
    ) -> Self {
        Self {
            client,
            app_secret: Arc::from(app_secret.into()),
            package_name: package_name.into(),
            endpoint: ENDPOINT.to_owned(),
            callback: None,
        }
    }

    /// This pusher sending to `endpoint` instead of [`ENDPOINT`], for
    /// example [`GLOBAL_ENDPOINT`].
    pub fn with_endpoint(self, endpoint: &Url) -> Self {
        Self {
            endpoint: endpoint.as_str().trim_end_matches('/').to_owned(),
            ..self
        }
    }

    /// This pusher asking MiPush to POST invalid-target receipts (type 16)
    /// to `callback` (`extra.callback`, "the maximum length is 128 bytes").
    /// Read each receipt with [`invalid_reg_ids`].
    pub fn with_callback(self, callback: Url) -> Self {
        Self {
            callback: Some(callback),
            ..self
        }
    }

    async fn send(&self, reg_id: &str, message: &Message) -> Outcome {
        let url = format!("{}/v3/message/regid", self.endpoint);
        // jmap-push-platform-guide §Xiaomi Push, Request: "`payload` is a
        // string; servers MUST URL-encode the JSON payload and place it
        // there. `pass_through=1` routes to the app's broadcast receiver
        // silently. `time_to_live` is in milliseconds." The form encoding
        // is that URL-encoding ("Server API Address", Form Data demo:
        // `payload=this+is+xiaomi+push`).
        let ttl_ms = message.ttl().as_millis().to_string();
        let mut form = vec![
            ("registration_id", reg_id),
            ("restricted_package_name", self.package_name.as_str()),
            ("payload", message.json()),
            ("pass_through", "1"),
            ("time_to_live", ttl_ms.as_str()),
        ];
        let callback_type = INVALID_TARGETS.to_string();
        if let Some(callback) = &self.callback {
            form.push(("extra.callback", callback.as_str()));
            form.push(("extra.callback.type", callback_type.as_str()));
        }
        // "Servers MUST include it on every request ... `Authorization:
        // key={app_secret}`" ("the "key" must be lowercase").
        let response = match self
            .client
            .post(&url)
            .header(AUTHORIZATION, format!("key={}", self.app_secret))
            .form(&form)
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
        let parsed = serde_json::from_slice::<ResponseBody>(&text).ok();
        if status.is_success() && parsed.as_ref().and_then(|b| b.result.as_deref()) == Some("ok") {
            let message_id = parsed
                .and_then(|b| b.data)
                .and_then(|d| d.get("id").and_then(|id| id.as_str()).map(str::to_owned));
            return Outcome::Delivered(Receipt {
                message_id,
                new_token: None,
            });
        }
        // "on failure, the `code` field identifies the error". Xiaomi
        // publishes no code table for this call; a regId is judged by the
        // type 16 receipt, so a failed send never destroys the subscription.
        let (code, description) = match parsed {
            Some(b) => (
                b.code.map(|c| match c {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                }),
                b.reason.or(b.description),
            ),
            None => (None, None),
        };
        crate::by_status(PlatformError::new(status, code, description), &headers)
    }
}

impl PlatformPusher for MiPusher {
    fn push<'a>(&'a self, destination: &'a str, message: &'a Message) -> PushFuture<'a> {
        Box::pin(self.send(destination, message))
    }
}

#[derive(Deserialize)]
struct ResponseBody {
    result: Option<String>,
    code: Option<serde_json::Value>,
    reason: Option<String>,
    description: Option<String>,
    data: Option<serde_json::Value>,
}

/// The regIds a MiPush message receipt reports invalid.
///
/// `data` is the JSON text of the receipt's `data` form field (§4.7: "The
/// request body is `data={json format string}`"): an object from message
/// ID to `{"type": ..., "targets": "<comma-separated>", "errorCode": ...}`.
/// Entries of type 16 with `errorCode` 1 ("invalid RegID") contribute
/// their targets. jmap-push-platform-guide §Xiaomi Push: "Invalid or
/// unregistered regIds indicate the subscription MUST be destroyed."
///
/// # Errors
///
/// The [`serde_json::Error`] when `data` is not a JSON object of receipt
/// entries.
pub fn invalid_reg_ids(data: &str) -> Result<Vec<String>, serde_json::Error> {
    #[derive(Deserialize)]
    struct Entry {
        #[serde(rename = "type")]
        kind: Option<u64>,
        #[serde(default)]
        targets: String,
        #[serde(rename = "errorCode")]
        error_code: Option<u64>,
    }
    let receipts: std::collections::BTreeMap<String, Entry> = serde_json::from_str(data)?;
    Ok(receipts
        .into_values()
        .filter(|e| e.kind == Some(INVALID_TARGETS) && e.error_code == Some(1))
        .flat_map(|e| {
            e.targets
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect())
}
