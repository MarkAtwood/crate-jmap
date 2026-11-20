//! FCM HTTP v1 delivery against a local stub.
//!
//! Oracles: jmap-push-platform-guide §Google FCM; the FCM v1 reference
//! (<https://firebase.google.com/docs/reference/fcm/rest/v1/projects.messages>,
//! Message and AndroidConfig); the FCM ErrorCode reference and the
//! error-codes page §REST error codes for the HTTP v1 API (status codes and
//! body shape, Example 2); Google's service-account OAuth guide §HTTP/REST
//! (JWT header and claims, the jwt-bearer token request).

mod common;

use std::time::Duration;

use common::{form_value, header, rsa_key, state_change, token_response, verify_rs256};
use jmap_platform_push::fcm::{DeviceFamily, FcmPusher, SCOPE, ServiceAccount};
use jmap_platform_push::url::Url;
use jmap_platform_push::{Outcome, PlatformPusher, PushError, PushKind, Urgency};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const SEND_PATH: &str = "/v1/projects/demo-project/messages:send";

fn account(server: &MockServer) -> ServiceAccount {
    let (pem, _) = rsa_key();
    let json = json!({
        "type": "service_account",
        "project_id": "demo-project",
        "private_key_id": "key-1",
        "private_key": pem,
        "client_email": "push@demo-project.iam.gserviceaccount.com",
        "token_uri": format!("{}/token", server.uri()),
    });
    ServiceAccount::from_json(&json.to_string()).unwrap()
}

fn pusher(server: &MockServer) -> FcmPusher {
    FcmPusher::new(account(server))
        .unwrap()
        .with_endpoint(&Url::parse(&server.uri()).unwrap())
}

async fn token_endpoint(server: &MockServer, token: &str, times: u64) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_response(token)))
        .up_to_n_times(times)
        .mount(server)
        .await;
}

async fn send_replies(server: &MockServer, status: u16, body: Value, times: u64) {
    Mock::given(method("POST"))
        .and(path(SEND_PATH))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .up_to_n_times(times)
        .mount(server)
        .await;
}

fn fcm_error(code: u16, status: &str, error_code: &str) -> Value {
    json!({ "error": {
        "code": code,
        "message": "error",
        "status": status,
        "details": [{
            "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError",
            "errorCode": error_code,
        }],
    }})
}

async fn requests(server: &MockServer, p: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == p)
        .collect()
}

fn success() -> Value {
    json!({ "name": "projects/demo-project/messages/0:1500415314455276%31bd1c9631bd1c96" })
}

/// Guide §Google FCM, Auth and Request; the service-account guide's JWT
/// header (`alg` RS256, `typ` JWT, `kid`) and claim set (`iss`, `scope`,
/// `aud` = token URI, `exp` at most an hour after `iat`); Message `token`
/// and string-valued `data`; no `notification`; AndroidConfig `ttl` as
/// seconds with an `s` suffix.
#[tokio::test]
async fn data_message_with_service_account_token() {
    let server = MockServer::start().await;
    token_endpoint(&server, "ya29.test-token", 1).await;
    send_replies(&server, 200, success(), 1).await;

    let message = state_change();
    let outcome = pusher(&server).push("device-token-1", &message).await;

    let Outcome::Delivered(receipt) = outcome else {
        panic!("not delivered: {outcome:?}");
    };
    assert_eq!(
        receipt.message_id.as_deref(),
        Some("projects/demo-project/messages/0:1500415314455276%31bd1c9631bd1c96")
    );

    let tokens = requests(&server, "/token").await;
    assert_eq!(tokens.len(), 1);
    assert_eq!(
        form_value(&tokens[0], "grant_type").as_deref(),
        Some("urn:ietf:params:oauth:grant-type:jwt-bearer")
    );
    let assertion = form_value(&tokens[0], "assertion").unwrap();
    let (jwt_header, claims) = verify_rs256(&assertion, &rsa_key().1);
    assert_eq!(
        jwt_header,
        json!({ "alg": "RS256", "typ": "JWT", "kid": "key-1" })
    );
    assert_eq!(claims["iss"], "push@demo-project.iam.gserviceaccount.com");
    assert_eq!(claims["scope"], SCOPE);
    assert_eq!(
        claims["scope"],
        "https://www.googleapis.com/auth/firebase.messaging"
    );
    assert_eq!(claims["aud"], format!("{}/token", server.uri()));
    let iat = claims["iat"].as_u64().unwrap();
    assert_eq!(claims["exp"].as_u64().unwrap() - iat, 3600);

    let sends = requests(&server, SEND_PATH).await;
    assert_eq!(sends.len(), 1);
    let req = &sends[0];
    assert_eq!(header(req, "authorization"), Some("Bearer ya29.test-token"));
    assert_eq!(header(req, "content-type"), Some("application/json"));
    let body: Value = req.body_json().unwrap();
    let msg = &body["message"];
    assert_eq!(msg["token"], "device-token-1");
    assert!(msg.get("notification").is_none());
    assert!(msg.get("apns").is_none());
    let data = msg["data"].as_object().unwrap();
    assert_eq!(data.len(), 1);
    let embedded: Value = serde_json::from_str(data["jmap-push"].as_str().unwrap()).unwrap();
    assert_eq!(
        embedded,
        json!({ "@type": "StateChange", "changed": { "u1": { "Email": "d35ecb040aab" } } })
    );
    assert_eq!(
        msg["android"],
        json!({ "priority": "NORMAL", "ttl": "3600s" })
    );
}

