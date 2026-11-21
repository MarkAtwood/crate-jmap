//! The Amazon IVS low-latency streaming API (`ivs`, version 2020-07-14).
//!
//! Oracle: the Amazon IVS Low-Latency Streaming API Reference,
//! <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/Welcome.html>,
//! one page per action cited at each method.

use std::collections::BTreeMap;
use std::fmt;

use jmap_types::impl_string_enum;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::client::JsonClient;
use crate::{AwsError, AwsErrorCode, Credentials};

/// The SigV4 service name of IVS.
const SERVICE: &str = "ivs";

/// The IVS channel `type`, "which determines the allowable resolution and
/// bitrate" (`CreateChannel` `type`; default `STANDARD`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChannelType {
    /// `BASIC`.
    Basic,
    /// `STANDARD`.
    Standard,
    /// `ADVANCED_SD`.
    AdvancedSd,
    /// `ADVANCED_HD`.
    AdvancedHd,
    /// A type this version does not list.
    Other(String),
}

impl_string_enum!(ChannelType, "an IVS channel type",
    "BASIC" => Basic,
    "STANDARD" => Standard,
    "ADVANCED_SD" => AdvancedSd,
    "ADVANCED_HD" => AdvancedHd,
);

/// The IVS channel `latencyMode`: "Use `NORMAL` to broadcast and deliver
/// live video up to Full HD. Use `LOW` for near-real-time interaction with
/// viewers. Default: `LOW`."
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LatencyMode {
    /// `LOW`.
    Low,
    /// `NORMAL`.
    Normal,
    /// A mode this version does not list.
    Other(String),
}

impl_string_enum!(LatencyMode, "an IVS latency mode",
    "LOW" => Low,
    "NORMAL" => Normal,
);

/// The `state` of an IVS [`Stream`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamState {
    /// `LIVE`.
    Live,
    /// `OFFLINE`.
    Offline,
    /// A state this version does not list.
    Other(String),
}

impl_string_enum!(StreamState, "an IVS stream state",
    "LIVE" => Live,
    "OFFLINE" => Offline,
);

/// The `health` of an IVS [`Stream`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamHealth {
    /// `HEALTHY`.
    Healthy,
    /// `STARVING`: the channel receives no media.
    Starving,
    /// `UNKNOWN`.
    Unknown,
    /// A value this version does not list.
    Other(String),
}

impl_string_enum!(StreamHealth, "an IVS stream health",
    "HEALTHY" => Healthy,
    "STARVING" => Starving,
    "UNKNOWN" => Unknown,
);

/// The `CreateChannel` request: the fields this crate sends. `None`
/// leaves the AWS default.
#[derive(Debug, Clone, Default)]
pub struct CreateChannelInput {
    /// `name`: pattern `[a-zA-Z0-9-_]*`, at most 128 characters.
    pub name: Option<String>,
    /// `type`.
    pub channel_type: Option<ChannelType>,
    /// `latencyMode`.
    pub latency_mode: Option<LatencyMode>,
    /// `authorized`: "Whether the channel is private (enabled for
    /// playback authorization)."
    pub authorized: bool,
    /// `insecureIngest`: allow plain RTMP and SRT ingest.
    pub insecure_ingest: bool,
    /// `recordingConfigurationArn`: records the channel to S3.
    pub recording_configuration_arn: Option<String>,
    /// `tags`.
    pub tags: BTreeMap<String, String>,
}

/// An IVS channel (the `Channel` object,
/// <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_Channel.html>).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    /// The channel ARN.
    pub arn: String,
    /// The channel name.
    #[serde(default)]
    pub name: String,
    /// `type`.
    #[serde(rename = "type", default)]
    pub channel_type: Option<ChannelType>,
    /// `latencyMode`.
    #[serde(default)]
    pub latency_mode: Option<LatencyMode>,
    /// The RTMPS ingest server, e.g. `a1b2c3d4e5f6.global-contribute.live-video.net`.
    #[serde(default)]
    pub ingest_endpoint: String,
    /// The HLS playback URL.
    #[serde(default)]
    pub playback_url: String,
    /// Private channel: playback needs a token.
    #[serde(default)]
    pub authorized: bool,
    /// Plain RTMP and SRT ingest allowed.
    #[serde(default)]
    pub insecure_ingest: bool,
    /// The SRT ingest endpoint and passphrase.
    #[serde(default)]
    pub srt: Option<SrtIngest>,
    /// The channel's tags.
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

/// A channel's SRT ingest (`Channel.srt`). The `Debug` impl never shows
/// the passphrase.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct SrtIngest {
    /// The SRT ingest server.
    #[serde(default)]
    pub endpoint: String,
    /// The SRT passphrase, a credential.
    #[serde(default)]
    pub passphrase: String,
}

impl fmt::Debug for SrtIngest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SrtIngest")
            .field("endpoint", &self.endpoint)
            .field("passphrase", &"<redacted>")
            .finish()
    }
}

