//! Wiremock integration tests for `VTCCall/*` client methods.
//!
//! Oracle: the request and response examples of draft-atwood-jmap-vtc-00
//! (jmap-chat-spec HEAD, line numbers cited per test) and the RFC 8620 §5
//! method shapes. Expected JSON is copied from the draft, never derived
//! from the code under test.

#[path = "helpers.rs"]
mod helpers;

use std::collections::BTreeMap;

use jmap_types::{query::Filter, Id, PatchObject, State};
use jmap_vtc_types::query::{VtcCallComparator, VtcCallFilterCondition, VtcCallSortProperty};
use jmap_vtc_types::{CallState, CallType};
use serde_json::json;
use wiremock::MockServer;

/// draft-atwood-jmap-vtc-00 L1846-L1856, "Bob's client sends `VTCCall/set`".
#[tokio::test]
async fn vtc_call_set_create_ring_matches_lifecycle_example() {
    let server = MockServer::start().await;
    // Server response: draft L1862-L1874, plus the RFC 8620 §5.3 `newState`
    // the abridged example leaves out.
    helpers::respond(
        &server,
        "VTCCall/set",
        helpers::set_response(
            "bob-account",
            json!({
                "created": {
                    "c0": {
                        "id": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
                        "state": "ringing",
                        "initiatorId": "user:bob@example.com",
                        "createdAt": "2026-06-05T14:30:00Z",
                        "joinUri": "https://meet.example.com/room/xkz",
                        "activeParticipantCount": 1
                    }
                }
            }),
        ),
    )
    .await;

    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_call_set(
            Some(json!({
                "c0": {
                    "callType": "ring",
                    "targetParticipantIds": ["user:alice@example.com"],
                    "mediaTypes": ["audio", "video"],
                    "subject": "Quick sync"
                }
            })),
            None,
            None,
        )
        .await
        .expect("VTCCall/set must succeed");

    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCCall/set", {
            "accountId": "bob-account",
            "create": {
                "c0": {
                    "callType": "ring",
                    "targetParticipantIds": ["user:alice@example.com"],
                    "mediaTypes": ["audio", "video"],
                    "subject": "Quick sync"
                }
            }
        }])
    );
    // L1864-L1872: the created entry carries only server-set properties.
    let created = resp.created.expect("created must be present");
    assert_eq!(created["c0"]["id"], json!("01J4XKZQN4MWVT8PPBEHTJ3AB0"));
    assert_eq!(created["c0"]["state"], json!("ringing"));
}

/// draft-atwood-jmap-vtc-00 L1997-L2004, "Bob ends the call".
#[tokio::test]
async fn vtc_call_set_end_matches_lifecycle_example() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCCall/set",
        helpers::set_response(
            "bob-account",
            json!({ "updated": { "01J4XKZQN4MWVT8PPBEHTJ3AB0": null } }),
        ),
    )
    .await;

    let mut patch = PatchObject::new();
    patch.as_map_mut().insert("state".into(), json!("ended"));
    let update = BTreeMap::from([(Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0"), patch)]);

    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_call_set(None, Some(update), None)
        .await
        .expect("VTCCall/set must succeed");

    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCCall/set", {
            "accountId": "bob-account",
            "update": {
                "01J4XKZQN4MWVT8PPBEHTJ3AB0": {
                    "state": "ended"
                }
            }
        }])
    );
    let updated = resp.updated.expect("updated must be present");
    assert!(updated.contains_key(&Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0")));
}

/// draft-atwood-jmap-vtc-00 L2031-L2043, the room-call-with-lobby create.
#[tokio::test]
async fn vtc_call_set_create_room_matches_lifecycle_example() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCCall/set",
        helpers::set_response("mod-account", json!({})),
    )
    .await;

    let sc = helpers::make_client(&server, "mod-account");
    sc.vtc_call_set(
        Some(json!({
            "c0": {
                "callType": "room",
                "mediaTypes": ["audio", "video"],
                "subject": "Office Hours",
                "lobbyEnabled": true,
                "spaceId": "01J3ABCDEF0000000000000000",
                "channelId": "01J3ABCDEF1111111111111111"
            }
        })),
        None,
        None,
    )
    .await
    .expect("VTCCall/set must succeed");

    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCCall/set", {
            "accountId": "mod-account",
            "create": {
                "c0": {
                    "callType": "room",
                    "mediaTypes": ["audio", "video"],
                    "subject": "Office Hours",
                    "lobbyEnabled": true,
                    "spaceId": "01J3ABCDEF0000000000000000",
                    "channelId": "01J3ABCDEF1111111111111111"
                }
            }
        }])
    );
}

