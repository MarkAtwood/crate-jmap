//! Delivery tests against a local HTTP stub.
//!
//! Oracles: RFC 8620 §7.2 / §7.2.2 (request shape, the PushVerification
//! example), RFC 8030 §5 / §7.3 (headers, status codes), RFC 8291 (body
//! format, and decryption by web-push-native, an independent
//! implementation, with the RFC 8291 §5 receiver keys).

use std::net::TcpListener;
use std::num::NonZeroU32;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jmap_web_push::jmap_types::{PushSubscription, PushVerification};
use jmap_web_push::p256::SecretKey;
use jmap_web_push::p256::ecdsa::SigningKey;
use jmap_web_push::url::Url;
use jmap_web_push::{
    EncryptError, Endpoint, EndpointError, Outcome, PushOptions, Retry, SendError, Sender, Topic,
    Urgency, Vapid,
};
use serde_json::{Value, json};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// RFC 8291 §5, "Receiver: public key" and "private key", and the
/// "Authentication Secret".
const UA_PUBLIC: &str =
    "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
const UA_PRIVATE: &str = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";

fn b64(s: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(s).unwrap()
}

fn sender() -> Sender {
    // A fixed signing key that is not one of the RFC 8291 ECDH keys.
    let key = SigningKey::from_slice(&[7u8; 32]).unwrap();
    let mut retry = Retry::default();
    retry.attempts = NonZeroU32::new(3).unwrap();
    retry.base_delay = Duration::from_millis(1);
    retry.max_delay = Duration::from_secs(1);
    Sender::new(Vapid::new(key, "mailto:ops@example.com"))
        .unwrap()
        .with_retry(retry)
}

fn subscription(url: &str, keys: bool) -> PushSubscription {
    let mut sub = json!({ "deviceClientId": "a889-ffea-910", "url": url });
    if keys {
        sub["keys"] = json!({ "p256dh": UA_PUBLIC, "auth": AUTH });
    }
    serde_json::from_value(sub).unwrap()
}

fn endpoint(server: &MockServer, keys: bool) -> Endpoint {
    Endpoint::try_from(&subscription(&format!("{}/push/abc", server.uri()), keys)).unwrap()
}

fn options() -> PushOptions {
    PushOptions::new(Duration::from_secs(60))
}

async fn reply(server: &MockServer, status: u16, times: u64) {
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(status))
        .up_to_n_times(times)
        .mount(server)
        .await;
}

async fn requests(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap()
}

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers.get(name).map(|v| v.to_str().unwrap())
}

/// RFC 8620 §7.2: "The POST request MUST have a content type of
/// "application/json" ... The request MUST have a "TTL" header and MAY
/// have "Urgency" and/or "Topic" headers". With `keys`, RFC 8291 §4: one
/// record whose "keyid" is "a 65-octet sequence that starts with a 0x04
/// octet", and "The Content-Encoding header field therefore has exactly
/// one value, which is "aes128gcm"."
#[tokio::test]
async fn encrypted_push_is_accepted_and_decrypts() {
    let server = MockServer::start().await;
    reply(&server, 201, 1).await;
    let payload =
        json!({ "@type": "StateChange", "changed": { "u1": { "Email": "d35ecb040aab" } } });
    let mut opts = options();
    opts.urgency = Some(Urgency::High);
    opts.topic = Some("chat-u1".parse().unwrap());

    let outcome = sender()
        .send(&endpoint(&server, true), &payload, &opts)
        .await
        .unwrap();

    assert_eq!(outcome, Outcome::Accepted);
    let reqs = requests(&server).await;
    assert_eq!(reqs.len(), 1);
    let req = &reqs[0];
    assert_eq!(req.url.path(), "/push/abc");
    assert_eq!(header(req, "content-type"), Some("application/json"));
    assert_eq!(header(req, "content-encoding"), Some("aes128gcm"));
    assert_eq!(header(req, "ttl"), Some("60"));
    assert_eq!(header(req, "urgency"), Some("high"));
    assert_eq!(header(req, "topic"), Some("chat-u1"));
    assert!(
        header(req, "authorization")
            .unwrap()
            .starts_with("vapid t=")
    );

    let body = &req.body;
    assert_eq!(&body[16..20], &4096u32.to_be_bytes(), "rs");
    assert_eq!(body[20], 65, "idlen");
    assert_eq!(body[21], 0x04, "uncompressed point");

    let ua_secret = SecretKey::from_slice(&b64(UA_PRIVATE)).unwrap();
    let auth = web_push_native::Auth::clone_from_slice(&b64(AUTH));
    let plaintext = web_push_native::decrypt(body.clone(), &ua_secret, &auth).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&plaintext).unwrap(),
        payload
    );
}

