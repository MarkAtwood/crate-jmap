//! Wiremock integration tests for the draft-atwood-jmap-graph-edit-00
//! `GraphNodeOp/*` and `GraphNode/cursor` client methods.
//!
//! Oracle: the examples and method definitions of
//! draft-atwood-jmap-graph-edit-00 (jmap-chat-spec 6bcd9d8, line numbers
//! cited per test) and the RFC 8620 §5 method shapes. Expected JSON is
//! copied from the draft or hand-written from its text, never derived from
//! the code under test.

use jmap_base_client::ClientError;
use jmap_graph_client::{JmapGraphExt, SessionClient};
use jmap_graph_types::{
    GraphNodeOpComparator, GraphNodeOpFilterCondition, GraphNodeOpSortProperty, OPS_COMPACTED,
};
use jmap_types::{Id, State};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// L414-L418: the `using` array of "Loading a node".
fn using_edit() -> Value {
    json!([
        "urn:ietf:params:jmap:core",
        "urn:ietf:params:jmap:graph",
        "urn:ietf:params:jmap:graph:edit"
    ])
}

/// RFC 8620 §2.1 session for a user whose primary Graph account is `A1`,
/// advertising the edit capability (L131-L148).
fn make_client(server: &MockServer) -> SessionClient {
    let session = json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:graph": {},
            "urn:ietf:params:jmap:graph:edit": {}
        },
        "accounts": {
            "A1": {
                "name": "alice@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:graph": { "maxAnnotationSize": null },
                    "urn:ietf:params:jmap:graph:edit": {
                        "maxOpSize": 65536,
                        "maxCursorSize": 256,
                        "opRetention": null
                    }
                }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:graph": "A1" },
        "username": "alice@example.com",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}", server.uri()),
        "uploadUrl": format!("{}/ul/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/sse/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}", server.uri()),
        "state": "s1"
    });
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

/// The `using` array and the `[name, arguments]` of the single invocation
/// the client sent, without the call id.
async fn sent_call(server: &MockServer) -> (Value, Value) {
    let reqs = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(reqs.len(), 1, "exactly one request must be sent");
    let body: Value = serde_json::from_slice(&reqs[0].body).expect("request body must be JSON");
    let calls = body["methodCalls"]
        .as_array()
        .expect("methodCalls must be an array");
    assert_eq!(calls.len(), 1, "exactly one method call must be sent");
    (body["using"].clone(), json!([calls[0][0], calls[0][1]]))
}

/// L377-L386 "Submitting an op" is sent verbatim, and the L392-L405
/// response's server-set properties are returned.
#[tokio::test]
async fn graph_node_op_set_matches_submitting_example() {
    let server = MockServer::start().await;
    respond(
        &server,
        "GraphNodeOp/set",
        json!({
            "accountId": "A1",
            "oldState": "s41",
            "newState": "s42",
            "created": {
                "o1": {
                    "id": "P42",
                    "graphId": "G1",
                    "seq": 42,
                    "created": "2026-10-20T17:00:00Z",
                    "createdBy": "U7"
                }
            }
        }),
    )
    .await;
    let resp = make_client(&server)
        .graph_node_op_set(
            json!({
                "o1": { "nodeId": "N1", "baseSeq": 41, "data": "AAECAwQF" }
            }),
            None,
        )
        .await
        .expect("GraphNodeOp/set must succeed");

    let (using, call) = sent_call(&server).await;
    assert_eq!(using, using_edit());
    assert_eq!(
        call,
        json!(["GraphNodeOp/set", {
            "accountId": "A1",
            "create": {
                "o1": {
                    "nodeId": "N1",
                    "baseSeq": 41,
                    "data": "AAECAwQF"
                }
            }
        }])
    );
    let created = resp.created;
    assert_eq!(created["o1"].extra["seq"], json!(42));
    assert_eq!(created["o1"].id, Some(Id::from("P42")));
}

