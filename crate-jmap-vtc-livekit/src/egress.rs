//! The LiveKit Egress service over its Twirp JSON API.
//!
//! Oracles: the Egress API reference,
//! <https://docs.livekit.io/reference/other/egress/api/> ("POST ... to
//! `https://<your-livekit-host>/twirp/livekit.Egress/<MethodName>`",
//! `Authorization: Bearer`, `Content-Type: application/json`, the
//! `roomRecord` permission, the request and info fields), the outputs page,
//! <https://docs.livekit.io/transport/media/ingress-egress/egress/outputs/>,
//! and `protobufs/livekit_egress.proto` in livekit/protocol (field names,
//! enum names, `EgressStatus` numbers).
//!
//! Egresses start with the unified `StartEgress` RPC
//! ([`StartEgressRequest`] with the `template` source), not the
//! `StartRoomCompositeEgress` RPC the proto marks "TODO: deprecate"
//! (decision 64); `UpdateLayout`, `StopEgress` and `ListEgress` are not
//! deprecated. LiveKit servers before v1.13.2 and egress workers before
//! v1.14.0 refuse `StartEgress`.
//!
//! JSON shape: requests use the proto field names (`room_name`), which the
//! server's protojson decoder accepts along with the lowerCamelCase JSON
//! names (and it discards unknown fields: `DiscardUnknown: true`). A oneof
//! is its member's field (`"template": {...}`, `"preset": "..."`,
//! `"file": {...}`). LiveKit's Twirp server answers with the proto names, every field
//! present, `int64` values as JSON strings and enums by name
//! (`livekit_egress.twirp.go`: `UseProtoNames: !jsonCamelCase,
//! EmitUnpopulated: !jsonSkipDefaults`, neither option set by the server).
//! Webhook bodies use protojson's defaults instead: lowerCamelCase names
//! and no default-valued fields. The response types here read both.

use std::fmt;
use std::time::{Duration, SystemTime};

use jmap_types::impl_string_enum;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use url::Url;

use crate::default_client;
use crate::error::{LiveKitError, TwirpCode};
use crate::token::{ApiKey, VideoGrant};

/// The lifetime of the token signed for each Egress API call.
pub const API_TOKEN_TTL: Duration = Duration::from_secs(600);

// ---------------------------------------------------------------- enums

/// `EncodedFileType`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EncodedFileType {
    /// `DEFAULT_FILETYPE`: "file type chosen based on codecs".
    Default,
    /// `MP4`.
    Mp4,
    /// `OGG`.
    Ogg,
    /// `MP3`.
    Mp3,
    /// A value this version does not list.
    Other(String),
}

impl_string_enum!(EncodedFileType, "a LiveKit EncodedFileType",
    "DEFAULT_FILETYPE" => Default,
    "MP4" => Mp4,
    "OGG" => Ogg,
    "MP3" => Mp3,
);

/// `StreamProtocol`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamProtocol {
    /// `DEFAULT_PROTOCOL`: "protocol chosen based on urls".
    Default,
    /// `RTMP` (`rtmp://`, `rtmps://`).
    Rtmp,
    /// `SRT` (`srt://`).
    Srt,
    /// A value this version does not list.
    Other(String),
}

impl_string_enum!(StreamProtocol, "a LiveKit StreamProtocol",
    "DEFAULT_PROTOCOL" => Default,
    "RTMP" => Rtmp,
    "SRT" => Srt,
);

/// `SegmentedFileProtocol`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SegmentedFileProtocol {
    /// `DEFAULT_SEGMENTED_FILE_PROTOCOL`.
    Default,
    /// `HLS_PROTOCOL`: "MPEG-TS segments".
    Hls,
    /// `HLS_FMP4_PROTOCOL`: "fragmented MP4 segments".
    HlsFmp4,
    /// A value this version does not list.
    Other(String),
}

impl_string_enum!(SegmentedFileProtocol, "a LiveKit SegmentedFileProtocol",
    "DEFAULT_SEGMENTED_FILE_PROTOCOL" => Default,
    "HLS_PROTOCOL" => Hls,
    "HLS_FMP4_PROTOCOL" => HlsFmp4,
);

