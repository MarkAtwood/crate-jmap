//! The `VTCLivestream` and `VTCRecording` adapter over room-composite
//! egresses.

use jmap_types::Id;
use jmap_vtc_server::VtcBackend;
use jmap_vtc_server::media_report::{MediaReporter, RecordingFile, ReportError};
use jmap_vtc_types::{
    CompositionLayout, LatencyHint, LivestreamFailureReason, LivestreamMode, LivestreamState,
    PlaybackProtocol, RecordingState, VtcLivestream, VtcPlaybackEndpoint, VtcRecording,
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::LiveKitError;
use crate::egress::{
    EgressClient, EgressInfo, EgressStatus, EncodingPreset, FileOutput, RoomCompositeRequest,
    S3Upload, SegmentOutput, StreamOutput, StreamProtocol,
};

/// The `streamUri` schemes a push livestream may use: LiveKit Egress
/// streams over RTMP(S) and SRT ("Protocols: RTMP and SRT"); it has no
/// WHIP output. Advertise them as `livestreamPushSchemes`.
pub const PUSH_SCHEMES: &[&str] = &["rtmp", "rtmps", "srt"];

/// The `{id}` placeholder of [`OriginOutput`] URL templates.
pub const ID_PLACEHOLDER: &str = "{id}";

/// Where hosted livestreams go.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedOutput {
    /// HLS segments uploaded to object storage and served from there.
    Hls(HlsOutput),
    /// An RTMP or SRT stream into a media origin (MediaMTX, for example)
    /// that serves the viewers.
    Origin(OriginOutput),
}

/// Hosted livestreams as HLS in object storage: the segments and
/// playlists of livestream `{id}` go under `{key_prefix}{id}/`, and
/// viewers play `{base_url}{id}/{live_playlist_name}`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlsOutput {
    /// The object key prefix, e.g. `jmap-vtc/live/`.
    pub key_prefix: String,
    /// The public URL of `key_prefix` (a CDN or the bucket), ending in
    /// `/`.
    pub base_url: Url,
    /// The full playlist (`index.m3u8`).
    pub playlist_name: String,
    /// The live playlist viewers play, "containing only the last few
    /// segments" (`live.m3u8`).
    pub live_playlist_name: String,
    /// The segment length in seconds; `None` is LiveKit's default.
    pub segment_duration: Option<u32>,
}

impl HlsOutput {
    /// HLS under `key_prefix`, served at `base_url`, with the playlist
    /// names `index.m3u8` and `live.m3u8` and 2-second segments.
    pub fn new(key_prefix: impl Into<String>, base_url: Url) -> Self {
        Self {
            key_prefix: key_prefix.into(),
            base_url,
            playlist_name: "index.m3u8".to_owned(),
            live_playlist_name: "live.m3u8".to_owned(),
            segment_duration: Some(2),
        }
    }

    /// These settings with `seconds`-long segments.
    pub fn with_segment_duration(self, seconds: u32) -> Self {
        Self {
            segment_duration: Some(seconds),
            ..self
        }
    }
}

/// One playback endpoint of an [`OriginOutput`], with `{id}` in `url`
/// standing for the livestream id.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginEndpoint {
    /// The protocol.
    pub protocol: PlaybackProtocol,
    /// The URL template.
    pub url: String,
    /// The latency hint.
    pub latency_hint: LatencyHint,
}

impl OriginEndpoint {
    /// `protocol` at template `url` with `latency_hint`.
    pub fn new(
        protocol: PlaybackProtocol,
        url: impl Into<String>,
        latency_hint: LatencyHint,
    ) -> Self {
        Self {
            protocol,
            url: url.into(),
            latency_hint,
        }
    }
}

/// Hosted livestreams pushed into a media origin: the egress streams
/// livestream `{id}` to `ingest` with `{id}` replaced, and viewers get
/// `endpoints` with `{id}` replaced.
///
/// For sub-second playback put MediaMTX here: ingest
/// `srt://media.example.com:8890?streamid=publish:vtc/{id}`, endpoints
/// WHEP `https://media.example.com/vtc/{id}/whep` (`"interactive"`) and
/// HLS `https://media.example.com/vtc/{id}/index.m3u8` (`"low"`). The
/// per-viewer reader token MediaMTX checks is the deployment's to mint in
/// `VtcBackend::playback_endpoints_for`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginOutput {
    /// The `rtmp://`, `rtmps://` or `srt://` ingest URL template.
    pub ingest: String,
    /// The playback endpoint templates.
    pub endpoints: Vec<OriginEndpoint>,
}

