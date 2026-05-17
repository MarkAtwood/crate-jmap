//! Wiremock smoke tests for `ChatContact/*` method paths in
//! jmap-chat-client.
//!
//! Pattern oracle (workspace canonical extension-client): see
//! `crate-jmap-mail-client/tests/thread_smoke_tests.rs` and
//! `crate-jmap-calendars-client/tests/event_smoke_tests.rs`.
//!
//! Spec oracles:
//!   - RFC 8620 §5.1 /get, §5.2 /changes, §5.3 /set, §5.5 /query,
//!     §5.6 /queryChanges
//!   - draft-atwood-jmap-chat-00 §5 (ChatContact/*) and §4.8
//!     (ChatContact object field set)

#[path = "helpers.rs"]
mod helpers;

use helpers::{
    jmap_response, mock_jmap_post, recorded_args, recorded_body, set_response, TEST_ACCOUNT_ID,
};
use jmap_types::{Id, State};
use serde_json::json;
use wiremock::MockServer;

/// `ChatContact/get` with `ids: None, properties: None` must omit both
/// keys on the wire (contact.rs:24-32) consistent with `chat_get`. Pins
/// the USING_CHAT capability set for the entire ChatContact/* family
/// (one assertion per method-family per the workspace
/// canonical-extension-client convention from bd:JMAP-26di.10).
#[tokio::test]
async fn chat_contact_get_omits_ids_and_properties_when_none() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "ChatContact/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "cc-state-1",
            "list": [],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .chat_contact_get(None, None)
        .await
        .expect("chat_contact_get: must succeed");

    assert_eq!(
        resp.account_id.as_ref(),
        TEST_ACCOUNT_ID,
        "accountId mismatch"
    );
    assert_eq!(resp.state, "cc-state-1", "state mismatch");

    let body = recorded_body(&server).await;
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["accountId"],
        json!(TEST_ACCOUNT_ID),
        "accountId mismatch"
    );
    assert!(args.get("ids").is_none(), "ids must be omitted when None");
    assert!(
        args.get("properties").is_none(),
        "properties must be omitted when None"
    );
    // RFC 8620 §3.3 — ChatContact/* MUST declare USING_CHAT
    // (`core` + `chat`).
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"]),
        "ChatContact/* using must equal USING_CHAT exactly"
    );
}