/// `EncodingOptionsPreset` (LiveKit's default is `H264_720P_30`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EncodingPreset {
    /// `H264_720P_30`: 1280x720, 30 fps, 3000 kbps.
    H264_720p30,
    /// `H264_720P_60`: 1280x720, 60 fps, 4500 kbps.
    H264_720p60,
    /// `H264_1080P_30`: 1920x1080, 30 fps, 4500 kbps.
    H264_1080p30,
    /// `H264_1080P_60`: 1920x1080, 60 fps, 6000 kbps.
    H264_1080p60,
    /// A value this version does not list.
    Other(String),
}

impl_string_enum!(EncodingPreset, "a LiveKit EncodingOptionsPreset",
    "H264_720P_30" => H264_720p30,
    "H264_720P_60" => H264_720p60,
    "H264_1080P_30" => H264_1080p30,
    "H264_1080P_60" => H264_1080p60,
);

/// `EgressStatus`.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum EgressStatus {
    /// `EGRESS_STARTING` (0), the proto default: omitted from webhook
    /// bodies.
    #[default]
    Starting,
    /// `EGRESS_ACTIVE` (1).
    Active,
    /// `EGRESS_ENDING` (2).
    Ending,
    /// `EGRESS_COMPLETE` (3).
    Complete,
    /// `EGRESS_FAILED` (4).
    Failed,
    /// `EGRESS_ABORTED` (5).
    Aborted,
    /// `EGRESS_LIMIT_REACHED` (6).
    LimitReached,
    /// A value this version does not list.
    Other(String),
}

impl_string_enum!(EgressStatus, "a LiveKit EgressStatus",
    "EGRESS_STARTING" => Starting,
    "EGRESS_ACTIVE" => Active,
    "EGRESS_ENDING" => Ending,
    "EGRESS_COMPLETE" => Complete,
    "EGRESS_FAILED" => Failed,
    "EGRESS_ABORTED" => Aborted,
    "EGRESS_LIMIT_REACHED" => LimitReached,
);

/// `StreamInfo.Status`.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum StreamStatus {
    /// `ACTIVE` (0).
    #[default]
    Active,
    /// `FINISHED` (1).
    Finished,
    /// `FAILED` (2).
    Failed,
    /// A value this version does not list.
    Other(String),
}

impl_string_enum!(StreamStatus, "a LiveKit StreamInfo status",
    "ACTIVE" => Active,
    "FINISHED" => Finished,
    "FAILED" => Failed,
);

// ------------------------------------------------------------- requests

/// `S3Upload`: an S3 or S3-compatible bucket the egress uploads to.
///
/// The `Debug` impl never shows the secret or the session token.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct S3Upload {
    /// `access_key`.
    pub access_key: String,
    /// `secret`.
    pub secret: String,
    /// `session_token`, for temporary credentials.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    /// `region`: "required when `endpoint` is not set".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// `endpoint`: an S3-compatible service; "Must start with
    /// `https://`".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// `bucket`.
    pub bucket: String,
    /// `force_path_style`: "When using non-AWS storage, set
    /// `force_path_style` to `true`".
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub force_path_style: bool,
}

impl S3Upload {
    /// Bucket `bucket` with access key `access_key` and secret `secret`.
    pub fn new(
        access_key: impl Into<String>,
        secret: impl Into<String>,
        bucket: impl Into<String>,
    ) -> Self {
        Self {
            access_key: access_key.into(),
            secret: secret.into(),
            session_token: None,
            region: None,
            endpoint: None,
            bucket: bucket.into(),
            force_path_style: false,
        }
    }

    /// This upload in AWS region `region`.
    pub fn with_region(self, region: impl Into<String>) -> Self {
        Self {
            region: Some(region.into()),
            ..self
        }
    }

    /// This upload to the S3-compatible `endpoint`, path-style.
    pub fn with_endpoint(self, endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: Some(endpoint.into()),
            force_path_style: true,
            ..self
        }
    }

    /// This upload with temporary credentials' session token.
    pub fn with_session_token(self, token: impl Into<String>) -> Self {
        Self {
            session_token: Some(token.into()),
            ..self
        }
    }
}

impl fmt::Debug for S3Upload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Upload")
            .field("access_key", &self.access_key)
            .field("secret", &"<redacted>")
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "<redacted>"),
            )
            .field("region", &self.region)
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("force_path_style", &self.force_path_style)
            .finish()
    }
}

