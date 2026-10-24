//! Wiremock integration tests for the `SceneAsset/*`, `SceneRegion/*`,
//! `SceneObject/*` and `SceneAvatar/*` client methods.
//!
//! Oracle: the request and response examples of draft-atwood-jmap-scene-00
//! (jmap-chat-spec 6bcd9d8, line numbers cited per test) and the RFC 8620
//! §5 method shapes. Expected JSON is copied from the draft, never derived
//! from the code under test.

use std::collections::BTreeMap;

use jmap_base_client::ClientError;
use jmap_scene_client::{JmapSceneExt, SessionClient};
use jmap_scene_types::{
    Filter, SceneAssetComparator, SceneAssetFilterCondition, SceneAssetSortProperty,
    SceneAvatarComparator, SceneAvatarFilterCondition, SceneAvatarSortProperty, SceneInteraction,
    SceneObjectComparator, SceneObjectFilterCondition, SceneObjectSortProperty,
    SceneRegionComparator, SceneRegionSortProperty, WithinRadius,
};
use jmap_types::{Id, PatchObject, SetErrorType, State, UTCDate};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The session JSON for a user whose primary Scene account is `account_id`.
///
/// Oracle: RFC 8620 §2.1 session shape; draft-atwood-jmap-scene-00 L208
/// "The value of `capabilities["urn:ietf:params:jmap:scene"]` at the
/// session level is an empty JSON object `{}`."
fn session_json(server: &MockServer, account_id: &str) -> Value {
    json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:scene": {}
        },
        "accounts": {
            account_id: {
                "name": "alice@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": { "urn:ietf:params:jmap:scene": {} }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:scene": account_id },
        "username": "alice@example.com",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}", server.uri()),
        "uploadUrl": format!("{}/ul/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/sse/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}", server.uri()),
        "state": "s1"
    })
}

fn client_for(server: &MockServer, session: Value) -> SessionClient {
    let session: jmap_base_client::Session =
        serde_json::from_value(session).expect("RFC 8620 §2.1 session must deserialize");
    jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .expect("JmapClient construction must succeed")
    .with_scene_session(session)
}

fn make_client(server: &MockServer, account_id: &str) -> SessionClient {
    client_for(server, session_json(server, account_id))
}