impl OriginOutput {
    /// Ingest at template `ingest`, no endpoints yet.
    pub fn new(ingest: impl Into<String>) -> Self {
        Self {
            ingest: ingest.into(),
            endpoints: Vec::new(),
        }
    }

    /// This origin with one more playback endpoint.
    pub fn with_endpoint(mut self, endpoint: OriginEndpoint) -> Self {
        self.endpoints.push(endpoint);
        self
    }
}

/// How [`LiveKitMedia`] builds egresses.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveKitConfig {
    /// The bucket for recordings and HLS output, or `None` for the
    /// storage configured on the egress server.
    pub storage: Option<S3Upload>,
    /// The object key prefix of recordings: recording `{id}` is written to
    /// `{recording_prefix}{id}.mp4`.
    pub recording_prefix: String,
    /// Where hosted livestreams go, or `None` for push mode only.
    pub hosted: Option<HostedOutput>,
    /// The LiveKit layout for `layout: null` (the server's default).
    pub default_layout: String,
    /// The VTC layouts offered and the LiveKit layout each one is drawn
    /// with.
    pub layouts: Vec<(CompositionLayout, String)>,
    /// The encoding preset, or `None` for LiveKit's (`H264_720P_30`).
    pub preset: Option<EncodingPreset>,
    /// A custom web template (`custom_base_url`), or `None` for the
    /// egress's built-in one.
    pub custom_base_url: Option<String>,
}

impl Default for LiveKitConfig {
    /// Server-configured storage, recordings under `jmap-vtc/recordings/`,
    /// push mode only, default layout `speaker`, and the three layouts of
    /// draft-atwood-jmap-vtc-00 §Composition Layouts drawn with the
    /// built-in templates of the same names.
    fn default() -> Self {
        Self {
            storage: None,
            recording_prefix: "jmap-vtc/recordings/".to_owned(),
            hosted: None,
            default_layout: "speaker".to_owned(),
            layouts: vec![
                (CompositionLayout::Grid, "grid".to_owned()),
                (CompositionLayout::Speaker, "speaker".to_owned()),
                (
                    CompositionLayout::SingleSpeaker,
                    "single-speaker".to_owned(),
                ),
            ],
            preset: None,
            custom_base_url: None,
        }
    }
}

impl LiveKitConfig {
    /// This configuration uploading to `storage`.
    pub fn with_storage(self, storage: S3Upload) -> Self {
        Self {
            storage: Some(storage),
            ..self
        }
    }

    /// This configuration writing recordings under `prefix`.
    pub fn with_recording_prefix(self, prefix: impl Into<String>) -> Self {
        Self {
            recording_prefix: prefix.into(),
            ..self
        }
    }

    /// This configuration serving hosted livestreams through `hosted`.
    pub fn with_hosted(self, hosted: HostedOutput) -> Self {
        Self {
            hosted: Some(hosted),
            ..self
        }
    }

    /// This configuration drawing `layout: null` as LiveKit layout
    /// `livekit`.
    pub fn with_default_layout(self, livekit: impl Into<String>) -> Self {
        Self {
            default_layout: livekit.into(),
            ..self
        }
    }

    /// This configuration offering `layout`, drawn as LiveKit layout
    /// `livekit` (for example `grid-light`, or a custom template's name),
    /// replacing any earlier mapping of `layout`.
    pub fn with_layout(mut self, layout: CompositionLayout, livekit: impl Into<String>) -> Self {
        let livekit = livekit.into();
        match self.layouts.iter_mut().find(|(l, _)| *l == layout) {
            Some(entry) => entry.1 = livekit,
            None => self.layouts.push((layout, livekit)),
        }
        self
    }

    /// This configuration offering no layout choice.
    pub fn without_layouts(self) -> Self {
        Self {
            layouts: Vec::new(),
            ..self
        }
    }

    /// This configuration encoding with `preset`.
    pub fn with_preset(self, preset: EncodingPreset) -> Self {
        Self {
            preset: Some(preset),
            ..self
        }
    }

    /// This configuration rendering the custom template at `url`.
    pub fn with_custom_base_url(self, url: impl Into<String>) -> Self {
        Self {
            custom_base_url: Some(url.into()),
            ..self
        }
    }
}