/// `FileOutput`: one file. `filepath` "Supports templating"
/// (`{room_name}`, `{room_id}`, `{time}`); the default is
/// `{room_name}-{time}`. Where it is uploaded is the [`Output`]'s
/// storage.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileOutput {
    /// `file_type`.
    pub file_type: EncodedFileType,
    /// `filepath`: the object key (or path) of the file.
    pub filepath: String,
}

impl FileOutput {
    /// An MP4 file at `filepath`.
    pub fn mp4(filepath: impl Into<String>) -> Self {
        Self {
            file_type: EncodedFileType::Mp4,
            filepath: filepath.into(),
        }
    }
}

/// `SegmentedFileOutput`: HLS segments and playlists. Where they are
/// uploaded is the [`Output`]'s storage (the message's own `s3` is marked
/// "TODO: deprecate" and never sent).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SegmentOutput {
    /// `protocol`.
    pub protocol: SegmentedFileProtocol,
    /// `filename_prefix`: the object key prefix of the segments.
    pub filename_prefix: String,
    /// `playlist_name`: the full playlist.
    pub playlist_name: String,
    /// `live_playlist_name`: "Path of a live playlist", one containing
    /// only the last few segments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_playlist_name: Option<String>,
    /// `segment_duration` in seconds (LiveKit's default when `None`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment_duration: Option<u32>,
}

impl SegmentOutput {
    /// HLS (MPEG-TS segments) under `filename_prefix`, with playlist
    /// `playlist_name`.
    pub fn hls(filename_prefix: impl Into<String>, playlist_name: impl Into<String>) -> Self {
        Self {
            protocol: SegmentedFileProtocol::Hls,
            filename_prefix: filename_prefix.into(),
            playlist_name: playlist_name.into(),
            live_playlist_name: None,
            segment_duration: None,
        }
    }

    /// This output with a live playlist named `name`.
    pub fn with_live_playlist(self, name: impl Into<String>) -> Self {
        Self {
            live_playlist_name: Some(name.into()),
            ..self
        }
    }

    /// This output with `seconds`-long segments.
    pub fn with_segment_duration(self, seconds: u32) -> Self {
        Self {
            segment_duration: Some(seconds),
            ..self
        }
    }
}

/// `StreamOutput`: RTMP or SRT destinations.
///
/// The URLs carry stream keys, so the `Debug` impl shows only their
/// count.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct StreamOutput {
    /// `protocol`.
    pub protocol: StreamProtocol,
    /// `urls`.
    pub urls: Vec<String>,
}

impl StreamOutput {
    /// `protocol` to `urls`.
    pub fn new(protocol: StreamProtocol, urls: Vec<String>) -> Self {
        Self { protocol, urls }
    }
}

impl fmt::Debug for StreamOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamOutput")
            .field("protocol", &self.protocol)
            .field("urls", &format_args!("[{} redacted]", self.urls.len()))
            .finish()
    }
}

/// The `config` oneof of an [`Output`]: what it writes. Serialized as the
/// oneof member's field (`file`, `stream`, `segments`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputConfig {
    /// `file`.
    File(FileOutput),
    /// `stream`.
    Stream(StreamOutput),
    /// `segments`.
    Segments(SegmentOutput),
}

impl OutputConfig {
    fn same_kind(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// `Output`: one output of a [`StartEgressRequest`] with its own storage.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Output {
    /// The `config` oneof.
    #[serde(flatten)]
    pub config: OutputConfig,
    /// `storage`: "Per-output storage override (falls back to request,
    /// then server)"; sent as `{"s3": ...}`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "storage_config"
    )]
    pub storage: Option<S3Upload>,
}

impl Output {
    /// An output writing `config`, to the request's or the server's
    /// storage.
    pub fn new(config: OutputConfig) -> Self {
        Self {
            config,
            storage: None,
        }
    }

    /// This output uploaded to `s3`.
    pub fn with_s3(self, s3: S3Upload) -> Self {
        Self {
            storage: Some(s3),
            ..self
        }
    }
}

impl From<FileOutput> for Output {
    fn from(file: FileOutput) -> Self {
        Self::new(OutputConfig::File(file))
    }
}

impl From<StreamOutput> for Output {
    fn from(stream: StreamOutput) -> Self {
        Self::new(OutputConfig::Stream(stream))
    }
}

impl From<SegmentOutput> for Output {
    fn from(segments: SegmentOutput) -> Self {
        Self::new(OutputConfig::Segments(segments))
    }
}