/// `VTCCall/get` "Standard JMAP `/get` ({{RFC8620}} Section 5.1)" (L576);
/// the returned object is the draft's example response, L581-L611.
#[tokio::test]
async fn vtc_call_get_parses_draft_example_response() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCCall/get",
        json!({
            "accountId": "bob-account",
            "state": "s5",
            "list": [{
                "id": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
                "callType": "ring",
                "state": "active",
                "createdAt": "2026-06-05T14:30:00Z",
                "endedAt": null,
                "endReason": null,
                "initiatorId": "user:bob@example.com",
                "subject": "Quick sync",
                "joinUri": "https://meet.example.com/room/xkz",
                "joinPassword": null,
                "mediaTypes": ["audio", "video"],
                "activeParticipantCount": 2,
                "lobbyEnabled": false,
                "policy": {
                    "muteOnEntry": false,
                    "videoOffOnEntry": false,
                    "participantsCanUnmute": true,
                    "participantsCanShareScreen": true,
                    "participantsCanStartVideo": true
                },
                "parentCallId": null,
                "breakoutRoomIds": [],
                "scheduledStartAt": null,
                "gatewayPin": null,
                "e2eeEnabled": false,
                "chatId": "01J4XL2RN5NXWS9QQCFJVJ4BC0",
                "spaceId": null,
                "channelId": null,
                "calendarEventId": null
            }],
            "notFound": []
        }),
    )
    .await;

    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_call_get(Some(&[Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0")]), None)
        .await
        .expect("VTCCall/get must succeed");

    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCCall/get", {
            "accountId": "bob-account",
            "ids": ["01J4XKZQN4MWVT8PPBEHTJ3AB0"]
        }])
    );
    let call = &resp.list[0];
    assert_eq!(call.call_type, CallType::Ring);
    assert_eq!(call.state, CallState::Active);
    assert_eq!(call.active_participant_count, 2);
    assert_eq!(
        call.chat_id,
        Some(Some(Id::from("01J4XL2RN5NXWS9QQCFJVJ4BC0")))
    );
}

/// `VTCCall/changes` "Standard JMAP `/changes` ({{RFC8620}} Section 5.2)"
/// (L616). RFC 8620 §5.2 request arguments: `accountId`, `sinceState`,
/// `maxChanges`.
#[tokio::test]
async fn vtc_call_changes_sends_rfc8620_arguments() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCCall/changes",
        json!({
            "accountId": "bob-account",
            "oldState": "s200",
            "newState": "s201",
            "hasMoreChanges": false,
            "created": [],
            "updated": ["01J4XKZQN4MWVT8PPBEHTJ3AB0"],
            "destroyed": []
        }),
    )
    .await;

    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_call_changes(&State::from("s200"), Some(10))
        .await
        .expect("VTCCall/changes must succeed");

    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCCall/changes", {
            "accountId": "bob-account",
            "sinceState": "s200",
            "maxChanges": 10
        }])
    );
    assert_eq!(resp.new_state, "s201");
}

/// `VTCCall/query` filter property `state` (L784-L785, "One of
/// `"creating"`, `"ringing"`, `"active"`, `"pending"`, or `"ended"`") and
/// `callType` (L787-L788); sort property `createdAt` (L816); RFC 8620 §5.5
/// Comparator `{"property", "isAscending"}`.
#[tokio::test]
async fn vtc_call_query_sends_filter_and_sort() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCCall/query",
        json!({
            "accountId": "bob-account",
            "queryState": "q1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": ["01J4XKZQN4MWVT8PPBEHTJ3AB0"]
        }),
    )
    .await;

    let mut cond = VtcCallFilterCondition::default();
    cond.state = Some(CallState::Active);
    cond.call_type = Some(CallType::Room);
    let mut sort = VtcCallComparator::new(VtcCallSortProperty::CreatedAt);
    sort.is_ascending = false;

    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_call_query(
            Some(&Filter::Condition(cond)),
            Some(&[sort]),
            &jmap_base_client::QueryParams {
                position: Some(0),
                limit: Some(20),
                ..Default::default()
            },
        )
        .await
        .expect("VTCCall/query must succeed");

    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCCall/query", {
            "accountId": "bob-account",
            "filter": { "state": "active", "callType": "room" },
            "sort": [{ "property": "createdAt", "isAscending": false }],
            "position": 0,
            "limit": 20
        }])
    );
    assert_eq!(resp.ids, vec![Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0")]);
}

/// `VTCCall/queryChanges` "Standard JMAP `/queryChanges` ({{RFC8620}}
/// Section 5.6)" (L820). RFC 8620 §5.6 request arguments: `sinceQueryState`,
/// `maxChanges`, `upToId`, `calculateTotal`.
#[tokio::test]
async fn vtc_call_query_changes_sends_rfc8620_arguments() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCCall/queryChanges",
        json!({
            "accountId": "bob-account",
            "oldQueryState": "q1",
            "newQueryState": "q2",
            "total": 1,
            "removed": [],
            "added": [{ "id": "01J4XKZQN4MWVT8PPBEHTJ3AB0", "index": 0 }]
        }),
    )
    .await;

    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_call_query_changes(
            &State::from("q1"),
            Some(5),
            None,
            None,
            Some(&Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0")),
            Some(true),
        )
        .await
        .expect("VTCCall/queryChanges must succeed");

    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCCall/queryChanges", {
            "accountId": "bob-account",
            "sinceQueryState": "q1",
            "maxChanges": 5,
            "upToId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
            "calculateTotal": true
        }])
    );
    assert_eq!(resp.total, Some(1));
}

