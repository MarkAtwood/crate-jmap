//! Wiremock smoke tests for `Space/*` method paths in jmap-chat-client.
//!
//! Pattern oracle (workspace canonical extension-client): see
//! `crate-jmap-mail-client/tests/thread_smoke_tests.rs` and
//! `crate-jmap-calendars-client/tests/event_smoke_tests.rs`.
//!
//! Spec oracles:
//!   - RFC 8620 §5.1 /get, §5.2 /changes, §5.3 /set, §5.5 /query,
//!     §5.6 /queryChanges
//!   - draft-atwood-jmap-chat-00 §Space/* (method-specific argument shapes)

#[path = "helpers.rs"]
mod helpers;

use helpers::{
    jmap_response, mock_jmap_post, recorded_args, recorded_body, set_destroy_response,
    set_response, SPACE_STATE_NEW, SPACE_STATE_OLD, TEST_ACCOUNT_ID,
};
use jmap_types::{Id, State};
use serde_json::json;
use wiremock::MockServer;

/// `Space/get` with `ids: None, properties: None` must omit both keys on
/// the wire (space.rs:34-42), consistent with `chat_get`.
#[tokio::test]
async fn space_get_omits_ids_and_properties_when_none() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Space/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "sp-state-1",
            "list": [],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let _ = sc
        .space_get(None, None)
        .await
        .expect("space_get: must succeed");

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
    // RFC 8620 §3.3 — Space/* methods MUST declare USING_CHAT
    // (`core` + `chat`). assert_eq! on the full array so a regression
    // that swapped to USING_CHAT_PUSH or added an extra capability is
    // also caught. One assertion per method-family per bd:JMAP-26di.10
    // — every other Space/* smoke test in this file goes through the
    // same build_request site and inherits the constant.
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"]),
        "Space/* using must equal USING_CHAT exactly"
    );
}

/// `Space/get` decode coverage: populated wire object must round-trip
/// through the [`jmap_chat_types::Space`] `Deserialize` impl with every
/// required field plus a representative optional (`description`) and
/// each nested collection (`roles`, `members`, `categories`) populated
/// with at least one entry. Without this test a regression that broke
/// `Space` deserialize would still pass every other `Space/get` smoke
/// test (they all return `"list": []`).
///
/// Mirrors the canonical extension-client shape
/// `crate-jmap-calendars-client/tests/calendar_smoke_tests.rs::calendar_get_smoke`.
///
/// Oracles:
///   - draft-atwood-jmap-chat-00 §Space — Space object field set
///   - RFC 8620 §5.1 — /get response envelope
#[tokio::test]
async fn space_get_decodes_populated_space() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Space/get",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "state": "sp-state-2",
            "list": [
                {
                    "id": "space-1",
                    "name": "Engineering",
                    "description": "Engineering team space",
                    "roles": [
                        {
                            "id": "role-admin",
                            "name": "Admin",
                            "permissions": ["manage_channels", "manage_roles"],
                            "position": 0
                        }
                    ],
                    "members": [
                        {
                            "id": "u1",
                            "roleIds": ["role-admin"],
                            "joinedAt": "2026-01-01T00:00:00Z"
                        }
                    ],
                    "categories": [
                        {
                            "id": "cat-1",
                            "name": "General",
                            "position": 0,
                            "channelIds": ["chat-c1"]
                        }
                    ],
                    "uncategorizedChannelIds": [],
                    "createdAt": "2026-01-01T00:00:00Z",
                    "isPublic": true,
                    "isPubliclyPreviewable": false,
                    "memberCount": 1
                }
            ],
            "notFound": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .space_get(None, None)
        .await
        .expect("space_get: must succeed");

    assert_eq!(
        resp.account_id.as_ref(),
        TEST_ACCOUNT_ID,
        "accountId mismatch"
    );
    assert_eq!(resp.state, "sp-state-2", "state mismatch");
    assert_eq!(resp.list.len(), 1, "list must contain exactly one Space");

    let space = &resp.list[0];
    assert_eq!(space.id.as_ref(), "space-1", "id mismatch");
    assert_eq!(space.name, "Engineering", "name mismatch");
    assert_eq!(
        space.description.as_deref(),
        Some("Engineering team space"),
        "description optional must round-trip"
    );
    assert_eq!(
        space.created_at.as_ref(),
        "2026-01-01T00:00:00Z",
        "createdAt mismatch"
    );
    assert!(space.is_public, "isPublic must be true");
    assert!(
        !space.is_publicly_previewable,
        "isPubliclyPreviewable must be false"
    );
    assert_eq!(space.member_count, 1, "memberCount mismatch");

    assert_eq!(space.roles.len(), 1, "roles must have 1 entry");
    assert_eq!(
        space.roles[0].id.as_ref(),
        "role-admin",
        "roles[0].id mismatch"
    );
    assert_eq!(space.roles[0].name, "Admin", "roles[0].name mismatch");
    assert_eq!(
        space.roles[0].permissions.len(),
        2,
        "roles[0].permissions must have 2 entries"
    );
    assert_eq!(space.roles[0].position, 0, "roles[0].position mismatch");

    assert_eq!(space.members.len(), 1, "members must have 1 entry");
    assert_eq!(space.members[0].id.as_ref(), "u1", "members[0].id mismatch");
    assert_eq!(
        space.members[0].role_ids.len(),
        1,
        "members[0].roleIds must have 1 entry"
    );
    assert_eq!(
        space.members[0].role_ids[0].as_ref(),
        "role-admin",
        "members[0].roleIds[0] mismatch"
    );

    assert_eq!(space.categories.len(), 1, "categories must have 1 entry");
    assert_eq!(
        space.categories[0].id.as_ref(),
        "cat-1",
        "categories[0].id mismatch"
    );
    assert_eq!(
        space.categories[0].name, "General",
        "categories[0].name mismatch"
    );
    assert_eq!(
        space.categories[0].channel_ids.len(),
        1,
        "categories[0].channelIds must have 1 entry"
    );
    assert_eq!(
        space.categories[0].channel_ids[0].as_ref(),
        "chat-c1",
        "categories[0].channelIds[0] mismatch"
    );

    assert!(
        space.uncategorized_channel_ids.is_empty(),
        "uncategorizedChannelIds must round-trip as empty"
    );
}