/// A `StorageConfig` holding its `s3` provider.
fn storage_config<S: serde::Serializer>(s3: &Option<S3Upload>, s: S) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct StorageConfig<'a> {
        s3: &'a S3Upload,
    }
    match s3 {
        Some(s3) => StorageConfig { s3 }.serialize(s),
        None => s.serialize_none(),
    }
}

/// `TemplateSource`: "Room composite recording via layout template".
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TemplateSource {
    /// `layout`, passed to the template; the default template knows
    /// `grid`, `speaker` and `single-speaker`, each with a `-light`
    /// variant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    /// `audio_only`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub audio_only: bool,
    /// `custom_base_url`: a custom template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_base_url: Option<String>,
}

/// `StartEgressRequest` with the `template` source: a room rendered by
/// the egress's web template in `layout` (a room-composite egress), to at
/// most one output of each kind (the egress refuses "multiple file
/// outputs", and likewise for stream and segmented outputs).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartEgressRequest {
    /// `room_name`.
    pub room_name: String,
    /// `template`, the `source` oneof member.
    pub template: TemplateSource,
    /// `preset`, the `encoding` oneof member ("Optional — default
    /// H264_720P_30").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<EncodingPreset>,
    /// `outputs`: "At least one required".
    pub outputs: Vec<Output>,
    /// `storage`: "Request-level storage default", sent as `{"s3": ...}`.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "storage_config"
    )]
    pub storage: Option<S3Upload>,
}

impl StartEgressRequest {
    /// A room composite of `room_name` with no output yet.
    pub fn room_composite(room_name: impl Into<String>) -> Self {
        Self {
            room_name: room_name.into(),
            template: TemplateSource::default(),
            preset: None,
            outputs: Vec::new(),
            storage: None,
        }
    }

    /// This request in `layout`.
    pub fn with_layout(mut self, layout: impl Into<String>) -> Self {
        self.template.layout = Some(layout.into());
        self
    }

    /// This request with encoding `preset`.
    pub fn with_preset(self, preset: EncodingPreset) -> Self {
        Self {
            preset: Some(preset),
            ..self
        }
    }

    /// This request rendering template `url`.
    pub fn with_custom_base_url(mut self, url: impl Into<String>) -> Self {
        self.template.custom_base_url = Some(url.into());
        self
    }

    /// This request uploading to `s3` unless an output says otherwise.
    pub fn with_storage(self, s3: S3Upload) -> Self {
        Self {
            storage: Some(s3),
            ..self
        }
    }

    /// This request with `output`, in place of any output of the same
    /// kind.
    pub fn with_output(mut self, output: impl Into<Output>) -> Self {
        let output = output.into();
        self.outputs.retain(|o| !o.config.same_kind(&output.config));
        self.outputs.push(output);
        self
    }
}

// ------------------------------------------------------------ responses

/// `EgressInfo`: the state of one egress, from the API or a webhook.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct EgressInfo {
    /// `egress_id`.
    #[serde(alias = "egressId")]
    pub egress_id: String,
    /// `room_id`.
    #[serde(alias = "roomId")]
    pub room_id: String,
    /// `room_name`.
    #[serde(alias = "roomName")]
    pub room_name: String,
    /// `status`.
    #[serde(deserialize_with = "status")]
    pub status: EgressStatus,
    /// `started_at`, Unix nanoseconds; `0` until the egress started.
    #[serde(alias = "startedAt", deserialize_with = "int64")]
    pub started_at: i64,
    /// `ended_at`, Unix nanoseconds; `0` until it ended.
    #[serde(alias = "endedAt", deserialize_with = "int64")]
    pub ended_at: i64,
    /// `updated_at`, Unix nanoseconds.
    #[serde(alias = "updatedAt", deserialize_with = "int64")]
    pub updated_at: i64,
    /// `error`: why it failed, if it did.
    pub error: String,
    /// `error_code`.
    #[serde(alias = "errorCode", deserialize_with = "int64")]
    pub error_code: i64,
    /// `details`.
    pub details: String,
    /// `stream_results`.
    #[serde(alias = "streamResults", deserialize_with = "list")]
    pub stream_results: Vec<StreamInfo>,
    /// `file_results`.
    #[serde(alias = "fileResults", deserialize_with = "list")]
    pub file_results: Vec<FileInfo>,
    /// `segment_results`.
    #[serde(alias = "segmentResults", deserialize_with = "list")]
    pub segment_results: Vec<SegmentsInfo>,
}

