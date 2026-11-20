//! Windows Push Notification Services, raw notifications.
//!
//! Oracles:
//! - jmap-push-platform-guide.md §Microsoft WNS.
//! - <https://learn.microsoft.com/en-us/windows/apps/develop/notifications/push-notifications/push-request-response-headers>
//!   §Requesting and receiving an access token, §Send notification request,
//!   §Send notification response (`X-WNS-Status`), §Response codes.
//! - <https://learn.microsoft.com/en-us/windows/apps/develop/notifications/push-notifications/wns-overview>
//!   §Requesting a notification channel, Important notes (the
//!   `notify.windows.com` domain check).
//! - <https://learn.microsoft.com/en-us/windows/apps/develop/notifications/push-notifications/push-quickstart>
//!   §Send a push notification to your app, Step 1 (the Microsoft Entra ID
//!   token request).

use std::fmt;
use std::sync::Arc;

use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap};
use url::Url;

use crate::auth::{self, TokenCache};
use crate::{
    CredentialError, Message, Outcome, PlatformError, PlatformPusher, PushError, PushFuture,
    Receipt,
};

/// The legacy token endpoint for a Package SID and secret.
pub const LEGACY_TOKEN_URL: &str = "https://login.live.com/accesstoken.srf";

/// The largest raw payload. §Response codes, 413: "The notification payload
/// exceeds the 5000 byte size limit."
pub const MAX_PAYLOAD: usize = 5000;

/// The domain every channel URI must be in. wns-overview: "the cloud
/// service always ensures that the channel URI uses the domain
/// "notify.windows.com". The service should never push notifications to a
/// channel on any other domain."
const CHANNEL_DOMAIN: &str = "notify.windows.com";

/// The WNS credentials of a cloud service.
#[derive(Clone)]
pub struct WnsCredentials {
    token_url: String,
    client_id: String,
    client_secret: String,
    scope: &'static str,
}

impl WnsCredentials {
    /// A UWP app's Package SID and secret from Partner Center, for
    /// [`LEGACY_TOKEN_URL`] with scope `notify.windows.com`.
    pub fn package(package_sid: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            token_url: LEGACY_TOKEN_URL.to_owned(),
            client_id: package_sid.into(),
            client_secret: secret.into(),
            scope: "notify.windows.com",
        }
    }

    /// A Windows App SDK app's Microsoft Entra ID registration: the
    /// tenant, the Application (client) ID and a client secret, for
    /// `https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token` with
    /// scope `https://wns.windows.com/.default`.
    pub fn entra(tenant_id: &str, client_id: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            token_url: format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token"),
            client_id: client_id.into(),
            client_secret: secret.into(),
            scope: "https://wns.windows.com/.default",
        }
    }

    /// These credentials with tokens from `token_url`.
    pub fn with_token_url(self, token_url: &Url) -> Self {
        Self {
            token_url: token_url.as_str().to_owned(),
            ..self
        }
    }
}

/// The `Debug` impl never shows the secret.
impl fmt::Debug for WnsCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WnsCredentials")
            .field("token_url", &self.token_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("scope", &self.scope)
            .finish()
    }
}

/// Sends raw notifications to WNS channel URIs for one cloud service.
#[derive(Debug, Clone)]
pub struct WnsPusher {
    client: reqwest::Client,
    credentials: Arc<WnsCredentials>,
    check_domain: bool,
    token: Arc<TokenCache>,
}

impl WnsPusher {
    /// A pusher over a client that follows no redirects and times out
    /// after [`SEND_TIMEOUT`](crate::SEND_TIMEOUT).
    ///
    /// # Errors
    ///
    /// The [`reqwest::Error`] when the client cannot be built.
    pub fn new(credentials: WnsCredentials) -> Result<Self, reqwest::Error> {
        crate::default_client().map(|client| Self::from_client(client, credentials))
    }

    /// A pusher over `client`, as configured.
    pub fn from_client(client: reqwest::Client, credentials: WnsCredentials) -> Self {
        Self {
            client,
            credentials: Arc::new(credentials),
            check_domain: true,
            token: Arc::default(),
        }
    }

    /// This pusher without the `https` and `notify.windows.com` check on
    /// channel URIs, for a local test double. Never in production: the
    /// check keeps the access token and payload from going to a channel URI
    /// an attacker registered.
    pub fn danger_accept_any_channel(self) -> Self {
        Self {
            check_domain: false,
            ..self
        }
    }

    async fn access_token(&self) -> Result<String, CredentialError> {
        let c = &self.credentials;
        self.token
            .get(|| async {
                let url = Url::parse(&c.token_url).map_err(|_| CredentialError::Response)?;
                auth::request_token(
                    &self.client,
                    &url,
                    &[
                        ("grant_type", "client_credentials"),
                        ("client_id", &c.client_id),
                        ("client_secret", &c.client_secret),
                        ("scope", c.scope),
                    ],
                )
                .await
            })
            .await
    }

