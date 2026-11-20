//! IVS private-channel playback tokens: ES384 JWTs signed with the
//! private half of a playback key pair imported into IVS.
//!
//! Oracle: "Generate and Sign IVS Playback Tokens",
//! <https://docs.aws.amazon.com/ivs/latest/LowLatencyUserGuide/private-channels-generate-tokens.html>
//! (§Token Schema, §Instructions), and "Setting Up Private Channels",
//! <https://docs.aws.amazon.com/ivs/latest/LowLatencyUserGuide/private-channels.html>.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jmap_types::impl_string_enum;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P384_SHA384_FIXED_SIGNING, EcdsaKeyPair, KeyPair as _};
use serde_json::{Map, Value, json};

use crate::time::unix_seconds;

/// "the maximum value for the `exp` claim is 10 minutes in the future"
/// with `single-use-uuid` or `viewer-id`.
const MAX_TRACKED_LIFETIME: Duration = Duration::from_secs(600);

/// "The maximum length is 40 characters" (`viewer-id`).
const MAX_VIEWER_ID: usize = 40;

/// Why a playback key or token could not be made.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    /// The key is not a PKCS #8 P-384 private key (with its public key)
    /// that ring accepts.
    #[error("not a PKCS #8 P-384 private key")]
    InvalidKey,
    /// `viewer-id` is longer than 40 characters.
    #[error("viewer-id is longer than {MAX_VIEWER_ID} characters")]
    ViewerIdTooLong,
    /// `exp` is more than 10 minutes ahead while `viewer-id` or
    /// `single-use-uuid` is set.
    #[error("exp is more than 10 minutes ahead with viewer-id or single-use-uuid")]
    ExpiryTooFar,
    /// The channel is private and no playback key is configured.
    #[error("the channel is private and no playback key is configured")]
    NoKey,
    /// Signing failed.
    #[error("signing failed")]
    Signing,
}

/// `maximum-resolution`: "manifest filtering by resolution for a viewer
/// session".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MaximumResolution {
    /// `SD`.
    Sd,
    /// `HD`.
    Hd,
    /// `FULL_HD`.
    FullHd,
    /// A value this version does not list.
    Other(String),
}

impl_string_enum!(MaximumResolution, "an IVS maximum resolution",
    "SD" => Sd,
    "HD" => Hd,
    "FULL_HD" => FullHd,
);

/// The claims of one playback token (§Token Schema, payload).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackClaims {
    /// `aws:channel-arn`.
    pub channel_arn: String,
    /// `exp`: when the token stops being accepted for starting playback
    /// ("The token is validated when the viewer initializes playback, not
    /// throughout the stream").
    pub expires_at: SystemTime,
    /// `aws:access-control-allow-origin`: comma-separated origins.
    pub access_control_allow_origin: Option<String>,
    /// `aws:strict-origin-enforcement`.
    pub strict_origin_enforcement: bool,
    /// `aws:single-use-uuid`.
    pub single_use_uuid: Option<String>,
    /// `aws:viewer-id`: at most 40 characters; "Do not use this field for
    /// personally identifying, confidential, or sensitive information."
    pub viewer_id: Option<String>,
    /// `aws:viewer-session-version` (Int64).
    pub viewer_session_version: Option<i64>,
    /// `aws:maximum-resolution`.
    pub maximum_resolution: Option<MaximumResolution>,
}

impl PlaybackClaims {
    /// Claims for `channel_arn` expiring at `expires_at`, with no origin
    /// restriction and no viewer tracking.
    pub fn new(channel_arn: impl Into<String>, expires_at: SystemTime) -> Self {
        Self {
            channel_arn: channel_arn.into(),
            expires_at,
            access_control_allow_origin: None,
            strict_origin_enforcement: false,
            single_use_uuid: None,
            viewer_id: None,
            viewer_session_version: None,
            maximum_resolution: None,
        }
    }

    /// These claims restricted to `origins` (comma-separated).
    pub fn with_allowed_origin(self, origins: impl Into<String>) -> Self {
        Self {
            access_control_allow_origin: Some(origins.into()),
            ..self
        }
    }

    /// These claims with `strict-origin-enforcement`.
    pub fn with_strict_origin_enforcement(self) -> Self {
        Self {
            strict_origin_enforcement: true,
            ..self
        }
    }

    /// These claims for viewer `viewer_id`, so IVS can revoke the session.
    pub fn with_viewer_id(self, viewer_id: impl Into<String>) -> Self {
        Self {
            viewer_id: Some(viewer_id.into()),
            ..self
        }
    }

