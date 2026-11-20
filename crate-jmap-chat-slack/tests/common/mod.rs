//! Test helpers: a wiremock Slack Web API, dummy tokens made per run, and
//! fixtures adapted from the examples on Slack's reference pages (cited at
//! each fixture). No test reaches the real network.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use jmap_chat_slack::client::{RetryPolicy, SlackClient};
use jmap_chat_slack::{BridgeConfig, SlackBridge, SlackToken};
use jmap_chat_types::{Chat, Message, Space};
use jmap_server::JmapReads;
use jmap_types::Id;
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

pub const ACCOUNT: &str = "a1";
/// The token's user (auth.test example `user_id`).
pub const OWNER: &str = "W12345678";
pub const TEAM: &str = "T12345678";

/// A bot token made up for this run; never a real credential.
pub fn dummy_token(prefix: &str) -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    format!(
        "{prefix}0000-test-{nanos}-{}",
        N.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn client(server: &MockServer, token: &str) -> SlackClient {
    let base = url::Url::parse(&format!("{}/api/", server.uri())).unwrap();
    SlackClient::new(SlackToken::new(token).unwrap())
        .unwrap()
        .with_base_url(&base)
        .with_retry_policy(RetryPolicy::new(2, Duration::from_secs(5)))
}

pub fn ok(body: Value) -> ResponseTemplate {
    let mut body = body;
    body["ok"] = json!(true);
    ResponseTemplate::new(200).set_body_json(body)
}

pub fn slack_error(code: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "ok": false, "error": code }))
}

/// Mount `template` for Web API `api_method`.
pub async fn on(server: &MockServer, api_method: &str, template: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path(format!("/api/{api_method}")))
        .respond_with(template)
        .mount(server)
        .await;
}

/// Mount `template` for `api_method` when the form body contains `needle`;
/// it takes precedence over [`on`] (wiremock priority 2; 1 is the most
/// specific match).
pub async fn on_with(
    server: &MockServer,
    api_method: &str,
    needle: &str,
    template: ResponseTemplate,
) {
    Mock::given(method("POST"))
        .and(path(format!("/api/{api_method}")))
        .and(body_string_contains(needle))
        .respond_with(template)
        .with_priority(2)
        .mount(server)
        .await;
}

/// The requests made to `api_method`.
pub async fn calls(server: &MockServer, api_method: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == format!("/api/{api_method}"))
        .collect()
}

