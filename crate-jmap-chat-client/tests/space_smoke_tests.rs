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

use jmap_types::{Id, State};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// `Space/get` with `ids: None, properties: None` must omit both keys on
/// the wire (space.rs:34-42), consistent with `chat_get`.
#[tokio::test]
async fn space_get_omits_ids_and_properties_when_none() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Space/get",
            {
                "accountId": "A13824",
                "state": "sp-state-1",
                "list": [],
                "notFound": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let _ = sc
        .space_get(None, None)
        .await
        .expect("space_get: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["accountId"], json!("A13824"), "accountId mismatch");
    assert!(args.get("ids").is_none(), "ids must be omitted when None");
    assert!(
        args.get("properties").is_none(),
        "properties must be omitted when None"
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
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Space/get",
            {
                "accountId": "A13824",
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
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let resp = sc
        .space_get(None, None)
        .await
        .expect("space_get: must succeed");

    assert_eq!(resp.account_id.as_ref(), "A13824", "accountId mismatch");
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

/// `Space/changes` must thread `since_state` and `max_changes` and
/// reject empty `since_state` client-side (space.rs:57-62, RFC 8620 §5.2).
#[tokio::test]
async fn space_changes_passthrough_and_empty_state_rejected() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Space/changes",
            {
                "accountId": "A13824",
                "oldState": "sp-old",
                "newState": "sp-new",
                "hasMoreChanges": false,
                "created": [],
                "updated": [],
                "destroyed": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let since = State::from("sp-old");
    let _ = sc
        .space_changes(&since, Some(25))
        .await
        .expect("space_changes: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["sinceState"], json!("sp-old"), "sinceState mismatch");
    assert_eq!(args["maxChanges"], json!(25), "maxChanges mismatch");

    // Empty state guard.
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

/// `Space/set` destroy must thread `ids` to the `destroy` wire key and
/// reject the empty slice client-side
/// (space.rs:80-97, RFC 8620 §5.3).
#[tokio::test]
async fn space_destroy_threads_ids_and_rejects_empty() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Space/set",
            {
                "accountId": "A13824",
                "oldState": "sp-1",
                "newState": "sp-2",
                "created": null,
                "updated": null,
                "destroyed": ["space-doomed"],
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let ids = [Id::from("space-doomed")];
    let _ = sc
        .space_destroy(&ids)
        .await
        .expect("space_destroy: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["destroy"],
        json!(["space-doomed"]),
        "destroy ids must thread through"
    );

    // Empty-slice guard.
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
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Space/query",
            {
                "accountId": "A13824",
                "queryState": "sq-1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let mut input = jmap_chat_client::methods::SpaceQueryInput::default();
    input.position = Some(0);
    input.limit = Some(20);
    let _ = sc
        .space_query(&input)
        .await
        .expect("space_query: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(args["filter"], json!(null), "filter must be null");
    assert_eq!(args["position"], json!(0), "position must thread");
    assert_eq!(args["limit"], json!(20), "limit must thread");
}

/// `Space/query` with `filter_is_public: Some(true)` must serialize a
/// filter object containing `{ "isPublic": true }` (space.rs:112-114).
#[tokio::test]
async fn space_query_filter_is_public_serializes() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Space/query",
            {
                "accountId": "A13824",
                "queryState": "sq-1",
                "canCalculateChanges": true,
                "position": 0,
                "ids": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let mut input = jmap_chat_client::methods::SpaceQueryInput::default();
    input.filter_is_public = Some(true);
    let _ = sc
        .space_query(&input)
        .await
        .expect("space_query: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
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
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Space/queryChanges",
            {
                "accountId": "A13824",
                "oldQueryState": "sqc-old",
                "newQueryState": "sqc-new",
                "total": null,
                "removed": [],
                "added": []
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let since = State::from("sqc-old");
    let _ = sc
        .space_query_changes(&since, Some(50))
        .await
        .expect("space_query_changes: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
    assert_eq!(
        args["sinceQueryState"],
        json!("sqc-old"),
        "sinceQueryState mismatch"
    );
    assert_eq!(args["maxChanges"], json!(50), "maxChanges mismatch");
}

/// `Space/set` create must serialize the create object with `name` and
/// any provided optional fields, keyed by the caller-supplied creation
/// id (space.rs:178-194). Empty `name` must short-circuit
/// (space.rs:173-177).
#[tokio::test]
async fn space_create_serializes_create_object_and_rejects_empty_name() {
    let server = MockServer::start().await;
    let resp_body = json!({
        "sessionState": "s1",
        "methodResponses": [[
            "Space/set",
            {
                "accountId": "A13824",
                "oldState": "sp-1",
                "newState": "sp-2",
                "created": { "my-space-key": { "id": "space-new-1" } },
                "updated": null,
                "destroyed": null,
                "notCreated": null,
                "notUpdated": null,
                "notDestroyed": null
            },
            "r1"
        ]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&resp_body))
        .mount(&server)
        .await;

    let sc = helpers::make_client(&server);
    let input = jmap_chat_client::methods::SpaceCreateInput::new("Engineering")
        .with_client_id("my-space-key");
    let _ = sc
        .space_create(&input)
        .await
        .expect("space_create: must succeed");

    let reqs = server
        .received_requests()
        .await
        .expect("must have recorded requests");
    let body: serde_json::Value =
        serde_json::from_slice(&reqs[0].body).expect("request body must be valid JSON");
    let args = &body["methodCalls"][0][1];
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

    // Empty-name guard.
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