/// An IVS stream key (the `StreamKey` object,
/// <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_StreamKey.html>).
/// `value` is the ingest credential; the `Debug` impl never shows it.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamKey {
    /// The stream key ARN.
    pub arn: String,
    /// The channel it belongs to.
    #[serde(default)]
    pub channel_arn: String,
    /// The stream key value, the ingest credential.
    #[serde(default)]
    pub value: String,
}

impl fmt::Debug for StreamKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamKey")
            .field("arn", &self.arn)
            .field("channel_arn", &self.channel_arn)
            .field("value", &"<redacted>")
            .finish()
    }
}

/// The `CreateChannel` response: the channel and its stream key.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedChannel {
    /// The new channel.
    pub channel: Channel,
    /// Its stream key.
    pub stream_key: StreamKey,
}

/// A live stream on a channel (the `Stream` object,
/// <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_Stream.html>).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    /// The channel.
    pub channel_arn: String,
    /// The playback URL.
    #[serde(default)]
    pub playback_url: String,
    /// When the stream started (ISO 8601), if given.
    #[serde(default)]
    pub start_time: Option<String>,
    /// `LIVE` or `OFFLINE`.
    pub state: StreamState,
    /// `HEALTHY`, `STARVING` or `UNKNOWN`.
    pub health: StreamHealth,
    /// Approximate concurrent viewers.
    #[serde(default)]
    pub viewer_count: Option<u64>,
    /// The stream session id, new each time the channel goes live.
    #[serde(default)]
    pub stream_id: Option<String>,
}

/// An Amazon IVS low-latency API client for one region.
#[derive(Debug, Clone)]
pub struct IvsClient {
    inner: JsonClient,
}

impl IvsClient {
    /// A client for `region` (e.g. `us-west-2`) at
    /// `https://ivs.{region}.amazonaws.com/`, over an HTTP client that
    /// follows no redirects and times out after
    /// [`REQUEST_TIMEOUT`](crate::REQUEST_TIMEOUT).
    ///
    /// # Errors
    ///
    /// [`AwsError::Config`] for a region that is not
    /// lower-case letters, digits and `-`; [`AwsError::Transport`] when
    /// the HTTP client cannot be built.
    pub fn new(credentials: Credentials, region: impl Into<String>) -> Result<Self, AwsError> {
        Self::from_client(crate::default_client()?, credentials, region)
    }