/// A form field of a request body.
pub fn form(r: &Request, key: &str) -> Option<String> {
    url::form_urlencoded::parse(&r.body)
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// auth.test, from the method page's example response.
pub fn auth_test() -> Value {
    json!({
        "url": "https://subarachnoid.slack.com/",
        "team": "Subarachnoid Workspace",
        "user": "grace",
        "team_id": TEAM,
        "user_id": OWNER,
    })
}

pub fn user(id: &str, name: &str, display: &str) -> Value {
    json!({
        "id": id, "team_id": TEAM, "name": name, "deleted": false,
        "real_name": display, "is_bot": false,
        "profile": { "display_name": display, "real_name": display },
    })
}

/// A small workspace: the owner, alice (U1) and bob (U2); #general (C1),
/// a DM with alice (D1) and a group DM (G1, history refused).
pub async fn mount_workspace(server: &MockServer) {
    on(server, "auth.test", ok(auth_test())).await;
    // users.list, two pages (pagination: response_metadata.next_cursor).
    on(
        server,
        "users.list",
        ok(json!({
            "members": [user(OWNER, "grace", "Grace"), user("U1", "alice", "Alice")],
            "response_metadata": { "next_cursor": "dXNlcjpVMDYxTkZUVDI=" },
        })),
    )
    .await;
    on_with(
        server,
        "users.list",
        "cursor=dXNlcjpVMDYxTkZUVDI",
        ok(json!({ "members": [user("U2", "bob", "Bob")], "response_metadata": { "next_cursor": "" } })),
    )
    .await;
    on(
        server,
        "conversations.list",
        ok(json!({ "channels": [
            { "id": "C1", "name": "general", "is_channel": true, "is_member": true,
              "created": 1449252889, "topic": { "value": "Company-wide announcements" } },
            { "id": "D1", "is_im": true, "user": "U1", "created": 1498500348 },
            { "id": "G1", "name": "mpdm-grace--alice--bob-1", "is_group": true,
              "is_mpim": true, "is_private": true, "created": 1493657761 },
        ]})),
    )
    .await;
    on(
        server,
        "conversations.members",
        ok(json!({ "members": [OWNER, "U1", "U2"] })),
    )
    .await;
    // conversations.history, newest first, two pages for C1.
    on_with(
        server,
        "conversations.history",
        "channel=C1",
        ok(json!({
            "messages": [
                { "type": "message", "user": "U2", "text": "second <@U1>", "ts": "1512104434.000490" },
                { "type": "message", "user": "U1", "text": "*hello* world", "ts": "1512085950.000216",
                  "thread_ts": "1512085950.000216", "reply_count": 1,
                  "reactions": [{ "name": "thumbsup", "users": ["U2", OWNER], "count": 2 }] },
                { "type": "message", "subtype": "channel_join", "user": "U2",
                  "text": "<@U2> has joined the channel", "ts": "1512085000.000100" },
            ],
            "has_more": true,
            "response_metadata": { "next_cursor": "bmV4dF90czoxNTEyMDg1ODYxMDAwNTQz" },
        })),
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/api/conversations.history"))
        .and(body_string_contains("channel=C1"))
        .and(body_string_contains("cursor=bmV4dF90czoxNTEyMDg1ODYxMDAwNTQz"))
        .respond_with(ok(json!({
            "messages": [{ "type": "message", "user": OWNER, "text": "first", "ts": "1512000000.000001" }],
            "has_more": false,
        })))
        .with_priority(1)
        .mount(server)
        .await;
    on_with(
        server,
        "conversations.history",
        "channel=D1",
        ok(json!({ "messages": [
            { "type": "message", "user": "U1", "text": "hi grace", "ts": "1512000001.000001" },
        ]})),
    )
    .await;
    on_with(
        server,
        "conversations.history",
        "channel=G1",
        slack_error("not_in_channel"),
    )
    .await;
    on(
        server,
        "conversations.replies",
        ok(json!({ "messages": [
            { "type": "message", "user": "U1", "text": "*hello* world", "ts": "1512085950.000216",
              "thread_ts": "1512085950.000216", "reply_count": 1 },
            { "type": "message", "user": "U2", "text": "reply", "ts": "1512090000.000300",
              "thread_ts": "1512085950.000216" },
        ]})),
    )
    .await;
}

/// A bridge over `server`, connected and synced.
pub async fn synced_bridge(server: &MockServer) -> SlackBridge {
    mount_workspace(server).await;
    let bridge = SlackBridge::connect(
        client(server, &dummy_token("xoxp-")),
        BridgeConfig::new(Id::from(ACCOUNT)),
    )
    .await
    .unwrap();
    bridge.initial_sync().await.unwrap();
    bridge
}

pub async fn messages(bridge: &SlackBridge) -> Vec<Message> {
    let (mut m, _) = bridge
        .memory()
        .get_objects::<Message>(&(), &Id::from(ACCOUNT), None, None)
        .await
        .unwrap();
    m.sort_by(|a, b| a.id.as_ref().cmp(b.id.as_ref()));
    m
}

pub async fn chats(bridge: &SlackBridge) -> Vec<Chat> {
    let (c, _) = bridge
        .memory()
        .get_objects::<Chat>(&(), &Id::from(ACCOUNT), None, None)
        .await
        .unwrap();
    c
}

pub async fn chat_named(bridge: &SlackBridge, name: &str) -> Chat {
    chats(bridge)
        .await
        .into_iter()
        .find(|c| c.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no chat {name}"))
}

pub async fn space(bridge: &SlackBridge) -> Space {
    let (s, _) = bridge
        .memory()
        .get_objects::<Space>(&(), &Id::from(ACCOUNT), None, None)
        .await
        .unwrap();
    s.into_iter().next().unwrap()
}

pub async fn message_with_body(bridge: &SlackBridge, needle: &str) -> Message {
    messages(bridge)
        .await
        .into_iter()
        .find(|m| m.body.contains(needle))
        .unwrap_or_else(|| panic!("no message with {needle:?}"))
}
