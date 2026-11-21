//! LiveKit errors and their VTC mappings.
//!
//! Oracles: the Twirp wire protocol's error codes,
//! <https://twitchtv.github.io/twirp/docs/spec_v7.html#error-codes>, and
//! LiveKit's Egress service (`pkg/service/egress.go` in livekit/livekit:
//! `permission_denied` without `roomRecord`, `failed_precondition` for
//! stopping an egress that has ended).

use jmap_types::{Id, SetError, SetErrorType, impl_string_enum};
use jmap_vtc_types::{CompositionLayout, LivestreamFailureReason, LivestreamMode};

/// A Twirp error code, from the error body's `code`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TwirpCode {
    /// `canceled` (408).
    Canceled,
    /// `unknown` (500).
    Unknown,
    /// `invalid_argument` (400).
    InvalidArgument,
    /// `malformed` (400): the body could not be decoded.
    Malformed,
    /// `deadline_exceeded` (408).
    DeadlineExceeded,
    /// `not_found` (404).
    NotFound,
    /// `bad_route` (404): no such method.
    BadRoute,
    /// `already_exists` (409).
    AlreadyExists,
    /// `permission_denied` (403): for Egress, a token without
    /// `roomRecord`.
    PermissionDenied,
    /// `unauthenticated` (401): no token, or one that does not verify.
    Unauthenticated,
    /// `resource_exhausted` (429).
    ResourceExhausted,
    /// `failed_precondition` (412): for `StopEgress`, an egress that has
    /// already ended.
    FailedPrecondition,
    /// `aborted` (409).
    Aborted,
    /// `out_of_range` (400).
    OutOfRange,
    /// `unimplemented` (501).
    Unimplemented,
    /// `internal` (500).
    Internal,
    /// `unavailable` (503): for Egress, no egress worker answered.
    Unavailable,
    /// `dataloss` (500).
    Dataloss,
    /// A code this version does not list, or none (`Other("")`).
    Other(String),
}

impl_string_enum!(TwirpCode, "a Twirp error code",
    "canceled" => Canceled,
    "unknown" => Unknown,
    "invalid_argument" => InvalidArgument,
    "malformed" => Malformed,
    "deadline_exceeded" => DeadlineExceeded,
    "not_found" => NotFound,
    "bad_route" => BadRoute,
    "already_exists" => AlreadyExists,
    "permission_denied" => PermissionDenied,
    "unauthenticated" => Unauthenticated,
    "resource_exhausted" => ResourceExhausted,
    "failed_precondition" => FailedPrecondition,
    "aborted" => Aborted,
    "out_of_range" => OutOfRange,
    "unimplemented" => Unimplemented,
    "internal" => Internal,
    "unavailable" => Unavailable,
    "dataloss" => Dataloss,
);

/// A failed LiveKit operation.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum LiveKitError {
    /// LiveKit answered with a Twirp error.
    #[error("LiveKit {code} (HTTP {status}){}", msg.as_deref().map(|m| format!(": {m}")).unwrap_or_default())]
    Twirp {
        /// The HTTP status.
        status: u16,
        /// The Twirp code; `Other("")` when the body named none.
        code: TwirpCode,
        /// The Twirp `msg`, if any.
        msg: Option<String>,
    },
    /// No response: connect, TLS, timeout, or a request that could not be
    /// built.
    #[error("LiveKit request failed: {0}")]
    Transport(#[from] reqwest::Error),
    /// A success response whose body is not an `EgressInfo`.
    #[error("unexpected LiveKit response: {0}")]
    Response(String),
    /// A server URL or configuration that cannot be used.
    #[error("invalid LiveKit configuration: {0}")]
    Config(String),
    /// The id has characters outside the RFC 8620 §1.2 `Id` alphabet
    /// (`A-Za-z0-9`, `-`, `_`) or is empty or longer than 255 octets, so
    /// it cannot name an object key or a URL path segment.
    #[error("id {0:?} cannot name an output")]
    UnsafeId(Id),
    /// The livestream's mode is not served: neither `"hosted"` (with a
    /// hosted output configured) nor `"push"`.
    #[error("livestream mode {0} is not served by this adapter")]
    UnsupportedMode(LivestreamMode),
    /// A push livestream without a `streamUri`.
    #[error("push livestream without a streamUri")]
    MissingStreamUri,
    /// The `streamUri` is not an absolute URI with a host. The value is
    /// not shown: it may carry a stream key.
    #[error("streamUri is not an absolute URI with a host")]
    InvalidStreamUri,
    /// The `streamUri` scheme is not one of
    /// [`PUSH_SCHEMES`](crate::PUSH_SCHEMES).
    #[error("push scheme {0:?} is not supported")]
    UnsupportedScheme(String),
    /// The `layout` has no LiveKit layout configured.
    #[error("layout {0} has no LiveKit layout")]
    UnsupportedLayout(CompositionLayout),
}

impl LiveKitError {
    /// The Twirp code, for a [`LiveKitError::Twirp`] error.
    pub fn code(&self) -> Option<&TwirpCode> {
        match self {
            Self::Twirp { code, .. } => Some(code),
            _ => None,
        }
    }

    /// `StopEgress` found nothing to stop: `failed_precondition` (the
    /// egress has ended) or `not_found`.
    pub(crate) fn is_gone(&self) -> bool {
        matches!(
            self.code(),
            Some(TwirpCode::FailedPrecondition | TwirpCode::NotFound)
        )
    }

