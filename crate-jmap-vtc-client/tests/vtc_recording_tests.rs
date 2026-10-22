//! Wiremock integration tests for `VTCRecording/*` client methods.
//!
//! Oracle: the request examples of draft-atwood-jmap-vtc-00 (jmap-chat-spec
//! HEAD, line numbers cited per test) and the RFC 8620 §5 method shapes.

#[path = "helpers.rs"]
mod helpers;

use std::collections::BTreeMap;

use jmap_types::{query::Filter, Id, PatchObject, State};
use jmap_vtc_types::query::{
    VtcMediaComparator, VtcMediaSortProperty, VtcRecordingFilterCondition,
};
use jmap_vtc_types::RecordingState;
use serde_json::json;
use wiremock::MockServer;

/// draft-atwood-jmap-vtc-00 L1936-L1944, "Bob starts recording".
#[tokio::test]
async fn vtc_recording_set_create_matches_lifecycle_example() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCRecording/set",
        helpers::set_response(
            "bob-account",
            json!({ "created": { "r0": { "id": "01J4XP2SN9RXVV1SSEGJVT6GH0", "state": "recording" } } }),
        ),
    )
    .await;
    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_recording_set(
            Some(json!({ "r0": { "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0" } })),
            None,
            None,
        )
        .await
        .expect("VTCRecording/set must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCRecording/set", {
            "accountId": "bob-account",
            "create": {
                "r0": {
                    "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0"
                }
            }
        }])
    );
    assert_eq!(
        resp.created.expect("created")["r0"]["id"],
        json!("01J4XP2SN9RXVV1SSEGJVT6GH0")
    );
}

/// Stop recording: draft-atwood-jmap-vtc-00 L1154-L1163, "Example update
/// (stop recording)"; and destroy (L1169, "`destroy` removes the
/// VTCRecording metadata") in the same request.
#[tokio::test]
async fn vtc_recording_set_stop_and_destroy_matches_draft_example() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCRecording/set",
        helpers::set_response(
            "bob-account",
            json!({
                "updated": { "01J4XP2SN9RXVV1SSEGJVT6GH0": null },
                "destroyed": ["01J4XOLDREC0000000000000000"]
            }),
        ),
    )
    .await;
    let mut patch = PatchObject::new();
    patch.as_map_mut().insert("state".into(), json!("stopped"));
    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_recording_set(
            None,
            Some(BTreeMap::from([(
                Id::from("01J4XP2SN9RXVV1SSEGJVT6GH0"),
                patch,
            )])),
            Some(vec![Id::from("01J4XOLDREC0000000000000000")]),
        )
        .await
        .expect("VTCRecording/set must succeed");
    let sent = helpers::sent_invocation(&server).await;
    assert_eq!(sent[0], json!("VTCRecording/set"));
    assert_eq!(
        sent[1]["update"],
        json!({
            "01J4XP2SN9RXVV1SSEGJVT6GH0": {
                "state": "stopped"
            }
        })
    );
    assert_eq!(sent[1]["destroy"], json!(["01J4XOLDREC0000000000000000"]));
    assert_eq!(
        resp.destroyed,
        Some(vec![Id::from("01J4XOLDREC0000000000000000")])
    );
}

/// `VTCRecording/get` (L1106, "Standard JMAP `/get`"). The returned object
/// uses the §VTCRecording fields (L512-L540) of an available recording.
#[tokio::test]
async fn vtc_recording_get_parses_recording() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCRecording/get",
        json!({
            "accountId": "bob-account",
            "state": "s9",
            "list": [{
                "id": "01J4XP2SN9RXVV1SSEGJVT6GH0",
                "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
                "state": "available",
                "startedAt": "2026-06-05T14:40:00Z",
                "stoppedAt": "2026-06-05T15:00:00Z",
                "initiatedBy": "user:bob@example.com",
                "blobId": "Gb5f55i6",
                "size": 1048576,
                "duration": 1200,
                "mediaType": "video/webm"
            }],
            "notFound": []
        }),
    )
    .await;
    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_recording_get(None, None)
        .await
        .expect("VTCRecording/get must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCRecording/get", { "accountId": "bob-account" }])
    );
    let rec = &resp.list[0];
    assert_eq!(rec.state, RecordingState::Available);
    assert_eq!(rec.duration, Some(1200));
    assert_eq!(rec.media_type.as_deref(), Some("video/webm"));
}

/// `VTCRecording/changes` (L1112), RFC 8620 §5.2.
#[tokio::test]
async fn vtc_recording_changes_sends_since_state() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCRecording/changes",
        json!({
            "accountId": "bob-account",
            "oldState": "s8",
            "newState": "s9",
            "hasMoreChanges": true,
            "created": ["01J4XP2SN9RXVV1SSEGJVT6GH0"],
            "updated": [],
            "destroyed": []
        }),
    )
    .await;
    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_recording_changes(&State::from("s8"), Some(100))
        .await
        .expect("VTCRecording/changes must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCRecording/changes", {
            "accountId": "bob-account",
            "sinceState": "s8",
            "maxChanges": 100
        }])
    );
    assert!(resp.has_more_changes);
}

/// `VTCRecording/query` filter properties `callId` (L1177) and `state`
/// (L1180); sort property `startedAt` (L1194, "Default sort: `startedAt`
/// descending").
#[tokio::test]
async fn vtc_recording_query_sends_filter_and_sort() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCRecording/query",
        json!({
            "accountId": "bob-account",
            "queryState": "q3",
            "canCalculateChanges": true,
            "position": 0,
            "ids": ["01J4XP2SN9RXVV1SSEGJVT6GH0"]
        }),
    )
    .await;
    let mut cond = VtcRecordingFilterCondition::default();
    cond.call_id = Some(Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0"));
    cond.state = Some(RecordingState::Available);
    let mut sort = VtcMediaComparator::new(VtcMediaSortProperty::StartedAt);
    sort.is_ascending = false;
    let sc = helpers::make_client(&server, "bob-account");
    sc.vtc_recording_query(
        Some(&Filter::Condition(cond)),
        Some(&[sort]),
        &jmap_base_client::QueryParams {
            limit: Some(1),
            ..Default::default()
        },
    )
    .await
    .expect("VTCRecording/query must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCRecording/query", {
            "accountId": "bob-account",
            "filter": { "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0", "state": "available" },
            "sort": [{ "property": "startedAt", "isAscending": false }],
            "limit": 1
        }])
    );
}

/// `VTCRecording/queryChanges` (L1198), RFC 8620 §5.6.
#[tokio::test]
async fn vtc_recording_query_changes_sends_since_query_state() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCRecording/queryChanges",
        json!({
            "accountId": "bob-account",
            "oldQueryState": "q3",
            "newQueryState": "q4",
            "removed": [],
            "added": []
        }),
    )
    .await;
    let sc = helpers::make_client(&server, "bob-account");
    sc.vtc_recording_query_changes(&State::from("q3"), None, None, None, None, None)
        .await
        .expect("VTCRecording/queryChanges must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCRecording/queryChanges", {
            "accountId": "bob-account",
            "sinceQueryState": "q3"
        }])
    );
}
