//! WNS raw-notification delivery against a local stub.
//!
//! Oracles: jmap-push-platform-guide §Microsoft WNS; Microsoft's "Push
//! notification service request and response headers" (§Access token
//! request parameters, §Request parameters, §X-WNS-Type, §X-WNS-Status,
//! §Response codes), "WNS overview" (channel domain check, resend once on
//! a rejected token) and the Windows App SDK push quickstart (Entra ID
//! token request and its string `expires_in`).

mod common;

use std::time::Duration;

use common::{form_value, header, state_change, token_response};
use jmap_platform_push::url::Url;
use jmap_platform_push::wns::{WnsCredentials, WnsPusher};
use jmap_platform_push::{Message, Outcome, PlatformPusher, PushError};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const CHANNEL_PATH: &str = "/channel";

fn credentials(server: &MockServer) -> WnsCredentials {
    WnsCredentials::package("ms-app://s-1-15-2-0000", "dummy-secret")
        .with_token_url(&Url::parse(&format!("{}/accesstoken.srf", server.uri())).unwrap())
}

fn pusher(server: &MockServer) -> WnsPusher {
    WnsPusher::new(credentials(server))
        .unwrap()
        .danger_accept_any_channel()
}

fn channel(server: &MockServer) -> String {
    format!("{}{CHANNEL_PATH}?token=AwYAAABa5cJ3", server.uri())
}

async fn token(server: &MockServer, value: &str, times: u64) {
    Mock::given(method("POST"))
        .and(path("/accesstoken.srf"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_response(value)))
        .up_to_n_times(times)
        .mount(server)
        .await;
}

async fn send(server: &MockServer, template: ResponseTemplate, times: u64) {
    Mock::given(method("POST"))
        .and(path(CHANNEL_PATH))
        .respond_with(template)
        .up_to_n_times(times)
        .mount(server)
        .await;
}

async fn sends(server: &MockServer) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == CHANNEL_PATH)
        .collect()
}

/// Token: `grant_type=client_credentials`, Package SID, secret,
/// `scope=notify.windows.com`. Push: "Servers MUST use `X-WNS-Type:
/// wns/raw` with `Content-Type: application/octet-stream`", the raw JSON
/// bytes, `X-WNS-TTL` in seconds; `received` is success.
#[tokio::test]
async fn raw_notification_with_access_token() {
    let server = MockServer::start().await;
    token(
        &server,
        "EgAcAQMAAAAALYAAY/c+Huwi3Fv4Ck10UrKNmtxRO6Njk2MgA=",
        1,
    )
    .await;
    send(
        &server,
        ResponseTemplate::new(200)
            .insert_header("X-WNS-Status", "received")
            .insert_header("X-WNS-Msg-ID", "1ACF8B35EA9C0F21"),
        1,
    )
    .await;

    let outcome = pusher(&server)
        .push(&channel(&server), &state_change())
        .await;
    let Outcome::Delivered(receipt) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(receipt.message_id.as_deref(), Some("1ACF8B35EA9C0F21"));

    let reqs = server.received_requests().await.unwrap();
    let tok = &reqs[0];
    assert_eq!(
        form_value(tok, "grant_type").as_deref(),
        Some("client_credentials")
    );
    assert_eq!(
        form_value(tok, "client_id").as_deref(),
        Some("ms-app://s-1-15-2-0000")
    );
    assert_eq!(
        form_value(tok, "client_secret").as_deref(),
        Some("dummy-secret")
    );
    assert_eq!(
        form_value(tok, "scope").as_deref(),
        Some("notify.windows.com")
    );

    let req = &reqs[1];
    assert_eq!(req.url.query(), Some("token=AwYAAABa5cJ3"));
    assert_eq!(
        header(req, "authorization"),
        Some("Bearer EgAcAQMAAAAALYAAY/c+Huwi3Fv4Ck10UrKNmtxRO6Njk2MgA=")
    );
    assert_eq!(header(req, "x-wns-type"), Some("wns/raw"));
    assert_eq!(
        header(req, "content-type"),
        Some("application/octet-stream")
    );
    assert_eq!(header(req, "x-wns-ttl"), Some("3600"));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&req.body).unwrap(),
        json!({ "@type": "StateChange", "changed": { "u1": { "Email": "d35ecb040aab" } } })
    );
}

/// Guide: "a `200 OK` with `dropped` means the notification was not
/// delivered" (the guide's header name `X-WNS-NotificationStatus` and
/// Microsoft's `X-WNS-Status` both count). `channelthrottled` is retried.
#[tokio::test]
async fn dropped_is_not_success_and_throttled_retries() {
    for name in ["X-WNS-Status", "X-WNS-NotificationStatus"] {
        let server = MockServer::start().await;
        token(&server, "t", 1).await;
        send(
            &server,
            ResponseTemplate::new(200).insert_header(name, "dropped"),
            1,
        )
        .await;
        let outcome = pusher(&server)
            .push(&channel(&server), &state_change())
            .await;
        let Outcome::Failed(PushError::Platform(e)) = outcome else {
            panic!("{name}: {outcome:?}");
        };
        assert_eq!(e.reason.as_deref(), Some("dropped"));
    }

    let server = MockServer::start().await;
    token(&server, "t", 1).await;
    send(
        &server,
        ResponseTemplate::new(406).insert_header("X-WNS-Status", "channelthrottled"),
        1,
    )
    .await;
    assert!(matches!(
        pusher(&server)
            .push(&channel(&server), &state_change())
            .await,
        Outcome::Retry { .. }
    ));
}