    /// The `failureReason` to report when starting a livestream failed
    /// with this error (draft-atwood-jmap-vtc-00 §VTCLivestream):
    ///
    /// - `"rejected"` ("The ingest destination or the media pipeline
    ///   refused the stream") for a request LiveKit refused as invalid
    ///   (`invalid_argument`, `malformed`, `out_of_range`, `not_found`,
    ///   `already_exists`, `failed_precondition`) and for a livestream
    ///   this adapter cannot serve ([`LiveKitError::UnsafeId`],
    ///   [`LiveKitError::UnsupportedMode`], [`LiveKitError::MissingStreamUri`],
    ///   [`LiveKitError::InvalidStreamUri`],
    ///   [`LiveKitError::UnsupportedScheme`],
    ///   [`LiveKitError::UnsupportedLayout`]);
    /// - `"unavailable"` ("The media pipeline could not be provisioned")
    ///   for everything else: no egress worker (`unavailable`,
    ///   `resource_exhausted`), credentials (`unauthenticated`,
    ///   `permission_denied`), LiveKit faults and transport failures.
    pub fn failure_reason(&self) -> LivestreamFailureReason {
        match self {
            Self::Twirp {
                code:
                    TwirpCode::InvalidArgument
                    | TwirpCode::Malformed
                    | TwirpCode::OutOfRange
                    | TwirpCode::NotFound
                    | TwirpCode::AlreadyExists
                    | TwirpCode::FailedPrecondition,
                ..
            }
            | Self::UnsafeId(_)
            | Self::UnsupportedMode(_)
            | Self::MissingStreamUri
            | Self::InvalidStreamUri
            | Self::UnsupportedScheme(_)
            | Self::UnsupportedLayout(_) => LivestreamFailureReason::Rejected,
            _ => LivestreamFailureReason::Unavailable,
        }
    }

    /// The RFC 8620 §5.3 [`SetError`] for a deployment that checks a create
    /// with this crate while answering the `/set` (for example
    /// [`LiveKitMedia::push_url`](crate::LiveKitMedia::push_url) on the
    /// `streamUri`), or `None` for a server failure the method answers
    /// with `serverFail`:
    ///
    /// | Error | SetError |
    /// |---|---|
    /// | [`LiveKitError::MissingStreamUri`], [`LiveKitError::InvalidStreamUri`], [`LiveKitError::UnsupportedScheme`] | `invalidProperties` naming `streamUri` |
    /// | [`LiveKitError::UnsupportedMode`] | `invalidProperties` naming `mode` |
    /// | [`LiveKitError::UnsupportedLayout`] | `invalidProperties` naming `layout` |
    /// | `invalid_argument`, `malformed` | `invalidProperties` |
    /// | `resource_exhausted` | `rateLimit` |
    /// | `permission_denied` | `forbidden` |
    /// | `not_found` | `notFound` |
    pub fn set_error(&self) -> Option<SetError> {
        let (kind, property) = match self {
            Self::MissingStreamUri | Self::InvalidStreamUri | Self::UnsupportedScheme(_) => {
                (SetErrorType::InvalidProperties, Some("streamUri"))
            }
            Self::UnsupportedMode(_) => (SetErrorType::InvalidProperties, Some("mode")),
            Self::UnsupportedLayout(_) => (SetErrorType::InvalidProperties, Some("layout")),
            Self::Twirp { code, .. } => match code {
                TwirpCode::InvalidArgument | TwirpCode::Malformed => {
                    (SetErrorType::InvalidProperties, None)
                }
                TwirpCode::ResourceExhausted => (SetErrorType::RateLimit, None),
                TwirpCode::PermissionDenied => (SetErrorType::Forbidden, None),
                TwirpCode::NotFound => (SetErrorType::NotFound, None),
                _ => return None,
            },
            _ => return None,
        };
        let error = SetError::new(kind).with_description(self.to_string());
        Some(match property {
            Some(p) => error.with_properties([p]),
            None => error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn twirp(code: &str) -> LiveKitError {
        LiveKitError::Twirp {
            status: 400,
            code: TwirpCode::from(code),
            msg: None,
        }
    }

    #[test]
    fn failure_reasons() {
        use LivestreamFailureReason::{Rejected, Unavailable};
        for (e, reason) in [
            (twirp("invalid_argument"), Rejected),
            (twirp("not_found"), Rejected),
            (twirp("failed_precondition"), Rejected),
            (twirp("unavailable"), Unavailable),
            (twirp("resource_exhausted"), Unavailable),
            (twirp("permission_denied"), Unavailable),
            (twirp("internal"), Unavailable),
            (twirp(""), Unavailable),
            (
                LiveKitError::UnsupportedScheme("https".to_owned()),
                Rejected,
            ),
            (
                LiveKitError::UnsupportedLayout(CompositionLayout::from("com.example.stage")),
                Rejected,
            ),
            (LiveKitError::Config("x".to_owned()), Unavailable),
        ] {
            assert_eq!(e.failure_reason(), reason, "{e}");
        }
    }

    #[test]
    fn set_errors() {
        let e = LiveKitError::UnsupportedScheme("https".to_owned())
            .set_error()
            .unwrap();
        assert_eq!(e.error_type, SetErrorType::InvalidProperties);
        assert_eq!(e.properties, Some(vec!["streamUri".to_owned()]));
        let e = LiveKitError::UnsupportedLayout(CompositionLayout::Grid)
            .set_error()
            .unwrap();
        assert_eq!(e.properties, Some(vec!["layout".to_owned()]));
        assert_eq!(
            twirp("resource_exhausted").set_error().unwrap().error_type,
            SetErrorType::RateLimit
        );
        assert_eq!(
            twirp("permission_denied").set_error().unwrap().error_type,
            SetErrorType::Forbidden
        );
        assert!(twirp("internal").set_error().is_none());
        assert!(LiveKitError::Response("x".to_owned()).set_error().is_none());
    }
}
