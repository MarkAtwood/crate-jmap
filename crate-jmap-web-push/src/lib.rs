//! Web Push delivery for JMAP push messages.
//!
//! This is a support crate (workspace AGENTS.md §Support crates): it does
//! network I/O and holds the VAPID signing key, and no `*-types` or
//! `*-server` crate depends on it. Payloads (`StateChange`,
//! `PushVerification`, `ChatMessagePush`) are built elsewhere; this crate
//! POSTs them to a `PushSubscription.url`.
//!
//! RFC 8620 §7.2: "The POST request MUST have a content type of
//! "application/json" and contain the UTF-8 JSON-encoded object as the
//! body.  The request MUST have a "TTL" header and MAY have "Urgency"
//! and/or "Topic" headers, as specified in Section 5 of \[RFC8030\]." When
//! the subscription has `keys`, "the server MUST use them as specified in
//! \[RFC8291\] to encrypt all data sent to the push subscription."
//!
//! - [`encrypt`]: RFC 8291 message encryption (`aes128gcm`, RFC 8188).
//! - [`Vapid`]: the RFC 8292 `Authorization: vapid` header.
//! - [`Sender`]: the POST, with retry and backoff. A `404` or `410` comes
//!   back as [`Outcome::Gone`]: the caller destroys the subscription.
//!
//! The push URL is used as given. SSRF checks on it and per-subscription
//! rate limits (RFC 8620 §7.2: "A "429" (Too Many Requests) response MUST
//! cause the JMAP server to reduce the frequency of pushes") are the
//! caller's.

#![forbid(unsafe_code)]

use std::fmt;
use std::num::NonZeroU32;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use jmap_types::PushSubscription;
use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint as _;
use p256::{PublicKey, SecretKey};
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE, HeaderMap, RETRY_AFTER};
use serde::Serialize;
use sha2::Sha256;
use url::Url;

/// The `Urgency` header (RFC 8030 §5.3), shared with every JMAP crate
/// that classifies a push (jmap-push-platform-guide §Web Push, Urgency).
pub use jmap_types::Urgency;

pub use jmap_types;
pub use p256;
pub use reqwest;
pub use url;

/// The largest plaintext a push may carry, in octets.
///
/// RFC 8291 §4: "A push service is not required to support more than 4096
/// octets of payload body ... this equates to, at most, 3993 octets of
/// plaintext." The same cap applies to unencrypted pushes.
// ponytail: one cap for both paths; an unencrypted push could use up to
// 4096 octets (RFC 8030 §7.2). Split the limit if a consumer needs it.
pub const MAX_PLAINTEXT: usize = 3993;

/// The `rs` of the single `aes128gcm` record (RFC 8291 §4, Appendix A).
const RECORD_SIZE: u32 = 4096;

/// The per-request timeout of [`Sender::new`].
pub const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a VAPID token from [`Sender`] stays valid. RFC 8292 §2: "An
/// "exp" claim MUST NOT be more than 24 hours from the time of the
/// request."
const VAPID_LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);

/// A user agent's RFC 8291 keys: the `keys` of a `PushSubscription`.
#[derive(Clone)]
pub struct UaKeys {
    public: PublicKey,
    auth: [u8; 16],
}

impl UaKeys {
    /// Keys from the user agent's ECDH public key and authentication
    /// secret.
    pub fn new(public: PublicKey, auth: [u8; 16]) -> Self {
        Self { public, auth }
    }

    /// Keys from the base64url `p256dh` and `auth` of RFC 8620 §7.2.
    /// Trailing `=` padding is accepted.
    ///
    /// # Errors
    ///
    /// [`EndpointError::P256dh`] when `p256dh` is not a base64url P-256
    /// point; [`EndpointError::Auth`] when `auth` is not base64url of
    /// exactly 16 octets (RFC 8291 §3.2: "a hard-to-guess sequence of 16
    /// octets").
    pub fn from_base64(p256dh: &str, auth: &str) -> Result<Self, EndpointError> {
        let public = decode_base64url(p256dh)
            .and_then(|b| PublicKey::from_sec1_bytes(&b).ok())
            .ok_or(EndpointError::P256dh)?;
        let auth = decode_base64url(auth)
            .and_then(|b| <[u8; 16]>::try_from(b).ok())
            .ok_or(EndpointError::Auth)?;
        Ok(Self { public, auth })
    }
}

