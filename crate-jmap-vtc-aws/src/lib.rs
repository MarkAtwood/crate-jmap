//! AWS media backend for `jmap-vtc-server` livestreams.
//!
//! This is a support crate (workspace AGENTS.md §Support crates): it does
//! network I/O against AWS and holds AWS credentials, and no `*-types` or
//! `*-server` crate depends on it. A deployment calls it from its
//! [`VtcBackend::run_media_effect`](jmap_vtc_server::VtcBackend::run_media_effect)
//! hook and feeds the AWS state events back through
//! [`MediaReporter`](jmap_vtc_server::media_report::MediaReporter).
//!
//! One AWS path is implemented, the one docs/design/vtc-broadcast-research.md
//! recommends for calls on the deployment's own SFU: the call's composited
//! program feed is pushed over RTMPS into an **Amazon IVS low-latency
//! channel**, one channel per hosted `VTCLivestream`, and viewers play its
//! HLS output. [`ivs`] holds the IVS API client ([`ivs::IvsClient`]: the
//! `CreateChannel`, `CreateStreamKey`, `GetStreamKey`, `GetStream`,
//! `StopStream` and `DeleteChannel` actions), the livestream adapter
//! ([`ivs::IvsLivestreams`]), the ES384 viewer tokens of private channels
//! ([`ivs::PlaybackKey`]) and the EventBridge stream events
//! ([`ivs::IvsEvent`]).
//!
//! Requests are signed with AWS Signature Version 4 by this crate (a small
//! signer over [`ring`] HMAC-SHA256), with no AWS SDK. The signer and the
//! JSON request path are service-neutral, so an Elemental MediaLive and
//! MediaPackage module (for DRM, DASH or DVR windows) can sit next to
//! [`ivs`] later and reuse them.
//!
//! Not covered: push-mode livestreams (IVS cannot push to an arbitrary
//! RTMP destination; the deployment's compositor pushes to `streamUri`
//! itself), recordings, IVS Real-Time stages and server-side composition,
//! MediaLive and MediaPackage, CloudFront signed URLs, credential
//! discovery (environment, profiles, instance metadata, STS), and retries.
//! See the README's Gotchas.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

use std::fmt;

mod client;
mod error;
pub mod ivs;
mod sigv4;
mod time;

pub use error::{AwsError, AwsErrorCode};

pub use reqwest;
pub use url;

/// The per-request timeout of the clients this crate's `new` constructors
/// build.
pub const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// AWS access keys for Signature Version 4.
///
/// Long-term keys of an IAM user, or temporary keys from AWS STS with
/// their session token. This crate does not look credentials up; the
/// deployment supplies them (and replaces the client before temporary keys
/// expire). The `Debug` impl never shows the secret key or the session
/// token.
#[derive(Clone)]
pub struct Credentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

impl Credentials {
    /// An access key id and its secret access key.
    pub fn new(access_key_id: impl Into<String>, secret_access_key: impl Into<String>) -> Self {
        Self {
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            session_token: None,
        }
    }

    /// These temporary credentials with their STS session token, sent as
    /// `X-Amz-Security-Token` and signed.
    pub fn with_session_token(self, session_token: impl Into<String>) -> Self {
        Self {
            session_token: Some(session_token.into()),
            ..self
        }
    }

    /// The access key id.
    pub fn access_key_id(&self) -> &str {
        &self.access_key_id
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"<redacted>")
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// A client that follows no redirects and times out after
/// [`REQUEST_TIMEOUT`].
fn default_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(REQUEST_TIMEOUT)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Security testing pattern 1 (workspace AGENTS.md): the secret key
    /// and session token never reach `Debug`.
    #[test]
    fn credentials_debug_redacts_secrets() {
        let c = Credentials::new("AKIDEXAMPLE", "canary-secret-7f3a")
            .with_session_token("canary-token-91bc");
        let shown = format!("{c:?}");
        assert!(!shown.contains("canary-secret-7f3a"), "{shown}");
        assert!(!shown.contains("canary-token-91bc"), "{shown}");
        assert!(shown.contains("AKIDEXAMPLE"), "{shown}");
    }
}