/// What an egress serves.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum JobObject {
    /// A `VTCLivestream`.
    Livestream {
        /// The livestream id.
        id: Id,
        /// Its mode.
        mode: LivestreamMode,
    },
    /// A `VTCRecording`.
    Recording {
        /// The recording id.
        id: Id,
    },
}

/// One egress serving one livestream or recording: what the start methods
/// return and every later call takes.
///
/// The deployment stores it with the object (it is `Serialize`) and finds
/// it again by `egress_id` when a webhook arrives, so the adapter itself
/// keeps no state.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressJob {
    /// The LiveKit egress id (`EG_...`).
    pub egress_id: String,
    /// What it serves.
    pub object: JobObject,
}

/// The file of a finished recording egress, to import into the JMAP blob
/// store.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedFile {
    /// Where the egress uploaded it (`FileInfo.location`, e.g. an S3 URL).
    pub location: String,
    /// Its name (`FileInfo.filename`).
    pub filename: String,
    /// Its size in bytes.
    pub size: u64,
    /// Its duration in whole seconds, rounded to nearest.
    pub duration: u64,
    /// `video/mp4`.
    pub media_type: String,
}

impl UploadedFile {
    /// The [`RecordingFile`] to report with
    /// [`MediaReporter::recording_available`] once this file is stored as
    /// blob `blob_id`.
    pub fn recording_file(&self, blob_id: Id) -> RecordingFile {
        RecordingFile::new(blob_id, self.size, self.duration, self.media_type.clone())
    }
}

/// What an egress state means for its livestream or recording: the pure
/// half of [`LiveKitMedia::report_egress`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressOutcome {
    /// Nothing to report.
    Nothing,
    /// The livestream is live, with these endpoints (none in push mode).
    LivestreamLive(Vec<VtcPlaybackEndpoint>),
    /// The livestream failed.
    LivestreamFailed(LivestreamFailureReason),
    /// The egress ended by itself: LiveKit ends a room composite "When all
    /// of the participants leave the room", the idle stop of
    /// draft-atwood-jmap-vtc-00 §Livestream States.
    LivestreamEnded,
    /// The recording egress ended and uploaded this file.
    RecordingUploaded(UploadedFile),
    /// The recording egress ended without a file.
    RecordingFailed,
}

/// What [`LiveKitMedia::report_egress`] changed.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reported {
    /// Nothing.
    Nothing,
    /// The livestream's state.
    Livestream,
    /// The recording is `"processing"`: import this file into the blob
    /// store, then report [`MediaReporter::recording_available`] (or
    /// [`MediaReporter::recording_failed`] if the import fails).
    RecordingProcessing(UploadedFile),
    /// The recording is `"failed"`.
    RecordingFailed,
}

/// Why [`LiveKitMedia::report_egress`] changed nothing.
#[non_exhaustive]
#[derive(Debug)]
pub enum EgressReportError<E> {
    /// The egress info is for another egress than the job's.
    OtherEgress,
    /// A livestream report failed; [`ReportError::WrongState`] for a late,
    /// duplicate or raced event.
    Livestream(ReportError<E, LivestreamState>),
    /// A recording report failed; [`ReportError::WrongState`] for a late,
    /// duplicate or raced event.
    Recording(ReportError<E, RecordingState>),
}

impl<E: std::fmt::Display> std::fmt::Display for EgressReportError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OtherEgress => f.write_str("the egress info is for another egress"),
            Self::Livestream(e) => write!(f, "livestream report: {e}"),
            Self::Recording(e) => write!(f, "recording report: {e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for EgressReportError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OtherEgress => None,
            Self::Livestream(e) => Some(e),
            Self::Recording(e) => Some(e),
        }
    }
}

/// The result of [`LiveKitMedia::report_egress`].
pub type EgressReport<E> = Result<Reported, EgressReportError<E>>;

/// Hosted and push `VTCLivestream`s and `VTCRecording`s as LiveKit
/// room-composite egresses, one egress per livestream and per recording.
///
/// Stateless: every call after a start takes the [`EgressJob`] the start
/// returned.
#[derive(Debug, Clone)]
pub struct LiveKitMedia {
    client: EgressClient,
    config: LiveKitConfig,
}

impl LiveKitMedia {
    /// An adapter over `client` configured by `config`.
    pub fn new(client: EgressClient, config: LiveKitConfig) -> Self {
        Self { client, config }
    }