impl fmt::Debug for UaKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UaKeys")
            .field("public", &self.public)
            .field("auth", &"<redacted>")
            .finish()
    }
}

/// Where a push goes: the push URL and, when the subscription has them,
/// the keys its pushes are encrypted to.
#[derive(Clone)]
pub struct Endpoint {
    url: Url,
    keys: Option<UaKeys>,
}

impl Endpoint {
    /// An endpoint at `url`. With `keys: None` pushes are sent
    /// unencrypted.
    pub fn new(url: Url, keys: Option<UaKeys>) -> Self {
        Self { url, keys }
    }

    /// The push URL.
    pub fn url(&self) -> &Url {
        &self.url
    }
}

/// The `Debug` impl redacts the URL, which is a bearer capability.
impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("url", &"<redacted>")
            .field("keys", &self.keys)
            .finish()
    }
}

impl TryFrom<&PushSubscription> for Endpoint {
    type Error = EndpointError;

    /// The endpoint of a stored subscription. The URL is not checked for
    /// `https://`; that, and any SSRF policy, is the caller's.
    fn try_from(sub: &PushSubscription) -> Result<Self, EndpointError> {
        let url = sub.url.as_deref().ok_or(EndpointError::MissingUrl)?;
        let url = Url::parse(url).map_err(EndpointError::Url)?;
        let keys = sub
            .keys
            .as_ref()
            .map(|k| UaKeys::from_base64(&k.p256dh, &k.auth))
            .transpose()?;
        Ok(Self { url, keys })
    }
}

/// Why an [`Endpoint`] or [`UaKeys`] could not be built.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EndpointError {
    /// The subscription has no `url`.
    #[error("push subscription has no url")]
    MissingUrl,
    /// The `url` does not parse.
    #[error("push subscription url does not parse")]
    Url(#[source] url::ParseError),
    /// `keys.p256dh` is not a base64url P-256 public key.
    #[error("push subscription p256dh is not a base64url P-256 public key")]
    P256dh,
    /// `keys.auth` is not base64url of 16 octets.
    #[error("push subscription auth is not base64url of 16 octets")]
    Auth,
}

fn decode_base64url(s: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).ok()
}

/// Encrypts `plaintext` to `keys` per RFC 8291: one `aes128gcm` record
/// with `rs` 4096, a fresh random salt and a fresh application server
/// key pair, whose public key is the `keyid` (RFC 8291 §4).
///
/// # Errors
///
/// [`EncryptError::TooLarge`] when `plaintext` exceeds
/// [`MAX_PLAINTEXT`]; [`EncryptError::Random`] when the OS random number
/// generator fails.
pub fn encrypt(plaintext: &[u8], keys: &UaKeys) -> Result<Vec<u8>, EncryptError> {
    if plaintext.len() > MAX_PLAINTEXT {
        return Err(EncryptError::TooLarge {
            len: plaintext.len(),
        });
    }
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| EncryptError::Random(e.into()))?;
    let as_secret = loop {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| EncryptError::Random(e.into()))?;
        if let Ok(key) = SecretKey::from_slice(&bytes) {
            break key;
        }
    };
    Ok(encrypt_with(plaintext, keys, salt, &as_secret))
}

/// RFC 8291 §3.4 for an application server, with the salt and key pair
/// given. `plaintext` must be at most [`MAX_PLAINTEXT`] octets.
fn encrypt_with(plaintext: &[u8], keys: &UaKeys, salt: [u8; 16], as_secret: &SecretKey) -> Vec<u8> {
    let ecdh_secret =
        p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), keys.public.as_affine());
    let ua_public = keys.public.to_encoded_point(false);
    let as_public = as_secret.public_key().to_encoded_point(false);

    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_public.as_bytes());
    key_info.extend_from_slice(as_public.as_bytes());
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&keys.auth), ecdh_secret.raw_secret_bytes())
        .expand(&key_info, &mut ikm)
        .expect("32 octets is a valid HKDF-SHA-256 output length");

    ece_native::encrypt(
        ikm,
        salt,
        as_public.as_bytes(),
        std::iter::once(plaintext.to_vec()),
        RECORD_SIZE,
    )
    .expect("a plaintext of at most MAX_PLAINTEXT octets and a 65-octet keyid fit one 4096-octet record")
}

/// Why [`encrypt`] failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EncryptError {
    /// The plaintext is longer than [`MAX_PLAINTEXT`].
    #[error("push plaintext is {len} octets, more than {MAX_PLAINTEXT}")]
    TooLarge {
        /// The plaintext length.
        len: usize,
    },
    /// The OS random number generator failed.
    #[error("OS random number generator failed")]
    Random(#[source] std::io::Error),
}

