//! The `VTCLivestream` adapter over IVS channels.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, SystemTime};

use jmap_types::{Id, UTCDate};
use jmap_vtc_server::VtcBackend;
use jmap_vtc_server::media_report::{LivestreamReport, MediaReporter, ReportError};
use jmap_vtc_types::{
    LatencyHint, LivestreamFailureReason, LivestreamMode, PlaybackProtocol, VtcLivestream,
    VtcPlaybackEndpoint,
};
use serde::{Deserialize, Serialize};

use super::api::{ChannelType, CreateChannelInput, IvsClient, LatencyMode};
use super::events::{IvsEvent, IvsEventKind, LimitName};
use super::token::{PlaybackClaims, PlaybackKey, TokenError};
use crate::time::Utc;
use crate::{AwsError, AwsErrorCode};

/// The default lifetime of a viewer token: "a client that is still
/// playing fetches the livestream again before `expiresAt`"
/// (draft-atwood-jmap-vtc-00 §Playback Authorization).
pub const DEFAULT_TOKEN_TTL: Duration = Duration::from_secs(300);

/// The tag naming the `VTCLivestream` a channel serves.
const LIVESTREAM_TAG: &str = "jmap-vtc:livestream-id";
/// The tag naming the livestream's `VTCCall`.
const CALL_TAG: &str = "jmap-vtc:call-id";

/// How [`IvsLivestreams::start`] creates channels.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct ChannelTemplate {
    /// The channel `type`; `None` is the AWS default, `STANDARD`.
    pub channel_type: Option<ChannelType>,
    /// The `latencyMode`; `None` is the AWS default, `LOW`.
    pub latency_mode: Option<LatencyMode>,
    /// A recording configuration (S3), or `None` for no IVS recording.
    pub recording_configuration_arn: Option<String>,
    /// Tags for every channel, besides the two this crate adds
    /// (`jmap-vtc:livestream-id`, `jmap-vtc:call-id`).
    pub tags: BTreeMap<String, String>,
}

impl ChannelTemplate {
    /// This template with channel `type` `t`.
    pub fn with_channel_type(self, t: ChannelType) -> Self {
        Self {
            channel_type: Some(t),
            ..self
        }
    }

    /// This template with `latencyMode` `m`.
    pub fn with_latency_mode(self, m: LatencyMode) -> Self {
        Self {
            latency_mode: Some(m),
            ..self
        }
    }

    /// This template recording through `arn`.
    pub fn with_recording_configuration_arn(self, arn: impl Into<String>) -> Self {
        Self {
            recording_configuration_arn: Some(arn.into()),
            ..self
        }
    }

    /// This template with one more tag.
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }
}

/// The IVS channel serving one hosted livestream: what
/// [`IvsLivestreams::start`] returns and every later call takes.
///
/// The deployment stores it with the livestream (it is `Serialize`), so
/// the adapter itself keeps no state. `stream_key` is the ingest
/// credential; the `Debug` impl never shows it.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IvsChannel {
    /// The `VTCLivestream` id.
    pub livestream_id: Id,
    /// The channel ARN.
    pub arn: String,
    /// The HLS playback URL.
    pub playback_url: String,
    /// The RTMPS ingest server.
    pub ingest_endpoint: String,
    /// The stream key value.
    pub stream_key: String,
    /// Private channel: playback needs a token.
    pub authorized: bool,
    /// The channel's latency mode, if IVS said.
    pub latency_mode: Option<LatencyMode>,
}

impl IvsChannel {
    /// The RTMPS URL the compositor pushes to, with [`Self::stream_key`]
    /// as the stream key: `rtmps://{ingestEndpoint}:443/app/`
    /// ("Getting Started: Set Up Streaming Software",
    /// <https://docs.aws.amazon.com/ivs/latest/LowLatencyUserGuide/getting-started-set-up-streaming.html>).
    pub fn rtmps_url(&self) -> String {
        format!("rtmps://{}:443/app/", self.ingest_endpoint)
    }

    /// The stream key.
    pub fn stream_key(&self) -> &str {
        &self.stream_key
    }
}