    /// The Egress client.
    pub fn client(&self) -> &EgressClient {
        &self.client
    }

    /// The configuration.
    pub fn config(&self) -> &LiveKitConfig {
        &self.config
    }

    /// The account-level `layouts` to advertise: the configured VTC
    /// layouts.
    pub fn advertised_layouts(&self) -> Vec<CompositionLayout> {
        self.config.layouts.iter().map(|(l, _)| l.clone()).collect()
    }

    /// The account-level `livestreamModes` to advertise: `"push"`, and
    /// `"hosted"` when a hosted output is configured.
    pub fn advertised_modes(&self) -> Vec<LivestreamMode> {
        let mut modes = vec![LivestreamMode::Push];
        if self.config.hosted.is_some() {
            modes.push(LivestreamMode::Hosted);
        }
        modes
    }

    /// The LiveKit layout for VTC `layout` (`None`: the default).
    ///
    /// # Errors
    ///
    /// [`LiveKitError::UnsupportedLayout`] for a layout with no mapping.
    pub fn livekit_layout(&self, layout: Option<&CompositionLayout>) -> Result<&str, LiveKitError> {
        match layout {
            None => Ok(&self.config.default_layout),
            Some(l) => self
                .config
                .layouts
                .iter()
                .find(|(v, _)| v == l)
                .map(|(_, lk)| lk.as_str())
                .ok_or_else(|| LiveKitError::UnsupportedLayout(l.clone())),
        }
    }

    /// The stream URL for push `stream_uri` and `stream_key`, presented as
    /// draft-atwood-jmap-vtc-00 §Presenting the Stream Key says:
    ///
    /// | Scheme | LiveKit protocol | Stream key |
    /// |---|---|---|
    /// | `rtmp`, `rtmps` | `RTMP` | "the RTMP stream name, appended to the application path given by `streamUri`" |
    /// | `srt` | `SRT` | "the SRT stream ID", the `streamid` query parameter; "A `streamUri` that already carries a stream ID is used as given" |
    ///
    /// The result carries the key: do not log it.
    ///
    /// # Errors
    ///
    /// [`LiveKitError::InvalidStreamUri`] for a `stream_uri` that is not an
    /// absolute URI with a host, [`LiveKitError::UnsupportedScheme`] for a
    /// scheme not in [`PUSH_SCHEMES`].
    pub fn push_url(
        stream_uri: &str,
        stream_key: Option<&str>,
    ) -> Result<(StreamProtocol, String), LiveKitError> {
        let mut url = Url::parse(stream_uri).map_err(|_| LiveKitError::InvalidStreamUri)?;
        if !PUSH_SCHEMES.contains(&url.scheme()) {
            return Err(LiveKitError::UnsupportedScheme(url.scheme().to_owned()));
        }
        if url.host_str().is_none_or(str::is_empty) {
            return Err(LiveKitError::InvalidStreamUri);
        }
        let key = stream_key.filter(|k| !k.is_empty());
        let protocol = if url.scheme() == "srt" {
            let has_id = url.query_pairs().any(|(k, _)| k == "streamid");
            if let (Some(key), false) = (key, has_id) {
                url.query_pairs_mut().append_pair("streamid", key);
            }
            StreamProtocol::Srt
        } else {
            if let Some(key) = key {
                url.path_segments_mut()
                    .map_err(|()| LiveKitError::InvalidStreamUri)?
                    .pop_if_empty()
                    .push(key);
            }
            StreamProtocol::Rtmp
        };
        Ok((protocol, url.into()))
    }

