//! APNs delivery against a local stub.
//!
//! Oracles: jmap-push-platform-guide §Apple APNs (delivery modes, urgency
//! table, error responses); jmap-chat-push-platform-guide §Payload
//! encoding, APNs; jmap-vtc-ring-push-guide §APNs request format; Apple's
//! "Sending notification requests to APNs" (headers, push types, sizes),
//! "Handling notification responses from APNs" (status codes and `reason`
//! strings) and "Establishing a token-based connection to APNs" (the
//! provider token).

mod common;

use std::time::Duration;

use common::{ec_key, header, state_change, verify_es256};
use jmap_platform_push::apns::{ApnsKey, ApnsPusher, Environment, MAX_PAYLOAD};
use jmap_platform_push::url::Url;
use jmap_platform_push::{Outcome, PlatformPusher, PushError, PushKind, Urgency};
use serde_json::{Value, json};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const DEVICE: &str = "00fc13adff785122b4ad28809a3420982341241421348097878e577c991de8f0";

fn pusher_with(server: &MockServer, client: reqwest::Client) -> (ApnsPusher, Vec<u8>) {
    let (pem, public) = ec_key();
    let key = ApnsKey::from_p8(&pem, "ABC123DEFG", "DEF123GHIJ").unwrap();
    let pusher = ApnsPusher::from_client(client, key, "com.example.app", Environment::Sandbox)
        .with_endpoint(&Url::parse(&server.uri()).unwrap());
    (pusher, public)
}

/// An HTTP/2 (prior knowledge, h2c) client: APNs speaks only HTTP/2.
fn pusher(server: &MockServer) -> (ApnsPusher, Vec<u8>) {
    let client = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()
        .unwrap();
    pusher_with(server, client)
}

async fn reply(server: &MockServer, template: ResponseTemplate, times: u64) {
    Mock::given(method("POST"))
        .respond_with(template)
        .up_to_n_times(times)
        .mount(server)
        .await;
}

fn reason(status: u16, reason: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "reason": reason }))
}

async fn requests(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap()
}

/// Background push: `apns-push-type: background`, "Always use priority
/// `5`", `"content-available": 1` and no `alert`; the payload inline under
/// `"jmap-push"`; the provider token `{"alg":"ES256","kid"}` over
/// `{"iss","iat"}`; `apns-topic` the bundle ID; path `/3/device/<token>`.
#[tokio::test]
async fn background_push_over_http2() {
    let server = MockServer::start().await;
    reply(
        &server,
        ResponseTemplate::new(200).insert_header("apns-id", "eabeae54-14a8-11e5-b60b-1697f925ec7b"),
        1,
    )
    .await;
    let (pusher, public) = pusher(&server);

    let outcome = pusher.push(DEVICE, &state_change()).await;
    let Outcome::Delivered(receipt) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(
        receipt.message_id.as_deref(),
        Some("eabeae54-14a8-11e5-b60b-1697f925ec7b")
    );

    let reqs = requests(&server).await;
    let req = &reqs[0];
    assert_eq!(req.url.path(), format!("/3/device/{DEVICE}"));
    assert_eq!(header(req, "apns-topic"), Some("com.example.app"));
    assert_eq!(header(req, "apns-push-type"), Some("background"));
    assert_eq!(header(req, "apns-priority"), Some("5"));
    let expiration: u64 = header(req, "apns-expiration").unwrap().parse().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!((now + 3590..=now + 3610).contains(&expiration));

    let auth = header(req, "authorization").unwrap();
    let jwt = auth.strip_prefix("bearer ").unwrap();
    let (jwt_header, claims) = verify_es256(jwt, &public);
    assert_eq!(jwt_header, json!({ "alg": "ES256", "kid": "ABC123DEFG" }));
    assert_eq!(claims["iss"], "DEF123GHIJ");
    assert!(claims["iat"].as_u64().unwrap().abs_diff(now) < 10);

    let body: Value = req.body_json().unwrap();
    assert_eq!(
        body,
        json!({
            "aps": { "content-available": 1 },
            "jmap-push": { "@type": "StateChange", "changed": { "u1": { "Email": "d35ecb040aab" } } },
        })
    );
}

