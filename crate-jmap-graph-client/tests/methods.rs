//! Wiremock integration tests for the `Graph/*`, `GraphNode/*` and
//! `GraphEdge/*` client methods.
//!
//! Oracle: the request examples of draft-atwood-jmap-graph-00 and
//! draft-atwood-jmap-graph-projection-00 (jmap-chat-spec 6bcd9d8, line
//! numbers cited per test) and the RFC 8620 §5 method shapes. Expected JSON
//! is copied from the drafts, never derived from the code under test;
//! response bodies are hand-written RFC 8620 shapes.

use std::collections::BTreeMap;

use jmap_base_client::ClientError;
use jmap_graph_client::{JmapGraphExt, SessionClient};
use jmap_graph_types::{
    Filter, GraphComparator, GraphEdgeFilterCondition, GraphFilterCondition, GraphNodeComparator,
    GraphNodeFilterCondition, GraphNodeSortProperty, GraphSortProperty,
};
use jmap_types::{Id, PatchObject, State};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// draft-atwood-jmap-graph-00 L358-L361: the `using` array of every
/// example request.
fn using_graph() -> Value {
    json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:graph"])
}

/// The session JSON for a user whose primary Graph account is `account_id`.
///
/// Oracle: RFC 8620 §2.1 session shape; draft-atwood-jmap-graph-00 L101
/// "The value of `capabilities["urn:ietf:params:jmap:graph"]` at the
/// session level is an empty JSON object `{}`."
fn session_json(server: &MockServer, account_id: &str) -> Value {
    json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:graph": {}
        },
        "accounts": {
            account_id: {
                "name": "alice@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:graph": { "maxAnnotationSize": null }
                }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:graph": account_id },
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
    .with_graph_session(session)
}

