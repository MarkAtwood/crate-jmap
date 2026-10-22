//! Wiremock integration tests for `VTCLivestream/*` client methods.
//!
//! Oracle: the request examples of draft-atwood-jmap-vtc-00 (jmap-chat-spec
//! HEAD, line numbers cited per test) and the RFC 8620 §5 method shapes.

#[path = "helpers.rs"]
mod helpers;

use std::collections::BTreeMap;

use jmap_types::{query::Filter, Id, PatchObject, State};
use jmap_vtc_types::query::{
    VtcLivestreamFilterCondition, VtcMediaComparator, VtcMediaSortProperty,
};
use jmap_vtc_types::LivestreamState;
use serde_json::json;
use wiremock::MockServer;

/// Start a livestream: draft-atwood-jmap-vtc-00 L1235-L1246, "Example
/// create".
#[tokio::test]
async fn vtc_livestream_set_create_matches_draft_example() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCLivestream/set",
        helpers::set_response(
            "bob-account",
            json!({ "created": { "s0": { "id": "01J4XQ7VN0SYWV2TTFHJVW7JJ0", "state": "starting" } } }),
        ),
    )
    .await;
    let sc = helpers::make_client(&server, "bob-account");
    sc.vtc_livestream_set(
        Some(json!({
            "s0": {
                "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
                "streamUri": "rtmp://live.example.com/ingest",
                "streamKey": "sk_live_abc123def456",
                "platform": "youtube"
            }
        })),
        None,
        None,
        None,
    )
    .await
    .expect("VTCLivestream/set must succeed");
    let sent = helpers::sent_invocation(&server).await;
    assert_eq!(sent[0], json!("VTCLivestream/set"));
    assert_eq!(sent[1]["accountId"], json!("bob-account"));
    assert_eq!(
        sent[1]["create"],
        json!({
            "s0": {
                "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
                "streamUri": "rtmp://live.example.com/ingest",
                "streamKey": "sk_live_abc123def456",
                "platform": "youtube"
            }
        })
    );
}

/// Stop a livestream: draft-atwood-jmap-vtc-00 L1257-L1264.
#[tokio::test]
async fn vtc_livestream_set_stop_matches_draft_example() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCLivestream/set",
        helpers::set_response(
            "bob-account",
            json!({ "updated": { "01J4XQ7VN0SYWV2TTFHJVW7JJ0": { "stoppedAt": "2026-06-05T15:10:00Z" } } }),
        ),
    )
    .await;
    let mut patch = PatchObject::new();
    patch.as_map_mut().insert("state".into(), json!("stopped"));
    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_livestream_set(
            None,
            Some(BTreeMap::from([(
                Id::from("01J4XQ7VN0SYWV2TTFHJVW7JJ0"),
                patch,
            )])),
            None,
            None,
        )
        .await
        .expect("VTCLivestream/set must succeed");
    let sent = helpers::sent_invocation(&server).await;
    assert_eq!(
        sent[1]["update"],
        json!({
            "01J4XQ7VN0SYWV2TTFHJVW7JJ0": {
                "state": "stopped"
            }
        })
    );
    // L1254: "The server sets `stoppedAt` to the current time."
    let updated = resp.updated.expect("updated");
    assert_eq!(
        updated[&Id::from("01J4XQ7VN0SYWV2TTFHJVW7JJ0")],
        Some(json!({ "stoppedAt": "2026-06-05T15:10:00Z" }))
    );
}

/// `VTCLivestream/get` (L1204). A non-moderator receives `streamKey` as
/// `""` (L1206, "Non-moderators receive `streamKey` as an empty string").
#[tokio::test]
async fn vtc_livestream_get_parses_redacted_livestream() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCLivestream/get",
        json!({
            "accountId": "bob-account",
            "state": "s4",
            "list": [{
                "id": "01J4XQ7VN0SYWV2TTFHJVW7JJ0",
                "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
                "state": "live",
                "platform": "youtube",
                "streamUri": "rtmp://live.example.com/ingest",
                "streamKey": "",
                "startedAt": "2026-06-05T14:50:00Z",
                "stoppedAt": null
            }],
            "notFound": []
        }),
    )
    .await;
    let sc = helpers::make_shared_client(&server, "alice-account", "bob-account");
    let resp = sc
        .vtc_livestream_get(Some(&[Id::from("01J4XQ7VN0SYWV2TTFHJVW7JJ0")]), None)
        .await
        .expect("VTCLivestream/get must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCLivestream/get", {
            "accountId": "bob-account",
            "ids": ["01J4XQ7VN0SYWV2TTFHJVW7JJ0"]
        }])
    );
    let ls = &resp.list[0];
    assert_eq!(ls.state, LivestreamState::Live);
    assert_eq!(ls.stream_key, "");
}

/// `VTCLivestream/changes` (L1210), RFC 8620 §5.2.
#[tokio::test]
async fn vtc_livestream_changes_sends_since_state() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCLivestream/changes",
        json!({
            "accountId": "bob-account",
            "oldState": "s3",
            "newState": "s4",
            "hasMoreChanges": false,
            "created": [],
            "updated": [],
            "destroyed": ["01J4XQ7VN0SYWV2TTFHJVW7JJ0"]
        }),
    )
    .await;
    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_livestream_changes(&State::from("s3"), None)
        .await
        .expect("VTCLivestream/changes must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCLivestream/changes", {
            "accountId": "bob-account",
            "sinceState": "s3"
        }])
    );
    assert_eq!(resp.destroyed, vec![Id::from("01J4XQ7VN0SYWV2TTFHJVW7JJ0")]);
}

/// `VTCLivestream/query` filter properties `callId` (L1282), `state`
/// (L1285) and `platform` (L1288); sort property `startedAt` (L1293).
#[tokio::test]
async fn vtc_livestream_query_sends_filter_and_sort() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCLivestream/query",
        json!({
            "accountId": "bob-account",
            "queryState": "q1",
            "canCalculateChanges": false,
            "position": 0,
            "ids": []
        }),
    )
    .await;
    let mut cond = VtcLivestreamFilterCondition::default();
    cond.call_id = Some(Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0"));
    cond.state = Some(LivestreamState::Live);
    cond.platform = Some("youtube".into());
    let sort = VtcMediaComparator::new(VtcMediaSortProperty::StartedAt);
    let sc = helpers::make_client(&server, "bob-account");
    sc.vtc_livestream_query(
        Some(&Filter::Condition(cond)),
        Some(&[sort]),
        &jmap_base_client::QueryParams::default(),
    )
    .await
    .expect("VTCLivestream/query must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCLivestream/query", {
            "accountId": "bob-account",
            "filter": {
                "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
                "state": "live",
                "platform": "youtube"
            },
            "sort": [{ "property": "startedAt", "isAscending": true }]
        }])
    );
}

/// `VTCLivestream/queryChanges` (L1297), RFC 8620 §5.6.
#[tokio::test]
async fn vtc_livestream_query_changes_sends_since_query_state() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCLivestream/queryChanges",
        json!({
            "accountId": "bob-account",
            "oldQueryState": "q1",
            "newQueryState": "q2",
            "removed": [],
            "added": []
        }),
    )
    .await;
    let sc = helpers::make_client(&server, "bob-account");
    sc.vtc_livestream_query_changes(&State::from("q1"), Some(3), None, None, None, Some(false))
        .await
        .expect("VTCLivestream/queryChanges must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCLivestream/queryChanges", {
            "accountId": "bob-account",
            "sinceQueryState": "q1",
            "maxChanges": 3,
            "calculateTotal": false
        }])
    );
}