/// Alert push (guide §Delivery modes): `"alert": {"body": ...}` and
/// `"mutable-content": 1`; "high" is priority 10, "normal" priority 5, and
/// "low" stays a background push (§Urgency).
#[tokio::test]
async fn alert_push_sets_mutable_content() {
    let server = MockServer::start().await;
    reply(&server, ResponseTemplate::new(200), 3).await;
    let (pusher, _) = pusher(&server);
    let alert = PushKind::Alert {
        body: "New message".into(),
    };
    for urgency in [Urgency::High, Urgency::Normal, Urgency::Low] {
        let m = state_change()
            .with_kind(alert.clone())
            .with_urgency(urgency);
        assert!(matches!(
            pusher.push(DEVICE, &m).await,
            Outcome::Delivered(_)
        ));
    }
    let reqs = requests(&server).await;
    let seen: Vec<(&str, &str)> = reqs
        .iter()
        .map(|r| {
            (
                header(r, "apns-push-type").unwrap(),
                header(r, "apns-priority").unwrap(),
            )
        })
        .collect();
    assert_eq!(seen, [("alert", "10"), ("alert", "5"), ("background", "5")]);
    let body: Value = reqs[0].body_json().unwrap();
    assert_eq!(
        body["aps"],
        json!({ "alert": { "body": "New message" }, "mutable-content": 1 })
    );
    assert_eq!(
        reqs[2].body_json::<Value>().unwrap()["aps"],
        json!({ "content-available": 1 })
    );
}

/// jmap-vtc-ring-push-guide §APNs request format: `apns-push-type: voip`,
/// "The `apns-topic` for VoIP push MUST use the `.voip` suffix", priority
/// 10, `"aps": {}` and the call under `"jmap-vtc-push"`; a zero TTL is
/// `apns-expiration: 0` (Apple: "attempts to deliver the notification only
/// once and doesn't store it").
#[tokio::test]
async fn voip_push_uses_the_voip_topic() {
    let server = MockServer::start().await;
    reply(&server, ResponseTemplate::new(200), 1).await;
    let (pusher, _) = pusher(&server);
    let ring = jmap_platform_push::Message::new(
        &json!({ "@type": "VTCCallPush", "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0" }),
        Duration::ZERO,
    )
    .unwrap()
    .with_kind(PushKind::Voip)
    .with_data_key("jmap-vtc-push");

    assert!(matches!(
        pusher.push(DEVICE, &ring).await,
        Outcome::Delivered(_)
    ));
    let req = &requests(&server).await[0];
    assert_eq!(header(req, "apns-topic"), Some("com.example.app.voip"));
    assert_eq!(header(req, "apns-push-type"), Some("voip"));
    assert_eq!(header(req, "apns-priority"), Some("10"));
    assert_eq!(header(req, "apns-expiration"), Some("0"));
    let body: Value = req.body_json().unwrap();
    assert_eq!(body["aps"], json!({}));
    assert_eq!(
        body["jmap-vtc-push"]["callId"],
        "01J4XKZQN4MWVT8PPBEHTJ3AB0"
    );
}

/// Guide §Apple APNs, Error responses: `BadDeviceToken`,
/// `DeviceTokenNotForTopic`, `Unregistered` and `ExpiredToken` destroy the
/// subscription; Apple lists the first two as 400, the others as 410.
#[tokio::test]
async fn device_token_reasons_are_gone() {
    for (status, r) in [
        (400, "BadDeviceToken"),
        (400, "DeviceTokenNotForTopic"),
        (410, "Unregistered"),
        (410, "ExpiredToken"),
    ] {
        let server = MockServer::start().await;
        reply(&server, reason(status, r), 1).await;
        let (pusher, _) = pusher(&server);
        let outcome = pusher.push(DEVICE, &state_change()).await;
        let Outcome::Gone(e) = outcome else {
            panic!("{r}: {outcome:?}");
        };
        assert_eq!(e.reason.as_deref(), Some(r));
    }
}

/// Credential and configuration errors never destroy: `BadTopic` ("Servers
/// MUST NOT destroy the `PushSubscription` in this case"),
/// `InvalidProviderToken`, `MissingProviderToken`, `TopicDisallowed`,
/// `PayloadTooLarge`.
#[tokio::test]
async fn credential_and_config_reasons_fail_without_destroy() {
    for (status, r) in [
        (400, "BadTopic"),
        (403, "InvalidProviderToken"),
        (403, "MissingProviderToken"),
        (400, "TopicDisallowed"),
        (413, "PayloadTooLarge"),
    ] {
        let server = MockServer::start().await;
        reply(&server, reason(status, r), 5).await;
        let (pusher, _) = pusher(&server);
        let outcome = pusher.push(DEVICE, &state_change()).await;
        assert!(
            matches!(outcome, Outcome::Failed(PushError::Platform(ref e)) if e.reason.as_deref() == Some(r)),
            "{r}: {outcome:?}"
        );
        assert_eq!(requests(&server).await.len(), 1, "{r}");
    }
}