impl fmt::Debug for IvsChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IvsChannel")
            .field("livestream_id", &self.livestream_id)
            .field("arn", &self.arn)
            .field("playback_url", &self.playback_url)
            .field("ingest_endpoint", &self.ingest_endpoint)
            .field("stream_key", &"<redacted>")
            .field("authorized", &self.authorized)
            .field("latency_mode", &self.latency_mode)
            .finish()
    }
}

/// How [`IvsLivestreams::stop`] ended.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    /// The channel is deleted (or was already gone).
    Deleted,
    /// The stream was stopped but IVS still counts the channel as live
    /// (`DeleteChannel` answered `ConflictException`): "To delete a channel
    /// that is live, call `StopStream`, wait for the Amazon EventBridge
    /// "Stream End" event ..., then call `DeleteChannel`." Call
    /// [`IvsLivestreams::stop`] again once that event arrives.
    DeletePending,
}

/// Hosted `VTCLivestream`s on IVS low-latency channels, one channel per
/// livestream.
///
/// The adapter holds no per-livestream state: every call takes the
/// [`IvsChannel`] that [`Self::start`] returned.
#[derive(Debug, Clone)]
pub struct IvsLivestreams {
    client: IvsClient,
    template: ChannelTemplate,
    playback_key: Option<PlaybackKey>,
    token_ttl: Duration,
    allowed_origin: Option<String>,
}

impl IvsLivestreams {
    /// An adapter creating public channels from `template`.
    pub fn new(client: IvsClient, template: ChannelTemplate) -> Self {
        Self {
            client,
            template,
            playback_key: None,
            token_ttl: DEFAULT_TOKEN_TTL,
            allowed_origin: None,
        }
    }

    /// This adapter creating private channels (`authorized: true`) whose
    /// viewers get tokens signed with `key`, valid for `token_ttl` (IVS
    /// caps it at 10 minutes when a viewer id is set). The public half of
    /// `key` must be imported into IVS in the same region
    /// (`ImportPlaybackKeyPair`).
    pub fn with_private_playback(self, key: PlaybackKey, token_ttl: Duration) -> Self {
        Self {
            playback_key: Some(key),
            token_ttl,
            ..self
        }
    }

    /// This adapter's tokens restricted to `origins`
    /// (`aws:access-control-allow-origin`).
    pub fn with_allowed_origin(self, origins: impl Into<String>) -> Self {
        Self {
            allowed_origin: Some(origins.into()),
            ..self
        }
    }

    /// The IVS client.
    pub fn client(&self) -> &IvsClient {
        &self.client
    }

    /// Provision the channel for hosted `livestream`
    /// ([`MediaEffect::StartLivestream`](jmap_vtc_server::MediaEffect::StartLivestream)):
    /// one `CreateChannel` call, named `jmap-vtc-{id}` and tagged with the
    /// livestream and call ids.
    ///
    /// # Errors
    ///
    /// [`AwsError::UnsupportedMode`] for a push-mode livestream (IVS cannot
    /// push to the moderator's `streamUri`), or the `CreateChannel`
    /// [`AwsError`]. Report either with
    /// [`MediaReporter::livestream_failed`] and
    /// [`AwsError::failure_reason`].
    pub async fn start(&self, livestream: &VtcLivestream) -> Result<IvsChannel, AwsError> {
        if livestream.mode != LivestreamMode::Hosted {
            return Err(AwsError::UnsupportedMode(livestream.mode.clone()));
        }
        let mut tags = self.template.tags.clone();
        tags.insert(LIVESTREAM_TAG.to_owned(), livestream.id.to_string());
        tags.insert(CALL_TAG.to_owned(), livestream.call_id.to_string());
        let input = CreateChannelInput {
            name: Some(channel_name(&livestream.id)),
            channel_type: self.template.channel_type.clone(),
            latency_mode: self.template.latency_mode.clone(),
            authorized: self.playback_key.is_some(),
            insecure_ingest: false,
            recording_configuration_arn: self.template.recording_configuration_arn.clone(),
            tags,
        };
        let created = self.client.create_channel(&input).await?;
        Ok(IvsChannel {
            livestream_id: livestream.id.clone(),
            arn: created.channel.arn,
            playback_url: created.channel.playback_url,
            ingest_endpoint: created.channel.ingest_endpoint,
            stream_key: created.stream_key.value,
            authorized: created.channel.authorized,
            latency_mode: created.channel.latency_mode,
        })
    }

