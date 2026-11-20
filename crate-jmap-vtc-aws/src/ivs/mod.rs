//! Amazon IVS low-latency streaming: one IVS channel per hosted
//! `VTCLivestream`.
//!
//! The flow, for a deployment whose calls run on its own SFU
//! (docs/design/vtc-broadcast-research.md §1.3 and Recommendation 4):
//!
//! 1. `VTCLivestream/set` stores a hosted livestream in `"starting"` and
//!    the deployment's [`run_media_effect`](jmap_vtc_server::VtcBackend::run_media_effect)
//!    receives [`MediaEffect::StartLivestream`](jmap_vtc_server::MediaEffect::StartLivestream).
//!    It calls [`IvsLivestreams::start`], which creates a channel
//!    (`CreateChannel`, with its stream key), keeps the returned
//!    [`IvsChannel`] with the livestream, and points the call's compositor
//!    at [`IvsChannel::rtmps_url`] with [`IvsChannel::stream_key`]. If
//!    `start` fails it reports `"failed"` with [`AwsError::failure_reason`](crate::AwsError::failure_reason).
//! 2. IVS sends EventBridge events; the deployment routes those of the
//!    channel to [`IvsLivestreams::report_event`]. "Stream Start" moves the
//!    livestream to `"live"` with an HLS [`VtcPlaybackEndpoint`](jmap_vtc_types::VtcPlaybackEndpoint);
//!    "Stream End", "Stream Failure" and an ingest limit breach move it to
//!    `"failed"`.
//! 3. [`VtcBackend::playback_endpoints_for`](jmap_vtc_server::VtcBackend::playback_endpoints_for)
//!    calls [`IvsLivestreams::endpoints_for`], which adds a short-lived
//!    viewer token to the URL when the channel is private.
//! 4. A stop of any kind (moderator, call end, destroy, failure, idle
//!    stop) arrives as [`MediaEffect::StopLivestream`](jmap_vtc_server::MediaEffect::StopLivestream);
//!    the deployment calls [`IvsLivestreams::stop`] (`StopStream`, then
//!    `DeleteChannel`).
//!
//! The channel only ingests the feed the deployment's compositor makes, so
//! the livestream's `layout` and
//! [`MediaEffect::ChangeLivestreamLayout`](jmap_vtc_server::MediaEffect::ChangeLivestreamLayout)
//! are the compositor's, not this module's. A deployment using this module
//! alone advertises `livestreamModes: ["hosted"]` and
//! `livestreamPushSchemes: []`: [`IvsLivestreams::start`] refuses push
//! mode.
//!
//! [`IvsClient`] is the API underneath, usable on its own.

mod api;
mod events;
mod livestreams;
mod token;

pub use api::{
    Channel, ChannelType, CreateChannelInput, CreatedChannel, IvsClient, LatencyMode, SrtIngest,
    Stream, StreamHealth, StreamKey, StreamState,
};
pub use events::{IvsEvent, IvsEventKind, LimitName};
pub use livestreams::{
    ChannelTemplate, DEFAULT_TOKEN_TTL, IvsChannel, IvsLivestreams, StopOutcome,
};
pub use token::{MaximumResolution, PlaybackClaims, PlaybackKey, TokenError};