/// RFC 8620 §3.3: `using` names the capabilities the request uses; the VTC
/// methods belong to `urn:ietf:params:jmap:vtc` (draft L188).
#[tokio::test]
async fn vtc_requests_declare_vtc_capability() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCCall/get",
        json!({ "accountId": "bob-account", "state": "s1", "list": [], "notFound": [] }),
    )
    .await;

    let sc = helpers::make_client(&server, "bob-account");
    sc.vtc_call_get(None, None)
        .await
        .expect("VTCCall/get must succeed");

    let body = helpers::sent_request(&server).await;
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:vtc"])
    );
}

/// A session without a primary `urn:ietf:params:jmap:vtc` account cannot
/// name the `accountId` RFC 8620 §5.1 requires; the client fails before
/// sending anything.
#[tokio::test]
async fn vtc_call_get_without_vtc_account_is_invalid_session() {
    use jmap_vtc_client::JmapVtcExt;
    let server = MockServer::start().await;
    let mut session = helpers::make_session(&server, "bob-account");
    session.primary_accounts.clear();
    let client = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("JmapClient construction must succeed");
    let sc = client.with_vtc_session(session);

    let err = sc
        .vtc_call_get(None, None)
        .await
        .expect_err("must fail without a VTC account");
    assert!(
        matches!(err, jmap_base_client::ClientError::InvalidSession(_)),
        "got {err:?}"
    );
    let reqs = server.received_requests().await.expect("recording enabled");
    assert!(reqs.is_empty(), "no request may be sent");
}

/// RFC 8620 §5.2: `sinceState` is a state string the server returned; an
/// empty one is rejected before any I/O.
#[tokio::test]
async fn vtc_call_changes_rejects_empty_since_state() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server, "bob-account");
    let err = sc
        .vtc_call_changes(&State::from(""), None)
        .await
        .expect_err("empty sinceState must be rejected");
    assert!(
        matches!(err, jmap_base_client::ClientError::InvalidArgument(_)),
        "got {err:?}"
    );
}

/// RFC 8620 §5.3: `create`, `update` and `destroy` are each optional; a
/// request with none of them does nothing, so the client refuses it.
#[tokio::test]
async fn vtc_call_set_rejects_empty_request() {
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server, "bob-account");
    let err = sc
        .vtc_call_set(None, None, None)
        .await
        .expect_err("all-None /set must be rejected");
    assert!(
        matches!(err, jmap_base_client::ClientError::InvalidArgument(_)),
        "got {err:?}"
    );
}

/// draft-atwood-jmap-vtc-00 L1654: "A user's calls can be spread across
/// several hosting accounts; a client that lists them queries each account
/// the user can access." The `for_account` handle names the shared
/// account; the original handle still names the primary one.
#[tokio::test]
async fn vtc_call_query_in_hosting_account_sends_its_account_id() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCCall/query",
        json!({
            "accountId": "bob-account",
            "queryState": "q1",
            "canCalculateChanges": false,
            "position": 0,
            "ids": ["01J4XKZQN4MWVT8PPBEHTJ3AB0"]
        }),
    )
    .await;

    let sc = helpers::make_shared_client(&server, "alice-account", "bob-account");
    assert_eq!(sc.vtc_account_id().expect("target"), "bob-account");
    sc.vtc_call_query(None, None, &jmap_base_client::QueryParams::default())
        .await
        .expect("VTCCall/query must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCCall/query", { "accountId": "bob-account" }])
    );
}

/// RFC 8620 §2: `accountCapabilities` is "the set of capability URIs for
/// the methods supported in this account". An account missing from the
/// session, or one without `urn:ietf:params:jmap:vtc`, cannot host a call,
/// so `for_account` refuses it without I/O.
#[tokio::test]
async fn for_account_rejects_account_without_vtc() {
    use jmap_vtc_client::JmapVtcExt;
    let server = MockServer::start().await;
    let sc = helpers::make_client(&server, "bob-account");
    for id in ["carol-account", ""] {
        let err = sc
            .for_account(id)
            .expect_err("unknown account must be rejected");
        assert!(
            matches!(err, jmap_base_client::ClientError::InvalidArgument(_)),
            "got {err:?}"
        );
    }

    let mut session = helpers::make_session(&server, "bob-account");
    session
        .accounts
        .get_mut("bob-account")
        .expect("account present")
        .account_capabilities
        .clear();
    let client = jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("JmapClient construction must succeed");
    let err = client
        .with_vtc_session(session)
        .for_account("bob-account")
        .expect_err("account without the VTC capability must be rejected");
    assert!(
        matches!(err, jmap_base_client::ClientError::InvalidArgument(_)),
        "got {err:?}"
    );
    let reqs = server.received_requests().await.expect("recording enabled");
    assert!(reqs.is_empty(), "no request may be sent");
}