    /// The `playbackEndpoints` of hosted livestream `id` once it is live:
    /// the live playlist for [`HostedOutput::Hls`] (HLS, `"standard"`), the
    /// configured endpoints for [`HostedOutput::Origin`]. No credential:
    /// a deployment that authorizes viewers adds it in
    /// `VtcBackend::playback_endpoints_for`.
    ///
    /// # Errors
    ///
    /// [`LiveKitError::UnsupportedMode`] without a hosted output,
    /// [`LiveKitError::UnsafeId`], [`LiveKitError::Config`] for a base URL
    /// that cannot be joined.
    pub fn playback_endpoints(&self, id: &Id) -> Result<Vec<VtcPlaybackEndpoint>, LiveKitError> {
        check_id(id)?;
        match &self.config.hosted {
            None => Err(LiveKitError::UnsupportedMode(LivestreamMode::Hosted)),
            Some(HostedOutput::Hls(hls)) => {
                let url = hls
                    .base_url
                    .join(&format!("{id}/{}", hls.live_playlist_name))
                    .map_err(|e| LiveKitError::Config(format!("HLS base URL: {e}")))?;
                Ok(vec![VtcPlaybackEndpoint::new(
                    PlaybackProtocol::Hls,
                    url.to_string(),
                    LatencyHint::Standard,
                )])
            }
            Some(HostedOutput::Origin(origin)) => Ok(origin
                .endpoints
                .iter()
                .map(|e| {
                    VtcPlaybackEndpoint::new(
                        e.protocol.clone(),
                        e.url.replace(ID_PLACEHOLDER, id.as_ref()),
                        e.latency_hint.clone(),
                    )
                })
                .collect()),
        }
    }

    fn composite(
        &self,
        room_name: &str,
        layout: Option<&CompositionLayout>,
    ) -> Result<RoomCompositeRequest, LiveKitError> {
        let mut req =
            RoomCompositeRequest::new(room_name).with_layout(self.livekit_layout(layout)?);
        if let Some(preset) = &self.config.preset {
            req = req.with_preset(preset.clone());
        }
        if let Some(url) = &self.config.custom_base_url {
            req = req.with_custom_base_url(url.clone());
        }
        Ok(req)
    }

    /// The `StartRoomCompositeEgress` request for `livestream` of LiveKit
    /// room `room_name`, in the livestream's `layout`:
    ///
    /// - push: one stream output to [`Self::push_url`];
    /// - hosted, [`HostedOutput::Hls`]: one HLS segment output under
    ///   `{key_prefix}{id}/` with the live playlist, uploaded to the
    ///   configured storage;
    /// - hosted, [`HostedOutput::Origin`]: one stream output to the
    ///   origin's ingest URL.
    ///
    /// # Errors
    ///
    /// [`LiveKitError::UnsafeId`], [`LiveKitError::UnsupportedMode`] (any
    /// other mode, or hosted without a hosted output), the
    /// [`Self::push_url`] errors and [`LiveKitError::UnsupportedLayout`].
    pub fn livestream_request(
        &self,
        livestream: &VtcLivestream,
        room_name: &str,
    ) -> Result<RoomCompositeRequest, LiveKitError> {
        check_id(&livestream.id)?;
        let req = self.composite(room_name, livestream.layout.as_ref())?;
        match &livestream.mode {
            LivestreamMode::Push => {
                let uri = livestream
                    .stream_uri
                    .as_deref()
                    .ok_or(LiveKitError::MissingStreamUri)?;
                let (protocol, url) = Self::push_url(uri, livestream.stream_key.as_deref())?;
                Ok(req.with_stream(StreamOutput::new(protocol, vec![url])))
            }
            LivestreamMode::Hosted => match &self.config.hosted {
                None => Err(LiveKitError::UnsupportedMode(LivestreamMode::Hosted)),
                Some(HostedOutput::Hls(hls)) => {
                    let mut seg = SegmentOutput::hls(
                        format!("{}{}/segment", hls.key_prefix, livestream.id),
                        format!("{}{}/{}", hls.key_prefix, livestream.id, hls.playlist_name),
                    )
                    .with_live_playlist(format!(
                        "{}{}/{}",
                        hls.key_prefix, livestream.id, hls.live_playlist_name
                    ));
                    if let Some(d) = hls.segment_duration {
                        seg = seg.with_segment_duration(d);
                    }
                    if let Some(s3) = &self.config.storage {
                        seg = seg.with_s3(s3.clone());
                    }
                    Ok(req.with_segments(seg))
                }
                Some(HostedOutput::Origin(origin)) => {
                    let ingest = origin
                        .ingest
                        .replace(ID_PLACEHOLDER, livestream.id.as_ref());
                    let (protocol, url) = Self::push_url(&ingest, None)?;
                    Ok(req.with_stream(StreamOutput::new(protocol, vec![url])))
                }
            },
            other => Err(LiveKitError::UnsupportedMode(other.clone())),
        }
    }

