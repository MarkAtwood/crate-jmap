//! draft-atwood-jmap-vtc-00 §VTCRecording and §VTCLivestream.

use jmap_types::{Id, UTCDate, impl_string_enum};
use serde::{Deserialize, Serialize};

use crate::call::REDACTED;

/// The `state` of a [`VtcRecording`] (draft-atwood-jmap-vtc-00
/// §VTCRecording), also carried by
/// [`VtcRecordingStateEvent`](crate::VtcRecordingStateEvent).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RecordingState {
    /// `"recording"`.
    Recording,
    /// `"paused"`.
    Paused,
    /// `"stopped"`.
    Stopped,
    /// `"processing"`: the file is being prepared.
    Processing,
    /// `"available"`: `blobId`, `size`, `duration` and `mediaType` are set.
    Available,
    /// `"failed"`.
    Failed,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(RecordingState, "a VTC recording state string",
    "recording" => Recording,
    "paused" => Paused,
    "stopped" => Stopped,
    "processing" => Processing,
    "available" => Available,
    "failed" => Failed,
);

/// The `state` of a [`VtcLivestream`] (draft-atwood-jmap-vtc-00
/// §VTCLivestream).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LivestreamState {
    /// `"starting"`.
    Starting,
    /// `"live"`.
    Live,
    /// `"stopped"`.
    Stopped,
    /// `"failed"`.
    Failed,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(LivestreamState, "a VTC livestream state string",
    "starting" => Starting,
    "live" => Live,
    "stopped" => Stopped,
    "failed" => Failed,
);

/// The `layout` of a [`VtcRecording`] or [`VtcLivestream`], and an entry
/// of [`VtcCapability::layouts`](crate::VtcCapability::layouts)
/// (draft-atwood-jmap-vtc-00 §Composition Layouts): how the server
/// arranges the participants' video in the one composed picture.
///
/// "Other layouts SHOULD use reverse-domain notation (e.g.,
/// `\"com.example.stage\"`)"; they land in `Other` and round-trip. "A
/// client treats a `layout` value it does not recognize as the server's
/// default."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CompositionLayout {
    /// `"grid"`: every participant with video in equal tiles.
    Grid,
    /// `"speaker"`: the active speaker large, the other participants
    /// smaller.
    Speaker,
    /// `"single-speaker"`: only the active speaker.
    SingleSpeaker,
    /// A reverse-domain or other layout the draft does not define.
    Other(String),
}

impl_string_enum!(CompositionLayout, "a VTC composition layout string",
    "grid" => Grid,
    "speaker" => Speaker,
    "single-speaker" => SingleSpeaker,
);