/// L426-L429 "Loading a node": the `GraphNodeOp/query` filter
/// `{ "nodeId": "N1", "afterSeq": 40 }`, with no `sort` (L230: `seq`
/// ascending "is also the order when `sort` is absent").
#[tokio::test]
async fn graph_node_op_query_matches_loading_example() {
    let server = MockServer::start().await;
    respond(
        &server,
        "GraphNodeOp/query",
        json!({
            "accountId": "A1",
            "queryState": "q1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": ["P41", "P42"]
        }),
    )
    .await;
    let mut cond = GraphNodeOpFilterCondition::new(Id::from("N1"));
    cond.after_seq = Some(40);
    let resp = make_client(&server)
        .graph_node_op_query(&cond, None, &jmap_base_client::QueryParams::default())
        .await
        .expect("GraphNodeOp/query must succeed");

    let (using, call) = sent_call(&server).await;
    assert_eq!(using, using_edit());
    assert_eq!(
        call,
        json!(["GraphNodeOp/query", {
            "accountId": "A1",
            "filter": { "nodeId": "N1", "afterSeq": 40 }
        }])
    );
    assert_eq!(resp.ids, vec![Id::from("P41"), Id::from("P42")]);
}

/// L234 / L261: a query past a compacted op fails with the `opsCompacted`
/// method-level error, which reaches the caller as its error type.
#[tokio::test]
async fn graph_node_op_query_surfaces_ops_compacted() {
    let server = MockServer::start().await;
    respond(&server, "error", json!({ "type": "opsCompacted" })).await;
    let cond = GraphNodeOpFilterCondition::new(Id::from("N1"));
    let err = make_client(&server)
        .graph_node_op_query(&cond, None, &jmap_base_client::QueryParams::default())
        .await
        .expect_err("opsCompacted must be an error");
    match err {
        ClientError::MethodError(e) => assert_eq!(e.error_type.to_string(), OPS_COMPACTED),
        other => panic!("expected MethodError, got {other:?}"),
    }
}

/// RFC 8620 §5.6 `/queryChanges` shape with the L230 `seq` comparator.
#[tokio::test]
async fn graph_node_op_query_changes_sends_rfc8620_shape() {
    let server = MockServer::start().await;
    respond(
        &server,
        "GraphNodeOp/queryChanges",
        json!({
            "accountId": "A1",
            "oldQueryState": "q1",
            "newQueryState": "q2",
            "removed": [],
            "added": [{ "id": "P43", "index": 2 }]
        }),
    )
    .await;
    let cond = GraphNodeOpFilterCondition::new(Id::from("N1"));
    let sort = [GraphNodeOpComparator::new(GraphNodeOpSortProperty::Seq)];
    make_client(&server)
        .graph_node_op_query_changes(&State::from("q1"), None, &cond, Some(&sort), None, None)
        .await
        .expect("GraphNodeOp/queryChanges must succeed");

    let (using, call) = sent_call(&server).await;
    assert_eq!(using, using_edit());
    assert_eq!(
        call,
        json!(["GraphNodeOp/queryChanges", {
            "accountId": "A1",
            "sinceQueryState": "q1",
            "filter": { "nodeId": "N1" },
            "sort": [{ "property": "seq", "isAscending": true }]
        }])
    );
}

/// RFC 8620 §5.1 `/get` decoding a GraphNodeOp with every property of
/// L172-L194, hand-written from the L392-L405 example values.
#[tokio::test]
async fn graph_node_op_get_decodes_op() {
    let server = MockServer::start().await;
    respond(
        &server,
        "GraphNodeOp/get",
        json!({
            "accountId": "A1",
            "state": "s42",
            "list": [{
                "id": "P42",
                "graphId": "G1",
                "nodeId": "N1",
                "seq": 42,
                "baseSeq": 41,
                "data": "AAECAwQF",
                "created": "2026-10-20T17:00:00Z",
                "createdBy": "U7"
            }],
            "notFound": []
        }),
    )
    .await;
    let ids = [Id::from("P42")];
    let resp = make_client(&server)
        .graph_node_op_get(Some(&ids), None)
        .await
        .expect("GraphNodeOp/get must succeed");

    let (using, call) = sent_call(&server).await;
    assert_eq!(using, using_edit());
    assert_eq!(
        call,
        json!(["GraphNodeOp/get", { "accountId": "A1", "ids": ["P42"] }])
    );
    let op = &resp.list[0];
    assert_eq!(op.seq, 42);
    assert_eq!(op.base_seq, 41);
    assert_eq!(op.node_id, Id::from("N1"));
    assert_eq!(op.data, "AAECAwQF");
    assert_eq!(op.created_by, Some(Id::from("U7")));
}

