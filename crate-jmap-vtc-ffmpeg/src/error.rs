//! Adapter errors and their VTC mappings.

use std::io;
use std::path::PathBuf;

use jmap_types::{Id, SetError, SetErrorType};
use jmap_vtc_types::{LivestreamFailureReason, LivestreamMode};

use crate::process::ProcessExit;

/// A media operation that failed.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum FfmpegError {
    /// The program could not be started (not installed, not executable).
    #[error("could not start {}: {source}", program.display())]
    Spawn {
        /// The program.
        program: PathBuf,
        /// The spawn error.
        #[source]
        source: io::Error,
    },
    /// A file or directory operation failed.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The error.
        #[source]
        source: io::Error,
    },
    /// Stopping, killing or waiting for a process failed.
    #[error("process control failed: {0}")]
    Process(#[source] io::Error),
    /// ffmpeg ended unsuccessfully where success was needed (joining the
    /// recording parts).
    #[error("ffmpeg ended with {0}")]
    Exited(ProcessExit),
    /// The id has characters outside the RFC 8620 §1.2 `Id` alphabet
    /// (`A-Za-z0-9`, `-`, `_`) or is empty or longer than 255 octets, so
    /// it cannot name a directory.
    #[error("id {0:?} cannot name a directory")]
    UnsafeId(Id),
    /// The livestream's mode is neither `"hosted"` nor `"push"`.
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
    /// [`args::PUSH_SCHEMES`](crate::args::PUSH_SCHEMES).
    #[error("push scheme {0:?} is not supported")]
    UnsupportedScheme(String),
    /// No recording with this id is running here.
    #[error("no recording {0} is running")]
    NotRunning(Id),
    /// The recording stopped with no media written.
    #[error("recording {0} has no media")]
    EmptyRecording(Id),
}

impl FfmpegError {
    /// The `failureReason` to report when starting a livestream failed
    /// with this error (draft-atwood-jmap-vtc-00 §VTCLivestream):
    ///
    /// - `"rejected"` ("The ingest destination or the media pipeline
    ///   refused the stream") for a livestream this adapter cannot serve:
    ///   [`FfmpegError::UnsafeId`], [`FfmpegError::UnsupportedMode`],
    ///   [`FfmpegError::MissingStreamUri`],
    ///   [`FfmpegError::InvalidStreamUri`],
    ///   [`FfmpegError::UnsupportedScheme`];
    /// - `"unavailable"` ("The media pipeline could not be provisioned")
    ///   for everything else: ffmpeg missing, a directory that cannot be
    ///   written, a process that cannot be controlled.
    pub fn failure_reason(&self) -> LivestreamFailureReason {
        match self {
            Self::UnsafeId(_)
            | Self::UnsupportedMode(_)
            | Self::MissingStreamUri
            | Self::InvalidStreamUri
            | Self::UnsupportedScheme(_) => LivestreamFailureReason::Rejected,
            _ => LivestreamFailureReason::Unavailable,
        }
    }

    /// The RFC 8620 §5.3 SetError for a deployment that checks a
    /// livestream create with this crate while answering the `/set` (for
    /// example [`PushTarget::new`](crate::args::PushTarget::new) on the
    /// `streamUri`), or `None` for a server failure the method answers
    /// with `serverFail`:
    ///
    /// | Error | SetError |
    /// |---|---|
    /// | [`FfmpegError::MissingStreamUri`], [`FfmpegError::InvalidStreamUri`], [`FfmpegError::UnsupportedScheme`] | `invalidProperties` naming `streamUri` |
    /// | [`FfmpegError::UnsupportedMode`] | `invalidProperties` naming `mode` |
    pub fn set_error(&self) -> Option<SetError> {
        let property = match self {
            Self::MissingStreamUri | Self::InvalidStreamUri | Self::UnsupportedScheme(_) => {
                "streamUri"
            }
            Self::UnsupportedMode(_) => "mode",
            _ => return None,
        };
        Some(
            SetError::new(SetErrorType::InvalidProperties)
                .with_description(self.to_string())
                .with_properties([property]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_reasons() {
        use LivestreamFailureReason::{Rejected, Unavailable};
        let io = || io::Error::from(io::ErrorKind::NotFound);
        for (e, reason) in [
            (FfmpegError::UnsafeId(Id::from("../x")), Rejected),
            (
                FfmpegError::UnsupportedMode(LivestreamMode::from("relay")),
                Rejected,
            ),
            (FfmpegError::MissingStreamUri, Rejected),
            (FfmpegError::InvalidStreamUri, Rejected),
            (FfmpegError::UnsupportedScheme("udp".to_owned()), Rejected),
            (
                FfmpegError::Spawn {
                    program: "ffmpeg".into(),
                    source: io(),
                },
                Unavailable,
            ),
            (
                FfmpegError::Io {
                    path: "/srv/hls".into(),
                    source: io(),
                },
                Unavailable,
            ),
            (FfmpegError::Process(io()), Unavailable),
        ] {
            assert_eq!(e.failure_reason(), reason, "{e}");
        }
    }

    #[test]
    fn set_errors() {
        let e = FfmpegError::UnsupportedScheme("udp".to_owned())
            .set_error()
            .expect("set error");
        assert_eq!(e.error_type, SetErrorType::InvalidProperties);
        assert_eq!(e.properties, Some(vec!["streamUri".to_owned()]));
        let e = FfmpegError::UnsupportedMode(LivestreamMode::from("relay"))
            .set_error()
            .expect("set error");
        assert_eq!(e.properties, Some(vec!["mode".to_owned()]));
        assert!(
            FfmpegError::Process(io::Error::other("x"))
                .set_error()
                .is_none()
        );
    }
}
