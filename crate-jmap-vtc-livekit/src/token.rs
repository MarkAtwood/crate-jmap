//! LiveKit access tokens: HS256 JWTs signed with an API secret.
//!
//! Every server API call carries one as `Authorization: Bearer`, and
//! every webhook carries one (with the body checksum) in `Authorization`.
//!
//! Oracles: "Tokens & grants",
//! <https://docs.livekit.io/frontends/reference/tokens-grants/> (the claim
//! and video grant tables), and the reference implementation in
//! livekit/protocol `auth/accesstoken.go` (`ToJWT`: `iss` the API key,
//! `sub` the identity, `iat`, `nbf`, `exp`, HS256 over the secret),
//! `auth/grants.go` (the JSON names of `ClaimGrants` and `VideoGrant`) and
//! `auth/verifier.go` (HS256 only for secrets, `exp` required, issuer =
//! API key, one minute of leeway).

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::hmac;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The clock skew [`ApiKey::verify`] accepts on `exp` and `nbf`, as the
/// LiveKit verifier does (`tokenLeeway = time.Minute`).
pub const LEEWAY: Duration = Duration::from_secs(60);

/// The JOSE header of every token this crate signs.
const HEADER: &str = r#"{"alg":"HS256","typ":"JWT"}"#;

/// Why a token could not be verified.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    /// Not three base64url segments with a JSON header and payload.
    #[error("not a JWT")]
    Malformed,
    /// The header's `alg` is not `HS256`. LiveKit also signs EdDSA tokens
    /// with Ed25519 keys (`livekit-v2+jwt`), which this crate does not
    /// verify.
    #[error("unsupported JWT algorithm {0:?}")]
    Algorithm(String),
    /// The signature does not verify with the API secret.
    #[error("bad JWT signature")]
    Signature,
    /// `iss` is not this API key.
    #[error("token issued by another API key")]
    Issuer,
    /// `exp` is missing.
    #[error("token without exp")]
    NoExpiry,
    /// `exp` passed more than [`LEEWAY`] ago.
    #[error("token expired")]
    Expired,
    /// `nbf` is more than [`LEEWAY`] ahead.
    #[error("token not yet valid")]
    NotYetValid,
}

/// The video grant of a token (`video`): what the bearer may do with
/// rooms and the Egress and Ingress services.
///
/// Field names follow the "Tokens & grants" table. `false` and `None`
/// fields are left out of the token, as LiveKit's own `omitempty` does.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VideoGrant {
    /// `roomCreate`: "Permission to create or delete rooms".
    #[serde(skip_serializing_if = "is_false")]
    pub room_create: bool,
    /// `roomList`: "Permission to list available rooms".
    #[serde(skip_serializing_if = "is_false")]
    pub room_list: bool,
    /// `roomRecord`: "Permission to use Egress service".
    #[serde(skip_serializing_if = "is_false")]
    pub room_record: bool,
    /// `roomAdmin`: "Permission to moderate a room".
    #[serde(skip_serializing_if = "is_false")]
    pub room_admin: bool,
    /// `roomJoin`: "Permission to join a room".
    #[serde(skip_serializing_if = "is_false")]
    pub room_join: bool,
    /// `room`: "Name of the room, required if join or admin is set".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    /// `ingressAdmin`: "Permission to use Ingress service".
    #[serde(skip_serializing_if = "is_false")]
    pub ingress_admin: bool,
    /// `canPublish` (LiveKit default `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_publish: Option<bool>,
    /// `canSubscribe` (LiveKit default `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_subscribe: Option<bool>,
    /// `canPublishData` (LiveKit default: the value of `canPublish`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_publish_data: Option<bool>,
    /// `hidden`: "Hides the participant from others in the room".
    #[serde(skip_serializing_if = "is_false")]
    pub hidden: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl VideoGrant {
    /// `{"roomRecord": true}`: the grant every Egress API call needs
    /// (requests "need the `roomRecord` permission on the access token").
    pub fn room_record() -> Self {
        Self {
            room_record: true,
            ..Self::default()
        }
    }

    /// `{"roomJoin": true, "room": room}`: a participant join grant.
    pub fn room_join(room: impl Into<String>) -> Self {
        Self {
            room_join: true,
            room: Some(room.into()),
            ..Self::default()
        }
    }

    /// `{"roomAdmin": true, "room": room}`: moderation of one room.
    pub fn room_admin(room: impl Into<String>) -> Self {
        Self {
            room_admin: true,
            room: Some(room.into()),
            ..Self::default()
        }
    }
}