/// `StreamInfo`: one stream destination of an egress.
///
/// `url` is redacted by LiveKit in API answers (`RedactStreamKey`).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct StreamInfo {
    /// `url`.
    pub url: String,
    /// `started_at`, Unix nanoseconds.
    #[serde(alias = "startedAt", deserialize_with = "int64")]
    pub started_at: i64,
    /// `ended_at`, Unix nanoseconds.
    #[serde(alias = "endedAt", deserialize_with = "int64")]
    pub ended_at: i64,
    /// `duration`, nanoseconds.
    #[serde(deserialize_with = "int64")]
    pub duration: i64,
    /// `status`.
    #[serde(deserialize_with = "stream_status")]
    pub status: StreamStatus,
    /// `error`.
    pub error: String,
}

/// `FileInfo`: the file an egress wrote.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct FileInfo {
    /// `filename`.
    pub filename: String,
    /// `started_at`, Unix nanoseconds.
    #[serde(alias = "startedAt", deserialize_with = "int64")]
    pub started_at: i64,
    /// `ended_at`, Unix nanoseconds.
    #[serde(alias = "endedAt", deserialize_with = "int64")]
    pub ended_at: i64,
    /// `duration`, nanoseconds ("duration (nanoseconds)").
    #[serde(deserialize_with = "int64")]
    pub duration: i64,
    /// `size`, bytes.
    #[serde(deserialize_with = "int64")]
    pub size: i64,
    /// `location`: where it was uploaded, "e.g., S3 URL".
    pub location: String,
}

/// `SegmentsInfo`: the HLS output of an egress.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SegmentsInfo {
    /// `playlist_name`.
    #[serde(alias = "playlistName")]
    pub playlist_name: String,
    /// `live_playlist_name`.
    #[serde(alias = "livePlaylistName")]
    pub live_playlist_name: String,
    /// `duration`, nanoseconds.
    #[serde(deserialize_with = "int64")]
    pub duration: i64,
    /// `size`, bytes.
    #[serde(deserialize_with = "int64")]
    pub size: i64,
    /// `playlist_location`.
    #[serde(alias = "playlistLocation")]
    pub playlist_location: String,
    /// `live_playlist_location`.
    #[serde(alias = "livePlaylistLocation")]
    pub live_playlist_location: String,
    /// `segment_count`.
    #[serde(alias = "segmentCount", deserialize_with = "int64")]
    pub segment_count: i64,
}

/// A protojson `int64`: a JSON string of digits or a number; `null` is 0.
pub(crate) fn int64<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(0),
        Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| serde::de::Error::custom("int64 out of range")),
        Value::String(s) => s.parse().map_err(serde::de::Error::custom),
        other => Err(serde::de::Error::custom(format!("not an int64: {other}"))),
    }
}

/// A repeated field; `null` is empty.
fn list<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// An enum by name or, as protojson also accepts, by number.
fn named_enum<'de, D, T>(d: D, names: &[&str]) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: From<String> + Default,
{
    match Value::deserialize(d)? {
        Value::Null => Ok(T::default()),
        Value::String(s) => Ok(T::from(s)),
        Value::Number(n) => Ok(n
            .as_u64()
            .and_then(|i| names.get(usize::try_from(i).ok()?))
            .map_or_else(|| T::from(n.to_string()), |s| T::from((*s).to_owned()))),
        other => Err(serde::de::Error::custom(format!("not an enum: {other}"))),
    }
}

fn status<'de, D: Deserializer<'de>>(d: D) -> Result<EgressStatus, D::Error> {
    named_enum(
        d,
        &[
            "EGRESS_STARTING",
            "EGRESS_ACTIVE",
            "EGRESS_ENDING",
            "EGRESS_COMPLETE",
            "EGRESS_FAILED",
            "EGRESS_ABORTED",
            "EGRESS_LIMIT_REACHED",
        ],
    )
}

fn stream_status<'de, D: Deserializer<'de>>(d: D) -> Result<StreamStatus, D::Error> {
    named_enum(d, &["ACTIVE", "FINISHED", "FAILED"])
}

// --------------------------------------------------------------- client