/// RFC 8620 §5.2 `/changes` shape; an empty state is rejected without I/O.
#[tokio::test]
async fn graph_node_op_changes_sends_rfc8620_shape() {
    let server = MockServer::start().await;
    respond(
        &server,
        "GraphNodeOp/changes",
        json!({
            "accountId": "A1",
            "oldState": "s41",
            "newState": "s42",
            "hasMoreChanges": false,
            "created": ["P42"],
            "updated": [],
            "destroyed": ["P1"]
        }),
    )
    .await;
    let client = make_client(&server);
    let err = client
        .graph_node_op_changes(&State::from(""), None)
        .await
        .expect_err("empty since_state must be rejected");
    assert!(matches!(err, ClientError::InvalidArgument(_)), "{err:?}");
    let resp = client
        .graph_node_op_changes(&State::from("s41"), Some(10))
        .await
        .expect("GraphNodeOp/changes must succeed");

    let (using, call) = sent_call(&server).await;
    assert_eq!(using, using_edit());
    assert_eq!(
        call,
        json!(["GraphNodeOp/changes", {
            "accountId": "A1",
            "sinceState": "s41",
            "maxChanges": 10
        }])
    );
    assert_eq!(resp.destroyed, vec![Id::from("P1")]);
}

/// L336-L347 `GraphNode/cursor`: the arguments are `accountId`, `nodeId`
/// and `data`, and "The response has the arguments `accountId` and
/// `nodeId` only."
#[tokio::test]
async fn graph_node_cursor_sends_arguments() {
    let server = MockServer::start().await;
    respond(
        &server,
        "GraphNode/cursor",
        json!({ "accountId": "A1", "nodeId": "N1" }),
    )
    .await;
    let resp = make_client(&server)
        .graph_node_cursor(&Id::from("N1"), "AAECAwQF")
        .await
        .expect("GraphNode/cursor must succeed");

    let (using, call) = sent_call(&server).await;
    assert_eq!(using, using_edit());
    assert_eq!(
        call,
        json!(["GraphNode/cursor", {
            "accountId": "A1",
            "nodeId": "N1",
            "data": "AAECAwQF"
        }])
    );
    assert_eq!(resp.account_id, Id::from("A1"));
    assert_eq!(resp.node_id, Id::from("N1"));
}

// RFC 8620 §5.3 `ifInState` on every `/set` method. Oracle: RFC 8620
// §5.3, "ifInState: `String|null` This is a state string as returned by
// the `Foo/get` method ... If supplied, the string must match the current
// state; otherwise, the method will be aborted and a `stateMismatch` error
// returned. If null, any changes will be applied to the current state."
// Each call is checked on the recorded request body only.

const SET_METHODS: [&str; 4] = [
    "Graph/set",
    "GraphEdge/set",
    "GraphNode/set",
    "GraphNodeOp/set",
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
    let sc = make_client(server);
    let d = || Some(vec![Id::from("x")]);
    let _ = sc.graph_set(None, None, d(), if_in_state).await;
    let _ = sc.graph_edge_set(None, None, d(), if_in_state).await;
    let _ = sc.graph_node_set(None, None, d(), if_in_state).await;
    let _ = sc.graph_node_op_set(json!({ "c1": {} }), if_in_state).await;
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
