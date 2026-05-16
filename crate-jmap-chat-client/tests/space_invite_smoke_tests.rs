//! Wiremock smoke tests for `SpaceInvite/*` method paths in
//! jmap-chat-client.
//!
//! Spec oracles:
//!   - RFC 8620 §5.1 /get, §5.2 /changes, §5.3 /set
//!   - draft-atwood-jmap-chat-00 §4.17 (SpaceInvite/* methods) and
//!     §4.18 (SpaceInvite object field set, including the unguessable
//!     `code` credential and the redemption fields)

#[path = "helpers.rs"]
mod helpers;

use helpers::{
    jmap_response, mock_jmap_post, recorded_args, recorded_body, set_response, TEST_ACCOUNT_ID,
};
use jmap_types::{Id, State, UTCDate};
use serde_json::json;
use wiremock::MockServer;

/// `SpaceInvite/get` with `ids: None, properties: None` must omit both
/// keys (space_invite.rs:22-30). Pins USING_CHAT for the SpaceInvite/*
/// family.
#[tokio::test]
async fn space_invite_get_omits_ids_and_properties_when_none() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "SpaceInvite/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "si-state-1",
            "list": [],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let _ = sc
        .space_invite_get(None, None)
        .await
        .expect("space_invite_get: must succeed");

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
        "SpaceInvite/* using must equal USING_CHAT exactly"
    );
}