    /// Release `channel`
    /// ([`MediaEffect::StopLivestream`](jmap_vtc_server::MediaEffect::StopLivestream)):
    /// `StopStream`, then `DeleteChannel`. Idempotent: a channel that is
    /// not broadcasting or already deleted is fine.
    ///
    /// # Errors
    ///
    /// The [`AwsError`] of either call, other than `ChannelNotBroadcasting`,
    /// `ResourceNotFoundException` and the `ConflictException` that
    /// [`StopOutcome::DeletePending`] reports.
    pub async fn stop(&self, channel: &IvsChannel) -> Result<StopOutcome, AwsError> {
        match self.client.stop_stream(&channel.arn).await {
            Ok(_) => {}
            Err(e) if e.code() == Some(&AwsErrorCode::ResourceNotFound) => {
                return Ok(StopOutcome::Deleted);
            }
            Err(e) => return Err(e),
        }
        match self.client.delete_channel(&channel.arn).await {
            Ok(()) => Ok(StopOutcome::Deleted),
            Err(e) => match e.code() {
                Some(AwsErrorCode::ResourceNotFound) => Ok(StopOutcome::Deleted),
                Some(AwsErrorCode::Conflict) => Ok(StopOutcome::DeletePending),
                _ => Err(e),
            },
        }
    }

    /// The playback endpoint to store when `channel` goes live: HLS at its
    /// playback URL, `latencyHint` `"low"` for an IVS `LOW` latency channel
    /// (IVS: "near-real-time interaction with viewers") and `"standard"`
    /// otherwise. No credential: [`Self::endpoints_for`] adds it per
    /// viewer.
    pub fn playback_endpoint(&self, channel: &IvsChannel) -> VtcPlaybackEndpoint {
        let hint = match channel.latency_mode {
            Some(LatencyMode::Normal) => LatencyHint::Standard,
            _ => LatencyHint::Low,
        };
        VtcPlaybackEndpoint::new(PlaybackProtocol::Hls, channel.playback_url.clone(), hint)
    }

    /// The `playbackEndpoints` a viewer receives
    /// ([`VtcBackend::playback_endpoints_for`](jmap_vtc_server::VtcBackend::playback_endpoints_for)),
    /// given `stored`, the stored list.
    ///
    /// For a public channel, `stored` unchanged. For a private one, each
    /// endpoint at the channel's playback URL gets a fresh token, signed
    /// for `now + token_ttl` and carrying `viewer_id` as `aws:viewer-id`
    /// when given, appended as the `token` query parameter ("Append the
    /// signed token to the playback URL as a query parameter"), with
    /// `expiresAt` set and `credential` `null`: IVS players take the token
    /// in the URL, which draft-atwood-jmap-vtc-00 §Playback Authorization
    /// allows ("a server can carry the authorization in `url` itself and
    /// set `credential` to `null`").
    ///
    /// # Errors
    ///
    /// [`TokenError::NoKey`] for a private channel without a configured
    /// key, or the [`PlaybackKey::sign`] error.
    pub fn endpoints_for(
        &self,
        channel: &IvsChannel,
        stored: Vec<VtcPlaybackEndpoint>,
        viewer_id: Option<&str>,
        now: SystemTime,
    ) -> Result<Vec<VtcPlaybackEndpoint>, TokenError> {
        if !channel.authorized {
            return Ok(stored);
        }
        let key = self.playback_key.as_ref().ok_or(TokenError::NoKey)?;
        let expires = now + self.token_ttl;
        let mut claims = PlaybackClaims::new(channel.arn.clone(), expires);
        if let Some(origin) = &self.allowed_origin {
            claims = claims.with_allowed_origin(origin.clone());
        }
        if let Some(viewer) = viewer_id {
            claims = claims.with_viewer_id(viewer);
        }
        let token = key.sign(&claims, now)?;
        let expires_at = UTCDate::new_validated(Utc::from_system_time(expires).rfc3339()).ok();
        Ok(stored
            .into_iter()
            .map(|mut ep| {
                if ep.url == channel.playback_url {
                    let sep = if ep.url.contains('?') { '&' } else { '?' };
                    ep.url = format!("{}{sep}token={token}", ep.url);
                    ep.credential = None;
                    ep.expires_at = expires_at.clone();
                }
                ep
            })
            .collect())
    }