/// RFC 8620 §7.2.2: "When a PushSubscription is created, the server MUST
/// immediately push a *PushVerification* object to the URL." The body is
/// the §7.2.2 example. With no `keys` the JSON goes out as is.
#[tokio::test]
async fn push_verification_unencrypted() {
    let server = MockServer::start().await;
    reply(&server, 201, 1).await;
    let example = json!({
        "@type": "PushVerification",
        "pushSubscriptionId": "P43dcfa4-1dd4-41ef-9156-2c89b3b19c60",
        "verificationCode": "da1f097b11ca17f06424e30bf02bfa67"
    });
    let verification: PushVerification = serde_json::from_value(example.clone()).unwrap();

    let outcome = sender()
        .send(&endpoint(&server, false), &verification, &options())
        .await
        .unwrap();

    assert_eq!(outcome, Outcome::Accepted);
    let reqs = requests(&server).await;
    let req = &reqs[0];
    assert_eq!(header(req, "content-type"), Some("application/json"));
    assert_eq!(header(req, "content-encoding"), None);
    assert_eq!(header(req, "ttl"), Some("60"));
    assert_eq!(header(req, "urgency"), None);
    assert_eq!(header(req, "topic"), None);
    assert_eq!(serde_json::from_slice::<Value>(&req.body).unwrap(), example);
}

/// RFC 8030 §7.3: "A push service MUST return a 404 (Not Found) status
/// code if an application server attempts to send a push message to an
/// expired push message subscription." 404 and 410 both mean gone, and
/// are not retried.
#[tokio::test]
async fn not_found_and_gone_mean_gone() {
    for status in [404, 410] {
        let server = MockServer::start().await;
        reply(&server, status, 10).await;
        let outcome = sender()
            .send(&endpoint(&server, true), &json!({}), &options())
            .await
            .unwrap();
        assert_eq!(outcome, Outcome::Gone, "{status}");
        assert_eq!(requests(&server).await.len(), 1, "{status}");
    }
}

/// A 5xx is retried with backoff until the push service accepts.
#[tokio::test]
async fn server_error_is_retried() {
    let server = MockServer::start().await;
    reply(&server, 500, 2).await;
    reply(&server, 201, 1).await;

    let outcome = sender()
        .send(&endpoint(&server, true), &json!({}), &options())
        .await
        .unwrap();

    assert_eq!(outcome, Outcome::Accepted);
    assert_eq!(requests(&server).await.len(), 3);
}

/// Retries stop after `Retry::attempts`.
#[tokio::test]
async fn retries_are_bounded() {
    let server = MockServer::start().await;
    reply(&server, 503, 10).await;

    let err = sender()
        .send(&endpoint(&server, true), &json!({}), &options())
        .await
        .unwrap_err();

    assert!(matches!(err, SendError::Unavailable { status, retry_after: None } if status == 503));
    assert_eq!(requests(&server).await.len(), 3);
}

/// RFC 8030 §8.4: a 429 "SHOULD also include a Retry-After header ... to
/// indicate how long the application server is requested to wait before
/// it makes another request". A short wait is honoured and retried.
#[tokio::test]
async fn too_many_requests_honours_short_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    reply(&server, 201, 1).await;

    let outcome = sender()
        .send(&endpoint(&server, true), &json!({}), &options())
        .await
        .unwrap();

    assert_eq!(outcome, Outcome::Accepted);
    assert_eq!(requests(&server).await.len(), 2);
}

/// A Retry-After past `Retry::max_delay` is not waited out: the send
/// stops and hands the delay to the caller.
#[tokio::test]
async fn too_many_requests_returns_long_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "3600"))
        .mount(&server)
        .await;

    let err = sender()
        .send(&endpoint(&server, true), &json!({}), &options())
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        SendError::Unavailable { status, retry_after: Some(d) }
            if status == 429 && d == Duration::from_secs(3600)
    ));
    assert_eq!(requests(&server).await.len(), 1);
}

