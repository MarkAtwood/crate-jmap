//! The Web API client against a local stub.
//!
//! Oracles: Slack "Rate limits"
//! (<https://docs.slack.dev/apis/web-api/rate-limits/>): "HTTP 429 Too Many
//! Requests" with a `Retry-After` header "giving the number of seconds to
//! wait"; "Pagination" (<https://docs.slack.dev/apis/web-api/pagination/>);
//! the `auth.test` and `apps.connections.open` reference pages' example
//! responses; Web API `"ok": false` / `error`.

mod common;

use std::time::{Duration, Instant};

use common::{auth_test, calls, client, dummy_token, ok, on, slack_error};
use jmap_chat_slack::TokenKind;
use jmap_chat_slack::client::{RetryPolicy, SlackError};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn bearer_token_in_header_and_form_body() {
    let server = MockServer::start().await;
    on(&server, "auth.test", ok(auth_test())).await;
    let token = dummy_token("xoxb-");
    let auth = client(&server, &token).auth_test().await.unwrap();
    assert_eq!(auth.team_id, "T12345678");
    assert_eq!(auth.user_id, "W12345678");
    let reqs = calls(&server, "auth.test").await;
    assert_eq!(reqs.len(), 1);
    assert_eq!(
        reqs[0]
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok()),
        Some(format!("Bearer {token}").as_str())
    );
    assert!(
        reqs[0]
            .headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"))
    );
}

#[tokio::test]
async fn waits_out_retry_after_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/auth.test"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    on(&server, "auth.test", ok(auth_test())).await;
    let started = Instant::now();
    let auth = client(&server, &dummy_token("xoxb-"))
        .auth_test()
        .await
        .unwrap();
    assert_eq!(auth.team_id, "T12345678");
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "slept out Retry-After"
    );
    assert_eq!(calls(&server, "auth.test").await.len(), 2);
}

#[tokio::test]
async fn gives_up_after_the_retry_budget() {
    let server = MockServer::start().await;
    on(
        &server,
        "conversations.history",
        ResponseTemplate::new(429).insert_header("Retry-After", "0"),
    )
    .await;
    let err = client(&server, &dummy_token("xoxb-"))
        .with_retry_policy(RetryPolicy::new(2, Duration::from_secs(5)))
        .conversations_history("C1", None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, SlackError::RateLimited { .. }), "{err:?}");
    // One attempt and two retries.
    assert_eq!(calls(&server, "conversations.history").await.len(), 3);
}

#[tokio::test]
async fn a_long_retry_after_fails_at_once() {
    let server = MockServer::start().await;
    on(
        &server,
        "chat.postMessage",
        ResponseTemplate::new(429).insert_header("Retry-After", "120"),
    )
    .await;
    let err = client(&server, &dummy_token("xoxb-"))
        .chat_post_message("C1", "hi", None, true)
        .await
        .unwrap_err();
    let SlackError::RateLimited { retry_after, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(retry_after, Duration::from_secs(120));
    assert_eq!(calls(&server, "chat.postMessage").await.len(), 1);
}

#[tokio::test]
async fn ok_false_is_an_api_error_with_the_code() {
    let server = MockServer::start().await;
    on(
        &server,
        "conversations.info",
        slack_error("channel_not_found"),
    )
    .await;
    let err = client(&server, &dummy_token("xoxb-"))
        .conversations_info("C404")
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some("channel_not_found"));
    assert!(!err.to_string().contains("xoxb-"));
}

#[tokio::test]
async fn pagination_follows_next_cursor() {
    let server = MockServer::start().await;
    common::mount_workspace(&server).await;
    let c = client(&server, &dummy_token("xoxb-"));
    let first = c.users_list(None).await.unwrap();
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.next_cursor.as_deref(), Some("dXNlcjpVMDYxTkZUVDI="));
    let second = c.users_list(first.next_cursor.as_deref()).await.unwrap();
    assert_eq!(second.items[0].id, "U2");
    assert_eq!(
        second.next_cursor, None,
        "an empty next_cursor ends the list"
    );
    let reqs = calls(&server, "users.list").await;
    assert_eq!(
        common::form(&reqs[1], "cursor").as_deref(),
        Some("dXNlcjpVMDYxTkZUVDI=")
    );
}

#[tokio::test]
async fn socket_mode_url_needs_an_app_token() {
    let server = MockServer::start().await;
    on(
        &server,
        "apps.connections.open",
        ok(json!({ "url": "wss://wss-somethiing.slack.com/link/?ticket=12348&app_id=5678" })),
    )
    .await;
    let bot = client(&server, &dummy_token("xoxb-"));
    let err = bot.apps_connections_open().await.unwrap_err();
    assert!(
        matches!(
            err,
            SlackError::WrongToken {
                needed: TokenKind::App,
                ..
            }
        ),
        "{err:?}"
    );
    let app = client(&server, &dummy_token("xapp-"));
    let url = app.apps_connections_open().await.unwrap();
    assert_eq!(url.scheme(), "wss");
    assert_eq!(calls(&server, "apps.connections.open").await.len(), 1);
}

/// Debug redaction canary (workspace AGENTS.md §Security testing).
#[tokio::test]
async fn client_debug_never_shows_the_token() {
    let server = MockServer::start().await;
    let token = dummy_token("xoxb-canary-");
    let shown = format!("{:?}", client(&server, &token));
    assert!(!shown.contains(&token), "{shown}");
    assert!(!shown.contains("canary"), "{shown}");
}