/// The claims of one access token.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claims {
    /// `iss`: "API key used to issue this token".
    pub issuer: String,
    /// `sub`: "Unique identity for the participant". Server API tokens and
    /// webhook tokens have none.
    pub subject: Option<String>,
    /// `name`: the participant's display name.
    pub name: Option<String>,
    /// `iat`.
    pub issued_at: Option<SystemTime>,
    /// `nbf`: "Start time that the token becomes valid".
    pub not_before: Option<SystemTime>,
    /// `exp`: "Expiration time of token".
    pub expires_at: SystemTime,
    /// `video`: the video grant.
    pub video: Option<VideoGrant>,
    /// `sha256`: on webhook tokens, the base64 SHA-256 of the body.
    pub sha256: Option<String>,
}

impl Claims {
    /// These claims with subject `identity`.
    pub fn with_subject(self, identity: impl Into<String>) -> Self {
        Self {
            subject: Some(identity.into()),
            ..self
        }
    }

    /// These claims with display name `name`.
    pub fn with_name(self, name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..self
        }
    }

    /// These claims with video grant `grant`.
    pub fn with_video(self, grant: VideoGrant) -> Self {
        Self {
            video: Some(grant),
            ..self
        }
    }

    /// These claims with the `sha256` body checksum `sha256`.
    pub fn with_sha256(self, sha256: impl Into<String>) -> Self {
        Self {
            sha256: Some(sha256.into()),
            ..self
        }
    }
}

/// A LiveKit API key and its secret.
///
/// The key names the secret (it is the token's `iss`); the secret signs
/// and verifies tokens. The `Debug` impl never shows the secret.
#[derive(Clone)]
pub struct ApiKey {
    key: String,
    secret: String,
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKey")
            .field("key", &self.key)
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl ApiKey {
    /// API key `key` with secret `secret` (LiveKit's `LIVEKIT_API_KEY` and
    /// `LIVEKIT_API_SECRET`).
    pub fn new(key: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            secret: secret.into(),
        }
    }

    /// The API key (the token issuer).
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Claims issued by this key at `now`, valid from `now` for
    /// `valid_for`, with no subject and no grant: `iss` this key, `iat`
    /// and `nbf` `now`, `exp` `now + valid_for`, as LiveKit's `ToJWT`
    /// sets them.
    pub fn claims(&self, now: SystemTime, valid_for: Duration) -> Claims {
        Claims {
            issuer: self.key.clone(),
            subject: None,
            name: None,
            issued_at: Some(now),
            not_before: Some(now),
            expires_at: now + valid_for,
            video: None,
            sha256: None,
        }
    }

    /// `claims` as an HS256 JWT signed with this key's secret: header
    /// `{"alg":"HS256","typ":"JWT"}`, times as whole Unix seconds.
    pub fn sign(&self, claims: &Claims) -> String {
        let mut payload = serde_json::Map::new();
        payload.insert("iss".to_owned(), claims.issuer.clone().into());
        if let Some(sub) = &claims.subject {
            payload.insert("sub".to_owned(), sub.clone().into());
        }
        if let Some(name) = &claims.name {
            payload.insert("name".to_owned(), name.clone().into());
        }
        if let Some(t) = claims.issued_at {
            payload.insert("iat".to_owned(), unix_seconds(t).into());
        }
        if let Some(t) = claims.not_before {
            payload.insert("nbf".to_owned(), unix_seconds(t).into());
        }
        payload.insert("exp".to_owned(), unix_seconds(claims.expires_at).into());
        if let Some(video) = &claims.video {
            // A struct of bools, strings and options always serializes.
            payload.insert(
                "video".to_owned(),
                serde_json::to_value(video).unwrap_or(Value::Null),
            );
        }
        if let Some(sha) = &claims.sha256 {
            payload.insert("sha256".to_owned(), sha.clone().into());
        }
        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(HEADER),
            URL_SAFE_NO_PAD.encode(Value::Object(payload).to_string())
        );
        let key = hmac::Key::new(hmac::HMAC_SHA256, self.secret.as_bytes());
        let tag = hmac::sign(&key, signing_input.as_bytes());
        format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(tag.as_ref()))
    }

    /// The claims of `token` if it is an HS256 JWT signed with this key's
    /// secret, issued by this key, with an `exp` no more than [`LEEWAY`]
    /// before `now` and an `nbf` (if any) no more than [`LEEWAY`] after
    /// it. Claims other than those of [`Claims`] are ignored.
    ///
    /// # Errors
    ///
    /// The [`TokenError`] naming the first check that failed.
    pub fn verify(&self, token: &str, now: SystemTime) -> Result<Claims, TokenError> {
        let (header, payload, signature) = split(token)?;
        let alg = header.get("alg").and_then(Value::as_str).unwrap_or("");
        if alg != "HS256" {
            return Err(TokenError::Algorithm(alg.to_owned()));
        }
        let (signing_input, _) = token.rsplit_once('.').ok_or(TokenError::Malformed)?;
        let key = hmac::Key::new(hmac::HMAC_SHA256, self.secret.as_bytes());
        hmac::verify(&key, signing_input.as_bytes(), &signature)
            .map_err(|_| TokenError::Signature)?;
        let claims = parse_claims(&payload)?;
        if claims.issuer != self.key {
            return Err(TokenError::Issuer);
        }
        if now > claims.expires_at + LEEWAY {
            return Err(TokenError::Expired);
        }
        if claims.not_before.is_some_and(|nbf| now + LEEWAY < nbf) {
            return Err(TokenError::NotYetValid);
        }
        Ok(claims)
    }
}