/// Guide §Google FCM, Auth: "Tokens are valid for one hour; servers SHOULD
/// cache and refresh proactively."
#[tokio::test]
async fn access_token_is_cached_across_pushes() {
    let server = MockServer::start().await;
    token_endpoint(&server, "ya29.cached", 5).await;
    send_replies(&server, 200, success(), 5).await;
    let pusher = pusher(&server);

    for _ in 0..3 {
        assert!(matches!(
            pusher.push("t", &state_change()).await,
            Outcome::Delivered(_)
        ));
    }
    assert_eq!(requests(&server, "/token").await.len(), 1);
    assert_eq!(requests(&server, SEND_PATH).await.len(), 3);
}

/// Guide §Google FCM, Urgency table: only "high" is HIGH; a VoIP ring is
/// HIGH at any urgency (jmap-vtc-ring-push-guide, `"priority": "HIGH"`).
#[tokio::test]
async fn urgency_maps_to_android_priority() {
    let server = MockServer::start().await;
    token_endpoint(&server, "t", 1).await;
    send_replies(&server, 200, success(), 10).await;
    let pusher = pusher(&server);
    let cases = [
        (Urgency::High, PushKind::Data, "HIGH"),
        (Urgency::Normal, PushKind::Data, "NORMAL"),
        (Urgency::Low, PushKind::Data, "NORMAL"),
        (Urgency::VeryLow, PushKind::Data, "NORMAL"),
        (Urgency::Low, PushKind::Voip, "HIGH"),
    ];
    for (urgency, kind, _) in &cases {
        let m = state_change()
            .with_urgency(*urgency)
            .with_kind(kind.clone());
        assert!(matches!(pusher.push("t", &m).await, Outcome::Delivered(_)));
    }
    let sent: Vec<String> = requests(&server, SEND_PATH)
        .await
        .iter()
        .map(|r| {
            r.body_json::<Value>().unwrap()["message"]["android"]["priority"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    let expected: Vec<&str> = cases.iter().map(|c| c.2).collect();
    assert_eq!(sent, expected);
}

/// Guide: "`error.status: "UNREGISTERED"` or `"SENDER_ID_MISMATCH"` —
/// servers MUST destroy the `PushSubscription`." ErrorCode: UNREGISTERED
/// is HTTP 404, SENDER_ID_MISMATCH HTTP 403.
#[tokio::test]
async fn unregistered_and_sender_id_mismatch_are_gone() {
    for (status, canonical, code) in [
        (404, "NOT_FOUND", "UNREGISTERED"),
        (403, "PERMISSION_DENIED", "SENDER_ID_MISMATCH"),
    ] {
        let server = MockServer::start().await;
        token_endpoint(&server, "t", 1).await;
        send_replies(&server, status, fcm_error(status, canonical, code), 1).await;

        let outcome = pusher(&server).push("t", &state_change()).await;
        let Outcome::Gone(e) = outcome else {
            panic!("{code}: {outcome:?}");
        };
        assert_eq!(e.status.as_u16(), status);
        assert_eq!(e.reason.as_deref(), Some(code));
    }
}

/// Guide: "FCM returns `200 OK` even when delivery fails — servers MUST
/// always check the response body".
#[tokio::test]
async fn error_body_on_200_is_not_success() {
    let server = MockServer::start().await;
    token_endpoint(&server, "t", 1).await;
    send_replies(&server, 200, fcm_error(404, "NOT_FOUND", "UNREGISTERED"), 1).await;

    assert!(matches!(
        pusher(&server).push("t", &state_change()).await,
        Outcome::Gone(_)
    ));
}

/// Guide §Delivery failures: "401 ... Server MUST refresh credential and
/// retry". A second 401 with a fresh token fails without destroying.
#[tokio::test]
async fn unauthorized_refreshes_the_token_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_response("old")))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    token_endpoint(&server, "new", 1).await;
    send_replies(
        &server,
        401,
        json!({ "error": { "code": 401, "message": "Request had invalid authentication credentials.", "status": "UNAUTHENTICATED" } }),
        1,
    )
    .await;
    send_replies(&server, 200, success(), 1).await;

    let outcome = pusher(&server).push("t", &state_change()).await;
    assert!(matches!(outcome, Outcome::Delivered(_)), "{outcome:?}");
    let sends = requests(&server, SEND_PATH).await;
    assert_eq!(header(&sends[0], "authorization"), Some("Bearer old"));
    assert_eq!(header(&sends[1], "authorization"), Some("Bearer new"));
    assert_eq!(requests(&server, "/token").await.len(), 2);

    let server = MockServer::start().await;
    token_endpoint(&server, "t", 5).await;
    send_replies(
        &server,
        401,
        json!({ "error": { "code": 401, "status": "UNAUTHENTICATED" } }),
        5,
    )
    .await;
    let outcome = pusher(&server).push("t", &state_change()).await;
    assert!(
        matches!(outcome, Outcome::Failed(PushError::Platform(_))),
        "{outcome:?}"
    );
    assert_eq!(requests(&server, SEND_PATH).await.len(), 2);
}