/// An application server's VAPID identity (RFC 8292): its P-256 signing
/// key and the `sub` contact URI.
///
/// RFC 8292 §3.2: "An application server MUST select a different private
/// key for the key exchange \[RFC8291\] and signing the authentication
/// token." [`encrypt`] makes a fresh key exchange key per message, so
/// this holds as long as the signing key is not reused elsewhere.
#[derive(Clone)]
pub struct Vapid {
    key: SigningKey,
    subject: String,
}

impl Vapid {
    /// A VAPID identity. `subject` is the `sub` claim, RFC 8292 §2.1: "a
    /// contact URI for the application server as either a "mailto:"
    /// (email) \[RFC6068\] or an "https:" \[RFC2818\] URI."
    pub fn new(key: SigningKey, subject: impl Into<String>) -> Self {
        Self {
            key,
            subject: subject.into(),
        }
    }

    /// The `k` parameter: the base64url uncompressed public key (RFC
    /// 8292 §3.2). Browsers take the same value as the
    /// `applicationServerKey` of `PushManager.subscribe`.
    pub fn public_key(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.key.verifying_key().to_encoded_point(false).as_bytes())
    }

    /// The `Authorization` header value, `vapid t=<jwt>, k=<key>` (RFC
    /// 8292 §3), for a push to `push_url`. `exp` is the token's expiry in
    /// seconds since the Unix epoch; RFC 8292 §2 bounds it to 24 hours
    /// after the request.
    ///
    /// The JWT is `{"typ":"JWT","alg":"ES256"}` over `aud` (the origin of
    /// `push_url`, RFC 6454 §6.1, port included when not the default),
    /// `exp` and `sub`.
    ///
    /// # Errors
    ///
    /// [`VapidError::OpaqueOrigin`] when `push_url` has no tuple origin
    /// (its scheme is not one of `http`, `https`, `ws`, `wss`, `ftp`).
    pub fn authorization(&self, push_url: &Url, exp: u64) -> Result<String, VapidError> {
        #[derive(Serialize)]
        struct Claims<'a> {
            aud: &'a str,
            exp: u64,
            sub: &'a str,
        }

        let origin = push_url.origin();
        if !origin.is_tuple() {
            return Err(VapidError::OpaqueOrigin);
        }
        let aud = origin.unicode_serialization();
        let claims = serde_json::to_vec(&Claims {
            aud: &aud,
            exp,
            sub: &self.subject,
        })
        .expect("a struct of strings and an integer serializes");
        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256"}"#),
            URL_SAFE_NO_PAD.encode(claims)
        );
        let signature: Signature = self.key.sign(signing_input.as_bytes());
        Ok(format!(
            "vapid t={signing_input}.{}, k={}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes()),
            self.public_key()
        ))
    }
}

/// The `Debug` impl shows the public key, never the private key.
impl fmt::Debug for Vapid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vapid")
            .field("public_key", &self.public_key())
            .field("subject", &self.subject)
            .finish()
    }
}

/// Why [`Vapid::authorization`] failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VapidError {
    /// The push URL has an opaque origin, so there is no `aud`.
    #[error("push URL has no origin for the VAPID aud claim")]
    OpaqueOrigin,
}

/// A `Topic` header value (RFC 8030 §5.4): "no more than 32 characters
/// from the URL and a filename-safe Base 64 alphabet". A push with a
/// topic replaces any stored push with the same topic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Topic(String);

impl Topic {
    /// The header value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Topic {
    type Error = TopicError;

    /// # Errors
    ///
    /// [`TopicError`] when `s` is empty, longer than 32 characters, or has
    /// a character outside `A-Z a-z 0-9 - _`.
    fn try_from(s: String) -> Result<Self, TopicError> {
        let valid = (1..=32).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if valid { Ok(Self(s)) } else { Err(TopicError) }
    }
}

impl std::str::FromStr for Topic {
    type Err = TopicError;

    fn from_str(s: &str) -> Result<Self, TopicError> {
        Self::try_from(s.to_owned())
    }
}

/// A [`Topic`] that is not 1 to 32 base64url characters.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("push topic must be 1 to 32 base64url characters")]
pub struct TopicError;