    /// The `StartRoomCompositeEgress` request for `recording` of LiveKit
    /// room `room_name`: one MP4 file output at
    /// `{recording_prefix}{id}.mp4`, uploaded to the configured storage,
    /// in the recording's `layout`.
    ///
    /// # Errors
    ///
    /// [`LiveKitError::UnsafeId`] and [`LiveKitError::UnsupportedLayout`].
    pub fn recording_request(
        &self,
        recording: &VtcRecording,
        room_name: &str,
    ) -> Result<RoomCompositeRequest, LiveKitError> {
        check_id(&recording.id)?;
        let mut file = FileOutput::mp4(format!(
            "{}{}.mp4",
            self.config.recording_prefix, recording.id
        ));
        if let Some(s3) = &self.config.storage {
            file = file.with_s3(s3.clone());
        }
        Ok(self
            .composite(room_name, recording.layout.as_ref())?
            .with_file(file))
    }

    /// Start `livestream`
    /// ([`MediaEffect::StartLivestream`](jmap_vtc_server::MediaEffect::StartLivestream))
    /// as an egress of LiveKit room `room_name` (the deployment's room for
    /// the livestream's call) built by [`Self::livestream_request`].
    ///
    /// # Errors
    ///
    /// The [`Self::livestream_request`] errors and the call's
    /// [`LiveKitError`]. Report either with
    /// [`MediaReporter::livestream_failed`] and
    /// [`LiveKitError::failure_reason`].
    pub async fn start_livestream(
        &self,
        livestream: &VtcLivestream,
        room_name: &str,
    ) -> Result<EgressJob, LiveKitError> {
        let req = self.livestream_request(livestream, room_name)?;
        let info = self.client.start_room_composite_egress(&req).await?;
        Ok(EgressJob {
            egress_id: info.egress_id,
            object: JobObject::Livestream {
                id: livestream.id.clone(),
                mode: livestream.mode.clone(),
            },
        })
    }

    /// Start `recording`
    /// ([`MediaEffect::StartRecording`](jmap_vtc_server::MediaEffect::StartRecording))
    /// as an egress of LiveKit room `room_name` built by
    /// [`Self::recording_request`].
    ///
    /// # Errors
    ///
    /// The [`Self::recording_request`] errors and the call's
    /// [`LiveKitError`].
    pub async fn start_recording(
        &self,
        recording: &VtcRecording,
        room_name: &str,
    ) -> Result<EgressJob, LiveKitError> {
        let req = self.recording_request(recording, room_name)?;
        let info = self.client.start_room_composite_egress(&req).await?;
        Ok(EgressJob {
            egress_id: info.egress_id,
            object: JobObject::Recording {
                id: recording.id.clone(),
            },
        })
    }

