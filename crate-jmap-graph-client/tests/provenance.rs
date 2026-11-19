//! draft-atwood-jmap-graph-provenance-00 on the wire (bd JMAP-98sl.8).
//!
//! Oracle: draft-atwood-jmap-graph-provenance-00.md at jmap-chat-spec
//! addb3fb, §Examples (L265-316) and §Query Filters (L240-256); RFC 8620
//! §3.3 (`using` names every capability the request relies on). Expected
//! JSON is copied from the draft; response bodies are hand-written.

use jmap_graph_client::{JmapGraphExt, SessionClient};
use jmap_graph_types::{GraphNodeFilterCondition, ProvenanceBasis, RecordStatus};
use jmap_types::{Id, PatchObject};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROVENANCE: &str = "urn:ietf:params:jmap:graph:provenance";

/// A session whose account `A1` advertises graph, and provenance when
/// `provenance` (§Capability: both objects are `{}`).
fn client(server: &MockServer, provenance: bool) -> SessionClient {
    let mut account_caps = json!({ "urn:ietf:params:jmap:graph": { "maxAnnotationSize": null } });
    let mut caps = json!({ "urn:ietf:params:jmap:core": {}, "urn:ietf:params:jmap:graph": {} });
    if provenance {
        account_caps[PROVENANCE] = json!({});
        caps[PROVENANCE] = json!({});
    }
    let session: jmap_base_client::Session = serde_json::from_value(json!({
        "capabilities": caps,
        "accounts": { "A1": {
            "name": "a", "isPersonal": true, "isReadOnly": false,
            "accountCapabilities": account_caps
        }},
        "primaryAccounts": { "urn:ietf:params:jmap:graph": "A1" },
        "username": "a",
        "apiUrl": format!("{}/api/", server.uri()),
        "downloadUrl": format!("{}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}", server.uri()),
        "uploadUrl": format!("{}/ul/{{accountId}}/", server.uri()),
        "eventSourceUrl": format!("{}/sse/", server.uri()),
        "state": "s1"
    }))
    .unwrap();
    jmap_base_client::JmapClient::new(
        jmap_base_client::DefaultTransport,
        jmap_base_client::NoneAuth,
        &server.uri(),
        jmap_base_client::ClientConfig::default(),
    )
    .unwrap()
    .with_graph_session(session)
}

async fn respond(server: &MockServer, name: &str, args: Value) {
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sessionState": "s1", "methodResponses": [[name, args, "r1"]]
        })))
        .mount(server)
        .await;
}

async fn sent(server: &MockServer) -> (Value, Value) {
    let reqs = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    (body["using"].clone(), body["methodCalls"][0][1].clone())
}

fn set_ok() -> Value {
    json!({ "accountId": "A1", "oldState": "1", "newState": "2" })
}

fn query_ok() -> Value {
    json!({ "accountId": "A1", "queryState": "q", "canCalculateChanges": false,
            "position": 0, "ids": [] })
}

/// §Examples L268-288: the agent's record and the credential node, sent
/// with the provenance capability in `using` on a provenance account.
#[tokio::test]
async fn the_example_create_carries_the_capability() {
    let server = MockServer::start().await;
    respond(&server, "GraphNode/set", set_ok()).await;
    let create = json!({
        "r1": {
            "graphId": "G1", "blobId": "Bk7q", "mediaType": "text/markdown",
            "basis": "inferred", "confidence": 0.6,
            "validUntil": "2027-01-31T00:00:00Z",
            "validFor": { "purposes": ["https://w3id.org/dpv#ServiceProvision"] }
        },
        "c1": {
            "graphId": "G1",
            "uri": "urn:uuid:4f1c9a3e-8d2b-4e7f-9a61-0c5d2b7e8f13",
            "credentialDigest": "htSyDucPJhKUVHKPpzpMNO8bl50vhWucS4VMDVuc_ds"
        }
    });
    let c = client(&server, true);
    assert!(c.supports_provenance());
    c.graph_node_set(Some(create.clone()), None, None, None, None)
        .await
        .unwrap();
    let (using, args) = sent(&server).await;
    assert_eq!(
        using,
        json!([
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:graph",
            PROVENANCE
        ])
    );
    assert_eq!(args["create"], create);
}

/// §Examples L289-301: evidence edges, on GraphEdge/set.
#[tokio::test]
async fn evidence_edges_carry_the_capability() {
    let server = MockServer::start().await;
    respond(&server, "GraphEdge/set", set_ok()).await;
    let create = json!({ "e1": {
        "graphId": "G1", "fromId": "#r1", "toId": "N42",
        "type": jmap_graph_types::EVIDENCE_EDGE_TYPE
    }});
    client(&server, true)
        .graph_edge_set(Some(create.clone()), None, None, None, None)
        .await
        .unwrap();
    let (using, args) = sent(&server).await;
    assert_eq!(using[2], PROVENANCE);
    assert_eq!(
        args["create"]["e1"]["type"],
        "urn:ietf:params:jmap:graph:provenance#evidence"
    );
}

/// §Examples L306-315: a curator demotes a record.
#[tokio::test]
async fn the_example_demotion() {
    let server = MockServer::start().await;
    respond(&server, "GraphNode/set", set_ok()).await;
    let patch: PatchObject = serde_json::from_value(json!({
        "status": "deprecated",
        "curationReason": "Superseded by the March inventory."
    }))
    .unwrap();
    let update = [(Id::from("N77"), patch)].into_iter().collect();
    client(&server, true)
        .graph_node_set(None, Some(update), None, None, None)
        .await
        .unwrap();
    let (_, args) = sent(&server).await;
    assert_eq!(
        args["update"],
        json!({ "N77": { "status": "deprecated",
                         "curationReason": "Superseded by the March inventory." } })
    );
}

/// An account without the capability gets the plain graph `using`.
#[tokio::test]
async fn no_capability_no_uri() {
    let server = MockServer::start().await;
    respond(&server, "GraphNode/set", set_ok()).await;
    let c = client(&server, false);
    assert!(!c.supports_provenance());
    c.graph_node_set(
        Some(json!({ "n": { "graphId": "G1" } })),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let (using, _) = sent(&server).await;
    assert_eq!(
        using,
        json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:graph"])
    );
}

/// §Query Filters L244-254: the four conditions on the wire, and the
/// capability in `using` because the filter relies on it.
#[tokio::test]
async fn the_four_filters_on_the_wire() {
    let server = MockServer::start().await;
    respond(&server, "GraphNode/query", query_ok()).await;
    let mut cond = GraphNodeFilterCondition::new(Id::from("G1"));
    cond.basis = Some(vec![None, Some(ProvenanceBasis::Declared)]);
    cond.status = Some(vec![RecordStatus::Active]);
    cond.pinned = Some(false);
    cond.valid_at = Some(jmap_types::UTCDate::from("2026-11-19T00:00:00Z"));
    client(&server, false)
        .graph_node_query(&cond, None, &jmap_base_client::QueryParams::default())
        .await
        .unwrap();
    let (using, args) = sent(&server).await;
    assert_eq!(using[2], PROVENANCE, "{using}");
    assert_eq!(
        args["filter"],
        json!({
            "graphId": "G1",
            "basis": [null, "declared"],
            "status": ["active"],
            "pinned": false,
            "validAt": "2026-11-19T00:00:00Z"
        })
    );
}