/// The Egress service of one LiveKit server.
///
/// Each call is `POST {url}/twirp/livekit.Egress/{Method}` with a JSON
/// body and a fresh token from the API key carrying the `roomRecord`
/// grant, valid for [`API_TOKEN_TTL`].
#[derive(Debug, Clone)]
pub struct EgressClient {
    http: reqwest::Client,
    api_key: ApiKey,
    base: Url,
}

impl EgressClient {
    /// A client for the LiveKit server at `url` (`https://`, or the
    /// `wss://` URL clients connect to, which is mapped to `https://`;
    /// `http://` and `ws://` likewise), following no redirects and timing
    /// out after [`REQUEST_TIMEOUT`](crate::REQUEST_TIMEOUT).
    ///
    /// # Errors
    ///
    /// [`LiveKitError::Config`] for a URL of another scheme or one that
    /// cannot be a base, [`LiveKitError::Transport`] if the HTTP client
    /// cannot be built.
    pub fn new(url: &Url, api_key: ApiKey) -> Result<Self, LiveKitError> {
        Self::with_http(default_client()?, url, api_key)
    }

    /// As [`Self::new`], over `http`.
    ///
    /// # Errors
    ///
    /// [`LiveKitError::Config`] as for [`Self::new`].
    pub fn with_http(
        http: reqwest::Client,
        url: &Url,
        api_key: ApiKey,
    ) -> Result<Self, LiveKitError> {
        let scheme = match url.scheme() {
            "https" | "wss" => "https",
            "http" | "ws" => "http",
            other => {
                return Err(LiveKitError::Config(format!(
                    "LiveKit URL scheme {other:?} is not https, wss, http or ws"
                )));
            }
        };
        let mut base: Url = format!("{scheme}://{}", &url[url::Position::BeforeHost..])
            .parse()
            .map_err(|e| LiveKitError::Config(format!("LiveKit URL: {e}")))?;
        if base.cannot_be_a_base() || base.host_str().is_none_or(str::is_empty) {
            return Err(LiveKitError::Config("LiveKit URL has no host".to_owned()));
        }
        base.set_query(None);
        base.set_fragment(None);
        if !base.path().ends_with('/') {
            let path = format!("{}/", base.path());
            base.set_path(&path);
        }
        Ok(Self {
            http,
            api_key,
            base,
        })
    }

    /// The HTTP base URL calls go to.
    pub fn base_url(&self) -> &Url {
        &self.base
    }

    /// The API key.
    pub fn api_key(&self) -> &ApiKey {
        &self.api_key
    }

    /// `StartEgress`, the unified start RPC, in place of
    /// `StartRoomCompositeEgress` (marked "TODO: deprecate").
    ///
    /// LiveKit servers before v1.13.2 refuse it: through v1.12.0 the Twirp
    /// router has no such method (`bad_route`, 404), and v1.13.1 answers
    /// "not implemented". Egress workers before v1.14.0 reject the request
    /// too.
    ///
    /// # Errors
    ///
    /// The call's [`LiveKitError`].
    pub async fn start_egress(
        &self,
        request: &StartEgressRequest,
    ) -> Result<EgressInfo, LiveKitError> {
        self.call("StartEgress", request).await
    }

    /// `UpdateLayout`: "Used to change the web layout on an active
    /// RoomCompositeEgress."
    ///
    /// # Errors
    ///
    /// The call's [`LiveKitError`].
    pub async fn update_layout(
        &self,
        egress_id: &str,
        layout: &str,
    ) -> Result<EgressInfo, LiveKitError> {
        self.call(
            "UpdateLayout",
            &json!({"egress_id": egress_id, "layout": layout}),
        )
        .await
    }

    /// `StopEgress`. LiveKit answers `failed_precondition` for an egress
    /// that already ended ("egress with status ... cannot be stopped").
    ///
    /// # Errors
    ///
    /// The call's [`LiveKitError`].
    pub async fn stop_egress(&self, egress_id: &str) -> Result<EgressInfo, LiveKitError> {
        self.call("StopEgress", &json!({"egress_id": egress_id}))
            .await
    }