    /// Stop `job`'s egress
    /// ([`MediaEffect::StopLivestream`](jmap_vtc_server::MediaEffect::StopLivestream),
    /// [`MediaEffect::StopRecording`](jmap_vtc_server::MediaEffect::StopRecording)).
    /// Idempotent: an egress that has already ended (`failed_precondition`)
    /// or is unknown (`not_found`) is fine. A recording's file arrives
    /// later, with the `egress_ended` webhook.
    ///
    /// # Errors
    ///
    /// The `StopEgress` [`LiveKitError`], other than those two.
    pub async fn stop(&self, job: &EgressJob) -> Result<(), LiveKitError> {
        match self.client.stop_egress(&job.egress_id).await {
            Ok(_) => Ok(()),
            Err(e) if e.is_gone() => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Re-arrange `job`'s egress in `layout`
    /// ([`MediaEffect::ChangeLivestreamLayout`](jmap_vtc_server::MediaEffect::ChangeLivestreamLayout),
    /// [`MediaEffect::ChangeRecordingLayout`](jmap_vtc_server::MediaEffect::ChangeRecordingLayout)):
    /// `UpdateLayout`.
    ///
    /// # Errors
    ///
    /// [`LiveKitError::UnsupportedLayout`] and the call's [`LiveKitError`].
    pub async fn change_layout(
        &self,
        job: &EgressJob,
        layout: Option<&CompositionLayout>,
    ) -> Result<(), LiveKitError> {
        let livekit = self.livekit_layout(layout)?;
        self.client.update_layout(&job.egress_id, livekit).await?;
        Ok(())
    }

    /// What `info`, the state of `job`'s egress, means for its object:
    ///
    /// | Egress status | Livestream | Recording |
    /// |---|---|---|
    /// | `EGRESS_ACTIVE` | live, with [`Self::playback_endpoints`] when hosted | nothing |
    /// | `EGRESS_COMPLETE` | ended (idle stop) | uploaded, with the first file result (failed if none) |
    /// | `EGRESS_LIMIT_REACHED` | failed, `"interrupted"` | as `EGRESS_COMPLETE` |
    /// | `EGRESS_FAILED` | failed: `"interrupted"` if the egress had started (`started_at` set), else `"rejected"` | failed |
    /// | `EGRESS_ABORTED` | failed, `"unavailable"` | failed |
    /// | `EGRESS_STARTING`, `EGRESS_ENDING`, other | nothing | nothing |
    ///
    /// The egress id is not checked here; [`Self::report_egress`] does.
    ///
    /// # Errors
    ///
    /// The [`Self::playback_endpoints`] errors for a hosted livestream.
    pub fn egress_outcome(
        &self,
        job: &EgressJob,
        info: &EgressInfo,
    ) -> Result<EgressOutcome, LiveKitError> {
        Ok(match &job.object {
            JobObject::Livestream { id, mode } => match info.status {
                EgressStatus::Active => EgressOutcome::LivestreamLive(match mode {
                    LivestreamMode::Hosted => self.playback_endpoints(id)?,
                    _ => Vec::new(),
                }),
                EgressStatus::Complete => EgressOutcome::LivestreamEnded,
                EgressStatus::LimitReached => {
                    EgressOutcome::LivestreamFailed(LivestreamFailureReason::Interrupted)
                }
                EgressStatus::Failed => EgressOutcome::LivestreamFailed(if info.started_at > 0 {
                    LivestreamFailureReason::Interrupted
                } else {
                    LivestreamFailureReason::Rejected
                }),
                EgressStatus::Aborted => {
                    EgressOutcome::LivestreamFailed(LivestreamFailureReason::Unavailable)
                }
                _ => EgressOutcome::Nothing,
            },
            JobObject::Recording { .. } => match info.status {
                EgressStatus::Complete | EgressStatus::LimitReached => {
                    match info.file_results.first() {
                        Some(f) => EgressOutcome::RecordingUploaded(UploadedFile {
                            location: f.location.clone(),
                            filename: f.filename.clone(),
                            size: u64::try_from(f.size).unwrap_or(0),
                            duration: nanos_to_seconds(f.duration),
                            media_type: "video/mp4".to_owned(),
                        }),
                        None => EgressOutcome::RecordingFailed,
                    }
                }
                EgressStatus::Failed | EgressStatus::Aborted => EgressOutcome::RecordingFailed,
                _ => EgressOutcome::Nothing,
            },
        })
    }

    /// Apply `info` (from an `egress_*` webhook's `egressInfo`, or from
    /// `ListEgress`) to `job`'s object through `reporter`, as
    /// [`Self::egress_outcome`] says:
    ///
    /// - live → [`MediaReporter::livestream_live`];
    /// - failed → [`MediaReporter::livestream_failed`];
    /// - ended → [`MediaReporter::livestream_stopped`] (after a
    ///   moderator's stop the livestream is already `"stopped"`, so this
    ///   is [`ReportError::WrongState`]);
    /// - uploaded → [`MediaReporter::recording_stopped`] if the recording
    ///   is still running (the room emptied), then
    ///   [`MediaReporter::recording_processing`] with the duration, and
    ///   [`Reported::RecordingProcessing`] with the file to import;
    /// - recording failed → `recording_stopped` if still running, then
    ///   [`MediaReporter::recording_failed`].
    ///
    /// A duplicate or late webhook finds the object past the transition
    /// and returns [`ReportError::WrongState`], so a file is handed out
    /// for import once.
    ///
    /// # Errors
    ///
    /// [`EgressReportError::OtherEgress`] when `info` names another
    /// egress, or the report's error. A hosted livestream whose endpoints
    /// cannot be built is reported failed with the error's
    /// [`LiveKitError::failure_reason`].
    pub async fn report_egress<B: VtcBackend>(
        &self,
        reporter: &MediaReporter<'_, B>,
        job: &EgressJob,
        info: &EgressInfo,
    ) -> EgressReport<B::Error> {
        if info.egress_id != job.egress_id {
            return Err(EgressReportError::OtherEgress);
        }
        let outcome = self
            .egress_outcome(job, info)
            .unwrap_or_else(|e| EgressOutcome::LivestreamFailed(e.failure_reason()));
        let live = |r: Result<(), ReportError<B::Error, LivestreamState>>| {
            r.map(|()| Reported::Livestream)
                .map_err(EgressReportError::Livestream)
        };
        match (&job.object, outcome) {
            (_, EgressOutcome::Nothing) => Ok(Reported::Nothing),
            (JobObject::Livestream { id, .. }, EgressOutcome::LivestreamLive(endpoints)) => {
                live(reporter.livestream_live(id, endpoints).await)
            }
            (JobObject::Livestream { id, .. }, EgressOutcome::LivestreamFailed(reason)) => {
                live(reporter.livestream_failed(id, reason).await)
            }
            (JobObject::Livestream { id, .. }, EgressOutcome::LivestreamEnded) => {
                live(reporter.livestream_stopped(id).await)
            }
            (JobObject::Recording { id }, EgressOutcome::RecordingUploaded(file)) => {
                stop_if_running(reporter, id).await?;
                reporter
                    .recording_processing(id, Some(file.duration))
                    .await
                    .map_err(EgressReportError::Recording)?;
                Ok(Reported::RecordingProcessing(file))
            }
            (JobObject::Recording { id }, EgressOutcome::RecordingFailed) => {
                stop_if_running(reporter, id).await?;
                reporter
                    .recording_failed(id)
                    .await
                    .map_err(EgressReportError::Recording)?;
                Ok(Reported::RecordingFailed)
            }
            _ => Ok(Reported::Nothing),
        }
    }
}

/// `"recording"`/`"paused"` → `"stopped"` for an egress that ended by
/// itself; a recording already stopped (or further) is fine.
async fn stop_if_running<B: VtcBackend>(
    reporter: &MediaReporter<'_, B>,
    id: &Id,
) -> Result<(), EgressReportError<B::Error>> {
    match reporter.recording_stopped(id).await {
        Ok(()) | Err(ReportError::WrongState(_)) => Ok(()),
        Err(e) => Err(EgressReportError::Recording(e)),
    }
}

/// Nanoseconds to whole seconds, rounded to nearest; `0` for a negative
/// value.
fn nanos_to_seconds(nanos: i64) -> u64 {
    u64::try_from(nanos).map_or(0, |n| n.saturating_add(500_000_000) / 1_000_000_000)
}

/// An RFC 8620 §1.2 `Id` (1-255 octets of `A-Za-z0-9-_`), safe in an
/// object key and a URL path segment.
fn check_id(id: &Id) -> Result<(), LiveKitError> {
    let s: &str = id.as_ref();
    if !s.is_empty()
        && s.len() <= 255
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Ok(())
    } else {
        Err(LiveKitError::UnsafeId(id.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_urls() {
        let (p, u) = LiveKitMedia::push_url("rtmp://live.example.com/app", Some("sk_1")).unwrap();
        assert_eq!(p, StreamProtocol::Rtmp);
        assert_eq!(u, "rtmp://live.example.com/app/sk_1");
        let (_, u) =
            LiveKitMedia::push_url("rtmps://live.example.com:443/app/", Some("sk_1")).unwrap();
        assert_eq!(u, "rtmps://live.example.com:443/app/sk_1");
        let (p, u) = LiveKitMedia::push_url("srt://srt.example.com:9000", Some("sk 1")).unwrap();
        assert_eq!(p, StreamProtocol::Srt);
        assert_eq!(u, "srt://srt.example.com:9000?streamid=sk+1");
        let (_, u) =
            LiveKitMedia::push_url("srt://srt.example.com:9000?streamid=given", Some("sk"))
                .unwrap();
        assert_eq!(u, "srt://srt.example.com:9000?streamid=given");
        assert!(matches!(
            LiveKitMedia::push_url("https://whip.example.com/whip", Some("t")),
            Err(LiveKitError::UnsupportedScheme(s)) if s == "https"
        ));
        assert!(matches!(
            LiveKitMedia::push_url("not a uri", None),
            Err(LiveKitError::InvalidStreamUri)
        ));
    }

    #[test]
    fn rounding_and_ids() {
        assert_eq!(nanos_to_seconds(4_499_999_999), 4);
        assert_eq!(nanos_to_seconds(4_500_000_000), 5);
        assert_eq!(nanos_to_seconds(-1), 0);
        assert!(check_id(&Id::from("01J4XQ-_a")).is_ok());
        assert!(check_id(&Id::from("../x")).is_err());
        assert!(check_id(&Id::from("")).is_err());
    }
}
