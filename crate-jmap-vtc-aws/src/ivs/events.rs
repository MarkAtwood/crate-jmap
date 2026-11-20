//! IVS events delivered through Amazon EventBridge.
//!
//! Oracle: "Using Amazon EventBridge with IVS Low-Latency Streaming",
//! <https://docs.aws.amazon.com/ivs/latest/LowLatencyUserGuide/eventbridge.html>
//! (the event table and the JSON examples).

use jmap_types::impl_string_enum;
use serde_json::Value;

/// The `limit_name` of an "IVS Limit Breach" event.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LimitName {
    /// "The incoming stream's bitrate exceeds the Amazon IVS limit."
    IngestBitrate,
    /// "The incoming stream's resolution exceeds the Amazon IVS limit."
    IngestResolution,
    /// "The total number of channels streaming at the same time exceeds
    /// the Amazon IVS limit."
    ConcurrentBroadcasts,
    /// "The total number of viewers watching your channels at the same
    /// time exceeds the Amazon IVS limit."
    ConcurrentViewers,
    /// A limit this version does not list.
    Other(String),
}

impl_string_enum!(LimitName, "an IVS limit name",
    "Ingest Bitrate" => IngestBitrate,
    "Ingest Resolution" => IngestResolution,
    "Concurrent Broadcasts" => ConcurrentBroadcasts,
    "Concurrent Viewers" => ConcurrentViewers,
);

/// What an IVS event says.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IvsEventKind {
    /// "Session Created": "A channel stream key was used successfully and
    /// a stream session was created", before any video is delivered.
    SessionCreated,
    /// "Session Ended": "The encoder disconnected"; `code` may say why.
    SessionEnded {
        /// The `code` field, if any.
        code: Option<String>,
    },
    /// "Stream Start": "segments are available for the viewer to watch".
    StreamStart,
    /// "Stream End": "A stream stops being processed and no longer
    /// produces video segments for the viewer."
    StreamEnd,
    /// "Stream Failure": "A stream is not being processed and is not
    /// available because processing capacity was exceeded."
    StreamFailure {
        /// The `reason` field, if any.
        reason: Option<String>,
    },
    /// "Starvation Start": the channel receives no data from the streamer.
    StarvationStart,
    /// "Starvation End": the starving stream receives data again.
    StarvationEnd,
    /// An "IVS Limit Breach" event.
    LimitBreach {
        /// Which limit.
        limit: LimitName,
    },
    /// Any other IVS event (stream takeover, failover slate, ad break,
    /// recording state).
    Other {
        /// The `detail-type`.
        detail_type: String,
        /// The `detail.event_name` (or `recording_status`), if any.
        event_name: Option<String>,
    },
}

/// One IVS EventBridge event.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IvsEvent {
    /// The channel ARN (`resources[0]`); `None` for account-wide events
    /// such as the "Concurrent Broadcasts" limit breach.
    pub channel_arn: Option<String>,
    /// The stream session (`detail.stream_id`), "a unique stream identifier
    /// assigned each time a channel goes live".
    pub stream_id: Option<String>,
    /// The event `time`, as sent.
    pub time: Option<String>,
    /// What happened.
    pub kind: IvsEventKind,
}

