//! Free-tools media backend for `jmap-vtc-server` livestreams and
//! recordings: ffmpeg writing HLS to a static origin, no DRM, no vendor.
//!
//! This is a support crate (workspace AGENTS.md §Support crates): it
//! starts processes and writes files, and no `*-types` or `*-server`
//! crate depends on it. A deployment calls it from its
//! [`VtcBackend::run_media_effect`](jmap_vtc_server::VtcBackend::run_media_effect)
//! hook and reports what the processes do through
//! [`MediaReporter`](jmap_vtc_server::media_report::MediaReporter).
//! It is the vendor-neutral reference backend
//! (docs/design/vtc-broadcast-research.md §3.3, decision 57) and the
//! backend the media hooks are tested against.
//!
//! [`FfmpegMedia`] runs one ffmpeg process per livestream and per
//! recording part:
//!
//! - a **hosted** `VTCLivestream` is encoded to HLS under a directory a
//!   static web server serves, and its playback endpoint is the playlist
//!   URL under the configured base URL;
//! - a **push** `VTCLivestream` is sent to its `streamUri` over RTMP(S),
//!   SRT or WHIP, with the `streamKey` presented as the draft says;
//! - a `VTCRecording` is written as Matroska parts, one per unpaused
//!   interval, and joined into one MP4 when it stops.
//!
//! The watchers ([`FfmpegMedia::watch_livestream`],
//! [`FfmpegMedia::watch_recording`]) poll the processes and their output
//! files and report `"live"`, `"failed"` and `"stopped"`.
//!
//! The media comes from a [`Source`](args::Source): the call's composited
//! feed, an SFU's RTP forward or any other ffmpeg input, or ffmpeg's
//! synthetic test pattern for tests. [`args`] builds every ffmpeg
//! argument vector as a pure function; [`process`] is the seam between
//! the adapter and the processes ([`process::ProcessRunner`]), real
//! ([`process::TokioRunner`]) or faked in tests. Nothing goes through a
//! shell.
//!
//! Not covered: compositing a call into one feed (the deployment's, as
//! for jmap-vtc-aws), `layout` changes, DASH, per-viewer playback
//! authorization, `timeReference`, caption tracks, idle stop and serving
//! the files. See the README's Gotchas.
//!
//! # Features
//!
//! `real-ffmpeg` compiles the tests that run the real ffmpeg binary
//! (also `#[ignore]`d, so `cargo test` never needs ffmpeg). It changes no
//! library code.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub mod args;
mod error;
mod media;
pub mod process;

pub use error::FfmpegError;
pub use media::{
    DEFAULT_POLL_INTERVAL, DEFAULT_START_TIMEOUT, DEFAULT_STOP_GRACE, FfmpegConfig, FfmpegMedia,
    RecordedFile,
};

pub use url;