/// ErrorCode THIRD_PARTY_AUTH_ERROR (401): "APNs certificate or web push
/// auth key was invalid or missing." Not an OAuth problem: no refresh, no
/// destroy.
#[tokio::test]
async fn third_party_auth_error_fails_without_refresh() {
    let server = MockServer::start().await;
    token_endpoint(&server, "t", 5).await;
    send_replies(
        &server,
        401,
        fcm_error(401, "UNAUTHENTICATED", "THIRD_PARTY_AUTH_ERROR"),
        5,
    )
    .await;
    let outcome = pusher(&server).push("t", &state_change()).await;
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    assert_eq!(requests(&server, SEND_PATH).await.len(), 1);
}

/// error-codes: QUOTA_EXCEEDED "Use exponential backoff with a minimum
/// initial delay of 1 minute"; UNAVAILABLE "Honor the Retry-After header".
#[tokio::test]
async fn quota_and_unavailable_are_retried() {
    let server = MockServer::start().await;
    token_endpoint(&server, "t", 1).await;
    send_replies(
        &server,
        429,
        fcm_error(429, "RESOURCE_EXHAUSTED", "QUOTA_EXCEEDED"),
        1,
    )
    .await;
    let outcome = pusher(&server).push("t", &state_change()).await;
    let Outcome::Retry { after, .. } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(after, Some(Duration::from_secs(60)));

    let server = MockServer::start().await;
    token_endpoint(&server, "t", 1).await;
    Mock::given(method("POST"))
        .and(path(SEND_PATH))
        .respond_with(
            ResponseTemplate::new(503)
                .insert_header("Retry-After", "7")
                .set_body_json(fcm_error(503, "UNAVAILABLE", "UNAVAILABLE")),
        )
        .mount(&server)
        .await;
    let outcome = pusher(&server).push("t", &state_change()).await;
    let Outcome::Retry { after, .. } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(after, Some(Duration::from_secs(7)));
}