/// The RFC 8030 §5 headers of one push.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PushOptions {
    /// The `TTL` header, in whole seconds. RFC 8030 §5.2: "An application
    /// server MUST include the TTL (Time-To-Live) header field".
    pub ttl: Duration,
    /// The `Urgency` header; absent means `normal`.
    pub urgency: Option<Urgency>,
    /// The `Topic` header.
    pub topic: Option<Topic>,
}

impl PushOptions {
    /// Options with `ttl` and no `Urgency` or `Topic`.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            urgency: None,
            topic: None,
        }
    }
}

/// How [`Sender::send`] retries a push the push service did not accept.
///
/// A `429`, a `5xx` or a transport error is retried, waiting
/// `base_delay * 2^(n-1)` (capped at `max_delay`) before retry `n`, or the
/// `Retry-After` delay when the response gives one in seconds. When
/// `Retry-After` is longer than `max_delay`, the send stops at once so
/// the caller can reschedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Retry {
    /// Total attempts, the first included. Default 3.
    pub attempts: NonZeroU32,
    /// The wait before the first retry. Default 1 s.
    pub base_delay: Duration,
    /// The longest wait between attempts. Default 60 s.
    pub max_delay: Duration,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            attempts: NonZeroU32::new(3).expect("3 is not zero"),
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
        }
    }
}

impl Retry {
    fn backoff(&self, retry: u32) -> Duration {
        self.base_delay
            .saturating_mul(1u32 << (retry - 1).min(31))
            .min(self.max_delay)
    }
}

/// What the push service did with a push.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A `2xx` response: the push service accepted the message (RFC 8030
    /// §5: "A 201 (Created) response indicates that the push message was
    /// accepted.").
    Accepted,
    /// A `404` or `410` response: the subscription has expired (RFC 8030
    /// §7.3: "A push service MUST return a 404 (Not Found) status code if
    /// an application server attempts to send a push message to an
    /// expired push message subscription."). Destroy the
    /// `PushSubscription`; nothing sent to it will arrive.
    Gone,
}