/// `Space/changes` must thread `since_state` and `max_changes`
/// (space.rs:57-62, RFC 8620 §5.2). Empty-state rejection lives in
/// the paired [`space_changes_rejects_empty_state`] test below.
#[tokio::test]
async fn space_changes_passthrough() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Space/changes",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldState": "sp-old",
            "newState": "sp-new",
            "hasMoreChanges": false,
            "created": [],
            "updated": [],
            "destroyed": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("sp-old");
    let _ = sc
        .space_changes(&since, Some(25))
        .await
        .expect("space_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["sinceState"], json!("sp-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(25), "maxChanges mismatch");
}

/// `Space/changes` must reject an empty `since_state` client-side
/// before any HTTP call, surfacing `ClientError::InvalidArgument`.
/// Paired with [`space_changes_passthrough`] above.
#[tokio::test]
async fn space_changes_rejects_empty_state() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);
    let empty = State::from("");
    let err = sc
        .space_changes(&empty, None)
        .await
        .expect_err("space_changes must reject empty since_state");
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

/// `Space/set` destroy must thread `ids` to the `destroy` wire key
/// (space.rs:80-97, RFC 8620 §5.3). Empty-slice rejection lives in
/// the paired [`space_destroy_rejects_empty_ids`] test below.
#[tokio::test]
async fn space_destroy_threads_ids() {
    let server = MockServer::start().await;
    let resp_body = set_destroy_response(
        "Space/set",
        SPACE_STATE_OLD,
        SPACE_STATE_NEW,
        "space-doomed",
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("space-doomed")];
    let _ = sc
        .space_destroy(&ids)
        .await
        .expect("space_destroy: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["destroy"],
        json!(["space-doomed"]),
        "destroy ids must thread through"
    );
}

/// `Space/set` destroy must reject an empty `ids` slice client-side
/// before any HTTP call, surfacing `ClientError::InvalidArgument`.
/// Paired with [`space_destroy_threads_ids`] above.
#[tokio::test]
async fn space_destroy_rejects_empty_ids() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);
    let empty: [Id; 0] = [];
    let err = sc
        .space_destroy(&empty)
        .await
        .expect_err("space_destroy must reject empty ids");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("ids may not be empty"),
                "error message must mention ids: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// `Space/query` with no filter set must emit `filter: null` while still
/// threading position/limit (space.rs:115-129).
#[tokio::test]
async fn space_query_empty_filter_sends_null() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Space/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": "sq-1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let mut input = jmap_chat_client::methods::SpaceQueryInput::default();
    input.position = Some(0);
    input.limit = Some(20);
    let _ = sc
        .space_query(&input)
        .await
        .expect("space_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(args["filter"], json!(null), "filter must be null");
    assert_eq!(args["position"], json!(0), "position must thread");
    assert_eq!(args["limit"], json!(20), "limit must thread");
}

/// `Space/query` with `filter_is_public: Some(true)` must serialize a
/// filter object containing `{ "isPublic": true }` (space.rs:112-114).
#[tokio::test]
async fn space_query_filter_is_public_serializes() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Space/query",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "queryState": "sq-1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let mut input = jmap_chat_client::methods::SpaceQueryInput::default();
    input.filter_is_public = Some(true);
    let _ = sc
        .space_query(&input)
        .await
        .expect("space_query: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["filter"],
        json!({ "isPublic": true }),
        "filter must contain isPublic=true"
    );
}

