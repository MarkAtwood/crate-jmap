//! Shared fixtures: an IVS client against a local wiremock server with the
//! AWS documentation's dummy credentials. No real AWS endpoint or account
//! is ever contacted.

#![allow(dead_code)]

use jmap_vtc_aws::Credentials;
use jmap_vtc_aws::ivs::IvsClient;
use jmap_vtc_aws::url::Url;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

pub const REGION: &str = "us-west-2";
pub const CHANNEL_ARN: &str = "arn:aws:ivs:us-west-2:123456789012:channel/abcdABCDefgh";
pub const PLAYBACK_URL: &str = "https://a1b2c3d4e5f6.us-west-2.playback.live-video.net/api/video/v1/us-west-2.123456789012.channel.abcdEFGH.m3u8";
pub const INGEST: &str = "a1b2c3d4e5f6.global-contribute.live-video.net";
pub const STREAM_KEY: &str = "sk_us-west-2_abcdABCDefgh567890";

/// The AWS signing test suite's example access key (not a real key).
pub fn credentials() -> Credentials {
    Credentials::new("AKIDEXAMPLE", "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY")
}

pub fn client(server: &MockServer) -> IvsClient {
    IvsClient::new(credentials(), REGION)
        .unwrap()
        .with_endpoint(Url::parse(&format!("{}/", server.uri())).unwrap())
}

/// The CreateChannel response, shaped after its Response Syntax
/// (<https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/API_CreateChannel.html#API_CreateChannel_ResponseSyntax>).
pub fn create_channel_response(authorized: bool) -> Value {
    json!({
        "channel": {
            "arn": CHANNEL_ARN,
            "authorized": authorized,
            "containerFormat": "TS",
            "ingestEndpoint": INGEST,
            "insecureIngest": false,
            "latencyMode": "LOW",
            "name": "jmap-vtc-L1",
            "playbackRestrictionPolicyArn": "",
            "playbackUrl": PLAYBACK_URL,
            "preset": "",
            "recordingConfigurationArn": "",
            "tags": {},
            "type": "STANDARD"
        },
        "streamKey": {
            "arn": "arn:aws:ivs:us-west-2:123456789012:stream-key/g1H2I3j4k5L6",
            "channelArn": CHANNEL_ARN,
            "tags": {},
            "value": STREAM_KEY
        }
    })
}

/// Answer `POST /{action}` with `response`.
pub async fn on(server: &MockServer, action: &str, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path(format!("/{action}")))
        .respond_with(response)
        .mount(server)
        .await;
}

/// An IVS error response: the `x-amzn-ErrorType` header and the
/// `exceptionMessage` body of the REST-JSON protocol.
pub fn aws_error(status: u16, code: &str, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("x-amzn-ErrorType", code)
        .set_body_json(json!({ "exceptionMessage": message }))
}

/// The requests sent to `POST /{action}`.
pub async fn sent(server: &MockServer, action: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == format!("/{action}"))
        .collect()
}

pub fn header(r: &Request, name: &str) -> String {
    r.headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

pub fn body(r: &Request) -> Value {
    serde_json::from_slice(&r.body).unwrap()
}