    /// These claims with `viewer-session-version`.
    pub fn with_viewer_session_version(self, version: i64) -> Self {
        Self {
            viewer_session_version: Some(version),
            ..self
        }
    }

    /// These claims usable once (`single-use-uuid`).
    pub fn with_single_use_uuid(self, uuid: impl Into<String>) -> Self {
        Self {
            single_use_uuid: Some(uuid.into()),
            ..self
        }
    }

    /// These claims capped at `resolution`.
    pub fn with_maximum_resolution(self, resolution: MaximumResolution) -> Self {
        Self {
            maximum_resolution: Some(resolution),
            ..self
        }
    }

    fn payload(&self) -> Value {
        let mut m = Map::new();
        m.insert("aws:channel-arn".to_owned(), json!(self.channel_arn));
        if let Some(o) = &self.access_control_allow_origin {
            m.insert("aws:access-control-allow-origin".to_owned(), json!(o));
        }
        if self.strict_origin_enforcement {
            m.insert("aws:strict-origin-enforcement".to_owned(), json!(true));
        }
        if let Some(u) = &self.single_use_uuid {
            m.insert("aws:single-use-uuid".to_owned(), json!(u));
        }
        if let Some(v) = &self.viewer_id {
            m.insert("aws:viewer-id".to_owned(), json!(v));
        }
        if let Some(v) = self.viewer_session_version {
            m.insert("aws:viewer-session-version".to_owned(), json!(v));
        }
        if let Some(r) = &self.maximum_resolution {
            m.insert("aws:maximum-resolution".to_owned(), json!(r));
        }
        // "Enter this value as an integer type value."
        m.insert("exp".to_owned(), json!(unix_seconds(self.expires_at)));
        Value::Object(m)
    }
}

/// The private key of an IVS playback key pair (ECDSA P-384). The `Debug`
/// impl never shows key material.
///
/// IVS imports the public half (`ImportPlaybackKeyPair`); the deployment
/// keeps the private half and signs viewer tokens with it. The key is
/// read as PKCS #8, so convert an OpenSSL `EC PRIVATE KEY` first:
/// `openssl pkcs8 -topk8 -nocrypt -in priv.pem -out priv.pk8.pem`.
#[derive(Clone)]
pub struct PlaybackKey {
    pair: Arc<EcdsaKeyPair>,
    rng: SystemRandom,
}

impl PlaybackKey {
    /// The key from a PEM `PRIVATE KEY` (PKCS #8) block.
    ///
    /// # Errors
    ///
    /// [`TokenError::InvalidKey`] when there is no such block or it is not
    /// a P-384 key.
    pub fn from_pkcs8_pem(pem: &str) -> Result<Self, TokenError> {
        let body: String = pem
            .lines()
            .skip_while(|l| !l.starts_with("-----BEGIN PRIVATE KEY-----"))
            .skip(1)
            .take_while(|l| !l.starts_with("-----END PRIVATE KEY-----"))
            .collect::<Vec<_>>()
            .concat();
        let der = STANDARD
            .decode(body.trim())
            .map_err(|_| TokenError::InvalidKey)?;
        Self::from_pkcs8_der(&der)
    }

    /// The key from PKCS #8 DER bytes.
    ///
    /// # Errors
    ///
    /// [`TokenError::InvalidKey`] when it is not a P-384 key.
    pub fn from_pkcs8_der(der: &[u8]) -> Result<Self, TokenError> {
        let rng = SystemRandom::new();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P384_SHA384_FIXED_SIGNING, der, &rng)
            .map_err(|_| TokenError::InvalidKey)?;
        Ok(Self {
            pair: Arc::new(pair),
            rng,
        })
    }

    /// The public key as an uncompressed SEC1 point.
    pub fn public_key_sec1(&self) -> &[u8] {
        self.pair.public_key().as_ref()
    }

    /// A signed token for `claims`: `base64UrlEncode(header) + "." +
    /// base64UrlEncode(payload) + "." + base64UrlEncode(signature)` with
    /// header `{"alg": "ES384", "typ": "JWT"}` (§Instructions). `now` checks
    /// the 10-minute cap that applies with `viewer-id` or `single-use-uuid`.
    ///
    /// # Errors
    ///
    /// [`TokenError::ViewerIdTooLong`], [`TokenError::ExpiryTooFar`] or
    /// [`TokenError::Signing`].
    pub fn sign(&self, claims: &PlaybackClaims, now: SystemTime) -> Result<String, TokenError> {
        if claims
            .viewer_id
            .as_ref()
            .is_some_and(|v| v.chars().count() > MAX_VIEWER_ID)
        {
            return Err(TokenError::ViewerIdTooLong);
        }
        if (claims.viewer_id.is_some() || claims.single_use_uuid.is_some())
            && claims.expires_at > now + MAX_TRACKED_LIFETIME
        {
            return Err(TokenError::ExpiryTooFar);
        }
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"ES384","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(claims.payload().to_string());
        let signing_input = format!("{header}.{payload}");
        let signature = self
            .pair
            .sign(&self.rng, signing_input.as_bytes())
            .map_err(|_| TokenError::Signing)?;
        Ok(format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        ))
    }
}