impl IvsEvent {
    /// Parse an EventBridge event, or `None` when its `source` is not
    /// `aws.ivs`.
    ///
    /// IVS sends events "on a best-effort basis": they may be missing,
    /// hours late, or out of order ("you could see Stream Down before
    /// Stream Up").
    pub fn from_eventbridge(event: &Value) -> Option<Self> {
        if event.get("source").and_then(Value::as_str) != Some("aws.ivs") {
            return None;
        }
        let detail_type = event
            .get("detail-type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let detail = event.get("detail").unwrap_or(&Value::Null);
        let field = |k: &str| detail.get(k).and_then(Value::as_str).map(str::to_owned);
        let event_name = field("event_name");
        let kind = match (detail_type, event_name.as_deref()) {
            ("IVS Stream State Change", Some("Session Created")) => IvsEventKind::SessionCreated,
            ("IVS Stream State Change", Some("Session Ended")) => IvsEventKind::SessionEnded {
                code: field("code"),
            },
            ("IVS Stream State Change", Some("Stream Start")) => IvsEventKind::StreamStart,
            ("IVS Stream State Change", Some("Stream End")) => IvsEventKind::StreamEnd,
            ("IVS Stream State Change", Some("Stream Failure")) => IvsEventKind::StreamFailure {
                reason: field("reason"),
            },
            ("IVS Stream Health Change", Some("Starvation Start")) => IvsEventKind::StarvationStart,
            ("IVS Stream Health Change", Some("Starvation End")) => IvsEventKind::StarvationEnd,
            ("IVS Limit Breach", _) => IvsEventKind::LimitBreach {
                limit: LimitName::from(field("limit_name").unwrap_or_default()),
            },
            _ => IvsEventKind::Other {
                detail_type: detail_type.to_owned(),
                event_name: event_name.or_else(|| field("recording_status")),
            },
        };
        Some(Self {
            channel_arn: event
                .get("resources")
                .and_then(Value::as_array)
                .and_then(|r| r.first())
                .and_then(Value::as_str)
                .map(|s| s.trim().to_owned()),
            stream_id: field("stream_id"),
            time: event.get("time").and_then(Value::as_str).map(str::to_owned),
            kind,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The "Stream Start" example of the EventBridge page, verbatim.
    #[test]
    fn stream_start_example() {
        let e = IvsEvent::from_eventbridge(&json!({
           "version": "0",
           "id": "01234567-0123-0123-0123-012345678901",
           "detail-type": "IVS Stream State Change",
           "source": "aws.ivs",
           "account": "aws_account_id",
           "time": "2017-06-12T10:23:43Z",
           "region": "us-east-1",
           "resources": [
             "arn:aws:ivs:us-east-1:aws_account_id:channel/12345678-1a23-4567-a1bc-1a2b34567890"
           ],
           "detail": {
             "event_name": "Stream Start",
             "channel_name": "Your Channel",
             "stream_id": "st-1A2b3c4D5e6F78ghij9Klmn"
           }
        }))
        .unwrap();
        assert_eq!(e.kind, IvsEventKind::StreamStart);
        assert_eq!(
            e.channel_arn.as_deref(),
            Some(
                "arn:aws:ivs:us-east-1:aws_account_id:channel/12345678-1a23-4567-a1bc-1a2b34567890"
            )
        );
        assert_eq!(e.stream_id.as_deref(), Some("st-1A2b3c4D5e6F78ghij9Klmn"));
    }

    /// The "Stream Failure" and "Ingest Bitrate" examples, verbatim
    /// except for whitespace.
    #[test]
    fn failure_and_limit_examples() {
        let failure = IvsEvent::from_eventbridge(&json!({
           "detail-type": "IVS Stream State Change",
           "source": "aws.ivs",
           "resources": ["arn:aws:ivs:us-east-1:aws_account_id:channel/12345678-1a23-4567-a1bc-1a2b34567890"],
           "detail": {
             "event_name": "Stream Failure",
             "channel_name": "Your Channel",
             "stream_id": "st-1A2b3c4D5e6F78ghij9Klmn",
             "reason": "Transcode capacity exceeded. Please try again."
           }
        }))
        .unwrap();
        assert_eq!(
            failure.kind,
            IvsEventKind::StreamFailure {
                reason: Some("Transcode capacity exceeded. Please try again.".to_owned())
            }
        );
        let breach = IvsEvent::from_eventbridge(&json!({
           "detail-type": "IVS Limit Breach",
           "source": "aws.ivs",
           "resources": ["arn:aws:ivs:us-east-1:aws_account_id:channel/12345678-1a23-4567-a1bc-1a2b34567890"],
           "detail": {
             "limit_name": "Ingest Bitrate",
             "limit_value": 1234,
             "exceeded_by": 3,
             "limit_unit": "bits per second",
             "channel_name": "Your Channel",
             "stream_id": "st-1A2b3c4D5e6F78ghij9Klmn"
           }
        }))
        .unwrap();
        assert_eq!(
            breach.kind,
            IvsEventKind::LimitBreach {
                limit: LimitName::IngestBitrate
            }
        );
    }

    #[test]
    fn other_sources_are_skipped() {
        assert!(IvsEvent::from_eventbridge(&json!({"source": "aws.medialive"})).is_none());
    }
}
