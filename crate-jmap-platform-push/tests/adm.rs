//! ADM delivery against a local stub.
//!
//! Oracles: jmap-push-platform-guide §Amazon ADM; Amazon's "Request an
//! Access Token" (§Request format, §Response format) and "Send a Message"
//! (§Request format, §Response format: headers, body fields, the
//! `registrationID` result and the `reason` table).

mod common;

use std::time::Duration;

use common::{form_value, header, state_change, token_response};
use jmap_platform_push::adm::{AdmCredentials, AdmPusher};
use jmap_platform_push::url::Url;
use jmap_platform_push::{Message, Outcome, PlatformPusher, PushError, Urgency};
use serde_json::{Value, json};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const REG: &str = "amzn1.adm-registration.v3.Y29tLmFtYXpvbi5EZXZpY2VNZXNzYWdpbmc";

fn pusher(server: &MockServer) -> AdmPusher {
    let base = Url::parse(&server.uri()).unwrap();
    AdmPusher::new(AdmCredentials::new(
        "amzn1.application-oa2-client.test",
        "dummy-secret",
    ))
    .unwrap()
    .with_endpoint(&base)
    .with_token_url(&base.join("/auth/O2/token").unwrap())
}

async fn token(server: &MockServer, value: &str, times: u64) {
    Mock::given(method("POST"))
        .and(path("/auth/O2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_response(value)))
        .up_to_n_times(times)
        .mount(server)
        .await;
}

async fn send(server: &MockServer, template: ResponseTemplate, times: u64) {
    Mock::given(method("POST"))
        .and(path_regex("^/messaging/registrations/.+/messages$"))
        .respond_with(template)
        .up_to_n_times(times)
        .mount(server)
        .await;
}

fn reason(status: u16, r: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "reason": r }))
}

async fn sends(server: &MockServer) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path().starts_with("/messaging/"))
        .collect()
}

