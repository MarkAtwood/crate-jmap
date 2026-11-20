//! Credential plumbing shared by the platform modules: the access-token
//! cache, the OAuth 2.0 token request, PEM decoding and JWT assembly.

use std::fmt;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde::Deserialize;
use tokio::sync::Mutex;
use url::Url;

use crate::{CredentialError, KeyError};

/// The lifetime assumed when a token response has no `expires_in`. RFC
/// 6749 §5.1 makes the field RECOMMENDED; the legacy WNS response example
/// omits it.
const DEFAULT_LIFETIME: Duration = Duration::from_secs(3600);

/// How long before expiry a cached token is replaced: a tenth of its
/// lifetime, at most five minutes. jmap-push-platform-guide §Credential
/// security: OAuth tokens "SHOULD be cached and refreshed proactively
/// before expiry, and MUST NOT be reused past their validity window."
fn refresh_margin(lifetime: Duration) -> Duration {
    (lifetime / 10).min(Duration::from_secs(300))
}

/// A bearer token and when to stop using it.
struct Cached {
    value: String,
    refresh_at: Instant,
}

/// One cached bearer token, refreshed under a lock so that concurrent
/// pushes make one token request between them.
#[derive(Default)]
pub(crate) struct TokenCache {
    slot: Mutex<Option<Cached>>,
}

impl fmt::Debug for TokenCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TokenCache(<redacted>)")
    }
}

impl TokenCache {
    /// The cached token, or a new one from `fetch`, which returns the token
    /// and how long it may be used from the moment `fetch` was called.
    pub(crate) async fn get<F, Fut>(&self, fetch: F) -> Result<String, CredentialError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(String, Duration), CredentialError>>,
    {
        let mut slot = self.slot.lock().await;
        let now = Instant::now();
        if let Some(cached) = slot.as_ref()
            && now < cached.refresh_at
        {
            return Ok(cached.value.clone());
        }
        let (value, usable_for) = fetch().await?;
        *slot = Some(Cached {
            value: value.clone(),
            refresh_at: now + usable_for,
        });
        Ok(value)
    }

    /// Drops the cached token if it is still `stale`, so the next
    /// [`get`](Self::get) fetches a new one. A token another push already
    /// replaced is kept.
    pub(crate) async fn invalidate(&self, stale: &str) {
        let mut slot = self.slot.lock().await;
        if slot.as_ref().is_some_and(|c| c.value == stale) {
            *slot = None;
        }
    }
}

/// An RFC 6749 §5.1 access token response. `expires_in` is a number, or
/// a string in the Microsoft Entra ID response (WNS push-quickstart, Step
/// 1: `"expires_in":"86399"`).
#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    #[serde(default)]
    expires_in: Option<serde_json::Value>,
}

/// An RFC 6749 §5.2 error response, or the `reason` of an ADM token error.
#[derive(Deserialize)]
struct TokenErrorResponse {
    error: Option<String>,
    reason: Option<String>,
}

/// POSTs `form` to the token endpoint `url` and returns the access token
/// and how long to use it (its `expires_in` less [`refresh_margin`]).
pub(crate) async fn request_token(
    client: &reqwest::Client,
    url: &Url,
    form: &[(&str, &str)],
) -> Result<(String, Duration), CredentialError> {
    let response = client
        .post(url.clone())
        .form(form)
        .send()
        .await
        .map_err(CredentialError::Transport)?;
    let status = response.status();
    let body = response.bytes().await.map_err(CredentialError::Transport)?;
    if !status.is_success() {
        let error = serde_json::from_slice::<TokenErrorResponse>(&body)
            .ok()
            .and_then(|e| e.error.or(e.reason));
        return Err(CredentialError::Rejected { status, error });
    }
    let parsed: TokenResponse =
        serde_json::from_slice(&body).map_err(|_| CredentialError::Response)?;
    let token = parsed
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or(CredentialError::Response)?;
    let lifetime = parsed
        .expires_in
        .and_then(|v| match v {
            serde_json::Value::Number(n) => n.as_u64(),
            serde_json::Value::String(s) => s.trim().parse().ok(),
            _ => None,
        })
        .map_or(DEFAULT_LIFETIME, Duration::from_secs);
    Ok((token, lifetime - refresh_margin(lifetime)))
}

/// The DER bytes of the first PEM `PRIVATE KEY` block (RFC 7468 §10,
/// PKCS #8) in `pem`.
pub(crate) fn pkcs8_der(pem: &str) -> Result<Vec<u8>, KeyError> {
    const BEGIN: &str = "-----BEGIN PRIVATE KEY-----";
    const END: &str = "-----END PRIVATE KEY-----";
    let start = pem.find(BEGIN).ok_or(KeyError::Pem)? + BEGIN.len();
    let len = pem[start..].find(END).ok_or(KeyError::Pem)?;
    let body: String = pem[start..start + len]
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    STANDARD.decode(body).map_err(|_| KeyError::Pem)
}

/// Base64url without padding (RFC 7515 §2).
pub(crate) fn b64url(bytes: impl AsRef<[u8]>) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The JWS signing input `b64url(header) . b64url(claims)` (RFC 7515
/// §5.1).
pub(crate) fn signing_input(header: &serde_json::Value, claims: &serde_json::Value) -> String {
    format!(
        "{}.{}",
        b64url(header.to_string()),
        b64url(claims.to_string())
    )
}

/// Seconds since the Unix epoch.
pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_margin_is_a_tenth_capped_at_five_minutes() {
        assert_eq!(
            refresh_margin(Duration::from_secs(3600)),
            Duration::from_secs(300)
        );
        assert_eq!(
            refresh_margin(Duration::from_secs(600)),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn pem_body_is_decoded_across_lines() {
        // base64 of the octets 01 02 03 04 05 06, split over two lines.
        let pem = "junk\n-----BEGIN PRIVATE KEY-----\nAQID\nBAUG\n-----END PRIVATE KEY-----\n";
        assert_eq!(pkcs8_der(pem).unwrap(), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(pkcs8_der("no key here"), Err(KeyError::Pem));
        assert_eq!(
            pkcs8_der("-----BEGIN PRIVATE KEY-----\n!!\n-----END PRIVATE KEY-----"),
            Err(KeyError::Pem)
        );
    }

    #[tokio::test]
    async fn cache_reuses_until_invalidated() {
        let cache = TokenCache::default();
        let first = cache
            .get(|| async { Ok(("a".to_owned(), Duration::from_secs(60))) })
            .await
            .unwrap();
        let second = cache
            .get(|| async { Ok(("b".to_owned(), Duration::from_secs(60))) })
            .await
            .unwrap();
        assert_eq!((first.as_str(), second.as_str()), ("a", "a"));

        // Invalidating a token that is no longer cached keeps the current one.
        cache.invalidate("zzz").await;
        let third = cache
            .get(|| async { Ok(("c".to_owned(), Duration::from_secs(60))) })
            .await
            .unwrap();
        assert_eq!(third, "a");

        cache.invalidate("a").await;
        let fourth = cache
            .get(|| async { Ok(("d".to_owned(), Duration::from_secs(60))) })
            .await
            .unwrap();
        assert_eq!(fourth, "d");
    }

    #[tokio::test]
    async fn cache_refetches_once_usable_time_is_over() {
        let cache = TokenCache::default();
        let _ = cache
            .get(|| async { Ok(("a".to_owned(), Duration::ZERO)) })
            .await
            .unwrap();
        let next = cache
            .get(|| async { Ok(("b".to_owned(), Duration::from_secs(60))) })
            .await
            .unwrap();
        assert_eq!(next, "b");
    }
}
