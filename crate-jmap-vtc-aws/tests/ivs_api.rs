//! The IVS API client against a local stub.
//!
//! Oracles: the Amazon IVS Low-Latency Streaming API Reference pages of
//! CreateChannel, CreateStreamKey, GetStreamKey, GetStream, StopStream and
//! DeleteChannel (request syntax `POST /<Action>` with a JSON body, the
//! response syntax, the Errors sections) and "Common Error Types",
//! <https://docs.aws.amazon.com/ivs/latest/LowLatencyAPIReference/CommonErrors.html>;
//! the SigV4 Authorization header form of "Create a signed AWS API
//! request", <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv-create-signed-request.html#add-signature-to-request>.

mod common;

use std::collections::BTreeMap;

use common::{
    CHANNEL_ARN, PLAYBACK_URL, STREAM_KEY, aws_error, body, client, create_channel_response,
    header, on, sent,
};
use jmap_vtc_aws::ivs::{ChannelType, CreateChannelInput, LatencyMode, StreamHealth, StreamState};
use jmap_vtc_aws::{AwsError, AwsErrorCode, Credentials};
use serde_json::json;
use wiremock::{MockServer, ResponseTemplate};

/// CreateChannel: `POST /CreateChannel`, `Content-type: application/json`,
/// the documented body fields; SigV4 header form with the `ivs` scope.
#[tokio::test]
async fn create_channel_request_and_response() {
    let server = MockServer::start().await;
    on(
        &server,
        "CreateChannel",
        ResponseTemplate::new(200).set_body_json(create_channel_response(false)),
    )
    .await;
    let input = CreateChannelInput {
        name: Some("jmap-vtc-L1".to_owned()),
        channel_type: Some(ChannelType::Basic),
        latency_mode: Some(LatencyMode::Low),
        tags: BTreeMap::from([("team".to_owned(), "media".to_owned())]),
        ..CreateChannelInput::default()
    };
    let created = client(&server).create_channel(&input).await.unwrap();
    assert_eq!(created.channel.arn, CHANNEL_ARN);
    assert_eq!(created.channel.playback_url, PLAYBACK_URL);
    assert_eq!(created.stream_key.value, STREAM_KEY);

    let reqs = sent(&server, "CreateChannel").await;
    assert_eq!(reqs.len(), 1);
    let r = &reqs[0];
    assert_eq!(header(r, "content-type"), "application/json");
    assert_eq!(
        body(r),
        json!({
            "name": "jmap-vtc-L1",
            "type": "BASIC",
            "latencyMode": "LOW",
            "authorized": false,
            "insecureIngest": false,
            "tags": {"team": "media"}
        })
    );
    // "Authorization: AWS4-HMAC-SHA256 Credential=<key>/<date>/<region>/<service>/aws4_request,
    // SignedHeaders=..., Signature=<64 hex>"
    let auth = header(r, "authorization");
    let rest = auth
        .strip_prefix("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/")
        .unwrap_or_else(|| panic!("{auth}"));
    let (scope, rest) = rest.split_once(", SignedHeaders=").unwrap();
    assert!(scope.ends_with("/us-west-2/ivs/aws4_request"), "{scope}");
    assert_eq!(scope.len(), "YYYYMMDD/us-west-2/ivs/aws4_request".len());
    let (signed, signature) = rest.split_once(", Signature=").unwrap();
    assert_eq!(signed, "content-type;host;x-amz-date");
    assert_eq!(signature.len(), 64);
    assert!(
        signature
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert_eq!(header(r, "x-amz-date").len(), "20150830T123600Z".len());
    assert!(r.headers.get("x-amz-security-token").is_none());
}

/// "When you use temporary security credentials, you must add
/// `X-Amz-Security-Token`"; it is signed with the other `x-amz-*` headers.
#[tokio::test]
async fn session_token_is_sent_and_signed() {
    let server = MockServer::start().await;
    on(&server, "DeleteChannel", ResponseTemplate::new(204)).await;
    let ivs = jmap_vtc_aws::ivs::IvsClient::new(
        Credentials::new("AKIDEXAMPLE", "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY")
            .with_session_token("AQoDYXdzEPT//////////wEXAMPLE"),
        "us-west-2",
    )
    .unwrap()
    .with_endpoint(jmap_vtc_aws::url::Url::parse(&format!("{}/", server.uri())).unwrap());
    ivs.delete_channel(CHANNEL_ARN).await.unwrap();
    let r = &sent(&server, "DeleteChannel").await[0];
    assert_eq!(
        header(r, "x-amz-security-token"),
        "AQoDYXdzEPT//////////wEXAMPLE"
    );
    assert!(
        header(r, "authorization")
            .contains("SignedHeaders=content-type;host;x-amz-date;x-amz-security-token,"),
        "{}",
        header(r, "authorization")
    );
    assert_eq!(body(r), json!({"arn": CHANNEL_ARN}));
}

/// ServiceQuotaExceededException is HTTP 402; the code comes from
/// `x-amzn-ErrorType` and the message from `exceptionMessage`.
#[tokio::test]
async fn service_errors_are_typed() {
    let server = MockServer::start().await;
    on(
        &server,
        "CreateChannel",
        aws_error(
            402,
            "ServiceQuotaExceededException:http://internal.amazon.com/coral/com.amazonaws.ivs/",
            "Channel quota exceeded",
        ),
    )
    .await;
    let err = client(&server)
        .create_channel(&CreateChannelInput::default())
        .await
        .unwrap_err();
    let AwsError::Service {
        status,
        code,
        message,
    } = &err
    else {
        panic!("{err:?}");
    };
    assert_eq!(*status, 402);
    assert_eq!(*code, AwsErrorCode::ServiceQuotaExceeded);
    assert_eq!(message.as_deref(), Some("Channel quota exceeded"));
}

/// GetStream: the Stream object; `ChannelNotBroadcasting` (404) is "The
/// stream is offline for the given channel ARN", so `None`.
#[tokio::test]
async fn get_stream_live_and_offline() {
    let server = MockServer::start().await;
    on(
        &server,
        "GetStream",
        ResponseTemplate::new(200).set_body_json(json!({
            "stream": {
                "channelArn": CHANNEL_ARN,
                "health": "STARVING",
                "playbackUrl": PLAYBACK_URL,
                "startTime": "2026-11-20T17:02:11Z",
                "state": "LIVE",
                "streamId": "st-1A2b3c4D5e6F78ghij9Klmn",
                "viewerCount": 12
            }
        })),
    )
    .await;
    let stream = client(&server)
        .get_stream(CHANNEL_ARN)
        .await
        .unwrap()
        .expect("live");
    assert_eq!(stream.state, StreamState::Live);
    assert_eq!(stream.health, StreamHealth::Starving);
    assert_eq!(stream.viewer_count, Some(12));
    assert_eq!(
        body(&sent(&server, "GetStream").await[0]),
        json!({"channelArn": CHANNEL_ARN})
    );

    let offline = MockServer::start().await;
    on(
        &offline,
        "GetStream",
        aws_error(404, "ChannelNotBroadcasting", "offline"),
    )
    .await;
    assert!(
        client(&offline)
            .get_stream(CHANNEL_ARN)
            .await
            .unwrap()
            .is_none()
    );
}

/// StopStream: HTTP 200 with an empty body; `ChannelNotBroadcasting` is
/// not an error for a stop.
#[tokio::test]
async fn stop_stream() {
    let server = MockServer::start().await;
    on(&server, "StopStream", ResponseTemplate::new(200)).await;
    assert!(client(&server).stop_stream(CHANNEL_ARN).await.unwrap());

    let offline = MockServer::start().await;
    on(
        &offline,
        "StopStream",
        aws_error(404, "ChannelNotBroadcasting", "offline"),
    )
    .await;
    assert!(!client(&offline).stop_stream(CHANNEL_ARN).await.unwrap());
}

/// CreateStreamKey and GetStreamKey answer `{"streamKey": {...}}`; the key
/// value never shows in `Debug`.
#[tokio::test]
async fn stream_keys() {
    let server = MockServer::start().await;
    let key = json!({"streamKey": {
        "arn": "arn:aws:ivs:us-west-2:123456789012:stream-key/g1H2I3j4k5L6",
        "channelArn": CHANNEL_ARN,
        "tags": {},
        "value": STREAM_KEY
    }});
    on(
        &server,
        "CreateStreamKey",
        ResponseTemplate::new(200).set_body_json(key.clone()),
    )
    .await;
    on(
        &server,
        "GetStreamKey",
        ResponseTemplate::new(200).set_body_json(key),
    )
    .await;
    let ivs = client(&server);
    let created = ivs.create_stream_key(CHANNEL_ARN).await.unwrap();
    assert_eq!(created.value, STREAM_KEY);
    assert!(!format!("{created:?}").contains(STREAM_KEY));
    let got = ivs.get_stream_key(&created.arn).await.unwrap();
    assert_eq!(got.channel_arn, CHANNEL_ARN);
    assert_eq!(
        body(&sent(&server, "CreateStreamKey").await[0]),
        json!({"channelArn": CHANNEL_ARN})
    );
    assert_eq!(
        body(&sent(&server, "GetStreamKey").await[0]),
        json!({"arn": created.arn})
    );
}

/// A 2xx body that is not the documented shape is `Response`.
#[tokio::test]
async fn malformed_success_is_a_response_error() {
    let server = MockServer::start().await;
    on(
        &server,
        "CreateStreamKey",
        ResponseTemplate::new(200).set_body_json(json!({"unexpected": true})),
    )
    .await;
    let err = client(&server)
        .create_stream_key(CHANNEL_ARN)
        .await
        .unwrap_err();
    assert!(matches!(err, AwsError::Response(_)), "{err:?}");
}