/// `SpaceInvite/get` decode: populated wire object must round-trip
/// through [`jmap_chat_types::SpaceInvite`] including all three
/// optionals (`default_channel_id`, `expires_at`, `max_uses`). The
/// `code` field is a credential per spec §4.18; the test value is
/// synthetic.
#[tokio::test]
async fn space_invite_get_decodes_populated_invite() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "SpaceInvite/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "si-state-2",
            "list": [
                {
                    "id": "si-1",
                    "code": "INVITE-CANARY-TEST-NOT-A-REAL-SECRET",
                    "spaceId": "space-eng",
                    "createdBy": "u-admin",
                    "uses": 0,
                    "createdAt": "2026-01-20T12:00:00Z",
                    "defaultChannelId": "chat-c1",
                    "expiresAt": "2026-02-20T12:00:00Z",
                    "maxUses": 25
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .space_invite_get(None, None)
        .await
        .expect("space_invite_get: must succeed");

    let inv = &resp.list[0];
    assert_eq!(inv.id.as_ref(), "si-1", "id mismatch");
    assert_eq!(
        inv.code, "INVITE-CANARY-TEST-NOT-A-REAL-SECRET",
        "code must round-trip verbatim"
    );
    assert_eq!(inv.space_id.as_ref(), "space-eng", "space_id mismatch");
    assert_eq!(inv.created_by.as_ref(), "u-admin", "created_by mismatch");
    assert_eq!(inv.uses, 0, "uses mismatch");
    assert_eq!(
        inv.default_channel_id.as_ref().map(|id| id.as_ref()),
        Some("chat-c1"),
        "default_channel_id optional mismatch"
    );
    assert_eq!(
        inv.expires_at.as_ref().map(|d| d.as_ref()),
        Some("2026-02-20T12:00:00Z"),
        "expires_at optional mismatch"
    );
    assert_eq!(inv.max_uses, Some(25), "max_uses optional mismatch");
}

/// `SpaceInvite/changes` must thread `since_state` + `max_changes` and
/// reject empty `since_state` (space_invite.rs:46-50).
#[tokio::test]
async fn space_invite_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "SpaceInvite/changes",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldState": "si-old",
            "newState": "si-new",
            "hasMoreChanges": false,
            "created": ["si-new-1"],
            "updated": [],
            "destroyed": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("si-old");
    let _ = sc
        .space_invite_changes(&since, Some(10))
        .await
        .expect("space_invite_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["sinceState"], json!("si-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(10), "maxChanges mismatch");

    let empty = State::from("");
    let err = sc
        .space_invite_changes(&empty, None)
        .await
        .expect_err("must reject empty since_state");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("since_state may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// `SpaceInvite/set` create must serialise `spaceId` and any
/// caller-supplied optionals (`defaultChannelId`, `expiresAt`,
/// `maxUses`) inside the `create` map keyed by the caller-supplied
/// client id (space_invite.rs:72-86). The `code` field is server-set
/// and MUST NOT appear in the create object: clients cannot supply or
/// influence the invite code (spec §4.18 — code is an unguessable
/// server-issued credential).
#[tokio::test]
async fn space_invite_create_serialises_optionals_and_omits_code() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "SpaceInvite/set",
        "si-1",
        "si-2",
        json!({ "created": { "my-inv-1": { "id": "si-server-1", "code": "SERVER-ISSUED-CODE" } } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let space_id = Id::from("space-eng");
    let default_channel = Id::from("chat-c1");
    let expires_at = UTCDate::from("2026-02-20T12:00:00Z");
    let mut input = jmap_chat_client::methods::SpaceInviteCreateInput::new(&space_id)
        .with_client_id("my-inv-1");
    input.default_channel_id = Some(&default_channel);
    input.expires_at = Some(&expires_at);
    input.max_uses = Some(25);
    let _ = sc
        .space_invite_create(&input)
        .await
        .expect("space_invite_create: must succeed");

    let args = recorded_args(&server).await;
    let create = &args["create"]["my-inv-1"];
    assert_eq!(create["spaceId"], json!("space-eng"), "spaceId mismatch");
    assert_eq!(
        create["defaultChannelId"],
        json!("chat-c1"),
        "defaultChannelId mismatch"
    );
    assert_eq!(
        create["expiresAt"],
        json!("2026-02-20T12:00:00Z"),
        "expiresAt mismatch"
    );
    assert_eq!(create["maxUses"], json!(25), "maxUses mismatch");
    // The `code` field is server-issued per spec §4.18 — clients MUST
    // NOT submit one in create.
    assert!(
        create.get("code").is_none(),
        "code must be absent on create (server-issued credential per spec §4.18)"
    );
}

/// `SpaceInvite/set` create with NO optional fields must serialise only
/// `spaceId` — all three optionals must be absent on the wire so the
/// server's defaults take effect (no expiry, no max use cap, server
/// chooses default channel).
#[tokio::test]
async fn space_invite_create_serialises_minimal_object() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "SpaceInvite/set",
        "si-1",
        "si-2",
        json!({ "created": { "my-inv-2": { "id": "si-server-2" } } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let space_id = Id::from("space-eng");
    let input = jmap_chat_client::methods::SpaceInviteCreateInput::new(&space_id)
        .with_client_id("my-inv-2");
    let _ = sc
        .space_invite_create(&input)
        .await
        .expect("space_invite_create: must succeed");

    let args = recorded_args(&server).await;
    let create = &args["create"]["my-inv-2"];
    assert_eq!(create["spaceId"], json!("space-eng"), "spaceId mismatch");
    assert!(
        create.get("defaultChannelId").is_none(),
        "defaultChannelId must be absent when None"
    );
    assert!(
        create.get("expiresAt").is_none(),
        "expiresAt must be absent when None"
    );
    assert!(
        create.get("maxUses").is_none(),
        "maxUses must be absent when None"
    );
}

/// `SpaceInvite/set` destroy must thread `ids` to the wire `destroy`
/// array and reject the empty slice client-side
/// (space_invite.rs:99-103).
#[tokio::test]
async fn space_invite_destroy_threads_ids_and_rejects_empty() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "SpaceInvite/set",
        "si-1",
        "si-2",
        json!({ "destroyed": ["si-doomed"] }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("si-doomed")];
    let _ = sc
        .space_invite_destroy(&ids)
        .await
        .expect("space_invite_destroy: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["destroy"], json!(["si-doomed"]), "destroy must thread");

    let empty: [Id; 0] = [];
    let err = sc
        .space_invite_destroy(&empty)
        .await
        .expect_err("must reject empty ids");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("ids may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
