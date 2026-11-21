//! AWS errors and their VTC mappings.
//!
//! Oracles: the IVS low-latency "Common Error Types",
//! <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/CommonErrors.html>,
//! and each action's Errors section (e.g.
//! <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_CreateChannel.html#API_CreateChannel_Errors>).

use jmap_types::{SetError, SetErrorType, impl_string_enum};
use jmap_vtc_types::{LivestreamFailureReason, LivestreamMode};

/// An AWS error code, from the `x-amzn-ErrorType` response header or the
/// body's `__type`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AwsErrorCode {
    /// `AccessDeniedException` (403): "User does not have sufficient
    /// access to perform this action."
    AccessDenied,
    /// `ChannelNotBroadcasting` (404): "The stream is offline for the
    /// given channel ARN."
    ChannelNotBroadcasting,
    /// `ConflictException` (409): "Updating or deleting a resource can
    /// cause an inconsistent state", e.g. `DeleteChannel` on a live
    /// channel.
    Conflict,
    /// `ExpiredTokenException` (403): the session token has expired.
    ExpiredToken,
    /// `InternalFailure` (500).
    InternalFailure,
    /// `InternalServerException` (500).
    InternalServer,
    /// `PendingVerification` (403): "Your account is pending
    /// verification."
    PendingVerification,
    /// `ResourceNotFoundException` (404).
    ResourceNotFound,
    /// `ServiceQuotaExceededException` (402): "Request would cause a
    /// service quota to be exceeded."
    ServiceQuotaExceeded,
    /// `ServiceUnavailable` (503).
    ServiceUnavailable,
    /// `StreamUnavailable` (503): "The stream is temporarily unavailable."
    StreamUnavailable,
    /// `ThrottlingException` (400 or 429): the request rate is too high.
    Throttling,
    /// `UnrecognizedClientException` (403): unknown access key id.
    UnrecognizedClient,
    /// `ValidationException` (400): "The input fails to satisfy the
    /// constraints specified by an AWS service."
    Validation,
    /// A code this version does not list.
    Other(String),
}

impl_string_enum!(AwsErrorCode, "an AWS error code",
    "AccessDeniedException" => AccessDenied,
    "ChannelNotBroadcasting" => ChannelNotBroadcasting,
    "ConflictException" => Conflict,
    "ExpiredTokenException" => ExpiredToken,
    "InternalFailure" => InternalFailure,
    "InternalServerException" => InternalServer,
    "PendingVerification" => PendingVerification,
    "ResourceNotFoundException" => ResourceNotFound,
    "ServiceQuotaExceededException" => ServiceQuotaExceeded,
    "ServiceUnavailable" => ServiceUnavailable,
    "StreamUnavailable" => StreamUnavailable,
    "ThrottlingException" => Throttling,
    "UnrecognizedClientException" => UnrecognizedClient,
    "ValidationException" => Validation,
);

/// A failed AWS call.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum AwsError {
    /// AWS answered with an error status.
    #[error("AWS {code} (HTTP {status}){}", message.as_deref().map(|m| format!(": {m}")).unwrap_or_default())]
    Service {
        /// The HTTP status.
        status: u16,
        /// The AWS error code; `Other("")` when the response named none.
        code: AwsErrorCode,
        /// The service's message (`exceptionMessage` or `message`), if
        /// any.
        message: Option<String>,
    },
    /// No response: connect, TLS, timeout, or a request that could not
    /// be built.
    #[error("AWS request failed: {0}")]
    Transport(#[from] reqwest::Error),
    /// A success response whose body is not what the API reference
    /// documents.
    #[error("unexpected AWS response: {0}")]
    Response(String),
    /// A region or endpoint that does not make a valid request URL.
    #[error("invalid AWS configuration: {0}")]
    Config(String),
    /// The livestream's mode has no AWS path in this crate: IVS channels
    /// serve hosted livestreams; a push livestream goes from the
    /// deployment's compositor straight to its `streamUri`.
    #[error("livestream mode {0} is not served by this adapter")]
    UnsupportedMode(LivestreamMode),
}

impl AwsError {
    /// The AWS error code, for a [`AwsError::Service`] error.
    pub fn code(&self) -> Option<&AwsErrorCode> {
        match self {
            Self::Service { code, .. } => Some(code),
            _ => None,
        }
    }