/// Guide: "`INVALID_ARGUMENT` — payload problem; servers SHOULD log and
/// fix, and MUST NOT retry." Error-codes Example 2.
#[tokio::test]
async fn invalid_argument_fails() {
    let server = MockServer::start().await;
    token_endpoint(&server, "t", 1).await;
    send_replies(
        &server,
        400,
        fcm_error(400, "INVALID_ARGUMENT", "INVALID_ARGUMENT"),
        1,
    )
    .await;
    let outcome = pusher(&server).push("t", &state_change()).await;
    let Outcome::Failed(PushError::Platform(e)) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(e.reason.as_deref(), Some("INVALID_ARGUMENT"));
}

/// Guide §Credential errors are not subscription errors: a token endpoint
/// that refuses the credentials fails, one that is down is retried; neither
/// destroys or sends.
#[tokio::test]
async fn token_endpoint_failures_never_destroy() {
    for (status, retry) in [(400, false), (503, true)] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(json!({ "error": "invalid_grant" })),
            )
            .mount(&server)
            .await;
        let outcome = pusher(&server).push("t", &state_change()).await;
        match outcome {
            Outcome::Retry {
                error: PushError::Credential(_),
                ..
            } if retry => {}
            Outcome::Failed(PushError::Credential(_)) if !retry => {}
            other => panic!("{status}: {other:?}"),
        }
        assert!(requests(&server, SEND_PATH).await.is_empty());
    }
}

/// Guide §Google FCM, iOS via FCM: "servers MUST replace the `"android"`
/// block with an `"apns"` block." ApnsConfig `headers` and `payload`; the
/// APNs background push (`apns-push-type: background`, priority 5,
/// `"content-available": 1`).
#[tokio::test]
async fn apple_devices_get_an_apns_block() {
    let server = MockServer::start().await;
    token_endpoint(&server, "t", 1).await;
    send_replies(&server, 200, success(), 1).await;
    let pusher = pusher(&server).for_family(DeviceFamily::Apple);

    assert!(matches!(
        pusher.push("ios-token", &state_change()).await,
        Outcome::Delivered(_)
    ));
    let body: Value = requests(&server, SEND_PATH).await[0].body_json().unwrap();
    let msg = &body["message"];
    assert!(msg.get("android").is_none());
    assert!(msg["data"]["jmap-push"].is_string());
    let apns = &msg["apns"];
    assert_eq!(apns["headers"]["apns-push-type"], "background");
    assert_eq!(apns["headers"]["apns-priority"], "5");
    assert!(
        apns["headers"]["apns-expiration"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    assert_eq!(
        apns["payload"],
        json!({ "aps": { "content-available": 1 } })
    );

    // FCM has no route to PushKit.
    let ring = state_change().with_kind(PushKind::Voip);
    assert!(matches!(
        pusher.push("ios-token", &ring).await,
        Outcome::Failed(PushError::InvalidMessage(_))
    ));
}

/// Message.data: keys "cannot be `from` or `message_type`, and cannot
/// start with `google.` or `gcm.notification.`". Refused before sending.
#[tokio::test]
async fn reserved_data_keys_are_refused() {
    let server = MockServer::start().await;
    token_endpoint(&server, "t", 5).await;
    send_replies(&server, 200, success(), 5).await;
    let pusher = pusher(&server);
    for key in ["from", "message_type", "google.x", "gcm.notification.body"] {
        let m = state_change().with_data_key(key);
        assert!(
            matches!(
                pusher.push("t", &m).await,
                Outcome::Failed(PushError::InvalidMessage(_))
            ),
            "{key}"
        );
    }
    assert!(requests(&server, SEND_PATH).await.is_empty());
}

#[tokio::test]
async fn debug_never_shows_the_private_key() {
    let server = MockServer::start().await;
    let account = account(&server);
    let shown = format!("{account:?} {:?}", pusher(&server));
    assert!(shown.contains("push@demo-project.iam.gserviceaccount.com"));
    assert!(!shown.contains("PRIVATE KEY"));
    assert!(!shown.contains(&rsa_key().0[40..80]));
}

#[test]
fn bad_service_account_json_is_an_error() {
    assert!(ServiceAccount::from_json("{}").is_err());
    let no_key = json!({
        "project_id": "p", "client_email": "e", "private_key": "nope",
        "token_uri": "https://oauth2.googleapis.com/token",
    });
    assert!(ServiceAccount::from_json(&no_key.to_string()).is_err());
}