/// Answer every POST to the API URL with one `[name, args, "r1"]`
/// response invocation (RFC 8620 §3.4).
async fn respond(server: &MockServer, name: &str, args: Value) {
    let body = json!({
        "sessionState": "s1",
        "methodResponses": [[name, args, "r1"]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(server)
        .await;
}

/// The request body of the single request the client sent.
async fn sent_request(server: &MockServer) -> Value {
    let reqs = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(reqs.len(), 1, "exactly one request must be sent");
    serde_json::from_slice(&reqs[0].body).expect("request body must be JSON")
}

/// The `[name, arguments]` of the single invocation the client sent,
/// without the call id (the draft's examples use their own call ids).
async fn sent_invocation(server: &MockServer) -> Value {
    let body = sent_request(server).await;
    let calls = body["methodCalls"]
        .as_array()
        .expect("methodCalls must be an array");
    assert_eq!(calls.len(), 1, "exactly one method call must be sent");
    json!([calls[0][0], calls[0][1]])
}

fn date(s: &str) -> UTCDate {
    UTCDate::new_validated(s).expect("RFC 3339 UTC date")
}

/// draft-atwood-jmap-scene-00 L718-L770, "Example create request with full
/// JMAP envelope" and its response.
#[tokio::test]
async fn scene_asset_set_create_matches_full_envelope_example() {
    let server = MockServer::start().await;
    respond(
        &server,
        "SceneAsset/set",
        json!({
            "accountId": "acct-001",
            "oldState": "state-asset-5",
            "newState": "state-asset-6",
            "created": {
                "a0": {
                    "id": "01J5AST0000000000000000001",
                    "accountId": "acct-001",
                    "ownerId": "user:curator@example.com",
                    "size": 2483712,
                    "createdAt": "2026-06-06T10:05:00Z"
                }
            },
            "updated": null,
            "destroyed": null,
            "notCreated": null,
            "notUpdated": null,
            "notDestroyed": null
        }),
    )
    .await;

    let sc = make_client(&server, "acct-001");
    let resp = sc
        .scene_asset_set(
            Some(json!({
                "a0": {
                    "blobId": "Gc0f032d390a5d5fa8a35",
                    "mediaType": "model/gltf-binary",
                    "name": "Sculpture: Convergence",
                    "assetUri": "https://cdn.example.com/assets/sculpture-001.glb",
                    "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                }
            })),
            None,
            None,
            None,
        )
        .await
        .expect("SceneAsset/set must succeed");

    let body = sent_request(&server).await;
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:scene"])
    );
    assert_eq!(
        sent_invocation(&server).await,
        json!(["SceneAsset/set", {
            "accountId": "acct-001",
            "create": {
                "a0": {
                    "blobId": "Gc0f032d390a5d5fa8a35",
                    "mediaType": "model/gltf-binary",
                    "name": "Sculpture: Convergence",
                    "assetUri": "https://cdn.example.com/assets/sculpture-001.glb",
                    "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                }
            }
        }])
    );
    let created = resp.created;
    assert_eq!(
        created["a0"].id,
        Some(Id::from("01J5AST0000000000000000001"))
    );
    assert_eq!(created["a0"].extra["size"], json!(2483712));
}

/// draft-atwood-jmap-scene-00 L898-L913, "Example query -- glTF assets
/// named "sculpture" created in a date range". The response is a
/// hand-written RFC 8620 §5.5 `/query` response.
#[tokio::test]
async fn scene_asset_query_matches_example() {
    let server = MockServer::start().await;
    respond(
        &server,
        "SceneAsset/query",
        json!({
            "accountId": "account-xyz",
            "queryState": "q1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": ["01J5AST0000000000000000001"]
        }),
    )
    .await;

    let mut cond = SceneAssetFilterCondition::default();
    cond.media_type = Some("model/gltf-binary".into());
    cond.name = Some("sculpture".into());
    cond.created_after = Some(date("2026-03-01T00:00:00Z"));
    cond.created_before = Some(date("2026-06-01T00:00:00Z"));
    let mut by_created = SceneAssetComparator::new(SceneAssetSortProperty::CreatedAt);
    by_created.is_ascending = false;

    let sc = make_client(&server, "account-xyz");
    let resp = sc
        .scene_asset_query(
            Some(&Filter::Condition(cond)),
            Some(&[by_created]),
            &jmap_base_client::QueryParams {
                position: Some(0),
                limit: Some(25),
                ..Default::default()
            },
        )
        .await
        .expect("SceneAsset/query must succeed");

    assert_eq!(
        sent_invocation(&server).await,
        json!(["SceneAsset/query", {
            "accountId": "account-xyz",
            "filter": {
                "mediaType": "model/gltf-binary",
                "name": "sculpture",
                "createdAfter": "2026-03-01T00:00:00Z",
                "createdBefore": "2026-06-01T00:00:00Z"
            },
            "sort": [{"property": "createdAt", "isAscending": false}],
            "position": 0,
            "limit": 25
        }])
    );
    assert_eq!(resp.ids, vec![Id::from("01J5AST0000000000000000001")]);
}

/// `filter` is generic over its condition type, so a caller can send a
/// condition the draft does not define. RFC 8620 §5.5: FilterCondition
/// properties are type-specific; the `mediaType` key is the
/// SceneAsset/query condition used above. The vendor key and the expected
/// JSON are hand-written.
#[tokio::test]
async fn scene_asset_query_sends_caller_condition_type() {
    let server = MockServer::start().await;
    respond(
        &server,
        "SceneAsset/query",
        json!({
            "accountId": "account-xyz",
            "queryState": "q1",
            "canCalculateChanges": false,
            "position": 0,
            "ids": []
        }),
    )
    .await;

    let filter = Filter::Condition(json!({
        "mediaType": "model/gltf-binary",
        "example.com:license": "cc-by"
    }));
    let sc = make_client(&server, "account-xyz");
    sc.scene_asset_query(
        Some(&filter),
        None::<&[SceneAssetComparator]>,
        &jmap_base_client::QueryParams::default(),
    )
    .await
    .expect("SceneAsset/query must succeed");

    assert_eq!(
        sent_invocation(&server).await,
        json!(["SceneAsset/query", {
            "accountId": "account-xyz",
            "filter": {
                "mediaType": "model/gltf-binary",
                "example.com:license": "cc-by"
            }
        }])
    );
}

/// draft-atwood-jmap-scene-00 L943-L1037, the `SceneRegion/get` request
/// and response.
#[tokio::test]
async fn scene_region_get_matches_example() {
    let server = MockServer::start().await;
    respond(
        &server,
        "SceneRegion/get",
        json!({
            "accountId": "acct-001",
            "state": "state-region-47",
            "list": [
                {
                    "id": "01J5ABC0000000000000000001",
                    "accountId": "acct-001",
                    "ownerId": "user:curator@example.com",
                    "name": "Gallery East Wing",
                    "description": "Contemporary sculpture exhibition",
                    "bounds": { "min": [-50, 0, -50], "max": [50, 10, 50] },
                    "environment": {
                        "skyColor": "#87CEEB",
                        "ambientIntensity": 0.6,
                        "gravity": 9.81
                    },
                    "simulationUri": "wss://sim.example.com/regions/01J5ABC0001",
                    "viewHint": "3d",
                    "spawnPosition": [0, 0, 10],
                    "spawnOrientation": [0, 0, 0, 1],
                    "activeAvatarCount": 3,
                    "accessPolicy": "public",
                    "invitedUserIds": [],
                    "createdAt": "2026-06-01T10:00:00Z",
                    "updatedAt": "2026-06-05T14:30:00Z",
                    "chatId": "01J5CHAT000000000000000099",
                    "spaceId": null,
                    "channelId": null,
                    "activeCallId": null
                },
                {
                    "id": "01J5ABC0000000000000000002",
                    "accountId": "acct-001",
                    "ownerId": "user:curator@example.com",
                    "name": "Design Review Room",
                    "description": null,
                    "bounds": { "min": [-20, 0, -20], "max": [20, 8, 20] },
                    "environment": null,
                    "simulationUri": "wss://sim.example.com/regions/01J5ABC0002",
                    "viewHint": "3d",
                    "spawnPosition": [0, 0, 5],
                    "spawnOrientation": [0, 0, 0, 1],
                    "activeAvatarCount": 0,
                    "accessPolicy": "invite",
                    "invitedUserIds": ["user:alice@example.com", "user:bob@example.com"],
                    "createdAt": "2026-06-02T08:00:00Z",
                    "updatedAt": "2026-06-02T08:00:00Z",
                    "chatId": null,
                    "spaceId": null,
                    "channelId": null,
                    "activeCallId": null
                }
            ],
            "notFound": []
        }),
    )
    .await;

    let ids = [
        Id::from("01J5ABC0000000000000000001"),
        Id::from("01J5ABC0000000000000000002"),
    ];
    let sc = make_client(&server, "acct-001");
    let resp = sc
        .scene_region_get(Some(&ids), None)
        .await
        .expect("SceneRegion/get must succeed");

    assert_eq!(
        sent_invocation(&server).await,
        json!(["SceneRegion/get", {
            "accountId": "acct-001",
            "ids": ["01J5ABC0000000000000000001", "01J5ABC0000000000000000002"]
        }])
    );
    let names: Vec<&str> = resp.list.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Gallery East Wing", "Design Review Room"]);
    assert_eq!(resp.list[1].spawn_position, [0.0, 0.0, 5.0]);
}

/// draft-atwood-jmap-scene-00 L1914-L1961, the `withinRadius` example
/// query and response.
///
/// The draft writes `"center": [5.0, 0, -2.0]` and `"radius": 20`; JSON
/// gives `0` and `0.0` the same value (RFC 8259 §6), but `serde_json`
/// compares integer and float numbers as different, so the expected
/// request spells the draft's integers as floats.
#[tokio::test]
async fn scene_object_query_within_radius_matches_example() {
    let server = MockServer::start().await;
    respond(
        &server,
        "SceneObject/query",
        json!({
            "accountId": "acct-001",
            "queryState": "qstate-obj-18",
            "canCalculateChanges": true,
            "position": 0,
            "ids": ["01J5QBJ0000000000000000002", "01J5QBJ0000000000000000001"],
            "total": 2
        }),
    )
    .await;

    let mut cond = SceneObjectFilterCondition::default();
    cond.region_id = Some(Id::from("01J5ABC0000000000000000001"));
    cond.interactable = Some(true);
    cond.within_radius = Some(WithinRadius::new([5.0, 0.0, -2.0], 20.0));

    let sc = make_client(&server, "acct-001");
    let resp = sc
        .scene_object_query(
            Some(&Filter::Condition(cond)),
            Some(&[SceneObjectComparator::new(SceneObjectSortProperty::Name)]),
            &jmap_base_client::QueryParams {
                position: Some(0),
                limit: Some(25),
                ..Default::default()
            },
        )
        .await
        .expect("SceneObject/query must succeed");

    assert_eq!(
        sent_invocation(&server).await,
        json!(["SceneObject/query", {
            "accountId": "acct-001",
            "filter": {
                "regionId": "01J5ABC0000000000000000001",
                "interactable": true,
                "withinRadius": { "center": [5.0, 0.0, -2.0], "radius": 20.0 }
            },
            // RFC 8620 §5.5 isAscending "(default: true)": the default is omitted.
            "sort": [{"property": "name"}],
            "position": 0,
            "limit": 25
        }])
    );
    assert_eq!(resp.query_state, State::from("qstate-obj-18"));
    assert_eq!(resp.total, Some(2));
}

/// draft-atwood-jmap-scene-00 L2067-L2114, the `SceneObject/interact`
/// example request and its response ("the second object has
/// `interactable: false`").
#[tokio::test]
async fn scene_object_interact_matches_example() {
    let server = MockServer::start().await;
    respond(
        &server,
        "SceneObject/interact",
        json!({
            "accountId": "acct-001",
            "accepted": ["i0"],
            "notAccepted": {
                "i1": {
                    "type": "forbidden",
                    "description": "The object is not interactable."
                }
            }
        }),
    )
    .await;

    let region = Id::from("01J5ABC0000000000000000001");
    let click = SceneInteraction::new(
        region.clone(),
        Id::from("01J5QBJ0000000000000000002"),
        "click".to_owned(),
    );
    let mut grab = SceneInteraction::new(
        region,
        Id::from("01J5QBJ0000000000000000003"),
        "grab".to_owned(),
    );
    grab.data = Some(serde_json::Map::from_iter([(
        "hand".to_owned(),
        json!("left"),
    )]));

    let sc = make_client(&server, "acct-001");
    let resp = sc
        .scene_object_interact(BTreeMap::from([
            ("i0".to_owned(), click),
            ("i1".to_owned(), grab),
        ]))
        .await
        .expect("SceneObject/interact must succeed");

    let body = sent_request(&server).await;
    assert_eq!(
        body["using"],
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:scene"])
    );
    assert_eq!(
        json!([body["methodCalls"][0][0], body["methodCalls"][0][1]]),
        json!(["SceneObject/interact", {
            "accountId": "acct-001",
            "interactions": {
                "i0": {
                    "regionId": "01J5ABC0000000000000000001",
                    "objectId": "01J5QBJ0000000000000000002",
                    "action": "click",
                    "data": null
                },
                "i1": {
                    "regionId": "01J5ABC0000000000000000001",
                    "objectId": "01J5QBJ0000000000000000003",
                    "action": "grab",
                    "data": {"hand": "left"}
                }
            }
        }])
    );
    assert_eq!(resp.account_id, Id::from("acct-001"));
    assert_eq!(resp.accepted, Some(vec!["i0".to_owned()]));
    let not_accepted = resp.not_accepted.expect("notAccepted must be present");
    assert_eq!(not_accepted.len(), 1);
    assert_eq!(not_accepted["i1"].error_type, SetErrorType::Forbidden);
    assert_eq!(
        not_accepted["i1"].description.as_deref(),
        Some("The object is not interactable.")
    );
}

/// draft-atwood-jmap-scene-00 L2130-L2230, "A user enters a region by
/// calling `SceneAvatar/set` with a `create`".
#[tokio::test]
async fn scene_avatar_set_enter_matches_example() {
    let server = MockServer::start().await;
    respond(
        &server,
        "SceneAvatar/set",
        json!({
            "accountId": "acct-001",
            "oldState": "state-avatar-20",
            "newState": "state-avatar-21",
            "created": {
                "av0": {
                    "id": "01J5AVA0000000000000000001",
                    "regionId": "01J5ABC0000000000000000001",
                    "userId": "user:alice@example.com",
                    "displayName": "Alice Chen",
                    "position": [0, 0, 10],
                    "orientation": [0, 0, 0, 1],
                    "visualRef": "blob-avatar-alice-001",
                    "visualType": "model/gltf-binary",
                    "joinedAt": "2026-06-06T10:30:00Z",
                    "leftAt": null,
                    "worldState": null
                }
            },
            "updated": null,
            "destroyed": null,
            "notCreated": null,
            "notUpdated": null,
            "notDestroyed": null
        }),
    )
    .await;

    let create = json!({
        "av0": {
            "regionId": "01J5ABC0000000000000000001",
            "displayName": "Alice Chen",
            "visualRef": "blob-avatar-alice-001",
            "visualType": "model/gltf-binary"
        }
    });
    let sc = make_client(&server, "acct-001");
    let resp = sc
        .scene_avatar_set(Some(create.clone()), None, None, None)
        .await
        .expect("SceneAvatar/set must succeed");

    assert_eq!(
        sent_invocation(&server).await,
        json!(["SceneAvatar/set", { "accountId": "acct-001", "create": create }])
    );
    assert_eq!(resp.new_state, State::from("state-avatar-21"));
    let created = resp.created;
    assert_eq!(
        created["av0"].extra["userId"],
        json!("user:alice@example.com")
    );
}

/// draft-atwood-jmap-scene-00 L2495-L2542, the `SceneAvatar/query`
/// example (integers spelled as floats; see
/// `scene_object_query_within_radius_matches_example`).
#[tokio::test]
async fn scene_avatar_query_matches_example() {
    let server = MockServer::start().await;
    respond(
        &server,
        "SceneAvatar/query",
        json!({
            "accountId": "acct-001",
            "queryState": "qstate-avatar-9",
            "canCalculateChanges": true,
            "position": 0,
            "ids": ["01J5AVB0000000000000000002", "01J5AVC0000000000000000003"],
            "total": 2
        }),
    )
    .await;

    let mut cond = SceneAvatarFilterCondition::default();
    cond.region_id = Some(Id::from("01J5ABC0000000000000000001"));
    cond.is_active = Some(true);
    cond.within_radius = Some(WithinRadius::new([10.0, 0.0, -5.0], 30.0));

    let sc = make_client(&server, "acct-001");
    let resp = sc
        .scene_avatar_query(
            Some(&Filter::Condition(cond)),
            Some(&[SceneAvatarComparator::new(
                SceneAvatarSortProperty::JoinedAt,
            )]),
            &jmap_base_client::QueryParams {
                position: Some(0),
                limit: Some(50),
                ..Default::default()
            },
        )
        .await
        .expect("SceneAvatar/query must succeed");

    assert_eq!(
        sent_invocation(&server).await,
        json!(["SceneAvatar/query", {
            "accountId": "acct-001",
            "filter": {
                "regionId": "01J5ABC0000000000000000001",
                "isActive": true,
                "withinRadius": { "center": [10.0, 0.0, -5.0], "radius": 30.0 }
            },
            // RFC 8620 §5.5 isAscending "(default: true)": the default is omitted.
            "sort": [{"property": "joinedAt"}],
            "position": 0,
            "limit": 50
        }])
    );
    assert_eq!(resp.ids.len(), 2);
}

/// draft-atwood-jmap-scene-00 L1532-L1534 "Standard JMAP `/changes`";
/// request and response written by hand from RFC 8620 §5.2.
#[tokio::test]
async fn scene_object_changes_sends_rfc8620_shape() {
    let server = MockServer::start().await;
    respond(
        &server,
        "SceneObject/changes",
        json!({
            "accountId": "acct-001",
            "oldState": "o1",
            "newState": "o2",
            "hasMoreChanges": false,
            "created": [],
            "updated": ["01J5QBJ0000000000000000001"],
            "destroyed": ["01J5QBJ0000000000000000002"]
        }),
    )
    .await;

    let sc = make_client(&server, "acct-001");
    let resp = sc
        .scene_object_changes(&State::from("o1"), Some(100))
        .await
        .expect("SceneObject/changes must succeed");

    assert_eq!(
        sent_invocation(&server).await,
        json!(["SceneObject/changes", {
            "accountId": "acct-001",
            "sinceState": "o1",
            "maxChanges": 100
        }])
    );
    assert_eq!(resp.updated, vec![Id::from("01J5QBJ0000000000000000001")]);
    assert_eq!(resp.destroyed, vec![Id::from("01J5QBJ0000000000000000002")]);
}

/// draft-atwood-jmap-scene-00 L1440-L1442 "Standard JMAP `/queryChanges`";
/// request and response written by hand from RFC 8620 §5.6, with the
/// L1378 sort property `activeAvatarCount`.
#[tokio::test]
async fn scene_region_query_changes_sends_rfc8620_shape() {
    let server = MockServer::start().await;
    respond(
        &server,
        "SceneRegion/queryChanges",
        json!({
            "accountId": "acct-001",
            "oldQueryState": "q1",
            "newQueryState": "q2",
            "removed": ["01J5ABC0000000000000000002"],
            "added": [{ "id": "01J5ABC0000000000000000003", "index": 0 }]
        }),
    )
    .await;

    let sort = [SceneRegionComparator::new(
        SceneRegionSortProperty::ActiveAvatarCount,
    )];
    let sc = make_client(&server, "acct-001");
    let resp = sc
        .scene_region_query_changes(
            &State::from("q1"),
            None,
            None::<&jmap_scene_types::SceneRegionFilter>,
            Some(&sort),
            None,
            Some(true),
        )
        .await
        .expect("SceneRegion/queryChanges must succeed");

    assert_eq!(
        sent_invocation(&server).await,
        json!(["SceneRegion/queryChanges", {
            "accountId": "acct-001",
            "sinceQueryState": "q1",
            // RFC 8620 §5.5 isAscending "(default: true)": the default is omitted.
            "sort": [{"property": "activeAvatarCount"}],
            "calculateTotal": true
        }])
    );
    assert_eq!(resp.removed, vec![Id::from("01J5ABC0000000000000000002")]);
    assert_eq!(resp.added[0].index, 0);
}

/// draft-atwood-jmap-scene-00 L167 "Other users reach a region's records
/// by accessing the hosting account as a shared account" and L169 "Every
/// `accountId` in a Scene method call ... is the id of the hosting
/// account": a user leaving a region hosted in a shared account patches
/// `leftAt` (L2302) with the hosting account's id.
#[tokio::test]
async fn for_account_targets_hosting_account() {
    let server = MockServer::start().await;
    respond(
        &server,
        "SceneAvatar/set",
        json!({
            "accountId": "gallery-account",
            "oldState": "s1",
            "newState": "s2",
            "updated": { "01J5AVA0000000000000000001": null }
        }),
    )
    .await;

    let mut session = session_json(&server, "alice-account");
    session["accounts"]["gallery-account"] = json!({
        "name": "Gallery",
        "isPersonal": false,
        "isReadOnly": false,
        "accountCapabilities": { "urn:ietf:params:jmap:scene": {} }
    });
    let sc = client_for(&server, session)
        .for_account("gallery-account")
        .expect("gallery-account is a Scene account");
    assert_eq!(sc.scene_account_id().unwrap(), "gallery-account");

    let mut patch = PatchObject::new();
    patch
        .as_map_mut()
        .insert("leftAt".into(), json!("2026-06-06T11:00:00Z"));
    let update = BTreeMap::from([(Id::from("01J5AVA0000000000000000001"), patch)]);
    sc.scene_avatar_set(None, Some(update), None, None)
        .await
        .expect("SceneAvatar/set must succeed");

    assert_eq!(
        sent_invocation(&server).await,
        json!(["SceneAvatar/set", {
            "accountId": "gallery-account",
            "update": {
                "01J5AVA0000000000000000001": { "leftAt": "2026-06-06T11:00:00Z" }
            }
        }])
    );
}

/// RFC 8620 §2: `accountCapabilities` is "the set of capability URIs for
/// the methods supported in this account"; an account without
/// `urn:ietf:params:jmap:scene` cannot be targeted.
#[tokio::test]
async fn for_account_rejects_non_scene_account() {
    let server = MockServer::start().await;
    let mut session = session_json(&server, "alice-account");
    session["accounts"]["mail-only"] = json!({
        "name": "Mail",
        "isPersonal": false,
        "isReadOnly": false,
        "accountCapabilities": { "urn:ietf:params:jmap:mail": {} }
    });
    let sc = client_for(&server, session);
    assert!(matches!(
        sc.for_account("mail-only"),
        Err(ClientError::InvalidArgument(_))
    ));
    assert!(matches!(
        sc.for_account("absent"),
        Err(ClientError::InvalidArgument(_))
    ));
}

/// RFC 8620 §2: `primaryAccounts` maps a capability URI to an account id;
/// with no `urn:ietf:params:jmap:scene` entry there is no account to
/// target, and nothing is sent.
#[tokio::test]
async fn no_primary_scene_account_is_invalid_session() {
    let server = MockServer::start().await;
    let mut session = session_json(&server, "alice-account");
    session["primaryAccounts"] = json!({});
    let sc = client_for(&server, session);
    let err = sc
        .scene_region_get(None, None)
        .await
        .expect_err("no primary Scene account");
    assert!(matches!(err, ClientError::InvalidSession(_)), "{err:?}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// RFC 8620 §5.2 / §5.6: `sinceState` and `sinceQueryState` are state
/// strings the server returned; an empty one never is, so it is rejected
/// before any request.
#[tokio::test]
async fn empty_state_tokens_are_rejected_without_io() {
    let server = MockServer::start().await;
    let sc = make_client(&server, "acct-001");
    let empty = State::from("");
    assert!(matches!(
        sc.scene_avatar_changes(&empty, None).await,
        Err(ClientError::InvalidArgument(_))
    ));
    assert!(matches!(
        sc.scene_asset_query_changes(
            &empty,
            None,
            None::<&jmap_scene_types::SceneAssetFilter>,
            None,
            None,
            None
        )
        .await,
        Err(ClientError::InvalidArgument(_))
    ));
    assert!(matches!(
        sc.scene_object_set(None, None, None, None).await,
        Err(ClientError::InvalidArgument(_))
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}

// RFC 8620 §5.3 `ifInState` on every `/set` method. Oracle: RFC 8620
// §5.3, "ifInState: `String|null` This is a state string as returned by
// the `Foo/get` method ... If supplied, the string must match the current
// state; otherwise, the method will be aborted and a `stateMismatch` error
// returned. If null, any changes will be applied to the current state."
// Each call is checked on the recorded request body only.

const SET_METHODS: [&str; 4] = [
    "SceneAsset/set",
    "SceneAvatar/set",
    "SceneObject/set",
    "SceneRegion/set",
];

async fn serve_empty() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "sessionState": "s1", "methodResponses": [] })),
        )
        .mount(&server)
        .await;
    server
}

/// `(method name, ifInState argument)` of each request sent.
async fn sent_if_in_state(server: &MockServer) -> Vec<(String, Option<Value>)> {
    let reqs = server.received_requests().await.expect("recorded requests");
    reqs.iter()
        .map(|r| {
            let body: Value = serde_json::from_slice(&r.body).expect("JSON body");
            let call = &body["methodCalls"][0];
            let name = call[0].as_str().expect("method name").to_owned();
            (name, call[1].get("ifInState").cloned())
        })
        .collect()
}

async fn send_all_set(server: &MockServer, if_in_state: Option<&State>) {
    let sc = make_client(server, "A1");
    let d = || Some(vec![Id::from("x")]);
    let _ = sc.scene_asset_set(None, None, d(), if_in_state).await;
    let _ = sc.scene_avatar_set(None, None, d(), if_in_state).await;
    let _ = sc.scene_object_set(None, None, d(), if_in_state).await;
    let _ = sc.scene_region_set(None, None, d(), if_in_state).await;
}

#[tokio::test]
async fn set_sends_some_if_in_state() {
    let server = serve_empty().await;
    send_all_set(&server, Some(&State::from("s-guard"))).await;
    let want: Vec<_> = SET_METHODS
        .iter()
        .map(|m| (m.to_string(), Some(json!("s-guard"))))
        .collect();
    assert_eq!(sent_if_in_state(&server).await, want);
}

#[tokio::test]
async fn set_omits_none_if_in_state() {
    let server = serve_empty().await;
    send_all_set(&server, None).await;
    let want: Vec<_> = SET_METHODS.iter().map(|m| (m.to_string(), None)).collect();
    assert_eq!(sent_if_in_state(&server).await, want);
}