    /// `ListEgress`, filtered by `room_name` and `egress_id` when given,
    /// and to active egresses when `active`. Only the first page.
    ///
    /// # Errors
    ///
    /// The call's [`LiveKitError`].
    pub async fn list_egress(
        &self,
        room_name: Option<&str>,
        egress_id: Option<&str>,
        active: bool,
    ) -> Result<Vec<EgressInfo>, LiveKitError> {
        #[derive(Deserialize)]
        struct List {
            #[serde(default, deserialize_with = "list")]
            items: Vec<EgressInfo>,
        }
        let mut body = serde_json::Map::new();
        if let Some(room) = room_name {
            body.insert("room_name".to_owned(), room.into());
        }
        if let Some(id) = egress_id {
            body.insert("egress_id".to_owned(), id.into());
        }
        if active {
            body.insert("active".to_owned(), true.into());
        }
        let list: List = self.call("ListEgress", &Value::Object(body)).await?;
        Ok(list.items)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        body: &impl Serialize,
    ) -> Result<T, LiveKitError> {
        let url = self
            .base
            .join(&format!("twirp/livekit.Egress/{method}"))
            .map_err(|e| LiveKitError::Config(format!("LiveKit URL: {e}")))?;
        let token = self.api_key.sign(
            &self
                .api_key
                .claims(SystemTime::now(), API_TOKEN_TTL)
                .with_video(VideoGrant::room_record()),
        );
        let response = self
            .http
            .post(url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if status.is_success() {
            return serde_json::from_slice(&bytes)
                .map_err(|e| LiveKitError::Response(format!("{method}: {e}")));
        }
        // Twirp error body: {"code": "...", "msg": "...", "meta": {...}}.
        let err: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let field = |k: &str| err.get(k).and_then(Value::as_str).map(str::to_owned);
        Err(LiveKitError::Twirp {
            status: status.as_u16(),
            code: TwirpCode::from(field("code").unwrap_or_default()),
            msg: field("msg"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_reads_twirp_and_webhook_shapes() {
        // Twirp: proto names, int64 as strings, everything present.
        let twirp: EgressInfo = serde_json::from_value(json!({
            "egress_id": "EG_1", "room_id": "RM_1", "room_name": "r",
            "status": "EGRESS_ACTIVE", "started_at": "1700000000000000000",
            "ended_at": "0", "updated_at": "0", "error": "", "error_code": 0,
            "details": "", "stream_results": [], "file_results": [],
            "segment_results": [], "image_results": [],
            "egress": {"room_name": "r", "template": {"layout": "grid"}}
        }))
        .unwrap();
        assert_eq!(twirp.status, EgressStatus::Active);
        assert_eq!(twirp.started_at, 1_700_000_000_000_000_000);
        // Webhook: JSON names, defaults omitted (status STARTING is 0).
        let hook: EgressInfo = serde_json::from_value(json!({
            "egressId": "EG_1", "roomName": "r",
            "fileResults": [{"filename": "a.mp4", "duration": "5000000000", "size": "42"}]
        }))
        .unwrap();
        assert_eq!(hook.status, EgressStatus::Starting);
        assert_eq!(hook.file_results[0].duration, 5_000_000_000);
        // Enums by number.
        let n: EgressInfo = serde_json::from_value(json!({"status": 3})).unwrap();
        assert_eq!(n.status, EgressStatus::Complete);
    }

    #[test]
    fn debug_redacts_secrets_and_stream_urls() {
        let s3 = S3Upload::new("AKID", "canary-s3-secret", "b").with_session_token("canary-tok");
        let shown = format!("{s3:?}");
        assert!(!shown.contains("canary"), "{shown}");
        let out = StreamOutput::new(
            StreamProtocol::Rtmp,
            vec!["rtmp://x/app/canary-key".to_owned()],
        );
        assert!(!format!("{out:?}").contains("canary"));
    }

    #[test]
    fn base_url_mapping() {
        let k = ApiKey::new("k", "s");
        let c = EgressClient::with_http(
            reqwest::Client::new(),
            &Url::parse("wss://lk.example.com").unwrap(),
            k.clone(),
        )
        .unwrap();
        assert_eq!(c.base_url().as_str(), "https://lk.example.com/");
        let c = EgressClient::with_http(
            reqwest::Client::new(),
            &Url::parse("http://127.0.0.1:7880/lk?x=1").unwrap(),
            k.clone(),
        )
        .unwrap();
        assert_eq!(c.base_url().as_str(), "http://127.0.0.1:7880/lk/");
        assert!(
            EgressClient::with_http(reqwest::Client::new(), &Url::parse("ftp://x").unwrap(), k)
                .is_err()
        );
    }
}