/// `Space/queryChanges` must thread `since_query_state` to
/// `sinceQueryState` (RFC 8620 §5.6, space.rs:140-162).
#[tokio::test]
async fn space_query_changes_since_state_passthrough() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Space/queryChanges",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "oldQueryState": "sqc-old",
            "newQueryState": "sqc-new",
            "total": null,
            "removed": [],
            "added": []
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let since = State::from("sqc-old");
    let _ = sc
        .space_query_changes(&since, Some(50), None, None, None, None)
        .await
        .expect("space_query_changes: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["sinceQueryState"],
        json!("sqc-old"),
        "sinceQueryState mismatch"
    );
    assert_eq!(args["maxChanges"], json!(50), "maxChanges mismatch");
}

/// `Space/set` create must serialize the create object with `name`
/// and any provided optional fields, keyed by the caller-supplied
/// creation id (space.rs:178-194). Empty-name rejection lives in the
/// paired [`space_create_rejects_empty_name`] test below.
#[tokio::test]
async fn space_create_serializes_create_object() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Space/set",
        SPACE_STATE_OLD,
        SPACE_STATE_NEW,
        json!({ "created": { "my-space-key": { "id": "space-new-1" } } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let input = jmap_chat_client::methods::SpaceCreateInput::new("Engineering")
        .with_client_id("my-space-key");
    let _ = sc
        .space_create(&input)
        .await
        .expect("space_create: must succeed");

    let args = recorded_args(&server).await;
    let create = &args["create"]["my-space-key"];
    assert_eq!(create["name"], json!("Engineering"), "name mismatch");
    assert!(
        create.get("description").is_none(),
        "description must be absent when None"
    );
    assert!(
        create.get("iconBlobId").is_none(),
        "iconBlobId must be absent when None"
    );
}

/// `Space/set` create must reject an empty `name` client-side before
/// any HTTP call (space.rs:173-177), surfacing
/// `ClientError::InvalidArgument`. Paired with
/// [`space_create_serializes_create_object`] above.
#[tokio::test]
async fn space_create_rejects_empty_name() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server);
    let bad = jmap_chat_client::methods::SpaceCreateInput::new("");
    let err = sc
        .space_create(&bad)
        .await
        .expect_err("space_create must reject empty name");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(
                msg.contains("name may not be empty"),
                "error message must mention name: got {msg:?}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// `Space/join` with `SpaceJoinInput::InviteCode` must serialise the
/// invite code under the `inviteCode` wire key (space.rs:206-214) and
/// reject the empty code client-side. The accountId travels in the
/// args.
#[tokio::test]
async fn space_join_via_invite_code_serialises_and_rejects_empty() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Space/join",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "spaceId": "space-joined"
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let input = jmap_chat_client::methods::SpaceJoinInput::InviteCode(
        "INVITE-CANARY-TEST-NOT-A-REAL-SECRET",
    );
    let resp = sc
        .space_join(&input)
        .await
        .expect("space_join: must succeed");
    assert_eq!(resp.space_id.as_ref(), "space-joined", "space_id mismatch");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["accountId"],
        json!(TEST_ACCOUNT_ID),
        "accountId mismatch"
    );
    assert_eq!(
        args["inviteCode"],
        json!("INVITE-CANARY-TEST-NOT-A-REAL-SECRET"),
        "inviteCode must thread verbatim"
    );
    assert!(
        args.get("spaceId").is_none(),
        "spaceId must be absent in invite-code path"
    );

    // Empty invite code rejected.
    let bad = jmap_chat_client::methods::SpaceJoinInput::InviteCode("");
    let err = sc
        .space_join(&bad)
        .await
        .expect_err("space_join must reject empty invite_code");
    match err {
        jmap_base_client::ClientError::InvalidArgument(msg) => {
            assert!(msg.contains("invite_code may not be empty"), "got: {msg:?}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

/// `Space/join` with `SpaceJoinInput::SpaceId` must serialise the
/// space id under the `spaceId` wire key (space.rs:215-218) — the
/// direct-join path used for public Spaces.
#[tokio::test]
async fn space_join_via_space_id_serialises() {
    let server = MockServer::start().await;
    let resp_body = jmap_response(
        "Space/join",
        json!({
            "accountId": TEST_ACCOUNT_ID,
            "spaceId": "space-public"
        }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let space_id = Id::from("space-public");
    let input = jmap_chat_client::methods::SpaceJoinInput::SpaceId(&space_id);
    let _ = sc
        .space_join(&input)
        .await
        .expect("space_join: must succeed");

    let args = recorded_args(&server).await;
    assert_eq!(
        args["spaceId"],
        json!("space-public"),
        "spaceId must thread verbatim"
    );
    assert!(
        args.get("inviteCode").is_none(),
        "inviteCode must be absent in space-id path"
    );
}

/// `Space/set` update with a non-trivial patch — `name`, `description`
/// `Patch::Clear`, `is_public`, `add_members`, `remove_members`,
/// `update_members` with a per-member nick `Patch::Set` — must thread
/// every field through the per-id patch object with camelCase wire
/// keys (space.rs:243-325).
#[tokio::test]
async fn space_update_patch_full_member_management_serialises() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Space/set",
        SPACE_STATE_OLD,
        SPACE_STATE_NEW,
        json!({ "updated": { "space-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let space_id = Id::from("space-1");
    let new_member_id = Id::from("u-alice");
    let new_member_role = Id::from("role-admin");
    let role_ids = [new_member_role.clone()];
    let mut new_member = jmap_chat_client::methods::SpaceAddMemberInput::new(&new_member_id);
    new_member.role_ids = Some(&role_ids);
    let add_members = [new_member];
    let removed_id = Id::from("u-malice");
    let remove_members = [removed_id];
    let updated_member_id = Id::from("u-bob");
    let mut updated_member =
        jmap_chat_client::methods::SpaceUpdateMemberInput::new(&updated_member_id);
    updated_member.nick = jmap_chat_client::methods::Patch::Set("Bob the Brave");
    let update_members = [updated_member];

    let mut patch = jmap_chat_client::methods::SpacePatch::default();
    patch.name = Some("Engineering");
    patch.description = jmap_chat_client::methods::Patch::Clear;
    patch.is_public = Some(true);
    patch.add_members = Some(&add_members);
    patch.remove_members = Some(&remove_members);
    patch.update_members = Some(&update_members);

    let _ = sc
        .space_update(&space_id, &patch)
        .await
        .expect("space_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"]["space-1"];
    assert_eq!(patch_obj["name"], json!("Engineering"), "name mismatch");
    assert_eq!(
        patch_obj["description"],
        json!(null),
        "description Patch::Clear must serialise as null"
    );
    assert_eq!(patch_obj["isPublic"], json!(true), "isPublic mismatch");
    assert_eq!(
        patch_obj["addMembers"],
        json!([{ "id": "u-alice", "roleIds": ["role-admin"] }]),
        "addMembers must thread id + roleIds"
    );
    assert_eq!(
        patch_obj["removeMembers"],
        json!(["u-malice"]),
        "removeMembers must thread the id slice"
    );
    assert_eq!(
        patch_obj["updateMembers"],
        json!([{ "id": "u-bob", "nick": "Bob the Brave" }]),
        "updateMembers must thread id + nick"
    );
    assert!(
        patch_obj.get("iconBlobId").is_none(),
        "iconBlobId must be absent when Patch::Keep"
    );
}

/// `Space/set` update with `add_members: Some(&[])` and
/// `remove_members: Some(&[])` must omit BOTH keys from the wire patch
/// (space.rs:270-289, 291-298 — empty-slice guard skips the insert).
/// Confirms the "empty slice = no-change" semantic that mirrors the
/// `None` case but is reached via a different control-flow branch.
#[tokio::test]
async fn space_update_empty_member_slices_omit_keys() {
    let server = MockServer::start().await;
    let resp_body = set_response(
        "Space/set",
        SPACE_STATE_OLD,
        SPACE_STATE_NEW,
        json!({ "updated": { "space-1": null } }),
    );
    mock_jmap_post(&server, resp_body).await;

    let sc = helpers::make_client(&server);
    let space_id = Id::from("space-1");
    let empty_add: [jmap_chat_client::methods::SpaceAddMemberInput<'_>; 0] = [];
    let empty_remove: [Id; 0] = [];
    let mut patch = jmap_chat_client::methods::SpacePatch::default();
    patch.name = Some("Just renaming");
    patch.add_members = Some(&empty_add);
    patch.remove_members = Some(&empty_remove);
    let _ = sc
        .space_update(&space_id, &patch)
        .await
        .expect("space_update: must succeed");

    let args = recorded_args(&server).await;
    let patch_obj = &args["update"]["space-1"];
    assert_eq!(patch_obj["name"], json!("Just renaming"));
    assert!(
        patch_obj.get("addMembers").is_none(),
        "empty addMembers slice must be omitted (no-change semantic)"
    );
    assert!(
        patch_obj.get("removeMembers").is_none(),
        "empty removeMembers slice must be omitted (no-change semantic)"
    );
}