/// Metadata of a recording session within a call
/// (draft-atwood-jmap-vtc-00 §VTCRecording).
///
/// `T|null` properties are `Option<T>` and serialize `None` as `null`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcRecording {
    /// Opaque server-assigned identifier.
    pub id: Id,
    /// The call this recording belongs to.
    pub call_id: Id,
    /// Current state.
    pub state: RecordingState,
    /// Time recording started.
    pub started_at: UTCDate,
    /// Time recording stopped; `None` while recording or paused.
    pub stopped_at: Option<UTCDate>,
    /// The userId of the participant who started the recording.
    pub initiated_by: String,
    /// Blob of the recording file, once `"available"`.
    pub blob_id: Option<Id>,
    /// File size in bytes, once `"available"`.
    pub size: Option<u64>,
    /// Duration in seconds, once `"available"` or `"stopped"`.
    pub duration: Option<u64>,
    /// MIME type of the file, e.g. `"video/webm"`, once `"available"`.
    pub media_type: Option<String>,
    /// How the participants' video is arranged in the recording
    /// (§Composition Layouts); `None` (`null`, or absent) means the
    /// server's default.
    #[serde(default)]
    pub layout: Option<CompositionLayout>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The `mode` of a [`VtcLivestream`] (draft-atwood-jmap-vtc-00
/// §VTCLivestream): push to a moderator-supplied ingest, or hosted by the
/// deployment. Default `"push"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum LivestreamMode {
    /// `"push"`: the deployment sends the call's media to `streamUri`.
    #[default]
    Push,
    /// `"hosted"`: the deployment serves the audience through
    /// `playbackEndpoints`.
    Hosted,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(LivestreamMode, "a VTC livestream mode string",
    "push" => Push,
    "hosted" => Hosted,
);

/// The `failureReason` of a failed [`VtcLivestream`]
/// (draft-atwood-jmap-vtc-00 §VTCLivestream). "Clients treat an
/// unrecognized value as an unspecified failure."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LivestreamFailureReason {
    /// `"timeout"`: the server stopped waiting for the livestream to go live.
    Timeout,
    /// `"rejected"`: the ingest destination or the media pipeline refused
    /// the stream.
    Rejected,
    /// `"unavailable"`: the media pipeline could not be provisioned.
    Unavailable,
    /// `"interrupted"`: the stream was live and was lost.
    Interrupted,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(LivestreamFailureReason, "a VTC livestream failure reason string",
    "timeout" => Timeout,
    "rejected" => Rejected,
    "unavailable" => Unavailable,
    "interrupted" => Interrupted,
);

/// The `protocol` of a [`VtcPlaybackEndpoint`]. Other protocols use
/// reverse-domain notation; "Clients MUST ignore endpoints whose
/// `protocol` they do not recognize."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlaybackProtocol {
    /// `"hls"`: HTTP Live Streaming (RFC 8216).
    Hls,
    /// `"dash"`: MPEG-DASH.
    Dash,
    /// `"whep"`: WebRTC-HTTP Egress Protocol.
    Whep,
    /// `"moq"`: Media over QUIC.
    Moq,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(PlaybackProtocol, "a VTC playback protocol string",
    "hls" => Hls,
    "dash" => Dash,
    "whep" => Whep,
    "moq" => Moq,
);

/// The `latencyHint` of a [`VtcPlaybackEndpoint`]. "Clients treat an
/// unrecognized value as `"standard"`."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LatencyHint {
    /// `"interactive"`: close enough to take part in conversation.
    Interactive,
    /// `"low"`: close enough for live audience reactions.
    Low,
    /// `"standard"`: delay traded for scale, reach or robustness.
    Standard,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(LatencyHint, "a VTC playback latency hint string",
    "interactive" => Interactive,
    "low" => Low,
    "standard" => Standard,
);

/// The `kind` of a [`VtcStreamTrack`]. "Clients MUST ignore tracks whose
/// `kind` they do not recognize."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamTrackKind {
    /// `"audio"`.
    Audio,
    /// `"video"`.
    Video,
    /// `"captions"`: a transcription of the audio, non-speech sound included.
    Captions,
    /// `"subtitles"`: a translation of the speech into another language.
    Subtitles,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(StreamTrackKind, "a VTC stream track kind string",
    "audio" => Audio,
    "video" => Video,
    "captions" => Captions,
    "subtitles" => Subtitles,
);

/// The `timeReference` of a [`VtcPlaybackEndpoint`]
/// (draft-atwood-jmap-vtc-00 §Stream Synchronization): maps the
/// endpoint's media timeline to wall-clock time on the server's clock.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcTimeReference {
    /// A position on the endpoint's media timeline, in milliseconds.
    pub media_time_ms: u64,
    /// When the call produced the media found at `media_time_ms`.
    pub wall_clock: UTCDate,
    /// Vendor / site / private extension fields, kept across round-trip.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl VtcTimeReference {
    /// A time reference from its two fields.
    pub fn new(media_time_ms: u64, wall_clock: UTCDate) -> Self {
        Self {
            media_time_ms,
            wall_clock,
            extra: serde_json::Map::new(),
        }
    }
}