/// `ExpiredProviderToken` (403): "The provider token is stale and a new
/// token should be generated." One new token and one resend; a second
/// rejection (a clock or key problem) fails, never destroyed.
#[tokio::test]
async fn expired_provider_token_is_regenerated_once() {
    let server = MockServer::start().await;
    reply(&server, reason(403, "ExpiredProviderToken"), 1).await;
    reply(&server, ResponseTemplate::new(200), 1).await;
    let (first, _) = pusher(&server);
    assert!(matches!(
        first.push(DEVICE, &state_change()).await,
        Outcome::Delivered(_)
    ));
    assert_eq!(requests(&server).await.len(), 2);

    let server = MockServer::start().await;
    reply(&server, reason(403, "ExpiredProviderToken"), 5).await;
    let (second, _) = pusher(&server);
    let outcome = second.push(DEVICE, &state_change()).await;
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    assert_eq!(requests(&server).await.len(), 2);
}

/// The provider token is reused ("Refresh your token no more than once
/// every 20 minutes").
#[tokio::test]
async fn provider_token_is_reused() {
    let server = MockServer::start().await;
    reply(&server, ResponseTemplate::new(200), 3).await;
    let (pusher, _) = pusher(&server);
    for _ in 0..3 {
        assert!(matches!(
            pusher.push(DEVICE, &state_change()).await,
            Outcome::Delivered(_)
        ));
    }
    let reqs = requests(&server).await;
    let first = header(&reqs[0], "authorization");
    assert!(reqs.iter().all(|r| header(r, "authorization") == first));
}

/// `TooManyRequests` (429): "You can retry with a delay"; 5xx: "you can
/// retry". `Retry-After` is passed on.
#[tokio::test]
async fn rate_limit_and_server_errors_are_retried() {
    for (status, r) in [
        (429, "TooManyRequests"),
        (500, "InternalServerError"),
        (503, "ServiceUnavailable"),
        (503, "Shutdown"),
    ] {
        let server = MockServer::start().await;
        reply(
            &server,
            reason(status, r).insert_header("Retry-After", "30"),
            1,
        )
        .await;
        let (pusher, _) = pusher(&server);
        let outcome = pusher.push(DEVICE, &state_change()).await;
        let Outcome::Retry { after, .. } = outcome else {
            panic!("{r}: {outcome:?}");
        };
        assert_eq!(after, Some(Duration::from_secs(30)));
    }
}

/// A device token is "the hexadecimal bytes"; anything else would change
/// the request path, so it is refused unsent. A body over 4096 octets is
/// refused too ("limited to a maximum size of 4 KB (4096 bytes)").
#[tokio::test]
async fn bad_token_and_oversized_payload_are_refused_unsent() {
    let server = MockServer::start().await;
    reply(&server, ResponseTemplate::new(200), 5).await;
    let (pusher, _) = pusher(&server);
    for token in ["", "../../x", "abc/def", "zz"] {
        assert!(
            matches!(
                pusher.push(token, &state_change()).await,
                Outcome::Failed(PushError::InvalidDestination)
            ),
            "{token}"
        );
    }
    let big = jmap_platform_push::Message::new(&"x".repeat(MAX_PAYLOAD), Duration::ZERO).unwrap();
    assert!(matches!(
        pusher.push(DEVICE, &big).await,
        Outcome::Failed(PushError::TooLarge { max: 4096, .. })
    ));
    assert!(requests(&server).await.is_empty());
}

#[test]
fn key_debug_is_redacted_and_bad_keys_are_errors() {
    let (pem, _) = ec_key();
    let key = ApnsKey::from_p8(&pem, "ABC123DEFG", "DEF123GHIJ").unwrap();
    let shown = format!("{key:?}");
    assert!(shown.contains("ABC123DEFG"));
    assert!(!shown.contains(&pem[30..60]));
    assert!(ApnsKey::from_p8("nope", "k", "t").is_err());
    // An RSA key is not a P-256 key.
    assert!(ApnsKey::from_p8(&common::rsa_key().0, "k", "t").is_err());
}

#[test]
fn new_builds_an_http2_client() {
    let (pem, _) = ec_key();
    let key = ApnsKey::from_p8(&pem, "k", "t").unwrap();
    assert!(ApnsPusher::new(key, "com.example.app", Environment::Production).is_ok());
}
