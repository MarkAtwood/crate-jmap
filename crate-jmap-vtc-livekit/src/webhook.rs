//! LiveKit webhooks: verifying the signed request and reading the event.
//!
//! Oracles: "Webhooks", <https://docs.livekit.io/home/server/webhooks/>
//! ("these requests have an `Authorization` header containing a signed JWT
//! token. The token includes a sha256 hash of the payload"; `Content-Type`
//! `application/webhook+json`; the event names; "no guarantees around
//! delivery"), and livekit/protocol `webhook/url_notifier.go` (the body is
//! `protojson.Marshal(event)`; the token is an access token of the API key
//! valid for 5 minutes whose `sha256` claim is the standard base64 SHA-256
//! of that body, set as the bare `Authorization` header value) and
//! `webhook/verifier.go` (`Receive`: verify the token with the issuer's
//! secret, then compare the checksum).

use std::time::SystemTime;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jmap_types::impl_string_enum;
use ring::digest;
use serde::Deserialize;

use crate::egress::EgressInfo;
use crate::token::{ApiKey, TokenError, unverified_issuer};

/// The `Content-Type` of a webhook request: "use a custom mime type to
/// ensure signature is checked prior to parsing".
pub const CONTENT_TYPE: &str = "application/webhook+json";

/// Why a webhook request was refused.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    /// No `Authorization` value.
    #[error("no Authorization token")]
    NoToken,
    /// The token's issuer is not a key this receiver knows.
    #[error("token issued by an unknown API key")]
    UnknownKey,
    /// The token does not verify.
    #[error("bad webhook token: {0}")]
    Token(#[from] TokenError),
    /// The token carries no `sha256`, or it does not match the body.
    #[error("the body does not match the token's sha256")]
    Checksum,
    /// The verified body is not a `WebhookEvent`.
    #[error("webhook body: {0}")]
    Body(#[from] serde_json::Error),
}

/// The `event` of a webhook.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WebhookEventKind {
    /// `room_started`.
    RoomStarted,
    /// `room_finished`.
    RoomFinished,
    /// `participant_joined`.
    ParticipantJoined,
    /// `participant_left`.
    ParticipantLeft,
    /// `track_published`.
    TrackPublished,
    /// `track_unpublished`.
    TrackUnpublished,
    /// `egress_started`: "Recording/streaming (egress) starts."
    EgressStarted,
    /// `egress_updated`: "Egress updates (for example, file size
    /// change)."
    EgressUpdated,
    /// `egress_ended`: "Egress ends."
    EgressEnded,
    /// `ingress_started`.
    IngressStarted,
    /// `ingress_ended`.
    IngressEnded,
    /// Any other event.
    Other(String),
}

impl_string_enum!(WebhookEventKind, "a LiveKit webhook event name",
    "room_started" => RoomStarted,
    "room_finished" => RoomFinished,
    "participant_joined" => ParticipantJoined,
    "participant_left" => ParticipantLeft,
    "track_published" => TrackPublished,
    "track_unpublished" => TrackUnpublished,
    "egress_started" => EgressStarted,
    "egress_updated" => EgressUpdated,
    "egress_ended" => EgressEnded,
    "ingress_started" => IngressStarted,
    "ingress_ended" => IngressEnded,
);

/// One webhook (`WebhookEvent`), with the fields this crate uses.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WebhookEvent {
    /// `event`.
    pub event: WebhookEventKind,
    /// `id`: "unique event uuid".
    #[serde(default)]
    pub id: String,
    /// `createdAt`: "timestamp in seconds".
    #[serde(
        default,
        alias = "created_at",
        rename = "createdAt",
        deserialize_with = "crate::egress::int64"
    )]
    pub created_at: i64,
    /// `egressInfo`, "set when event is egress_*".
    #[serde(default, alias = "egress_info", rename = "egressInfo")]
    pub egress_info: Option<EgressInfo>,
}

/// Verifies webhook requests signed by one or more API keys.
#[derive(Debug, Clone)]
pub struct WebhookReceiver {
    keys: Vec<ApiKey>,
}

impl WebhookReceiver {
    /// A receiver for webhooks signed with `key`.
    pub fn new(key: ApiKey) -> Self {
        Self { keys: vec![key] }
    }

    /// This receiver also accepting webhooks signed with `key` (for a key
    /// rotation).
    pub fn with_key(mut self, key: ApiKey) -> Self {
        self.keys.push(key);
        self
    }

    /// Check a webhook request: `authorization` is its `Authorization`
    /// header value (a bare token, as LiveKit sends it, or `Bearer`
    /// followed by one), `body` the exact bytes received. The token must
    /// verify with the secret of its issuer at `now` ([`ApiKey::verify`])
    /// and carry the base64 SHA-256 of `body` as `sha256`.
    ///
    /// The checksum comparison is not constant-time: it compares a hash of
    /// the body the sender chose with a value inside a token whose
    /// signature has already been checked in constant time.
    ///
    /// # Errors
    ///
    /// The [`WebhookError`] of the first check that failed.
    pub fn verify(
        &self,
        body: &[u8],
        authorization: &str,
        now: SystemTime,
    ) -> Result<(), WebhookError> {
        let token = authorization.trim();
        let token = token.strip_prefix("Bearer ").unwrap_or(token).trim();
        if token.is_empty() {
            return Err(WebhookError::NoToken);
        }
        let issuer = unverified_issuer(token)?;
        let key = self
            .keys
            .iter()
            .find(|k| k.key() == issuer)
            .ok_or(WebhookError::UnknownKey)?;
        let claims = key.verify(token, now)?;
        let sum = STANDARD.encode(digest::digest(&digest::SHA256, body));
        if claims.sha256.as_deref() != Some(sum.as_str()) {
            return Err(WebhookError::Checksum);
        }
        Ok(())
    }

    /// [`Self::verify`], then parse `body` as a [`WebhookEvent`]. Unknown
    /// fields are ignored, as LiveKit's own receiver does
    /// (`DiscardUnknown`).
    ///
    /// # Errors
    ///
    /// As for [`Self::verify`], and [`WebhookError::Body`] for a body that
    /// is not a `WebhookEvent`.
    pub fn receive(
        &self,
        body: &[u8],
        authorization: &str,
        now: SystemTime,
    ) -> Result<WebhookEvent, WebhookError> {
        self.verify(body, authorization, now)?;
        Ok(serde_json::from_slice(body)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn verify_round_trip_and_tamper() {
        let key = ApiKey::new("APIkey", "secret");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let body = br#"{"event":"room_started","id":"EV_1","createdAt":"1700000000"}"#;
        let sum = STANDARD.encode(digest::digest(&digest::SHA256, body));
        let token = key.sign(&key.claims(now, Duration::from_secs(300)).with_sha256(sum));
        let rx = WebhookReceiver::new(ApiKey::new("other", "x")).with_key(key);
        let e = rx.receive(body, &token, now).unwrap();
        assert_eq!(e.event, WebhookEventKind::RoomStarted);
        assert_eq!(e.created_at, 1_700_000_000);
        assert!(rx.verify(body, &format!("Bearer {token}"), now).is_ok());
        assert!(matches!(
            rx.verify(b"{}", &token, now),
            Err(WebhookError::Checksum)
        ));
        assert!(matches!(
            rx.verify(body, "", now),
            Err(WebhookError::NoToken)
        ));
        assert!(matches!(
            WebhookReceiver::new(ApiKey::new("nobody", "x")).verify(body, &token, now),
            Err(WebhookError::UnknownKey)
        ));
    }
}
