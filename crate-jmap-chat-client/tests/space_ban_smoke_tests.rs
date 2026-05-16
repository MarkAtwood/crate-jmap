//! Wiremock smoke tests for `SpaceBan/*` method paths in
//! jmap-chat-client.
//!
//! Spec oracles:
//!   - RFC 8620 §5.1 /get, §5.2 /changes, §5.3 /set
//!   - draft-atwood-jmap-chat-00 §4.18 (SpaceBan/* methods) and §4.19
//!     (SpaceBan object field set)

#[path = "helpers.rs"]
mod helpers;

use helpers::{
    jmap_response, mock_jmap_post, recorded_args, recorded_body, set_response, TEST_ACCOUNT_ID,
};
use jmap_types::{Id, State, UTCDate};
use serde_json::json;
use wiremock::MockServer;

/// `SpaceBan/get` with `ids: None, properties: None` must omit both keys
/// (space_ban.rs:22-30). Pins USING_CHAT for the SpaceBan/* family.
#[tokio::test]
async fn space_ban_get_omits_ids_and_properties_when_none() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "SpaceBan/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "sb-state-1",
            "list": [],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let _ = sc
        .space_ban_get(None, None)
        .await
        .expect("space_ban_get: must succeed");

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
        "SpaceBan/* using must equal USING_CHAT exactly"
    );
}

/// `SpaceBan/get` decode: populated wire object must round-trip through
/// [`jmap_chat_types::SpaceBan`] including both optionals (`reason` and
/// `expires_at`).
#[tokio::test]
async fn space_ban_get_decodes_populated_ban() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "SpaceBan/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "sb-state-2",
            "list": [
                {
                    "id": "sb-1",
                    "spaceId": "space-eng",
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
        .space_ban_get(None, None)
        .await
        .expect("space_ban_get: must succeed");

    let b = &resp.list[0];
    assert_eq!(b.id.as_ref(), "sb-1", "id mismatch");
    assert_eq!(b.space_id.as_ref(), "space-eng", "space_id mismatch");
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

/// `SpaceBan/changes` must thread `since_state` + `max_changes` and
/// reject empty `since_state` client-side (space_ban.rs:46-50).
#[tokio::test]
async fn space_ban_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "SpaceBan/changes",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldState": "sb-old",
            "newState": "sb-new",
            "hasMoreChanges": false,
            "created": ["sb-new-1"],
            "updated": [],
            "destroyed": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("sb-old");
    let _ = sc
        .space_ban_changes(&since, Some(10))
        .await
        .expect("space_ban_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["sinceState"], json!("sb-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(10), "maxChanges mismatch");

    let empty = State::from("");
    let err = sc
        .space_ban_changes(&empty, None)
        .await
        .expect_err("must reject empty since_state");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("since_state may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// `SpaceBan/set` create must serialise `spaceId` + `userId` and
/// optional `reason` + `expiresAt` inside the `create` map keyed by the
/// caller-supplied client id (space_ban.rs:71-86). Fields not set
/// (here, `reason` deliberately omitted) must be absent on the wire.
#[tokio::test]
async fn space_ban_create_serialises_full_object() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "SpaceBan/set",
        "sb-1",
        "sb-2",
        json!({ "created": { "my-ban-1": { "id": "sb-server-1" } } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let space_id = Id::from("space-eng");
    let user_id = Id::from("u-malice");
    let expires_at = UTCDate::from("2026-02-20T12:00:00Z");
    let mut input = jmap_chat_client::methods::SpaceBanCreateInput::new(&space_id, &user_id)
        .with_client_id("my-ban-1");
    input.expires_at = Some(&expires_at);
    let _ = sc
        .space_ban_create(&input)
        .await
        .expect("space_ban_create: must succeed");

    let args = recorded_args(&server).await;
    let create = &args["create"]["my-ban-1"];
    assert_eq!(create["spaceId"], json!("space-eng"), "spaceId mismatch");
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

/// `SpaceBan/set` destroy must thread `ids` to the wire `destroy` array
/// and reject the empty slice client-side
/// (space_ban.rs:99-103).
#[tokio::test]
async fn space_ban_destroy_threads_ids_and_rejects_empty() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "SpaceBan/set",
        "sb-1",
        "sb-2",
        json!({ "destroyed": ["sb-doomed"] }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("sb-doomed")];
    let _ = sc
        .space_ban_destroy(&ids)
        .await
        .expect("space_ban_destroy: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["destroy"], json!(["sb-doomed"]), "destroy must thread");

    let empty: [Id; 0] = [];
    let err = sc
        .space_ban_destroy(&empty)
        .await
        .expect_err("must reject empty ids");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("ids may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