    /// Apply `event`, an IVS event for `channel`, to livestream
    /// `livestream_id` through `reporter`:
    ///
    /// | IVS event | Report |
    /// |---|---|
    /// | Stream Start | `livestream_live` with [`Self::playback_endpoint`] |
    /// | Stream End | `livestream_failed`, `"interrupted"` ("The stream was live and was lost") |
    /// | Stream Failure | `livestream_failed`, `"unavailable"` (processing capacity exceeded) |
    /// | Limit Breach: Ingest Bitrate, Ingest Resolution | `livestream_failed`, `"rejected"` ("an unsupported media format") |
    /// | Limit Breach: Concurrent Broadcasts | `livestream_failed`, `"unavailable"` |
    /// | anything else | nothing |
    ///
    /// A Stream End that follows a stop the server asked for finds the
    /// livestream already `"stopped"` and returns
    /// [`ReportError::WrongState`], as does a duplicate or late event;
    /// those are expected and change nothing.
    ///
    /// # Errors
    ///
    /// [`ReportError::Invalid`] when the event names another channel, or
    /// the report's error.
    pub async fn report_event<B: VtcBackend>(
        &self,
        reporter: &MediaReporter<'_, B>,
        channel: &IvsChannel,
        event: &IvsEvent,
    ) -> LivestreamReport<B::Error> {
        if event
            .channel_arn
            .as_deref()
            .is_some_and(|arn| arn != channel.arn)
        {
            return Err(ReportError::Invalid("the event is for another channel"));
        }
        let id = &channel.livestream_id;
        let failed = |reason| reporter.livestream_failed(id, reason);
        match &event.kind {
            IvsEventKind::StreamStart => {
                reporter
                    .livestream_live(id, vec![self.playback_endpoint(channel)])
                    .await
            }
            IvsEventKind::StreamEnd => failed(LivestreamFailureReason::Interrupted).await,
            IvsEventKind::StreamFailure { .. } => {
                failed(LivestreamFailureReason::Unavailable).await
            }
            IvsEventKind::LimitBreach { limit } => match limit {
                LimitName::IngestBitrate | LimitName::IngestResolution => {
                    failed(LivestreamFailureReason::Rejected).await
                }
                LimitName::ConcurrentBroadcasts => {
                    failed(LivestreamFailureReason::Unavailable).await
                }
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }
}

/// `jmap-vtc-{id}`, cut to the `CreateChannel` `name` rules: "Pattern:
/// `[a-zA-Z0-9-_]*`", "Maximum length of 128".
fn channel_name(id: &Id) -> String {
    format!("jmap-vtc-{id}")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(128)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names_follow_the_pattern() {
        assert_eq!(
            channel_name(&Id::from("01J4XQ7VN0SYWV2TTFHJVW7JK0")),
            "jmap-vtc-01J4XQ7VN0SYWV2TTFHJVW7JK0"
        );
        let long = Id::from("x".repeat(200).as_str());
        assert_eq!(channel_name(&long).len(), 128);
    }

    #[test]
    fn channel_debug_redacts_the_stream_key() {
        let c = IvsChannel {
            livestream_id: Id::from("L1"),
            arn: "arn:aws:ivs:us-west-2:123456789012:channel/abcdABCDefgh".to_owned(),
            playback_url: "https://example.playback.live-video.net/x.m3u8".to_owned(),
            ingest_endpoint: "a1b2c3d4e5f6.global-contribute.live-video.net".to_owned(),
            stream_key: "sk_us-west-2_canary-key".to_owned(),
            authorized: false,
            latency_mode: None,
        };
        assert!(!format!("{c:?}").contains("canary-key"));
        assert_eq!(
            c.rtmps_url(),
            "rtmps://a1b2c3d4e5f6.global-contribute.live-video.net:443/app/"
        );
    }
}