/// Why [`Sender::send`] failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SendError {
    /// The payload does not serialize to JSON.
    #[error("push payload does not serialize to JSON")]
    Json(#[source] serde_json::Error),
    /// The serialized payload is longer than [`MAX_PLAINTEXT`], or
    /// encryption failed.
    #[error("push payload encryption failed")]
    Encrypt(#[from] EncryptError),
    /// The push URL has no origin for VAPID.
    #[error("VAPID token failed")]
    Vapid(#[from] VapidError),
    /// A response other than `2xx`, `404`, `410`, `429` or `5xx`, not
    /// retried. A `403` usually means the push service rejected the VAPID
    /// token (RFC 8292 §2), which says nothing about the subscription;
    /// redirects are not followed and land here too.
    #[error("push service rejected the message with {status}")]
    Rejected {
        /// The response status.
        status: StatusCode,
    },
    /// A `429` or `5xx` on the last attempt, or a `Retry-After` longer
    /// than [`Retry::max_delay`].
    #[error("push service unavailable ({status})")]
    Unavailable {
        /// The response status.
        status: StatusCode,
        /// The response's `Retry-After`, when it gave one in seconds.
        retry_after: Option<Duration>,
    },
    /// The request failed on the last attempt (connect, TLS, timeout).
    #[error("push request failed")]
    Transport(#[source] reqwest::Error),
}

/// Sends pushes over a [`reqwest::Client`], signing each request with a
/// [`Vapid`] identity.
#[derive(Debug, Clone)]
pub struct Sender {
    client: reqwest::Client,
    vapid: Vapid,
    retry: Retry,
}

impl Sender {
    /// A sender that does not follow redirects, times requests out after
    /// [`SEND_TIMEOUT`] and retries per [`Retry::default`].
    ///
    /// Redirects are off so a redirect cannot take a push past the
    /// caller's check of the push URL.
    ///
    /// # Errors
    ///
    /// The [`reqwest::Error`] when reqwest cannot build the client (its
    /// TLS backend fails to initialize).
    pub fn new(vapid: Vapid) -> Result<Self, reqwest::Error> {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(SEND_TIMEOUT)
            .build()
            .map(|client| Self::from_client(client, vapid))
    }

    /// A sender over `client`, as configured.
    pub fn from_client(client: reqwest::Client, vapid: Vapid) -> Self {
        Self {
            client,
            vapid,
            retry: Retry::default(),
        }
    }

    /// This sender with `retry` in place of the default.
    pub fn with_retry(self, retry: Retry) -> Self {
        Self { retry, ..self }
    }

    /// POSTs `payload`, as JSON, to `endpoint`: encrypted per RFC 8291
    /// when the endpoint has keys, with a fresh VAPID token, the `TTL`,
    /// `Urgency` and `Topic` of `options`, and `Content-Type:
    /// application/json` (RFC 8620 §7.2). Retries per [`Retry`].
    ///
    /// For the RFC 8620 §7.2.2 verification push, pass the
    /// [`PushVerification`](jmap_types::PushVerification) as `payload`.
    ///
    /// # Errors
    ///
    /// See [`SendError`]. A gone subscription is not an error but
    /// [`Outcome::Gone`].
    pub async fn send<T: Serialize + ?Sized>(
        &self,
        endpoint: &Endpoint,
        payload: &T,
        options: &PushOptions,
    ) -> Result<Outcome, SendError> {
        let json = serde_json::to_vec(payload).map_err(SendError::Json)?;
        let body = match &endpoint.keys {
            Some(keys) => encrypt(&json, keys)?,
            None if json.len() > MAX_PLAINTEXT => {
                return Err(EncryptError::TooLarge { len: json.len() }.into());
            }
            None => json,
        };

        let mut attempt = 1;
        loop {
            let exp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .saturating_add(VAPID_LIFETIME)
                .as_secs();
            let mut request = self
                .client
                .post(endpoint.url.clone())
                .header(AUTHORIZATION, self.vapid.authorization(&endpoint.url, exp)?)
                .header(CONTENT_TYPE, "application/json")
                .header("TTL", options.ttl.as_secs())
                .body(body.clone());
            if endpoint.keys.is_some() {
                request = request.header(CONTENT_ENCODING, "aes128gcm");
            }
            if let Some(urgency) = options.urgency {
                request = request.header("Urgency", urgency.as_str());
            }
            if let Some(topic) = &options.topic {
                request = request.header("Topic", topic.as_str());
            }

            let (failure, retry_after) = match request.send().await {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        return Ok(Outcome::Accepted);
                    }
                    if status == StatusCode::NOT_FOUND || status == StatusCode::GONE {
                        return Ok(Outcome::Gone);
                    }
                    if status != StatusCode::TOO_MANY_REQUESTS && !status.is_server_error() {
                        return Err(SendError::Rejected { status });
                    }
                    let retry_after = retry_after(response.headers());
                    (
                        SendError::Unavailable {
                            status,
                            retry_after,
                        },
                        retry_after,
                    )
                }
                Err(e) => (SendError::Transport(e), None),
            };
            let delay = retry_after.unwrap_or_else(|| self.retry.backoff(attempt));
            if attempt >= self.retry.attempts.get() || delay > self.retry.max_delay {
                return Err(failure);
            }
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }
}

// ponytail: only the delay-seconds form; an HTTP-date Retry-After falls
// back to the exponential backoff.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(s: &str) -> Vec<u8> {
        URL_SAFE_NO_PAD.decode(s.replace([' ', '\n'], "")).unwrap()
    }

    /// RFC 8291 §5 and Appendix A: the example push message, from the
    /// fixed sender key and salt to the exact 144-octet body.
    #[test]
    fn rfc8291_example_ciphertext() {
        let keys = UaKeys::from_base64(
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
            "BTBZMqHH6r4Tts7J_aSIgg",
        )
        .unwrap();
        let as_secret =
            SecretKey::from_slice(&b64("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")).unwrap();
        let salt: [u8; 16] = b64("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();

        let body = encrypt_with(
            b"When I grow up, I want to be a watermelon",
            &keys,
            salt,
            &as_secret,
        );

        let expected = b64(
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml
             mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT
             pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN",
        );
        assert_eq!(body, expected);
    }

    /// RFC 8291 §4: "at most, 3993 octets of plaintext."
    #[test]
    fn encrypt_caps_plaintext_at_3993_octets() {
        let keys = UaKeys::from_base64(
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
            "BTBZMqHH6r4Tts7J_aSIgg",
        )
        .unwrap();
        // 86-octet header + 3993 + delimiter + 16-octet tag.
        assert_eq!(encrypt(&[b'a'; 3993], &keys).unwrap().len(), 4096);
        assert!(matches!(
            encrypt(&[b'a'; 3994], &keys),
            Err(EncryptError::TooLarge { len: 3994 })
        ));
    }
}
