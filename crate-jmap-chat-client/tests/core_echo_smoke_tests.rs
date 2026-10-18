//! Wiremock smoke tests for `Core/echo` in jmap-chat-client.
//!
//! Spec oracle: RFC 8620 §4 / §4.1 (Core/echo and its example).

#[path = "helpers.rs"]
mod helpers;

use helpers::{jmap_response, mock_jmap_post, recorded_body};
use serde_json::json;
use wiremock::MockServer;

/// `Core/echo` must send the caller's arguments unchanged with only the
/// core capability, and return the echoed arguments. Oracle: RFC 8620
/// §4 — "The "Core/echo" method returns exactly the same arguments as
/// it is given." — using the §4.1 example arguments
/// `{ "hello": true, "high": 5 }`.
#[tokio::test]
async fn core_echo_sends_and_returns_rfc8620_example_args() {
    let server = MockServer::start().await;
    mock_jmap_post(
        &server,
        jmap_response("Core/echo", json!({ "hello": true, "high": 5 })),
    )
    .await;

    let sc = helpers::make_client(&server);
    let mut args = serde_json::Map::new();
    args.insert("hello".into(), json!(true));
    args.insert("high".into(), json!(5));
    let echoed = sc.core_echo(args).await.expect("core_echo: must succeed");
    assert_eq!(
        serde_json::Value::Object(echoed),
        json!({ "hello": true, "high": 5 }),
        "echoed arguments must match the RFC 8620 §4.1 response"
    );

    let body = recorded_body(&server).await;
    assert_eq!(body["using"], json!(["urn:ietf:params:jmap:core"]));
    assert_eq!(body["methodCalls"][0][0], json!("Core/echo"));
    assert_eq!(
        body["methodCalls"][0][1],
        json!({ "hello": true, "high": 5 }),
        "request arguments must match the RFC 8620 §4.1 request"
    );
}
