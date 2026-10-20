//! draft-atwood-jmap-vtc-00 §VTCRecording and §VTCLivestream.

use jmap_types::{impl_string_enum, Id, UTCDate};
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
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Metadata of an outbound livestream from a call to an external
/// platform (draft-atwood-jmap-vtc-00 §VTCLivestream).
///
/// `streamUri` is "Treated as sensitive" and `streamKey` is an ingest
/// credential, so the `Debug` impl redacts both. The server MUST return
/// `streamKey` as `""` to non-moderators; that is method behaviour, not
/// enforced here.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcLivestream {
    /// Opaque server-assigned identifier.
    pub id: Id,
    /// The call this livestream belongs to.
    pub call_id: Id,
    /// Current state.
    pub state: LivestreamState,
    /// Deployment-defined platform, e.g. `"youtube"`. "Clients MUST ignore
    /// unrecognized values."
    pub platform: Option<String>,
    /// The RTMP or equivalent ingest endpoint.
    pub stream_uri: String,
    /// The ingest stream key or credential.
    pub stream_key: String,
    /// Time the stream went live; `None` before then.
    pub started_at: Option<UTCDate>,
    /// Time the stream stopped; `None` while active.
    pub stopped_at: Option<UTCDate>,
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
            .field("state", &self.state)
            .field("platform", &self.platform)
            .field("stream_uri", &REDACTED)
            .field("stream_key", &REDACTED)
            .field("started_at", &self.started_at)
            .field("stopped_at", &self.stopped_at)
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
            "mediaType": "video/webm"
        })
    }

    /// A live stream, hand-written from L495-517 with the stream values of
    /// the L1168 create example.
    fn livestream() -> serde_json::Value {
        json!({
            "id": "01J4XQ7VN0SYWV2TTFHJVW7IJ",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "state": "live",
            "platform": "youtube",
            "streamUri": "rtmp://live.example.com/ingest",
            "streamKey": "sk_live_abc123def456",
            "startedAt": "2026-06-05T14:41:00Z",
            "stoppedAt": null
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
        assert_eq!(l.stream_key, "sk_live_abc123def456");
        assert_eq!(serde_json::to_value(&l).unwrap(), raw);
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