    fn channel(&self, channel_uri: &str) -> Option<Url> {
        let url = Url::parse(channel_uri).ok()?;
        if !self.check_domain {
            return Some(url);
        }
        // "The subdomain of the channel URI is subject to change and should
        // not be considered when validating the channel URI."
        let host = url.host_str()?;
        let in_domain = host == CHANNEL_DOMAIN
            || host
                .strip_suffix(CHANNEL_DOMAIN)
                .is_some_and(|sub| sub.ends_with('.'));
        (url.scheme() == "https" && in_domain).then_some(url)
    }

    async fn send(&self, channel_uri: &str, message: &Message) -> Outcome {
        let Some(url) = self.channel(channel_uri) else {
            return Outcome::Failed(PushError::InvalidDestination);
        };
        let body = message.json().as_bytes().to_vec();
        if body.len() > MAX_PAYLOAD {
            return Outcome::Failed(PushError::TooLarge {
                len: body.len(),
                max: MAX_PAYLOAD,
            });
        }

        let mut refreshed = false;
        loop {
            let bearer = match self.access_token().await {
                Ok(t) => t,
                Err(e) => return Outcome::from_credential(e),
            };
            // jmap-push-platform-guide §Microsoft WNS, Request: "Servers MUST
            // use `X-WNS-Type: wns/raw` with `Content-Type:
            // application/octet-stream`." Raw notifications have no priority.
            let mut request = self
                .client
                .post(url.clone())
                .header(AUTHORIZATION, format!("Bearer {bearer}"))
                .header(CONTENT_TYPE, "application/octet-stream")
                .header("X-WNS-Type", "wns/raw")
                .body(body.clone());
            if !message.ttl().is_zero() {
                request = request.header("X-WNS-TTL", message.ttl().as_secs());
            }
            let response = match request.send().await {
                Ok(r) => r,
                Err(e) => return crate::transport_failure(e),
            };
            let status = response.status();
            let headers = response.headers().clone();
            let notification_status = header(&headers, "X-WNS-Status")
                .or_else(|| header(&headers, "X-WNS-NotificationStatus"));
            let error = PlatformError::new(
                status,
                notification_status.clone(),
                header(&headers, "X-WNS-Error-Description"),
            );
            match (status, notification_status.as_deref()) {
                // "Servers MUST check on every response: `X-WNS-NotificationStatus`
                // ... a `200 OK` with `dropped` means the notification was not
                // delivered." `channelthrottled`: "dropped because the app
                // server exceeded the rate limit for this specific channel."
                (_, Some("channelthrottled")) => {
                    return Outcome::retry(crate::retry_after(&headers), error);
                }
                (_, Some("dropped")) => return Outcome::Failed(error.into()),
                (s, _) if s.is_success() => {
                    return Outcome::Delivered(Receipt {
                        message_id: header(&headers, "X-WNS-Msg-ID"),
                        new_token: None,
                    });
                }
                // "`404 Not Found` / `410 Gone` — channel URI invalid; servers
                // MUST destroy the `PushSubscription`."
                (StatusCode::NOT_FOUND | StatusCode::GONE, _) => return Outcome::Gone(error),
                // 401: "Request a valid access token". wns-overview: "We
                // recommend that you do not try to re-send your notification
                // more than once if the access token is rejected."
                (StatusCode::UNAUTHORIZED, _) if !refreshed => {
                    refreshed = true;
                    self.token.invalidate(&bearer).await;
                }
                // 406: "The cloud service exceeded its throttle limit. Please
                // send your request after the Retry-After header value".
                (StatusCode::NOT_ACCEPTABLE, _) => {
                    return Outcome::retry(crate::retry_after(&headers), error);
                }
                // 403 (credentials do not match the channel's app), 413, 400
                // fail; 5xx retried.
                _ => return crate::by_status(error, &headers),
            }
        }
    }
}

impl PlatformPusher for WnsPusher {
    fn push<'a>(&'a self, destination: &'a str, message: &'a Message) -> PushFuture<'a> {
        Box::pin(self.send(destination, message))
    }
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pusher() -> WnsPusher {
        WnsPusher::from_client(reqwest::Client::new(), WnsCredentials::package("sid", "s"))
    }

    /// wns-overview: the channel URI "uses the domain "notify.windows.com"";
    /// the push-quickstart example channel is on
    /// `wns2-by3p.notify.windows.com`.
    #[test]
    fn channel_must_be_https_in_notify_windows_com() {
        let p = pusher();
        assert!(
            p.channel("https://wns2-by3p.notify.windows.com/?token=AwYAAABa5cJ3")
                .is_some()
        );
        assert!(p.channel("https://notify.windows.com/?token=x").is_some());
        assert!(
            p.channel("http://db5.notify.windows.com/?token=x")
                .is_none()
        );
        assert!(
            p.channel("https://evilnotify.windows.com/?token=x")
                .is_none()
        );
        assert!(
            p.channel("https://notify.windows.com.evil.example/?token=x")
                .is_none()
        );
        assert!(p.channel("not a url").is_none());
        assert!(
            p.danger_accept_any_channel()
                .channel("http://127.0.0.1:9/x")
                .is_some()
        );
    }
}