/// Token request: `grant_type=client_credentials`, `scope=messaging:push`,
/// client ID and secret. Send: the four ADM headers, `data` with string
/// values, `priority`, `expiresAfter` in seconds.
#[tokio::test]
async fn data_message_with_client_credentials() {
    let server = MockServer::start().await;
    token(&server, "Atc|test", 1).await;
    send(
        &server,
        ResponseTemplate::new(200).set_body_json(json!({ "registrationID": REG })),
        1,
    )
    .await;

    let outcome = pusher(&server).push(REG, &state_change()).await;
    let Outcome::Delivered(receipt) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(receipt.new_token, None);

    let reqs = server.received_requests().await.unwrap();
    let tok = &reqs[0];
    assert_eq!(tok.url.path(), "/auth/O2/token");
    assert_eq!(
        header(tok, "content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(
        form_value(tok, "grant_type").as_deref(),
        Some("client_credentials")
    );
    assert_eq!(form_value(tok, "scope").as_deref(), Some("messaging:push"));
    assert_eq!(
        form_value(tok, "client_id").as_deref(),
        Some("amzn1.application-oa2-client.test")
    );
    assert_eq!(
        form_value(tok, "client_secret").as_deref(),
        Some("dummy-secret")
    );

    let req = &reqs[1];
    assert_eq!(
        req.url.path(),
        format!("/messaging/registrations/{REG}/messages")
    );
    assert_eq!(header(req, "authorization"), Some("Bearer Atc|test"));
    assert_eq!(header(req, "content-type"), Some("application/json"));
    assert_eq!(
        header(req, "x-amzn-type-version"),
        Some("com.amazon.device.messaging.ADMMessage@1.0")
    );
    assert_eq!(header(req, "accept"), Some("application/json"));
    assert_eq!(
        header(req, "x-amzn-accept-type"),
        Some("com.amazon.device.messaging.ADMSendResult@1.0")
    );
    let body: Value = req.body_json().unwrap();
    assert_eq!(body["priority"], "normal");
    assert_eq!(body["expiresAfter"], 3600);
    let data = body["data"].as_object().unwrap();
    assert_eq!(data.len(), 1);
    let embedded: Value = serde_json::from_str(data["jmap-push"].as_str().unwrap()).unwrap();
    assert_eq!(embedded["@type"], "StateChange");
    assert!(body.get("notification").is_none());
}

/// Guide §Amazon ADM, Urgency: only "high" is `high`. `expiresAfter`:
/// "Allowed range is 60 to 2678400 inclusive."
#[tokio::test]
async fn priority_and_expiry_bounds() {
    let server = MockServer::start().await;
    token(&server, "t", 1).await;
    send(
        &server,
        ResponseTemplate::new(200).set_body_json(json!({})),
        3,
    )
    .await;
    let pusher = pusher(&server);
    let high = state_change().with_urgency(Urgency::High);
    let short = Message::new(&json!({}), Duration::from_secs(5))
        .unwrap()
        .with_urgency(Urgency::Low);
    let long = Message::new(&json!({}), Duration::from_secs(90 * 24 * 3600)).unwrap();
    for m in [&high, &short, &long] {
        assert!(matches!(pusher.push(REG, m).await, Outcome::Delivered(_)));
    }
    let bodies: Vec<Value> = sends(&server)
        .await
        .iter()
        .map(|r| r.body_json().unwrap())
        .collect();
    assert_eq!(bodies[0]["priority"], "high");
    assert_eq!(bodies[1]["priority"], "normal");
    assert_eq!(bodies[1]["expiresAfter"], 60);
    assert_eq!(bodies[2]["expiresAfter"], 2_678_400);
}

/// §Response format: if the returned `registrationID` "differs from the one
/// your server sent, your server must update its records to use this
/// value."
#[tokio::test]
async fn changed_registration_id_is_reported() {
    let server = MockServer::start().await;
    token(&server, "t", 1).await;
    send(
        &server,
        ResponseTemplate::new(200).set_body_json(json!({ "registrationID": "amzn1.new" })),
        1,
    )
    .await;
    let Outcome::Delivered(receipt) = pusher(&server).push(REG, &state_change()).await else {
        panic!("not delivered");
    };
    assert_eq!(receipt.new_token.as_deref(), Some("amzn1.new"));
}

/// Guide: "`InvalidRegistrationId` / `Unregistered` — servers MUST destroy
/// the `PushSubscription`." Both are 400 in Amazon's table, beside 400
/// reasons that are payload errors.
#[tokio::test]
async fn reason_decides_gone_not_status() {
    for (r, gone) in [
        ("InvalidRegistrationId", true),
        ("Unregistered", true),
        ("InvalidData", false),
        ("InvalidConsolidationKey", false),
    ] {
        let server = MockServer::start().await;
        token(&server, "t", 1).await;
        send(&server, reason(400, r), 1).await;
        let outcome = pusher(&server).push(REG, &state_change()).await;
        match outcome {
            Outcome::Gone(e) if gone => assert_eq!(e.reason.as_deref(), Some(r)),
            Outcome::Failed(PushError::Platform(e)) if !gone => {
                assert_eq!(e.reason.as_deref(), Some(r));
            }
            other => panic!("{r}: {other:?}"),
        }
    }
}

/// 401 `AccessTokenExpired`: refresh and resend once.
#[tokio::test]
async fn access_token_expired_refreshes() {
    let server = MockServer::start().await;
    token(&server, "first", 1).await;
    token(&server, "second", 1).await;
    send(&server, reason(401, "AccessTokenExpired"), 1).await;
    send(
        &server,
        ResponseTemplate::new(200).set_body_json(json!({})),
        1,
    )
    .await;
    assert!(matches!(
        pusher(&server).push(REG, &state_change()).await,
        Outcome::Delivered(_)
    ));
    let s = sends(&server).await;
    assert_eq!(header(&s[0], "authorization"), Some("Bearer first"));
    assert_eq!(header(&s[1], "authorization"), Some("Bearer second"));
}

/// 429 `MaxRateExceeded`, 500, 503: "Retry later, honoring `Retry-After`".
/// 413 `MessageTooLarge` fails.
#[tokio::test]
async fn rate_and_server_errors_retry_too_large_fails() {
    for status in [429, 500, 503] {
        let server = MockServer::start().await;
        token(&server, "t", 1).await;
        send(
            &server,
            reason(status, "MaxRateExceeded").insert_header("Retry-After", "12"),
            1,
        )
        .await;
        let outcome = pusher(&server).push(REG, &state_change()).await;
        assert!(
            matches!(outcome, Outcome::Retry { after: Some(d), .. } if d == Duration::from_secs(12)),
            "{status}: {outcome:?}"
        );
    }
    let server = MockServer::start().await;
    token(&server, "t", 1).await;
    send(&server, reason(413, "MessageTooLarge"), 1).await;
    assert!(matches!(
        pusher(&server).push(REG, &state_change()).await,
        Outcome::Failed(_)
    ));
}

/// The token endpoint's `INVALID_CLIENT` (401) is a credential failure: no
/// send, no destroy.
#[tokio::test]
async fn bad_client_credentials_fail_unsent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/O2/token"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": "invalid_client", "reason": "INVALID_CLIENT"
        })))
        .mount(&server)
        .await;
    let outcome = pusher(&server).push(REG, &state_change()).await;
    assert!(
        matches!(outcome, Outcome::Failed(PushError::Credential(_))),
        "{outcome:?}"
    );
    assert!(sends(&server).await.is_empty());
}

#[test]
fn debug_hides_the_secret() {
    let shown = format!("{:?}", AdmCredentials::new("id", "dummy-secret"));
    assert!(shown.contains("id"));
    assert!(!shown.contains("dummy-secret"));
}