    /// A client over `http`, as configured.
    ///
    /// # Errors
    ///
    /// [`AwsError::Config`] for an invalid region.
    pub fn from_client(
        http: reqwest::Client,
        credentials: Credentials,
        region: impl Into<String>,
    ) -> Result<Self, AwsError> {
        let region = region.into();
        if region.is_empty()
            || !region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(AwsError::Config(format!("invalid region {region:?}")));
        }
        let endpoint = Url::parse(&format!("https://{SERVICE}.{region}.amazonaws.com/"))
            .map_err(|e| AwsError::Config(e.to_string()))?;
        Ok(Self {
            inner: JsonClient::new(http, credentials, region, SERVICE, endpoint),
        })
    }

    /// This client sending to `endpoint` instead (a VPC endpoint, or a
    /// local test double); requests are still signed for the region.
    pub fn with_endpoint(self, endpoint: Url) -> Self {
        Self {
            inner: self.inner.with_endpoint(endpoint),
        }
    }

    /// The region requests are signed for.
    pub fn region(&self) -> &str {
        self.inner.region()
    }

    /// `CreateChannel`: "Creates a new channel and an associated stream key
    /// to start streaming",
    /// <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_CreateChannel.html>.
    ///
    /// # Errors
    ///
    /// The [`AwsError`]; IVS lists `AccessDeniedException`,
    /// `PendingVerification`, `ResourceNotFoundException`,
    /// `ServiceQuotaExceededException` and `ValidationException`.
    pub async fn create_channel(
        &self,
        input: &CreateChannelInput,
    ) -> Result<CreatedChannel, AwsError> {
        let mut body = serde_json::Map::new();
        if let Some(name) = &input.name {
            body.insert("name".to_owned(), json!(name));
        }
        if let Some(t) = &input.channel_type {
            body.insert("type".to_owned(), json!(t));
        }
        if let Some(m) = &input.latency_mode {
            body.insert("latencyMode".to_owned(), json!(m));
        }
        body.insert("authorized".to_owned(), json!(input.authorized));
        body.insert("insecureIngest".to_owned(), json!(input.insecure_ingest));
        if let Some(arn) = &input.recording_configuration_arn {
            body.insert("recordingConfigurationArn".to_owned(), json!(arn));
        }
        if !input.tags.is_empty() {
            body.insert("tags".to_owned(), json!(input.tags));
        }
        let response = self
            .inner
            .post("CreateChannel", &Value::Object(body))
            .await?;
        decode(response)
    }

    /// `CreateStreamKey`: a new stream key for `channel_arn`,
    /// <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_CreateStreamKey.html>.
    ///
    /// # Errors
    ///
    /// The [`AwsError`].
    pub async fn create_stream_key(&self, channel_arn: &str) -> Result<StreamKey, AwsError> {
        let response = self
            .inner
            .post("CreateStreamKey", &json!({ "channelArn": channel_arn }))
            .await?;
        decode_field(response, "streamKey")
    }

    /// `GetStreamKey`: the stream key `arn`, value included,
    /// <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_GetStreamKey.html>.
    ///
    /// # Errors
    ///
    /// The [`AwsError`].
    pub async fn get_stream_key(&self, arn: &str) -> Result<StreamKey, AwsError> {
        let response = self
            .inner
            .post("GetStreamKey", &json!({ "arn": arn }))
            .await?;
        decode_field(response, "streamKey")
    }

    /// `GetStream`: "Gets information about the active (live) stream on a
    /// specified channel",
    /// <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_GetStream.html>.
    /// `None` when IVS answers `ChannelNotBroadcasting` ("The stream is
    /// offline for the given channel ARN").
    ///
    /// # Errors
    ///
    /// Any other [`AwsError`].
    pub async fn get_stream(&self, channel_arn: &str) -> Result<Option<Stream>, AwsError> {
        match self
            .inner
            .post("GetStream", &json!({ "channelArn": channel_arn }))
            .await
        {
            Ok(response) => decode_field(response, "stream").map(Some),
            Err(e) if e.code() == Some(&AwsErrorCode::ChannelNotBroadcasting) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// `StopStream`: "Disconnects the incoming RTMPS stream for the
    /// specified channel",
    /// <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_StopStream.html>.
    /// `false` when the channel was not broadcasting
    /// (`ChannelNotBroadcasting`).
    ///
    /// # Errors
    ///
    /// Any other [`AwsError`].
    pub async fn stop_stream(&self, channel_arn: &str) -> Result<bool, AwsError> {
        match self
            .inner
            .post("StopStream", &json!({ "channelArn": channel_arn }))
            .await
        {
            Ok(_) => Ok(true),
            Err(e) if e.code() == Some(&AwsErrorCode::ChannelNotBroadcasting) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// `DeleteChannel`: "Deletes the specified channel and its associated
    /// stream keys",
    /// <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_DeleteChannel.html>.
    ///
    /// # Errors
    ///
    /// The [`AwsError`]; "If you try to delete a live channel, you will
    /// get an error (409 `ConflictException`)".
    pub async fn delete_channel(&self, arn: &str) -> Result<(), AwsError> {
        self.inner
            .post("DeleteChannel", &json!({ "arn": arn }))
            .await
            .map(|_| ())
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, AwsError> {
    serde_json::from_value(value).map_err(|e| AwsError::Response(e.to_string()))
}

fn decode_field<T: serde::de::DeserializeOwned>(
    mut value: Value,
    field: &str,
) -> Result<T, AwsError> {
    match value.get_mut(field).map(Value::take) {
        Some(v) => decode(v),
        None => Err(AwsError::Response(format!("no {field} in the response"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The response example shape of CreateChannel (Response Syntax).
    #[test]
    fn created_channel_decodes() {
        let created: CreatedChannel = serde_json::from_value(json!({
            "channel": {
                "arn": "arn:aws:ivs:us-west-2:123456789012:channel/abcdABCDefgh",
                "authorized": false,
                "ingestEndpoint": "a1b2c3d4e5f6.global-contribute.live-video.net",
                "insecureIngest": false,
                "latencyMode": "LOW",
                "name": "channel-01",
                "playbackUrl": "https://a1b2c3d4e5f6.us-west-2.playback.live-video.net/api/video/v1/us-west-2.123456789012.channel.abcdEFGH.m3u8",
                "srt": {"endpoint": "a1b2c3d4e5f6.srt.live-video.net", "passphrase": "canary-srt-pass"},
                "tags": {},
                "type": "STANDARD"
            },
            "streamKey": {
                "arn": "arn:aws:ivs:us-west-2:123456789012:stream-key/g1H2I3j4k5L6",
                "channelArn": "arn:aws:ivs:us-west-2:123456789012:channel/abcdABCDefgh",
                "tags": {},
                "value": "sk_us-west-2_canary-key"
            }
        }))
        .unwrap();
        assert_eq!(created.channel.latency_mode, Some(LatencyMode::Low));
        assert_eq!(created.channel.channel_type, Some(ChannelType::Standard));
        let shown = format!("{created:?}");
        assert!(!shown.contains("canary-key"), "{shown}");
        assert!(!shown.contains("canary-srt-pass"), "{shown}");
    }

    #[test]
    fn region_is_checked() {
        let creds = Credentials::new("AKIDEXAMPLE", "secret");
        assert!(matches!(
            IvsClient::new(creds.clone(), "us-west-2/../evil"),
            Err(AwsError::Config(_))
        ));
        assert!(IvsClient::new(creds, "us-west-2").is_ok());
    }
}