/// Guide: "`404 Not Found` / `410 Gone` — channel URI invalid; servers MUST
/// destroy the `PushSubscription`."
#[tokio::test]
async fn not_found_and_gone_destroy() {
    for status in [404, 410] {
        let server = MockServer::start().await;
        token(&server, "t", 1).await;
        send(
            &server,
            ResponseTemplate::new(status)
                .insert_header("X-WNS-Error-Description", "Channel expired"),
            1,
        )
        .await;
        let outcome = pusher(&server)
            .push(&channel(&server), &state_change())
            .await;
        let Outcome::Gone(e) = outcome else {
            panic!("{status}: {outcome:?}");
        };
        assert_eq!(e.description.as_deref(), Some("Channel expired"));
    }
}

/// 401: "Request a valid access token"; overview: "do not try to re-send
/// your notification more than once if the access token is rejected."
#[tokio::test]
async fn unauthorized_refreshes_and_resends_once() {
    let server = MockServer::start().await;
    token(&server, "first", 1).await;
    token(&server, "second", 5).await;
    send(&server, ResponseTemplate::new(401), 1).await;
    send(&server, ResponseTemplate::new(200), 1).await;
    assert!(matches!(
        pusher(&server)
            .push(&channel(&server), &state_change())
            .await,
        Outcome::Delivered(_)
    ));
    let s = sends(&server).await;
    assert_eq!(header(&s[0], "authorization"), Some("Bearer first"));
    assert_eq!(header(&s[1], "authorization"), Some("Bearer second"));

    let server = MockServer::start().await;
    token(&server, "t", 5).await;
    send(&server, ResponseTemplate::new(401), 5).await;
    let outcome = pusher(&server)
        .push(&channel(&server), &state_change())
        .await;
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    assert_eq!(sends(&server).await.len(), 2);
}

/// 406: "send your request after the Retry-After header value"; 403 (token
/// for another app) fails without destroying.
#[tokio::test]
async fn throttle_retries_forbidden_fails() {
    let server = MockServer::start().await;
    token(&server, "t", 1).await;
    send(
        &server,
        ResponseTemplate::new(406).insert_header("Retry-After", "60"),
        1,
    )
    .await;
    let outcome = pusher(&server)
        .push(&channel(&server), &state_change())
        .await;
    assert!(
        matches!(outcome, Outcome::Retry { after: Some(d), .. } if d == Duration::from_secs(60)),
        "{outcome:?}"
    );

    let server = MockServer::start().await;
    token(&server, "t", 1).await;
    send(&server, ResponseTemplate::new(403), 1).await;
    assert!(matches!(
        pusher(&server)
            .push(&channel(&server), &state_change())
            .await,
        Outcome::Failed(PushError::Platform(_))
    ));
}

/// wns-overview: "The service should never push notifications to a channel
/// on any other domain." The default pusher refuses the stub's
/// `http://127.0.0.1` channel without requesting a token or sending.
/// 413: "The notification payload exceeds the 5000 byte size limit."
#[tokio::test]
async fn foreign_channels_and_large_payloads_are_refused_unsent() {
    let server = MockServer::start().await;
    token(&server, "t", 5).await;
    send(&server, ResponseTemplate::new(200), 5).await;
    let strict = WnsPusher::new(credentials(&server)).unwrap();
    assert!(matches!(
        strict.push(&channel(&server), &state_change()).await,
        Outcome::Failed(PushError::InvalidDestination)
    ));
    let big = Message::new(&"x".repeat(5000), Duration::ZERO).unwrap();
    assert!(matches!(
        pusher(&server).push(&channel(&server), &big).await,
        Outcome::Failed(PushError::TooLarge { max: 5000, .. })
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// Push quickstart Step 1: the Entra ID token request (`scope` of
/// `https://wns.windows.com/.default`) answers `"expires_in":"86399"`, a
/// string.
#[tokio::test]
async fn entra_credentials() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/tenant/oauth2/v2.0/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token_type": "Bearer", "expires_in": "86399", "ext_expires_in": "86399",
            "access_token": "entra-token"
        })))
        .mount(&server)
        .await;
    send(&server, ResponseTemplate::new(200), 2).await;
    let creds = WnsCredentials::entra("tenant", "app-id", "dummy-secret").with_token_url(
        &Url::parse(&format!("{}/tenant/oauth2/v2.0/token", server.uri())).unwrap(),
    );
    let pusher = WnsPusher::new(creds).unwrap().danger_accept_any_channel();
    for _ in 0..2 {
        assert!(matches!(
            pusher.push(&channel(&server), &state_change()).await,
            Outcome::Delivered(_)
        ));
    }
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(
        form_value(&reqs[0], "scope").as_deref(),
        Some("https://wns.windows.com/.default")
    );
    assert_eq!(
        header(&reqs[1], "authorization"),
        Some("Bearer entra-token")
    );
    // One token request for both pushes.
    assert_eq!(reqs.len(), 3);
}

#[test]
fn entra_token_url_names_the_tenant_and_debug_hides_the_secret() {
    let shown = format!("{:?}", WnsCredentials::entra("t-1", "app", "dummy-secret"));
    assert!(shown.contains("https://login.microsoftonline.com/t-1/oauth2/v2.0/token"));
    assert!(!shown.contains("dummy-secret"));
}