/// The `iss` of `token`, read without verifying it, to pick the key that
/// verifies it.
pub(crate) fn unverified_issuer(token: &str) -> Result<String, TokenError> {
    let (_, payload, _) = split(token)?;
    payload
        .get("iss")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(TokenError::Malformed)
}

/// The decoded header, payload and signature of a compact JWS.
fn split(token: &str) -> Result<(Value, Value, Vec<u8>), TokenError> {
    let mut parts = token.split('.');
    let (Some(h), Some(p), Some(s), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(TokenError::Malformed);
    };
    let json = |seg: &str| -> Result<Value, TokenError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(seg)
            .map_err(|_| TokenError::Malformed)?;
        let v: Value = serde_json::from_slice(&bytes).map_err(|_| TokenError::Malformed)?;
        if v.is_object() {
            Ok(v)
        } else {
            Err(TokenError::Malformed)
        }
    };
    let sig = URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|_| TokenError::Malformed)?;
    Ok((json(h)?, json(p)?, sig))
}

fn parse_claims(payload: &Value) -> Result<Claims, TokenError> {
    let string = |k: &str| payload.get(k).and_then(Value::as_str).map(str::to_owned);
    let time = |k: &str| -> Result<Option<SystemTime>, TokenError> {
        match payload.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => {
                // RFC 7519 NumericDate: seconds, possibly fractional.
                let secs = v.as_f64().ok_or(TokenError::Malformed)?;
                if !secs.is_finite() || secs < 0.0 {
                    return Err(TokenError::Malformed);
                }
                Ok(Some(UNIX_EPOCH + Duration::from_secs_f64(secs)))
            }
        }
    };
    let video = match payload.get("video") {
        None | Some(Value::Null) => None,
        Some(v) => Some(serde_json::from_value(v.clone()).map_err(|_| TokenError::Malformed)?),
    };
    Ok(Claims {
        issuer: string("iss").ok_or(TokenError::Malformed)?,
        subject: string("sub").filter(|s| !s.is_empty()),
        name: string("name"),
        issued_at: time("iat")?,
        not_before: time("nbf")?,
        expires_at: time("exp")?.ok_or(TokenError::NoExpiry)?,
        video,
        sha256: string("sha256"),
    })
}

/// Whole seconds since the Unix epoch; `0` for a time before it.
pub(crate) fn unix_seconds(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_the_secret() {
        let k = ApiKey::new("APIkey", "canary-secret-55aa");
        let shown = format!("{k:?}");
        assert!(!shown.contains("canary-secret-55aa"), "{shown}");
        assert!(shown.contains("APIkey"));
    }

    #[test]
    fn round_trip() {
        let k = ApiKey::new("APIkey", "secret");
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let claims = k
            .claims(now, Duration::from_secs(600))
            .with_subject("alice")
            .with_video(VideoGrant::room_join("r1"));
        let token = k.sign(&claims);
        assert_eq!(k.verify(&token, now).unwrap(), claims);
    }

    #[test]
    fn rejections() {
        let k = ApiKey::new("APIkey", "secret");
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let token = k.sign(&k.claims(now, Duration::from_secs(600)));
        assert_eq!(
            ApiKey::new("APIkey", "other").verify(&token, now),
            Err(TokenError::Signature)
        );
        assert_eq!(
            ApiKey::new("APIother", "secret").verify(&token, now),
            Err(TokenError::Issuer)
        );
        // Within the leeway is fine; past it is not.
        let later = now + Duration::from_secs(600 + 60);
        assert!(k.verify(&token, later).is_ok());
        assert_eq!(
            k.verify(&token, later + Duration::from_secs(1)),
            Err(TokenError::Expired)
        );
        let earlier = now - Duration::from_secs(61);
        assert_eq!(k.verify(&token, earlier), Err(TokenError::NotYetValid));
        assert_eq!(k.verify("a.b", now), Err(TokenError::Malformed));
        let mut tampered = token.clone();
        tampered.push('A');
        assert!(k.verify(&tampered, now).is_err());
    }

    #[test]
    fn other_algorithms_are_refused() {
        let k = ApiKey::new("APIkey", "secret");
        // {"alg":"none"}.{"iss":"APIkey","exp":1}.
        let token = format!(
            "{}.{}.",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
            URL_SAFE_NO_PAD.encode(r#"{"iss":"APIkey","exp":1}"#)
        );
        assert_eq!(
            k.verify(&token, UNIX_EPOCH),
            Err(TokenError::Algorithm("none".to_owned()))
        );
    }
}