impl fmt::Debug for PlaybackKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PlaybackKey(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn key() -> PlaybackKey {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P384_SHA384_FIXED_SIGNING, &rng).unwrap();
        PlaybackKey::from_pkcs8_der(pkcs8.as_ref()).unwrap()
    }

    fn segment(token: &str, i: usize) -> Value {
        let part = token.split('.').nth(i).unwrap();
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
    }

    /// §Token Schema: header ES384/JWT, `aws:` claims, integer `exp`.
    #[test]
    fn token_shape() {
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let claims = PlaybackClaims::new(
            "arn:aws:ivs:us-west-2:123456789012:channel/abcdABCDefgh",
            now + Duration::from_secs(300),
        )
        .with_allowed_origin("https://app.example.com")
        .with_viewer_id("v-42")
        .with_maximum_resolution(MaximumResolution::Hd);
        let token = key().sign(&claims, now).unwrap();
        assert_eq!(segment(&token, 0), json!({"alg": "ES384", "typ": "JWT"}));
        assert_eq!(
            segment(&token, 1),
            json!({
                "aws:channel-arn": "arn:aws:ivs:us-west-2:123456789012:channel/abcdABCDefgh",
                "aws:access-control-allow-origin": "https://app.example.com",
                "aws:viewer-id": "v-42",
                "aws:maximum-resolution": "HD",
                "exp": 1_700_000_300
            })
        );
        // ES384 fixed-width signature: r || s, 48 bytes each.
        let sig = URL_SAFE_NO_PAD
            .decode(token.split('.').nth(2).unwrap())
            .unwrap();
        assert_eq!(sig.len(), 96);
    }

    /// "when using `viewer-id`, the maximum value for `exp` is 10 minutes
    /// in the future"; "The maximum length is 40 characters".
    #[test]
    fn viewer_limits() {
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let arn = "arn:aws:ivs:us-west-2:123456789012:channel/abcdABCDefgh";
        let k = key();
        let far = PlaybackClaims::new(arn, now + Duration::from_secs(601)).with_viewer_id("v");
        assert!(matches!(k.sign(&far, now), Err(TokenError::ExpiryTooFar)));
        let ok = PlaybackClaims::new(arn, now + Duration::from_secs(600)).with_viewer_id("v");
        assert!(k.sign(&ok, now).is_ok());
        let long = PlaybackClaims::new(arn, now).with_viewer_id("x".repeat(41));
        assert!(matches!(
            k.sign(&long, now),
            Err(TokenError::ViewerIdTooLong)
        ));
        // Without viewer tracking there is no 10-minute cap.
        let untracked = PlaybackClaims::new(arn, now + Duration::from_secs(86_400));
        assert!(k.sign(&untracked, now).is_ok());
    }

    #[test]
    fn rejects_a_p256_key() {
        let rng = SystemRandom::new();
        let p256 =
            EcdsaKeyPair::generate_pkcs8(&ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
                .unwrap();
        assert!(matches!(
            PlaybackKey::from_pkcs8_der(p256.as_ref()),
            Err(TokenError::InvalidKey)
        ));
        assert!(matches!(
            PlaybackKey::from_pkcs8_pem("no pem here"),
            Err(TokenError::InvalidKey)
        ));
    }

    #[test]
    fn pem_round_trip() {
        let rng = SystemRandom::new();
        let der = EcdsaKeyPair::generate_pkcs8(&ECDSA_P384_SHA384_FIXED_SIGNING, &rng).unwrap();
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            STANDARD.encode(der.as_ref())
        );
        let key = PlaybackKey::from_pkcs8_pem(&pem).unwrap();
        assert_eq!(format!("{key:?}"), "PlaybackKey(<redacted>)");
    }
}