fn make_client(server: &MockServer) -> SessionClient {
    client_for(server, session_json(server, "A1"))
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

/// The `using` array and the `[name, arguments]` of the single invocation
/// the client sent, without the call id (the drafts' examples use their
/// own call ids).
async fn sent_call(server: &MockServer) -> (Value, Value) {
    let body = sent_request(server).await;
    let calls = body["methodCalls"]
        .as_array()
        .expect("methodCalls must be an array");
    assert_eq!(calls.len(), 1, "exactly one method call must be sent");
    (body["using"].clone(), json!([calls[0][0], calls[0][1]]))
}

/// A hand-written RFC 8620 §5.3 `/set` response creating `creation_id`.
fn set_created(creation_id: &str, id: &str) -> Value {
    json!({
        "accountId": "A1",
        "oldState": "1",
        "newState": "2",
        "created": { creation_id: { "id": id } },
        "updated": null,
        "destroyed": null,
        "notCreated": null,
        "notUpdated": null,
        "notDestroyed": null
    })
}

/// A hand-written RFC 8620 §5.5 `/query` response.
fn query_ids(ids: &[&str]) -> Value {
    json!({
        "accountId": "A1",
        "queryState": "q1",
        "canCalculateChanges": true,
        "position": 0,
        "ids": ids
    })
}

/// draft-atwood-jmap-graph-00 L363-L366, call "0" of "Building a small
/// graph in one request".
#[tokio::test]
async fn graph_set_create_matches_example() {
    let server = MockServer::start().await;
    respond(&server, "Graph/set", set_created("g", "G1")).await;

    let resp = make_client(&server)
        .graph_set(
            Some(json!({ "g": { "name": "Design notes" } })),
            None,
            None,
            None,
        )
        .await
        .expect("Graph/set must succeed");
    assert_eq!(resp.created["g"].id, Some(Id::from("G1")));

    let (using, call) = sent_call(&server).await;
    assert_eq!(using, using_graph());
    assert_eq!(
        call,
        json!(["Graph/set", {
            "accountId": "A1",
            "create": { "g": { "name": "Design notes" } }
        }])
    );
}

/// draft-atwood-jmap-graph-00 L367-L380, call "1" of "Building a small
/// graph in one request": `graphId` is the creation id `#g` (L277).
#[tokio::test]
async fn graph_node_set_create_matches_example() {
    let server = MockServer::start().await;
    respond(&server, "GraphNode/set", set_created("a", "N1")).await;

    let create = json!({
        "a": {
            "graphId": "#g",
            "annotations": { "com.example.label": "Root" }
        },
        "b": {
            "graphId": "#g",
            "blobId": "B7f3",
            "mediaType": "text/markdown"
        }
    });
    make_client(&server)
        .graph_node_set(Some(create.clone()), None, None, None)
        .await
        .expect("GraphNode/set must succeed");

    let (using, call) = sent_call(&server).await;
    assert_eq!(using, using_graph());
    assert_eq!(
        call,
        json!(["GraphNode/set", { "accountId": "A1", "create": create }])
    );
}

/// draft-atwood-jmap-graph-00 L381-L391, call "2" of "Building a small
/// graph in one request".
#[tokio::test]
async fn graph_edge_set_create_matches_example() {
    let server = MockServer::start().await;
    respond(&server, "GraphEdge/set", set_created("e", "E1")).await;

    let create = json!({
        "e": {
            "graphId": "#g",
            "fromId": "#a",
            "toId": "#b",
            "type": "com.example.child"
        }
    });
    make_client(&server)
        .graph_edge_set(Some(create.clone()), None, None, None)
        .await
        .expect("GraphEdge/set must succeed");

    let (_, call) = sent_call(&server).await;
    assert_eq!(
        call,
        json!(["GraphEdge/set", { "accountId": "A1", "create": create }])
    );
}

/// draft-atwood-jmap-graph-00 L439-L444, "Changing one annotation".
#[tokio::test]
async fn graph_node_set_patch_matches_annotation_example() {
    let server = MockServer::start().await;
    respond(
        &server,
        "GraphNode/set",
        json!({
            "accountId": "A1",
            "oldState": "2",
            "newState": "3",
            "updated": { "N1": null }
        }),
    )
    .await;

    let mut patch = PatchObject::new();
    patch
        .as_map_mut()
        .insert("annotations/com.example.color".into(), json!("red"));
    make_client(&server)
        .graph_node_set(
            None,
            Some(BTreeMap::from([(Id::from("N1"), patch)])),
            None,
            None,
        )
        .await
        .expect("GraphNode/set must succeed");

    let (_, call) = sent_call(&server).await;
    assert_eq!(
        call,
        json!(["GraphNode/set", {
            "accountId": "A1",
            "update": {
                "N1": { "annotations/com.example.color": "red" }
            }
        }])
    );
}

/// draft-atwood-jmap-graph-00 L407-L410, call "0" of "One hop of
/// traversal".
#[tokio::test]
async fn graph_edge_query_matches_traversal_example() {
    let server = MockServer::start().await;
    respond(&server, "GraphEdge/query", query_ids(&["E1", "E2"])).await;

    let mut cond = GraphEdgeFilterCondition::new(Id::from("G1"));
    cond.from_id = Some(vec![Id::from("N1")]);
    let resp = make_client(&server)
        .graph_edge_query(&cond, None, &jmap_base_client::QueryParams::default())
        .await
        .expect("GraphEdge/query must succeed");
    assert_eq!(resp.ids, vec![Id::from("E1"), Id::from("E2")]);

    let (using, call) = sent_call(&server).await;
    assert_eq!(using, using_graph());
    assert_eq!(
        call,
        json!(["GraphEdge/query", {
            "accountId": "A1",
            "filter": { "graphId": "G1", "fromId": ["N1"] }
        }])
    );
}

/// draft-atwood-jmap-graph-projection-00 L240-L270, call "0" of its
/// Example: an `objectUri` filter, with the projection capability in
/// `using` (L242-L246).
#[tokio::test]
async fn graph_node_query_object_uri_matches_projection_example() {
    let server = MockServer::start().await;
    respond(&server, "GraphNode/query", query_ids(&["N9"])).await;

    let mut cond = GraphNodeFilterCondition::new(Id::from("PG1"));
    cond.object_uri = Some(vec!["jmap:email:A1:M42".into()]);
    let resp = make_client(&server)
        .graph_node_query(&cond, None, &jmap_base_client::QueryParams::default())
        .await
        .expect("GraphNode/query must succeed");
    assert_eq!(resp.ids, vec![Id::from("N9")]);

    let (using, call) = sent_call(&server).await;
    assert_eq!(
        using,
        json!([
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:graph",
            "urn:ietf:params:jmap:graph:projection"
        ])
    );
    assert_eq!(
        call,
        json!(["GraphNode/query", {
            "accountId": "A1",
            "filter": {
                "graphId": "PG1",
                "objectUri": ["jmap:email:A1:M42"]
            }
        }])
    );
}

/// draft-atwood-jmap-graph-00 L289 (`graphId` only) and L292 (sort by
/// `created`); RFC 8620 §5.6 `/queryChanges` arguments. Without
/// `objectUri` the request uses only the graph capability (L358-L361).
#[tokio::test]
async fn graph_node_query_changes_sends_rfc8620_shape() {
    let server = MockServer::start().await;
    respond(
        &server,
        "GraphNode/queryChanges",
        json!({
            "accountId": "A1",
            "oldQueryState": "q1",
            "newQueryState": "q2",
            "removed": ["N2"],
            "added": [{ "id": "N3", "index": 0 }]
        }),
    )
    .await;

    let sort = [GraphNodeComparator::new(GraphNodeSortProperty::Created)];
    let resp = make_client(&server)
        .graph_node_query_changes(
            &State::from("q1"),
            Some(50),
            &GraphNodeFilterCondition::new(Id::from("G1")),
            Some(&sort),
            None,
            Some(true),
        )
        .await
        .expect("GraphNode/queryChanges must succeed");
    assert_eq!(resp.removed, vec![Id::from("N2")]);

    let (using, call) = sent_call(&server).await;
    assert_eq!(using, using_graph());
    assert_eq!(
        call,
        json!(["GraphNode/queryChanges", {
            "accountId": "A1",
            "sinceQueryState": "q1",
            "maxChanges": 50,
            "filter": { "graphId": "G1" },
            // RFC 8620 §5.5 isAscending "(default: true)": the default is omitted.
            "sort": [{ "property": "created" }],
            "calculateTotal": true
        }])
    );
}

/// draft-atwood-jmap-graph-00 L262 (`isSubscribed` condition) and L265
/// (sort by `name`); Graph/query is a standard `/query` (L258).
#[tokio::test]
async fn graph_query_sends_filter_and_sort() {
    let server = MockServer::start().await;
    respond(&server, "Graph/query", query_ids(&["G1"])).await;

    let mut cond = GraphFilterCondition::default();
    cond.is_subscribed = Some(true);
    let sort = [GraphComparator::new(GraphSortProperty::Name)];
    make_client(&server)
        .graph_query(
            Some(&Filter::Condition(cond)),
            Some(&sort),
            &jmap_base_client::QueryParams {
                position: Some(0),
                limit: Some(10),
                ..Default::default()
            },
        )
        .await
        .expect("Graph/query must succeed");

    let (_, call) = sent_call(&server).await;
    assert_eq!(
        call,
        json!(["Graph/query", {
            "accountId": "A1",
            "filter": { "isSubscribed": true },
            // RFC 8620 §5.5 isAscending "(default: true)": the default is omitted.
            "sort": [{ "property": "name" }],
            "position": 0,
            "limit": 10
        }])
    );
}

/// `filter` is generic over its condition type, so a caller can send a
/// condition the draft does not define. RFC 8620 §5.5: FilterCondition
/// properties are type-specific; `isSubscribed` is
/// draft-atwood-jmap-graph-00 L262. The vendor key and the expected JSON
/// are hand-written.
#[tokio::test]
async fn graph_query_sends_caller_condition_type() {
    let server = MockServer::start().await;
    respond(&server, "Graph/query", query_ids(&[])).await;

    let filter = Filter::Condition(json!({
        "isSubscribed": true,
        "example.com:team": "ops"
    }));
    make_client(&server)
        .graph_query(
            Some(&filter),
            None,
            &jmap_base_client::QueryParams::default(),
        )
        .await
        .expect("Graph/query must succeed");

    let (_, call) = sent_call(&server).await;
    assert_eq!(
        call,
        json!(["Graph/query", {
            "accountId": "A1",
            "filter": { "isSubscribed": true, "example.com:team": "ops" }
        }])
    );
}

/// RFC 8620 §5.1 `/get`; the GraphNode is the L367-L380 node "b" with
/// hand-written server-set properties (draft §GraphNode).
#[tokio::test]
async fn graph_node_get_decodes_node() {
    let server = MockServer::start().await;
    respond(
        &server,
        "GraphNode/get",
        json!({
            "accountId": "A1",
            "state": "3",
            "list": [{
                "id": "N2",
                "graphId": "G1",
                "annotations": {},
                "blobId": "B7f3",
                "mediaType": "text/markdown",
                "uri": null,
                "created": "2026-10-20T12:00:00Z",
                "updated": "2026-10-20T12:00:00Z",
                "createdBy": null,
                "updatedBy": null
            }],
            "notFound": ["N404"]
        }),
    )
    .await;

    let ids = [Id::from("N2"), Id::from("N404")];
    let resp = make_client(&server)
        .graph_node_get(Some(&ids), None)
        .await
        .expect("GraphNode/get must succeed");
    assert_eq!(resp.list.len(), 1);
    assert_eq!(resp.list[0].graph_id, Id::from("G1"));
    assert_eq!(resp.list[0].blob_id, Some(Id::from("B7f3")));
    assert_eq!(resp.list[0].media_type.as_deref(), Some("text/markdown"));
    assert_eq!(resp.not_found, vec![Id::from("N404")]);

    let (_, call) = sent_call(&server).await;
    assert_eq!(
        call,
        json!(["GraphNode/get", { "accountId": "A1", "ids": ["N2", "N404"] }])
    );
}

/// RFC 8620 §5.2 `/changes` arguments for GraphEdge/changes (L296-L298).
#[tokio::test]
async fn graph_edge_changes_sends_rfc8620_shape() {
    let server = MockServer::start().await;
    respond(
        &server,
        "GraphEdge/changes",
        json!({
            "accountId": "A1",
            "oldState": "1",
            "newState": "2",
            "hasMoreChanges": false,
            "created": [],
            "updated": [],
            "destroyed": ["E1"]
        }),
    )
    .await;

    let resp = make_client(&server)
        .graph_edge_changes(&State::from("1"), Some(100))
        .await
        .expect("GraphEdge/changes must succeed");
    assert_eq!(resp.destroyed, vec![Id::from("E1")]);

    let (_, call) = sent_call(&server).await;
    assert_eq!(
        call,
        json!(["GraphEdge/changes", {
            "accountId": "A1",
            "sinceState": "1",
            "maxChanges": 100
        }])
    );
}

/// draft-atwood-jmap-graph-00 L93: "Other users reach a shared Graph by
/// accessing that account as a shared account ... Every `accountId` in a
/// method call or response defined here is the id of the account that
/// holds the Graph."
#[tokio::test]
async fn for_account_targets_holding_account() {
    let server = MockServer::start().await;
    respond(
        &server,
        "Graph/get",
        json!({
            "accountId": "A2",
            "state": "1",
            "list": [],
            "notFound": []
        }),
    )
    .await;
    let mut session = session_json(&server, "A1");
    session["accounts"]["A2"] = json!({
        "name": "bob@example.com",
        "isPersonal": false,
        "isReadOnly": false,
        "accountCapabilities": { "urn:ietf:params:jmap:graph": {} }
    });
    let gc = client_for(&server, session)
        .for_account("A2")
        .expect("A2 is a Graph account");
    assert_eq!(gc.graph_account_id().unwrap(), "A2");
    gc.graph_get(None, None).await.expect("Graph/get");

    let (_, call) = sent_call(&server).await;
    assert_eq!(call, json!(["Graph/get", { "accountId": "A2" }]));
}

/// RFC 8620 §2: `accountCapabilities` is "the set of capability URIs for
/// the methods supported in this account"; an account without
/// `urn:ietf:params:jmap:graph` cannot be targeted.
#[tokio::test]
async fn for_account_rejects_non_graph_account() {
    let server = MockServer::start().await;
    let mut session = session_json(&server, "A1");
    session["accounts"]["mail-only"] = json!({
        "name": "Mail",
        "isPersonal": false,
        "isReadOnly": false,
        "accountCapabilities": { "urn:ietf:params:jmap:mail": {} }
    });
    let gc = client_for(&server, session);
    assert!(matches!(
        gc.for_account("mail-only"),
        Err(ClientError::InvalidArgument(_))
    ));
    assert!(matches!(
        gc.for_account("absent"),
        Err(ClientError::InvalidArgument(_))
    ));
}

/// RFC 8620 §2: `primaryAccounts` maps a capability URI to an account id;
/// with no `urn:ietf:params:jmap:graph` entry there is no account to
/// target, and nothing is sent.
#[tokio::test]
async fn no_primary_graph_account_is_invalid_session() {
    let server = MockServer::start().await;
    let mut session = session_json(&server, "A1");
    session["primaryAccounts"] = json!({});
    let err = client_for(&server, session)
        .graph_get(None, None)
        .await
        .expect_err("no primary Graph account");
    assert!(matches!(err, ClientError::InvalidSession(_)), "{err:?}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// RFC 8620 §5.2 / §5.6: `sinceState` and `sinceQueryState` are state
/// strings the server returned; an empty one never is, so it is rejected
/// before any request. An all-`None` `/set` is rejected likewise.
#[tokio::test]
async fn empty_state_tokens_are_rejected_without_io() {
    let server = MockServer::start().await;
    let gc = make_client(&server);
    let empty = State::from("");
    assert!(matches!(
        gc.graph_changes(&empty, None).await,
        Err(ClientError::InvalidArgument(_))
    ));
    assert!(matches!(
        gc.graph_edge_query_changes(
            &empty,
            None,
            &GraphEdgeFilterCondition::new(Id::from("G1")),
            None,
            None,
            None
        )
        .await,
        Err(ClientError::InvalidArgument(_))
    ));
    assert!(matches!(
        gc.graph_node_set(None, None, None, None).await,
        Err(ClientError::InvalidArgument(_))
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}
