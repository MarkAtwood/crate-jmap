//! LiveKit media backend for `jmap-vtc-server` livestreams and
//! recordings.
//!
//! This is a support crate (workspace AGENTS.md §Support crates): it does
//! network I/O against a LiveKit server and holds its API secret, and no
//! `*-types` or `*-server` crate depends on it. A deployment whose calls
//! run in LiveKit rooms calls it from its
//! [`VtcBackend::run_media_effect`](jmap_vtc_server::VtcBackend::run_media_effect)
//! hook and feeds LiveKit's egress webhooks back through
//! [`MediaReporter`](jmap_vtc_server::media_report::MediaReporter).
//!
//! Every livestream and recording is one LiveKit **room-composite
//! egress**: the egress renders the call's room in a web template (the
//! `layout`) and encodes it ([`LiveKitMedia`]):
//!
//! - a **push** `VTCLivestream` streams to its `streamUri` over RTMP(S) or
//!   SRT, with the `streamKey` presented as the draft says;
//! - a **hosted** `VTCLivestream` writes HLS segments to object storage
//!   ([`HostedOutput::Hls`]) or streams into a media origin such as
//!   MediaMTX, which serves WHEP and low-latency HLS
//!   ([`HostedOutput::Origin`]);
//! - a `VTCRecording` writes one MP4 file to object storage, which the
//!   deployment imports into its blob store while the recording is
//!   `"processing"`.
//!
//! The `layout` of draft-atwood-jmap-vtc-00 §Composition Layouts maps to
//! the egress layout (`grid`, `speaker`, `single-speaker` by default), and
//! a layout change is an `UpdateLayout` call. LiveKit has no egress pause,
//! so this crate does not pause recordings: advertise
//! `supportsRecordingPause: false`.
//!
//! [`egress`] is the Egress Twirp client underneath ([`egress::EgressClient`]:
//! `StartEgress`, `UpdateLayout`, `StopEgress`, `ListEgress`) with the
//! request and `EgressInfo` types. Egresses start with LiveKit's unified
//! `StartEgress` RPC, not the deprecated `StartRoomCompositeEgress`:
//! LiveKit servers before v1.13.2 and egress workers before v1.14.0
//! refuse it (see the README). [`token`] signs and verifies
//! LiveKit access tokens (HS256 JWTs with the video grant) with an
//! [`token::ApiKey`]; [`webhook`] verifies signed webhooks
//! ([`webhook::WebhookReceiver`]) and reads their events.
//! [`LiveKitMedia::egress_outcome`] maps an egress state to the VTC
//! transition (pure), and [`LiveKitMedia::report_egress`] applies it.
//!
//! Not covered: joining calls to rooms (the deployment names the room),
//! the LiveKit `RoomService`, participant and track egress, WHIP output
//! (LiveKit has none), recording pause, per-viewer playback tokens,
//! `timeReference`, and Ed25519-signed (`livekit-v2+jwt`) tokens. See the
//! README's Gotchas.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub mod egress;
mod error;
mod media;
pub mod token;
pub mod webhook;

pub use error::{LiveKitError, TwirpCode};
pub use media::{
    EgressJob, EgressOutcome, EgressReport, EgressReportError, HlsOutput, HostedOutput,
    ID_PLACEHOLDER, JobObject, LiveKitConfig, LiveKitMedia, OriginEndpoint, OriginOutput,
    PUSH_SCHEMES, Reported, UploadedFile,
};

pub use reqwest;
pub use url;

/// The per-request timeout of the clients this crate's `new` constructors
/// build.
pub const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// A client that follows no redirects and times out after
/// [`REQUEST_TIMEOUT`].
fn default_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(REQUEST_TIMEOUT)
        .build()
}