    /// The `failureReason` to report when starting a livestream failed
    /// with this error (draft-atwood-jmap-vtc-00 §VTCLivestream):
    ///
    /// - `"rejected"` ("The ingest destination or the media pipeline
    ///   refused the stream") for a request IVS refused as invalid
    ///   (`ValidationException`, `ConflictException`) and for
    ///   [`AwsError::UnsupportedMode`];
    /// - `"unavailable"` ("The media pipeline could not be provisioned,
    ///   for example for lack of capacity") for everything else: quotas
    ///   (`ServiceQuotaExceededException`), throttling, an account that
    ///   is not allowed or not verified, credential errors, AWS outages and
    ///   transport failures.
    pub fn failure_reason(&self) -> LivestreamFailureReason {
        match self {
            Self::Service {
                code: AwsErrorCode::Validation | AwsErrorCode::Conflict,
                ..
            }
            | Self::UnsupportedMode(_) => LivestreamFailureReason::Rejected,
            _ => LivestreamFailureReason::Unavailable,
        }
    }

    /// The RFC 8620 §5.3 [`SetError`] for a deployment that calls AWS while
    /// answering a `/set` (for example a capacity check before accepting a
    /// livestream create), or `None` when the error is a server failure
    /// the method answers with `serverFail`:
    ///
    /// | AWS | SetError |
    /// |---|---|
    /// | `ServiceQuotaExceededException` | `overQuota` |
    /// | `ThrottlingException` | `rateLimit` |
    /// | `AccessDeniedException`, `PendingVerification` | `forbidden` |
    /// | `ResourceNotFoundException` | `notFound` |
    /// | `ValidationException` | `invalidProperties` |
    /// | [`AwsError::UnsupportedMode`] | `invalidProperties` naming `mode` |
    pub fn set_error(&self) -> Option<SetError> {
        let (kind, properties): (SetErrorType, &[&str]) = match self {
            Self::Service { code, .. } => match code {
                AwsErrorCode::ServiceQuotaExceeded => (SetErrorType::OverQuota, &[]),
                AwsErrorCode::Throttling => (SetErrorType::RateLimit, &[]),
                AwsErrorCode::AccessDenied | AwsErrorCode::PendingVerification => {
                    (SetErrorType::Forbidden, &[])
                }
                AwsErrorCode::ResourceNotFound => (SetErrorType::NotFound, &[]),
                AwsErrorCode::Validation => (SetErrorType::InvalidProperties, &[]),
                _ => return None,
            },
            Self::UnsupportedMode(_) => (SetErrorType::InvalidProperties, &["mode"]),
            _ => return None,
        };
        let error = SetError::new(kind).with_description(self.to_string());
        Some(if properties.is_empty() {
            error
        } else {
            error.with_properties(properties.iter().copied())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(code: &str, status: u16) -> AwsError {
        AwsError::Service {
            status,
            code: AwsErrorCode::from(code),
            message: None,
        }
    }

    #[test]
    fn failure_reasons() {
        use LivestreamFailureReason::{Rejected, Unavailable};
        for (code, status, reason) in [
            ("ValidationException", 400, Rejected),
            ("ConflictException", 409, Rejected),
            ("ServiceQuotaExceededException", 402, Unavailable),
            ("ThrottlingException", 429, Unavailable),
            ("AccessDeniedException", 403, Unavailable),
            ("PendingVerification", 403, Unavailable),
            ("InternalServerException", 500, Unavailable),
            ("SomethingNew", 500, Unavailable),
        ] {
            assert_eq!(service(code, status).failure_reason(), reason, "{code}");
        }
        assert_eq!(
            AwsError::UnsupportedMode(LivestreamMode::Push).failure_reason(),
            Rejected
        );
    }

    #[test]
    fn set_errors() {
        let kind = |e: &AwsError| e.set_error().map(|s| s.error_type);
        assert_eq!(
            kind(&service("ServiceQuotaExceededException", 402)),
            Some(SetErrorType::OverQuota)
        );
        assert_eq!(
            kind(&service("ThrottlingException", 429)),
            Some(SetErrorType::RateLimit)
        );
        assert_eq!(
            kind(&service("AccessDeniedException", 403)),
            Some(SetErrorType::Forbidden)
        );
        assert_eq!(kind(&service("InternalServerException", 500)), None);
        let mode = AwsError::UnsupportedMode(LivestreamMode::Push)
            .set_error()
            .expect("set error");
        assert_eq!(mode.properties, Some(vec!["mode".to_owned()]));
    }

    #[test]
    fn unknown_code_round_trips() {
        assert_eq!(
            AwsErrorCode::from("NewException"),
            AwsErrorCode::Other("NewException".to_owned())
        );
        assert_eq!(AwsErrorCode::Validation.as_str(), "ValidationException");
    }
}
