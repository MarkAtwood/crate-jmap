//! Huawei Push Kit delivery against a local stub.
//!
//! Oracles: jmap-push-platform-guide §Huawei Push Kit (HPK); Huawei's
//! "Sending Downlink Messages" (§Prototype URL, §Request Parameters,
//! §Response Example, §Result Codes) and "OAuth 2.0-based Authentication"
//! (client credentials mode).

mod common;

use common::{form_value, header, state_change, token_response};
use jmap_platform_push::hpk::{HpkCredentials, HpkPusher};
use jmap_platform_push::url::Url;
use jmap_platform_push::{Outcome, PlatformPusher, PushError, Urgency};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const SEND_PATH: &str = "/v1/104999999/messages:send";

fn pusher(server: &MockServer) -> HpkPusher {
    let base = Url::parse(&server.uri()).unwrap();
    HpkPusher::new(HpkCredentials::new("104999999", "dummy-secret"))
        .unwrap()
        .with_endpoint(&base)
        .with_token_url(&base.join("/oauth2/v3/token").unwrap())
}

async fn token(server: &MockServer, value: &str, times: u64) {
    Mock::given(method("POST"))
        .and(path("/oauth2/v3/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_response(value)))
        .up_to_n_times(times)
        .mount(server)
        .await;
}

async fn send(server: &MockServer, status: u16, body: Value, times: u64) {
    Mock::given(method("POST"))
        .and(path(SEND_PATH))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .up_to_n_times(times)
        .mount(server)
        .await;
}

fn result(code: &str, msg: &str) -> Value {
    json!({ "code": code, "msg": msg, "requestId": "157440955549375935505" })
}

async fn sends(server: &MockServer) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == SEND_PATH)
        .collect()
}

/// Guide §Huawei Push Kit, Request: `"data"` "is a single string (not a
/// map); servers MUST serialize the entire payload JSON into it"; token as
/// an array; `android.urgency`; no `notification`. Success is
/// `"code": "80000000"` (§Response Example).
#[tokio::test]
async fn data_message_and_success_code() {
    let server = MockServer::start().await;
    token(&server, "CF3Xl2XV6jMKZgqYSZFws9IPlg", 1).await;
    send(&server, 200, result("80000000", "Success"), 1).await;

    let outcome = pusher(&server).push("hpk-token", &state_change()).await;
    let Outcome::Delivered(receipt) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(receipt.message_id.as_deref(), Some("157440955549375935505"));

    let reqs = server.received_requests().await.unwrap();
    let tok = &reqs[0];
    assert_eq!(
        form_value(tok, "grant_type").as_deref(),
        Some("client_credentials")
    );
    assert_eq!(form_value(tok, "client_id").as_deref(), Some("104999999"));
    assert_eq!(
        form_value(tok, "client_secret").as_deref(),
        Some("dummy-secret")
    );

    let req = &reqs[1];
    assert_eq!(req.url.path(), SEND_PATH);
    assert_eq!(
        header(req, "authorization"),
        Some("Bearer CF3Xl2XV6jMKZgqYSZFws9IPlg")
    );
    let body: Value = req.body_json().unwrap();
    assert_eq!(body["validate_only"], false);
    let msg = &body["message"];
    assert_eq!(msg["token"], json!(["hpk-token"]));
    assert!(msg.get("notification").is_none());
    assert!(msg["android"].get("notification").is_none());
    assert_eq!(msg["android"]["urgency"], "NORMAL");
    assert_eq!(msg["android"]["ttl"], "3600s");
    let data: Value = serde_json::from_str(msg["data"].as_str().unwrap()).unwrap();
    assert_eq!(
        data,
        json!({ "@type": "StateChange", "changed": { "u1": { "Email": "d35ecb040aab" } } })
    );
}

/// Guide §Urgency: "Servers MUST map `"low"` and `"very-low"` to
/// `"NORMAL"`."
#[tokio::test]
async fn urgency_maps_to_high_or_normal() {
    let server = MockServer::start().await;
    token(&server, "t", 1).await;
    send(&server, 200, result("80000000", "Success"), 4).await;
    let pusher = pusher(&server);
    for u in [
        Urgency::High,
        Urgency::Normal,
        Urgency::Low,
        Urgency::VeryLow,
    ] {
        let m = state_change().with_urgency(u);
        assert!(matches!(pusher.push("t", &m).await, Outcome::Delivered(_)));
    }
    let urgencies: Vec<String> = sends(&server)
        .await
        .iter()
        .map(|r| r.body_json::<Value>().unwrap()["message"]["android"]["urgency"].to_string())
        .collect();
    assert_eq!(
        urgencies,
        ["\"HIGH\"", "\"NORMAL\"", "\"NORMAL\"", "\"NORMAL\""]
    );
}

/// "Servers MUST parse the `code` field to classify errors — HTTP status
/// alone is not sufficient": 80300007 "All tokens are invalid" arrives
/// with HTTP 200 and destroys; 80100000 destroys when `illegal_tokens`
/// lists the token.
#[tokio::test]
async fn invalid_token_codes_are_gone() {
    let server = MockServer::start().await;
    token(&server, "t", 1).await;
    send(
        &server,
        200,
        result("80300007", "All the tokens are invalid"),
        1,
    )
    .await;
    let outcome = pusher(&server).push("t", &state_change()).await;
    let Outcome::Gone(e) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(e.reason.as_deref(), Some("80300007"));

    let server = MockServer::start().await;
    token(&server, "t", 1).await;
    let partial = json!({
        "code": "80100000",
        "msg": "{\"success\":0,\"failure\":1,\"illegal_tokens\":[\"dead-token\"]}",
        "requestId": "",
    });
    send(&server, 200, partial, 1).await;
    assert!(matches!(
        pusher(&server).push("dead-token", &state_change()).await,
        Outcome::Gone(_)
    ));
}

/// 80200003 "OAuth token expired ... Obtain a new access token": refresh
/// and resend once.
#[tokio::test]
async fn expired_oauth_token_refreshes() {
    let server = MockServer::start().await;
    token(&server, "first", 1).await;
    token(&server, "second", 1).await;
    send(&server, 401, result("80200003", "OAuth token expired."), 1).await;
    send(&server, 200, result("80000000", "Success"), 1).await;
    assert!(matches!(
        pusher(&server).push("t", &state_change()).await,
        Outcome::Delivered(_)
    ));
    let s = sends(&server).await;
    assert_eq!(header(&s[1], "authorization"), Some("Bearer second"));
}

/// 80300002 (no permission), 80300008 (body too large), 80100003 (bad
/// message structure) fail without destroying; 81000001 (internal error)
/// and HTTP 503 (traffic control) are retried.
#[tokio::test]
async fn other_codes_fail_or_retry() {
    for (status, code, retry) in [
        (200, "80300002", false),
        (200, "80300008", false),
        (400, "80100003", false),
        (500, "81000001", true),
        (503, "80000001", true),
    ] {
        let server = MockServer::start().await;
        token(&server, "t", 1).await;
        send(&server, status, result(code, "x"), 1).await;
        let outcome = pusher(&server).push("t", &state_change()).await;
        match outcome {
            Outcome::Retry { .. } if retry => {}
            Outcome::Failed(PushError::Platform(e)) if !retry => {
                assert_eq!(e.reason.as_deref(), Some(code));
            }
            other => panic!("{code}: {other:?}"),
        }
    }
}
