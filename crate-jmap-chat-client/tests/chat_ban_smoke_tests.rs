//! Wiremock smoke tests for `ChatBan/*` method paths in
//! jmap-chat-client.
//!
//! Spec oracles:
//!   - RFC 8620 §5.1 /get, §5.2 /changes, §5.3 /set
//!   - draft-atwood-jmap-chat-00 §ChatBan Methods and §ChatBan (object
//!     field set)

#[path = "helpers.rs"]
mod helpers;

use helpers::{
    TEST_ACCOUNT_ID, jmap_response, mock_jmap_post, recorded_args, recorded_body, set_response,
};
use jmap_types::{Id, State, UTCDate};
use serde_json::json;
use wiremock::MockServer;

/// `ChatBan/get` with `ids: None, properties: None` must omit both keys
/// (chat_ban.rs:22-30). Pins USING_CHAT for the ChatBan/* family.
#[tokio::test]
async fn chat_ban_get_omits_ids_and_properties_when_none() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "ChatBan/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "cb-state-1",
            "list": [],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let _ = sc
        .chat_ban_get(None, None)
        .await
        .expect("chat_ban_get: must succeed");

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
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"]),
        "ChatBan/* using must equal USING_CHAT exactly"
    );
}

/// `ChatBan/get` decode: populated wire object must round-trip through
/// [`jmap_chat_types::ChatBan`] including both optionals (`reason` and
/// `expires_at`).
#[tokio::test]
async fn chat_ban_get_decodes_populated_ban() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "ChatBan/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "cb-state-2",
            "list": [
                {
                    "id": "cb-1",
                    "chatId": "chat-eng",
                    "userId": "u-malice",
                    "bannedBy": "u-admin",
                    "createdAt": "2026-01-20T12:00:00Z",
                    "reason": "Harassment",
                    "expiresAt": "2026-02-20T12:00:00Z"
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .chat_ban_get(None, None)
        .await
        .expect("chat_ban_get: must succeed");

    let b = &resp.list[0];
    assert_eq!(b.id.as_ref(), "cb-1", "id mismatch");
    assert_eq!(b.chat_id.as_ref(), "chat-eng", "chat_id mismatch");
    assert_eq!(b.user_id.as_ref(), "u-malice", "user_id mismatch");
    assert_eq!(b.banned_by.as_ref(), "u-admin", "banned_by mismatch");
    assert_eq!(
        b.created_at.as_ref(),
        "2026-01-20T12:00:00Z",
        "created_at mismatch"
    );
    assert_eq!(
        b.reason.as_deref(),
        Some("Harassment"),
        "reason optional mismatch"
    );
    assert_eq!(
        b.expires_at.as_ref().map(|d| d.as_ref()),
        Some("2026-02-20T12:00:00Z"),
        "expires_at optional mismatch"
    );
}

/// `ChatBan/changes` must thread `since_state` + `max_changes` and
/// reject empty `since_state` client-side (chat_ban.rs:46-50).
#[tokio::test]
async fn chat_ban_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "ChatBan/changes",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldState": "cb-old",
            "newState": "cb-new",
            "hasMoreChanges": false,
            "created": ["cb-new-1"],
            "updated": [],
            "destroyed": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("cb-old");
    let _ = sc
        .chat_ban_changes(&since, Some(10))
        .await
        .expect("chat_ban_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["sinceState"], json!("cb-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(10), "maxChanges mismatch");

    let empty = State::from("");
    let err = sc
        .chat_ban_changes(&empty, None)
        .await
        .expect_err("must reject empty since_state");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("since_state may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// `ChatBan/set` create must serialise `chatId` + `userId` and
/// optional `reason` + `expiresAt` inside the `create` map keyed by the
/// caller-supplied client id (chat_ban.rs:71-86). Fields not set
/// (here, `reason` deliberately omitted) must be absent on the wire.
#[tokio::test]
async fn chat_ban_create_serialises_full_object() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "ChatBan/set",
        "cb-1",
        "cb-2",
        json!({ "created": { "my-ban-1": { "id": "cb-server-1" } } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let chat_id = Id::from("chat-eng");
    let user_id = Id::from("u-malice");
    let expires_at = UTCDate::from("2026-02-20T12:00:00Z");
    let input = jmap_chat_client::ChatBanCreateInput {
        client_id: Some("my-ban-1"),
        chat_id: &chat_id,
        user_id: &user_id,
        reason: None,
        expires_at: Some(&expires_at),
    };
    let _ = sc
        .chat_ban_create(&input, None)
        .await
        .expect("chat_ban_create: must succeed");

    let args = recorded_args(&server).await;
    let create = &args["create"]["my-ban-1"];
    assert_eq!(create["chatId"], json!("chat-eng"), "chatId mismatch");
    assert_eq!(create["userId"], json!("u-malice"), "userId mismatch");
    assert_eq!(
        create["expiresAt"],
        json!("2026-02-20T12:00:00Z"),
        "expiresAt mismatch"
    );
    assert!(
        create.get("reason").is_none(),
        "reason must be absent when input.reason is None"
    );
}

/// `ChatBan/set` destroy must thread `ids` to the wire `destroy` array
/// and reject the empty slice client-side
/// (chat_ban.rs:99-103).
#[tokio::test]
async fn chat_ban_destroy_threads_ids_and_rejects_empty() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "ChatBan/set",
        "cb-1",
        "cb-2",
        json!({ "destroyed": ["cb-doomed"] }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("cb-doomed")];
    let _ = sc
        .chat_ban_destroy(&ids, None)
        .await
        .expect("chat_ban_destroy: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["destroy"], json!(["cb-doomed"]), "destroy must thread");

    let empty: [Id; 0] = [];
    let err = sc
        .chat_ban_destroy(&empty, None)
        .await
        .expect_err("must reject empty ids");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("ids may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// `ChatBan/set` update must send `reason` and `expiresAt` under the
/// per-id patch key, with `Patch::Clear` emitting JSON `null`. Oracle:
/// draft-atwood-jmap-chat-00 §ChatBan/set — "`update` supports:
/// `reason`, `expiresAt`." — and RFC 8620 §5.3 PatchObject semantics
/// (a `null` value resets the property; an absent key leaves it
/// unchanged).
#[tokio::test]
async fn chat_ban_update_patch_serialises_reason_and_expires_at() {
    let server = MockServer::start().await;
    let resp_body = helpers::set_update_response("ChatBan/set", "cb-1", "cb-2", "ban-1");
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let id = Id::from("ban-1");
    let expires_at = UTCDate::from("2026-03-01T00:00:00Z");
    let patch = jmap_chat_client::ChatBanPatch {
        reason: jmap_chat_client::Patch::Set("repeat spam"),
        expires_at: jmap_chat_client::Patch::Set(&expires_at),
    };
    let _ = sc
        .chat_ban_update(&id, &patch, None)
        .await
        .expect("chat_ban_update: must succeed");

    let body = recorded_body(&server).await;
    assert_eq!(body["methodCalls"][0][0], json!("ChatBan/set"));
    assert_eq!(
        body["methodCalls"][0][1]["update"],
        json!({ "ban-1": {
            "reason": "repeat spam",
            "expiresAt": "2026-03-01T00:00:00Z"
        } }),
        "update must carry reason + expiresAt"
    );

    server.reset().await;
    mock_jmap_post(
        &server,
        helpers::set_update_response("ChatBan/set", "cb-2", "cb-3", "ban-1"),
    )
    .await;
    let clear = jmap_chat_client::ChatBanPatch {
        expires_at: jmap_chat_client::Patch::Clear,
        ..Default::default()
    };
    let _ = sc
        .chat_ban_update(&id, &clear, None)
        .await
        .expect("chat_ban_update: must succeed");
    let args = recorded_args(&server).await;
    assert_eq!(
        args["update"],
        json!({ "ban-1": { "expiresAt": null } }),
        "Patch::Clear must emit null and Patch::Keep must omit reason"
    );
}