/// One selectable track of a [`VtcPlaybackEndpoint`]
/// (draft-atwood-jmap-vtc-00 §VTCStreamTrack).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcStreamTrack {
    /// The track kind.
    pub kind: StreamTrackKind,
    /// RFC 5646 language tag, or `None` when not applicable.
    pub language: Option<String>,
    /// Human-readable label for a track picker.
    pub label: Option<String>,
    /// Media type of the track's content where it differs from the
    /// endpoint's main media (for example `"text/vtt"`).
    pub format: Option<String>,
    /// Vendor / site / private extension fields, kept across round-trip.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl VtcStreamTrack {
    /// A track of `kind` with no language, label or format.
    pub fn new(kind: StreamTrackKind) -> Self {
        Self {
            kind,
            language: None,
            label: None,
            format: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// A place from which a viewer's player can receive a hosted livestream
/// (draft-atwood-jmap-vtc-00 §VTCPlaybackEndpoint).
///
/// `url` "MAY carry authorization for the requesting user" and
/// `credential` is a playback credential, so the `Debug` impl redacts both.
/// The server computes them per requesting user (§Playback Authorization).
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcPlaybackEndpoint {
    /// The playback protocol.
    pub protocol: PlaybackProtocol,
    /// The URL the viewer's player opens.
    pub url: String,
    /// Opaque playback credential for the requesting user, or `None`.
    pub credential: Option<String>,
    /// When `url` and `credential` stop being valid, or `None`.
    pub expires_at: Option<UTCDate>,
    /// Coarse delay between the call and what viewers see.
    pub latency_hint: LatencyHint,
    /// Media timeline to wall-clock mapping (server-set), or `None`.
    pub time_reference: Option<VtcTimeReference>,
    /// Selectable tracks; empty when the endpoint offers no choice.
    #[serde(default)]
    pub tracks: Vec<VtcStreamTrack>,
    /// Whether the media is encrypted under a content protection system.
    /// Default `false`.
    #[serde(default)]
    pub encrypted: bool,
    /// Key system identifiers (EME) any one of which can decrypt the
    /// media; empty when `encrypted` is `false`.
    #[serde(default)]
    pub key_systems: Vec<String>,
    /// Vendor / site / private extension fields, kept across round-trip.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl VtcPlaybackEndpoint {
    /// An unencrypted endpoint with no credential, expiry, time reference
    /// or tracks.
    pub fn new(
        protocol: PlaybackProtocol,
        url: impl Into<String>,
        latency_hint: LatencyHint,
    ) -> Self {
        Self {
            protocol,
            url: url.into(),
            credential: None,
            expires_at: None,
            latency_hint,
            time_reference: None,
            tracks: Vec::new(),
            encrypted: false,
            key_systems: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }
}

impl std::fmt::Debug for VtcPlaybackEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VtcPlaybackEndpoint")
            .field("protocol", &self.protocol)
            .field("url", &REDACTED)
            .field("credential", &self.credential.as_ref().map(|_| REDACTED))
            .field("expires_at", &self.expires_at)
            .field("latency_hint", &self.latency_hint)
            .field("time_reference", &self.time_reference)
            .field("tracks", &self.tracks)
            .field("encrypted", &self.encrypted)
            .field("key_systems", &self.key_systems)
            .field("extra", &self.extra)
            .finish()
    }
}

/// Metadata of a one-to-many broadcast of a call
/// (draft-atwood-jmap-vtc-00 §VTCLivestream), in push or hosted mode.
///
/// `streamUri` is "Treated as sensitive" and `streamKey` is an ingest
/// credential, so the `Debug` impl redacts both (and the playback URLs and
/// credentials, see [`VtcPlaybackEndpoint`]). The server SHOULD return
/// `streamKey` as `""` to non-moderators; that is method behaviour, not
/// enforced here. In hosted mode `platform`, `streamUri` and `streamKey`
/// are always `None`.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcLivestream {
    /// Opaque server-assigned identifier.
    pub id: Id,
    /// The call this livestream belongs to.
    pub call_id: Id,
    /// Push or hosted; immutable. Absent on the wire means `"push"`.
    #[serde(default)]
    pub mode: LivestreamMode,
    /// Current state.
    pub state: LivestreamState,
    /// Why the livestream failed; `None` unless `state` is `"failed"`.
    #[serde(default)]
    pub failure_reason: Option<LivestreamFailureReason>,
    /// Push mode: deployment-defined platform, e.g. `"youtube"`. "Clients
    /// MUST ignore unrecognized values."
    #[serde(default)]
    pub platform: Option<String>,
    /// Push mode: the ingest endpoint, a URI whose scheme is listed in
    /// [`VtcCapability::livestream_push_schemes`](crate::VtcCapability::livestream_push_schemes).
    #[serde(default)]
    pub stream_uri: Option<String>,
    /// Push mode: the ingest stream key or credential, presented as
    /// §Presenting the Stream Key describes for the `streamUri` scheme.
    #[serde(default)]
    pub stream_key: Option<String>,
    /// Hosted mode: the endpoints viewers play from (server-set, computed
    /// per requesting user). Always empty in push mode, and once
    /// `"stopped"` or `"failed"`.
    #[serde(default)]
    pub playback_endpoints: Vec<VtcPlaybackEndpoint>,
    /// Time the stream went live; `None` before then.
    pub started_at: Option<UTCDate>,
    /// Time the stream stopped or failed; `None` while active.
    pub stopped_at: Option<UTCDate>,
    /// How the participants' video is arranged in the stream, in both
    /// modes (§Composition Layouts); `None` (`null`, or absent) means the
    /// server's default.
    #[serde(default)]
    pub layout: Option<CompositionLayout>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl std::fmt::Debug for VtcLivestream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VtcLivestream")
            .field("id", &self.id)
            .field("call_id", &self.call_id)
            .field("mode", &self.mode)
            .field("state", &self.state)
            .field("failure_reason", &self.failure_reason)
            .field("platform", &self.platform)
            .field("stream_uri", &self.stream_uri.as_ref().map(|_| REDACTED))
            .field("stream_key", &self.stream_key.as_ref().map(|_| REDACTED))
            .field("playback_endpoints", &self.playback_endpoints)
            .field("started_at", &self.started_at)
            .field("stopped_at", &self.stopped_at)
            .field("layout", &self.layout)
            .field("extra", &self.extra)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An available recording, hand-written from draft-atwood-jmap-vtc-00
    /// L461-489. The id is the one in the L1088 stop example.
    fn recording() -> serde_json::Value {
        json!({
            "id": "01J4XP2SN9RXVU1SSEGIUT6GH",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "state": "available",
            "startedAt": "2026-06-05T14:40:00Z",
            "stoppedAt": "2026-06-05T14:50:00Z",
            "initiatedBy": "user:bob@example.com",
            "blobId": "B1",
            "size": 1048576,
            "duration": 600,
            "mediaType": "video/webm",
            "layout": null
        })
    }

    /// A live stream, hand-written from L495-517 with the stream values of
    /// the L1168 create example.
    ///
    /// The draft's push examples predate `mode`, `failureReason` and
    /// `playbackEndpoints`; they are given here with the values the draft
    /// defines for a live push livestream ("push" is the default mode,
    /// failureReason "`null` unless `state` is `\"failed\"`",
    /// playbackEndpoints "Always empty in push mode").
    fn livestream() -> serde_json::Value {
        json!({
            "id": "01J4XQ7VN0SYWV2TTFHJVW7IJ",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "mode": "push",
            "state": "live",
            "failureReason": null,
            "playbackEndpoints": [],
            "platform": "youtube",
            "streamUri": "rtmp://live.example.com/ingest",
            "streamKey": "sk_live_abc123def456",
            "startedAt": "2026-06-05T14:41:00Z",
            "stoppedAt": null,
            "layout": "speaker"
        })
    }

    #[test]
    fn recording_round_trips() {
        let raw = recording();
        let r: VtcRecording = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(r.state, RecordingState::Available);
        assert_eq!(r.size, Some(1_048_576));
        assert_eq!(r.duration, Some(600));
        assert_eq!(serde_json::to_value(&r).unwrap(), raw);
    }

    #[test]
    fn livestream_round_trips() {
        let raw = livestream();
        let l: VtcLivestream = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(l.state, LivestreamState::Live);
        assert_eq!(l.stream_key.as_deref(), Some("sk_live_abc123def456"));
        assert_eq!(serde_json::to_value(&l).unwrap(), raw);
    }

    /// The draft's live hosted-mode example (§Starting a Livestream),
    /// copied verbatim. The example leaves out the server-set
    /// `failureReason`, which is `null` unless `state` is `"failed"`.
    fn hosted() -> serde_json::Value {
        json!({
          "id": "01J4XQ7VN0SYWV2TTFHJVW7JK0",
          "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
          "mode": "hosted",
          "state": "live",
          "platform": null,
          "streamUri": null,
          "streamKey": null,
          "playbackEndpoints": [
            {
              "protocol": "whep",
              "url": "https://media.example.com/whep/01J4XQ7V",
              "latencyHint": "interactive",
              "credential": null,
              "expiresAt": null,
              "timeReference": null,
              "tracks": [],
              "encrypted": false,
              "keySystems": []
            },
            {
              "protocol": "hls",
              "url": "https://cdn.example.com/live/01J4XQ7V/index.m3u8",
              "latencyHint": "standard",
              "credential": null,
              "expiresAt": null,
              "timeReference": {
                "mediaTimeMs": 0,
                "wallClock": "2026-10-21T17:02:09.400Z"
              },
              "tracks": [
                { "kind": "audio", "language": "en", "label": "English",
                  "format": null },
                { "kind": "audio", "language": "es", "label": "Espanol",
                  "format": null },
                { "kind": "captions", "language": "en",
                  "label": "English CC", "format": "text/vtt" }
              ],
              "encrypted": false,
              "keySystems": []
            }
          ],
          "startedAt": "2026-10-21T17:02:11Z",
          "stoppedAt": null,
          "layout": null
        })
    }

    #[test]
    fn hosted_example_round_trips() {
        let raw = hosted();
        let l: VtcLivestream = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(l.mode, LivestreamMode::Hosted);
        assert_eq!(
            (
                l.platform.as_ref(),
                l.stream_uri.as_ref(),
                l.stream_key.as_ref()
            ),
            (None, None, None)
        );
        let [whep, hls] = &l.playback_endpoints[..] else {
            panic!("two endpoints: {l:?}");
        };
        assert_eq!(whep.protocol, PlaybackProtocol::Whep);
        assert_eq!(whep.latency_hint, LatencyHint::Interactive);
        assert_eq!(hls.protocol, PlaybackProtocol::Hls);
        assert_eq!(
            hls.time_reference.as_ref().map(|t| t.media_time_ms),
            Some(0)
        );
        assert_eq!(hls.tracks[2].kind, StreamTrackKind::Captions);
        assert_eq!(hls.tracks[2].format.as_deref(), Some("text/vtt"));
        let mut want = raw;
        want["failureReason"] = json!(null);
        assert_eq!(serde_json::to_value(&l).unwrap(), want);
    }

    #[test]
    fn failed_livestream_round_trips() {
        // Oracle: §VTCLivestream failureReason "`null` unless `state` is
        // `\"failed\"`" with the four listed values; stoppedAt "Time the
        // stream stopped or failed"; playbackEndpoints "empty once it is
        // `\"stopped\"` or `\"failed\"`". Hand-written from those rules.
        let mut raw = hosted();
        raw["state"] = json!("failed");
        raw["failureReason"] = json!("interrupted");
        raw["playbackEndpoints"] = json!([]);
        raw["stoppedAt"] = json!("2026-10-21T17:30:00Z");
        let l: VtcLivestream = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(l.failure_reason, Some(LivestreamFailureReason::Interrupted));
        assert_eq!(serde_json::to_value(&l).unwrap(), raw);
    }

    #[test]
    fn layout_wire_values_and_default() {
        // Oracle: §Composition Layouts defines `"grid"`, `"speaker"` and
        // `"single-speaker"`; "Other layouts SHOULD use reverse-domain
        // notation (e.g., `\"com.example.stage\"`)". §VTCRecording and
        // §VTCLivestream `layout` "(String|null) ... Default: `null`".
        let l: Vec<CompositionLayout> = serde_json::from_value(json!([
            "grid",
            "speaker",
            "single-speaker",
            "com.example.stage"
        ]))
        .unwrap();
        assert_eq!(
            l,
            [
                CompositionLayout::Grid,
                CompositionLayout::Speaker,
                CompositionLayout::SingleSpeaker,
                CompositionLayout::Other("com.example.stage".into())
            ]
        );
        assert_eq!(
            serde_json::to_value(&l[3]).unwrap(),
            json!("com.example.stage")
        );

        let mut raw = recording();
        raw.as_object_mut().unwrap().remove("layout");
        let r: VtcRecording = serde_json::from_value(raw).unwrap();
        assert_eq!(r.layout, None);
        let mut raw = livestream();
        raw.as_object_mut().unwrap().remove("layout");
        let l: VtcLivestream = serde_json::from_value(raw).unwrap();
        assert_eq!(l.layout, None);
        let l: VtcLivestream = serde_json::from_value(livestream()).unwrap();
        assert_eq!(l.layout, Some(CompositionLayout::Speaker));
    }

    #[test]
    fn mode_defaults_to_push() {
        // Oracle: §VTCLivestream `mode` "Default: `\"push\"`".
        let mut raw = livestream();
        raw.as_object_mut().unwrap().remove("mode");
        let l: VtcLivestream = serde_json::from_value(raw).unwrap();
        assert_eq!(l.mode, LivestreamMode::Push);
    }

    #[test]
    fn open_string_enums_keep_unknown_values() {
        // Oracle: protocol "Other protocols SHOULD use reverse-domain
        // notation"; latencyHint, kind and failureReason say clients handle
        // unrecognized values, so they must survive a round-trip.
        let p: PlaybackProtocol = serde_json::from_value(json!("org.example.playback")).unwrap();
        assert_eq!(p, PlaybackProtocol::Other("org.example.playback".into()));
        for (v, ok) in [
            (
                json!("x-ultra"),
                serde_json::from_value::<LatencyHint>(json!("x-ultra"))
                    .map(|v| serde_json::to_value(v).unwrap()),
            ),
            (
                json!("x-sign"),
                serde_json::from_value::<StreamTrackKind>(json!("x-sign"))
                    .map(|v| serde_json::to_value(v).unwrap()),
            ),
            (
                json!("x-cost"),
                serde_json::from_value::<LivestreamFailureReason>(json!("x-cost"))
                    .map(|v| serde_json::to_value(v).unwrap()),
            ),
            (
                json!("x-relay"),
                serde_json::from_value::<LivestreamMode>(json!("x-relay"))
                    .map(|v| serde_json::to_value(v).unwrap()),
            ),
        ] {
            assert_eq!(ok.unwrap(), v);
        }
    }

    #[test]
    fn debug_redacts_playback_url_and_credential() {
        // Oracle: §VTCPlaybackEndpoint url "MAY carry authorization for the
        // requesting user"; credential "An opaque playback credential minted
        // for the requesting user".
        let mut raw = hosted();
        raw["playbackEndpoints"][0]["credential"] = json!("tok_secret_123");
        let l: VtcLivestream = serde_json::from_value(raw).unwrap();
        let dbg = format!("{l:?}");
        assert!(!dbg.contains("tok_secret_123"), "{dbg}");
        assert!(!dbg.contains("media.example.com"), "{dbg}");
    }

    #[test]
    fn state_wire_values() {
        // Oracle: L468 "`\"recording\"`, `\"paused\"`, `\"stopped\"`,
        // `\"processing\"`, `\"available\"`, or `\"failed\"`"; L502
        // "`\"starting\"`, `\"live\"`, `\"stopped\"`, or `\"failed\"`".
        let r: Vec<RecordingState> = serde_json::from_value(json!([
            "recording",
            "paused",
            "stopped",
            "processing",
            "available",
            "failed"
        ]))
        .unwrap();
        assert_eq!(
            r,
            [
                RecordingState::Recording,
                RecordingState::Paused,
                RecordingState::Stopped,
                RecordingState::Processing,
                RecordingState::Available,
                RecordingState::Failed
            ]
        );
        let l: Vec<LivestreamState> =
            serde_json::from_value(json!(["starting", "live", "stopped", "failed"])).unwrap();
        assert_eq!(
            l,
            [
                LivestreamState::Starting,
                LivestreamState::Live,
                LivestreamState::Stopped,
                LivestreamState::Failed
            ]
        );
        let o: LivestreamState = serde_json::from_value(json!("x-buffering")).unwrap();
        assert_eq!(serde_json::to_value(&o).unwrap(), json!("x-buffering"));
    }

    #[test]
    fn debug_redacts_stream_uri_and_key() {
        // Oracle: L508 streamUri "Treated as sensitive"; L511 streamKey "the
        // stream key or authentication credential".
        let l: VtcLivestream = serde_json::from_value(livestream()).unwrap();
        let dbg = format!("{l:?}");
        assert!(!dbg.contains("sk_live_abc123def456"), "{dbg}");
        assert!(!dbg.contains("rtmp://live.example.com"), "{dbg}");
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = livestream();
        raw["vendor.example:bitrate"] = json!(6000);
        let l: VtcLivestream = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(l.extra.get("vendor.example:bitrate"), Some(&json!(6000)));
        assert_eq!(serde_json::to_value(&l).unwrap(), raw);
    }
}