/// Other 4xx responses are not retried. RFC 8292 §2: "A push service MAY
/// reject a request with a 403 (Forbidden) status code [RFC7231] if the
/// JWT signature or its claims are invalid" -- a credential problem, not
/// a gone subscription.
#[tokio::test]
async fn client_errors_are_rejected_without_retry() {
    for status in [400, 403, 413] {
        let server = MockServer::start().await;
        reply(&server, status, 10).await;
        let err = sender()
            .send(&endpoint(&server, true), &json!({}), &options())
            .await
            .unwrap_err();
        assert!(
            matches!(err, SendError::Rejected { status: s } if s == status),
            "{status}"
        );
        assert_eq!(requests(&server).await.len(), 1, "{status}");
    }
}

/// A redirect is not followed: the push goes only to the URL the caller
/// checked.
#[tokio::test]
async fn redirect_is_not_followed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(307).insert_header("Location", "http://169.254.169.254/"),
        )
        .mount(&server)
        .await;

    let err = sender()
        .send(&endpoint(&server, true), &json!({}), &options())
        .await
        .unwrap_err();

    assert!(matches!(err, SendError::Rejected { status } if status == 307));
    assert_eq!(requests(&server).await.len(), 1);
}

/// A connection failure is retried, then reported.
#[tokio::test]
async fn transport_error_after_retries() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let endpoint = Endpoint::new(
        Url::parse(&format!("http://127.0.0.1:{port}/push")).unwrap(),
        None,
    );

    let err = sender()
        .send(&endpoint, &json!({}), &options())
        .await
        .unwrap_err();

    assert!(matches!(err, SendError::Transport(_)));
}

/// RFC 8291 §4: "at most, 3993 octets of plaintext." A larger payload is
/// refused before any request.
#[tokio::test]
async fn oversized_payload_is_refused_before_sending() {
    let server = MockServer::start().await;
    reply(&server, 201, 10).await;
    // `"` + 3992 octets + `"` serializes to 3994 octets.
    let payload = "a".repeat(3992);

    for keys in [true, false] {
        let err = sender()
            .send(&endpoint(&server, keys), &payload, &options())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            SendError::Encrypt(EncryptError::TooLarge { len: 3994 })
        ));
    }
    assert!(requests(&server).await.is_empty());
}

/// RFC 8030 §5.4: "the Topic header field MUST be restricted to no more
/// than 32 characters from the URL and a filename-safe Base 64 alphabet
/// [RFC4648]."
#[test]
fn topic_is_at_most_32_base64url_characters() {
    assert!("ABCXYZabcxyz0189-_ABCXYZabcxyz01".parse::<Topic>().is_ok());
    assert!("a".repeat(33).parse::<Topic>().is_err());
    assert!("a+b".parse::<Topic>().is_err());
    assert!("a/b".parse::<Topic>().is_err());
    assert!("a=".parse::<Topic>().is_err());
    assert!("".parse::<Topic>().is_err());
}

/// RFC 8620 §7.2 `keys`: "p256dh ... The P-256 Elliptic Curve
/// Diffie-Hellman (ECDH) public key ... encoded in URL-safe base64";
/// RFC 8291 §3.2: the auth secret is "a hard-to-guess sequence of 16
/// octets".
#[test]
fn endpoint_from_subscription_checks_keys() {
    let url = "https://example.com/push/?device=X8980fc&client=12c6d086";
    assert!(Endpoint::try_from(&subscription(url, true)).is_ok());
    assert!(Endpoint::try_from(&subscription(url, false)).is_ok());

    let mut no_url = subscription(url, true);
    no_url.url = None;
    assert!(matches!(
        Endpoint::try_from(&no_url),
        Err(EndpointError::MissingUrl)
    ));

    let mut sub = subscription(url, true);
    sub.keys.as_mut().unwrap().auth = URL_SAFE_NO_PAD.encode([0u8; 15]);
    assert!(matches!(Endpoint::try_from(&sub), Err(EndpointError::Auth)));

    let mut sub = subscription(url, true);
    sub.keys.as_mut().unwrap().p256dh = URL_SAFE_NO_PAD.encode([4u8; 65]);
    assert!(matches!(
        Endpoint::try_from(&sub),
        Err(EndpointError::P256dh)
    ));

    let mut padded = subscription(url, true);
    padded.keys.as_mut().unwrap().auth = format!("{AUTH}==");
    assert!(Endpoint::try_from(&padded).is_ok());
}