/// `ChatContact/get` decode coverage: a populated ChatContact wire object
/// must round-trip through the [`jmap_chat_types::ChatContact`]
/// `Deserialize` impl with every required field plus representative
/// optionals (`display_name`, `presence`, `last_active_at`). Without
/// this test a regression that broke `ChatContact` deserialize would
/// still pass the every-list-empty smoke tests.
///
/// Oracle: draft-atwood-jmap-chat-00 §4.8 — ChatContact field set.
#[tokio::test]
async fn chat_contact_get_decodes_populated_contact() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "ChatContact/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "cc-state-2",
            "list": [
                {
                    "id": "alice@example.org",
                    "login": "alice",
                    "firstSeenAt": "2026-01-01T00:00:00Z",
                    "lastSeenAt": "2026-01-20T15:00:00Z",
                    "blocked": false,
                    "displayName": "Alice",
                    "presence": "online",
                    "lastActiveAt": "2026-01-20T14:55:00Z"
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .chat_contact_get(None, None)
        .await
        .expect("chat_contact_get: must succeed");

    assert_eq!(resp.list.len(), 1, "list must contain exactly one contact");
    let c = &resp.list[0];
    assert_eq!(c.id.as_ref(), "alice@example.org", "id mismatch");
    assert_eq!(c.login, "alice", "login mismatch");
    assert!(!c.blocked, "blocked must be false");
    assert_eq!(
        c.display_name.as_deref(),
        Some("Alice"),
        "display_name optional mismatch"
    );
    assert!(
        matches!(c.presence, Some(jmap_chat_types::Presence::Online)),
        "presence 'online' must deserialise to Presence::Online, got {:?}",
        c.presence
    );
    assert_eq!(
        c.last_active_at.as_ref().map(|d| d.as_ref()),
        Some("2026-01-20T14:55:00Z"),
        "last_active_at optional mismatch"
    );
}

/// `ChatContact/changes` must thread `since_state` and `max_changes` and
/// reject empty `since_state` client-side (contact.rs:45-49, RFC 8620
/// §5.2).
#[tokio::test]
async fn chat_contact_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "ChatContact/changes",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldState": "cc-old",
            "newState": "cc-new",
            "hasMoreChanges": false,
            "created": [],
            "updated": ["alice@example.org"],
            "destroyed": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("cc-old");
    let _ = sc
        .chat_contact_changes(&since, Some(40))
        .await
        .expect("chat_contact_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["sinceState"], json!("cc-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(40), "maxChanges mismatch");

    // Empty-state guard.
    let empty = State::from("");
    let err = sc
        .chat_contact_changes(&empty, None)
        .await
        .expect_err("chat_contact_changes must reject empty since_state");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("since_state may not be empty"),
                "error message must explain validation: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// `ChatContact/set` update with `blocked: Some(true)` must produce a
/// patch object containing `{"blocked": true}` keyed by the contact id
/// (contact.rs:73-76, RFC 8620 §5.3).
#[tokio::test]
async fn chat_contact_update_blocked_patch_serialises() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "ChatContact/set",
        "cc-1",
        "cc-2",
        json!({ "updated": { "alice@example.org": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let id = Id::from("alice@example.org");
    let mut patch = jmap_chat_client::methods::ChatContactPatch::default();
    patch.blocked = Some(true);
    let _ = sc
        .chat_contact_update(&id, &patch)
        .await
        .expect("chat_contact_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["alice@example.org"],
        json!({ "blocked": true }),
        "patch must contain only blocked=true"
    );
}

/// `ChatContact/set` update with `display_name: Patch::Clear` must emit
/// JSON `null` (RFC 8620 §5.3 patch null-clear semantics) so the server
/// removes the local display-name override. The `Patch::Set("Alice")`
/// case is implicitly exercised by serde — `Patch::Clear` is the
/// interesting half because it's the only way to reach the spec's
/// nullable `displayName` clear path.
#[tokio::test]
async fn chat_contact_update_display_name_clear_emits_null() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "ChatContact/set",
        "cc-1",
        "cc-2",
        json!({ "updated": { "alice@example.org": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let id = Id::from("alice@example.org");
    let mut patch = jmap_chat_client::methods::ChatContactPatch::default();
    patch.display_name = jmap_chat_client::methods::Patch::Clear;
    let _ = sc
        .chat_contact_update(&id, &patch)
        .await
        .expect("chat_contact_update: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"]["alice@example.org"],
        json!({ "displayName": null }),
        "Patch::Clear must serialise displayName as JSON null"
    );
}

/// `ChatContact/query` with `filter_blocked`, `filter_presence`,
/// `sort_property` and `sort_ascending` must thread all four through
/// to the wire (contact.rs:106-138). Empty filter would emit
/// `filter: null` (mirrors `chat_query`), but exercising the filter +
/// sort plumbing in the same test covers the busiest path; the
/// null-filter behaviour is identical to the well-tested `chat_query`
/// equivalent.
#[tokio::test]
async fn chat_contact_query_filter_presence_sort_serialise() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "ChatContact/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": "ccq-1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let mut input = jmap_chat_client::methods::ChatContactQueryInput::default();
    input.filter_blocked = Some(false);
    input.filter_presence = Some(jmap_chat_client::types::ContactPresenceFilter::Online);
    input.sort_property = Some(jmap_chat_client::methods::ContactSortProperty::LastSeenAt);
    input.sort_ascending = Some(true);
    let _ = sc
        .chat_contact_query(&input)
        .await
        .expect("chat_contact_query: must succeed");

    let args = recorded_args(&server).await;
    // Filter (RFC 9425-style; spec §5 ChatContact/query): {blocked,
    // presence} keys, lowercase wire strings on presence.
    assert_eq!(
        args["filter"],
        json!({ "blocked": false, "presence": "online" }),
        "filter must serialise blocked + lowercase presence"
    );
    // Sort: camelCase property + boolean isAscending (RFC 8620 §5.5
    // sort comparator shape). Spec line 1140 — sort property names are
    // `"lastSeenAt"`, `"login"`, `"lastActiveAt"`.
    assert_eq!(
        args["sort"],
        json!([{ "property": "lastSeenAt", "isAscending": true }]),
        "sort must serialise LastSeenAt ascending"
    );
}

/// `ChatContact/queryChanges` must thread `since_query_state` to
/// `sinceQueryState` and reject the empty token client-side
/// (contact.rs:152-156, RFC 8620 §5.6).
#[tokio::test]
async fn chat_contact_query_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "ChatContact/queryChanges",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldQueryState": "ccqc-old",
            "newQueryState": "ccqc-new",
            "total": null,
            "removed": [],
            "added": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("ccqc-old");
    let _ = sc
        .chat_contact_query_changes(&since, Some(20), None, None, None, None)
        .await
        .expect("chat_contact_query_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["sinceQueryState"],
        json!("ccqc-old"),
        "sinceQueryState mismatch"
    );
    assert_eq!(args["maxChanges"], json!(20), "maxChanges mismatch");

    // Empty-state guard.
    let empty = State::from("");
    let err = sc
        .chat_contact_query_changes(&empty, None, None, None, None, None)
        .await
        .expect_err("chat_contact_query_changes must reject empty since_query_state");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("since_query_state may not be empty"),
                "error message must explain validation: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
