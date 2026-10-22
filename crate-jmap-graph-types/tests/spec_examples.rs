//! Checks the types against draft-atwood-jmap-graph-00 at jmap-chat-spec
//! 6bcd9d8; line numbers cite that revision. The oracle is the draft text
//! (examples copied byte-for-byte, quoted property definitions) and
//! hand-written JSON, never the code under test.
//!
//! The draft's examples are requests. Their filters are instances of these
//! types and round-trip verbatim. Their `/set` create arguments hold no
//! server-set properties, so the object tests below add hand-written
//! server-set values (and resolve the creation ids) to make whole objects.

use jmap_graph_types::{
    Graph, GraphCapability, GraphEdge, GraphNode, GraphProjectionCapability, GraphRights,
    PrincipalGraphCapability, JMAP_GRAPH_URI, JMAP_URI_GRAPH_EDGE_TYPE, JMAP_URI_GRAPH_NODE_TYPE,
    JMAP_URI_GRAPH_TYPE,
};
use jmap_graph_types::{
    GraphComparator, GraphEdgeComparator, GraphEdgeFilterCondition, GraphEdgeSortProperty,
    GraphFilter, GraphNodeComparator, GraphNodeFilterCondition, GraphNodeSortProperty,
    GraphSortProperty,
};
use jmap_types::{Id, JmapObject, JmapUri};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

/// L356-393, "Building a small graph in one request", verbatim.
const BUILD_REQUEST: &str = r##"{
  "using": [
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:graph"
  ],
  "methodCalls": [
    ["Graph/set", {
      "accountId": "A1",
      "create": { "g": { "name": "Design notes" } }
    }, "0"],
    ["GraphNode/set", {
      "accountId": "A1",
      "create": {
        "a": {
          "graphId": "#g",
          "annotations": { "com.example.label": "Root" }
        },
        "b": {
          "graphId": "#g",
          "blobId": "B7f3",
          "mediaType": "text/markdown"
        }
      }
    }, "1"],
    ["GraphEdge/set", {
      "accountId": "A1",
      "create": {
        "e": {
          "graphId": "#g",
          "fromId": "#a",
          "toId": "#b",
          "type": "com.example.child"
        }
      }
    }, "2"]
  ]
}"##;

/// L400-429, "One hop of traversal", verbatim.
const HOP_REQUEST: &str = r##"{
  "using": [
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:graph"
  ],
  "methodCalls": [
    ["GraphEdge/query", {
      "accountId": "A1",
      "filter": { "graphId": "G1", "fromId": ["N1"] }
    }, "0"],
    ["GraphEdge/get", {
      "accountId": "A1",
      "#ids": {
        "resultOf": "0",
        "name": "GraphEdge/query",
        "path": "/ids"
      },
      "properties": ["toId", "type"]
    }, "1"],
    ["GraphNode/get", {
      "accountId": "A1",
      "#ids": {
        "resultOf": "1",
        "name": "GraphEdge/get",
        "path": "/list/*/toId"
      }
    }, "2"]
  ]
}"##;

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// Parse `json` as `T`, assert it re-serializes to the same JSON value,
/// and return it.
fn round_trip<T: DeserializeOwned + Serialize>(json: &Value) -> T {
    let typed: T = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&typed).unwrap(), *json);
    typed
}

/// Arguments of method call `i` of a request.
fn call_args(request: &Value, i: usize) -> &Value {
    &request["methodCalls"][i][1]
}

/// `create` entry `key` of method call `i`, with the hand-written
/// server-set properties `server` merged in and creation ids resolved
/// (`#g` → `G1`, `#a` → `N1`, `#b` → `N2`).
fn created(request: &Value, i: usize, key: &str, server: Value) -> Value {
    let mut obj = call_args(request, i)["create"][key].clone();
    for (k, v) in obj.as_object_mut().unwrap().iter_mut() {
        if k.ends_with("Id") {
            match v.as_str() {
                Some("#g") => *v = json!("G1"),
                Some("#a") => *v = json!("N1"),
                Some("#b") => *v = json!("N2"),
                _ => {}
            }
        }
    }
    obj.as_object_mut()
        .unwrap()
        .extend(server.as_object().unwrap().clone());
    obj
}

fn node_server_set(id: &str) -> Value {
    json!({
        "id": id,
        "created": "2026-10-20T12:00:00Z",
        "updated": "2026-10-20T12:00:00Z",
        "createdBy": "P1",
        "updatedBy": null
    })
}

#[test]
fn capability_uri_matches_draft() {
    // L97: "MUST advertise the `urn:ietf:params:jmap:graph` capability".
    assert_eq!(JMAP_GRAPH_URI, "urn:ietf:params:jmap:graph");
    assert_eq!(parse(BUILD_REQUEST)["using"][1], json!(JMAP_GRAPH_URI));
}

#[test]
fn data_type_names_match_registration() {
    // L502-517: Type Name: Graph, GraphNode, GraphEdge.
    assert_eq!(Graph::TYPE_NAME, "Graph");
    assert_eq!(GraphNode::TYPE_NAME, "GraphNode");
    assert_eq!(GraphEdge::TYPE_NAME, "GraphEdge");
}

#[test]
fn object_uri_type_names_match_registration() {
    // L524-528: graph, graphnode, graphedge.
    assert_eq!(JMAP_URI_GRAPH_TYPE, "graph");
    assert_eq!(JMAP_URI_GRAPH_NODE_TYPE, "graphnode");
    assert_eq!(JMAP_URI_GRAPH_EDGE_TYPE, "graphedge");
    // L342: "a JMAP object URI ... such as `jmap:graphnode:A2:N9` can name a
    // node in another Graph".
    let uri: JmapUri = "jmap:graphnode:A2:N9".parse().unwrap();
    assert_eq!(uri.data_type(), JMAP_URI_GRAPH_NODE_TYPE);
    assert_eq!(uri.account_id().as_ref(), "A2");
    assert_eq!(uri.object_id().as_ref(), "N9");
}

#[test]
fn account_capability_round_trips() {
    // L107-108: `maxAnnotationSize` (UnsignedInt|null); `null` means no
    // advertised limit.
    let cap: GraphCapability = round_trip(&json!({ "maxAnnotationSize": 4096 }));
    assert_eq!(cap.max_annotation_size, Some(4096));
    let cap: GraphCapability = round_trip(&json!({ "maxAnnotationSize": null }));
    assert_eq!(cap.max_annotation_size, None);
    assert!(serde_json::from_value::<GraphCapability>(json!({ "maxAnnotationSize": -1 })).is_err());
}

#[test]
fn account_capability_preserves_vendor_extras() {
    let raw = json!({ "maxAnnotationSize": null, "com.example.maxNodes": 1000 });
    let cap: GraphCapability = round_trip(&raw);
    assert_eq!(cap.extra["com.example.maxNodes"], json!(1000));
}

#[test]
fn principal_capability_round_trips() {
    // L116-120: `accountId` (Id|null), `mayShareWith` (Boolean).
    let cap: PrincipalGraphCapability =
        round_trip(&json!({ "accountId": "A1", "mayShareWith": true }));
    assert_eq!(cap.account_id, Some(Id::from("A1")));
    assert!(cap.may_share_with);
    let cap: PrincipalGraphCapability =
        round_trip(&json!({ "accountId": null, "mayShareWith": false, "x.example": 1 }));
    assert_eq!(cap.account_id, None);
    assert_eq!(cap.extra["x.example"], json!(1));
}

#[test]
fn graph_from_example_takes_draft_defaults() {
    // L363-366 create { "name": "Design notes" } plus server-set values.
    let req = parse(BUILD_REQUEST);
    let raw = created(
        &req,
        0,
        "g",
        json!({
            "id": "G1",
            "created": "2026-10-20T12:00:00Z",
            "updated": "2026-10-20T12:00:00Z",
            "myRights": { "mayRead": true, "mayWrite": true, "mayAdmin": true }
        }),
    );
    let graph: Graph = serde_json::from_value(raw).unwrap();
    assert_eq!(graph.name, "Design notes");
    // L144 annotations default `{}`, L153 isSubscribed default `true` for
    // the creator, L159 shareWith default `null`.
    assert!(graph.annotations.is_empty());
    assert!(graph.is_subscribed);
    assert_eq!(graph.share_with, None);
    assert_eq!(
        serde_json::to_value(&graph).unwrap(),
        json!({
            "id": "G1",
            "name": "Design notes",
            "annotations": {},
            "created": "2026-10-20T12:00:00Z",
            "updated": "2026-10-20T12:00:00Z",
            "isSubscribed": true,
            "myRights": { "mayRead": true, "mayWrite": true, "mayAdmin": true },
            "shareWith": null
        })
    );
}

#[test]
fn graph_name_defaults_to_empty() {
    // L141: `name` (String, default: `""`).
    let graph: Graph = serde_json::from_value(json!({
        "id": "G1",
        "created": "2026-10-20T12:00:00Z",
        "updated": "2026-10-20T12:00:00Z",
        "myRights": { "mayRead": true, "mayWrite": false, "mayAdmin": false }
    }))
    .unwrap();
    assert_eq!(graph.name, "");
}

#[test]
fn graph_share_with_round_trips_sorted() {
    // L159: `shareWith` (Id[GraphRights]|null). Keys written out of order
    // serialize in sorted order (workspace BTreeMap rule).
    let text = r#"{"id":"G1","name":"n","annotations":{},"created":"2026-10-20T12:00:00Z","updated":"2026-10-20T12:00:00Z","isSubscribed":false,"myRights":{"mayRead":true,"mayWrite":true,"mayAdmin":true},"shareWith":{"P9":{"mayRead":true,"mayWrite":false,"mayAdmin":false},"P2":{"mayRead":true,"mayWrite":true,"mayAdmin":false}}}"#;
    let sorted = r#"{"id":"G1","name":"n","annotations":{},"created":"2026-10-20T12:00:00Z","updated":"2026-10-20T12:00:00Z","isSubscribed":false,"myRights":{"mayRead":true,"mayWrite":true,"mayAdmin":true},"shareWith":{"P2":{"mayRead":true,"mayWrite":true,"mayAdmin":false},"P9":{"mayRead":true,"mayWrite":false,"mayAdmin":false}}}"#;
    let graph: Graph = serde_json::from_str(text).unwrap();
    let rights = &graph.share_with.as_ref().unwrap()[&Id::from("P2")];
    assert!(rights.may_write && !rights.may_admin);
    assert_eq!(serde_json::to_string(&graph).unwrap(), sorted);
}

#[test]
fn graph_rights_require_all_three_fields() {
    // L164-171 list mayRead, mayWrite and mayAdmin without a default.
    for field in ["mayRead", "mayWrite", "mayAdmin"] {
        let mut raw = json!({ "mayRead": true, "mayWrite": true, "mayAdmin": true });
        raw.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<GraphRights>(raw).is_err(),
            "missing {field} must be rejected"
        );
    }
}

#[test]
fn graph_preserves_vendor_extras() {
    let raw = json!({
        "id": "G1",
        "name": "",
        "annotations": { "com.example.k": [1, 2] },
        "created": "2026-10-20T12:00:00Z",
        "updated": "2026-10-20T12:00:00Z",
        "isSubscribed": true,
        "myRights": { "mayRead": true, "mayWrite": true, "mayAdmin": true, "x.example": 1 },
        "shareWith": null,
        "com.example.color": "blue"
    });
    let graph: Graph = round_trip(&raw);
    assert_eq!(graph.extra["com.example.color"], json!("blue"));
    assert_eq!(graph.my_rights.extra["x.example"], json!(1));
}

#[test]
fn nodes_from_example_round_trip() {
    let req = parse(BUILD_REQUEST);

    // L370-373: annotations only; blobId, mediaType and uri take their
    // `null` defaults (L188, L191, L194).
    let a: GraphNode =
        serde_json::from_value(created(&req, 1, "a", node_server_set("N1"))).unwrap();
    assert_eq!(a.graph_id, Id::from("G1"));
    assert_eq!(a.annotations["com.example.label"], json!("Root"));
    assert_eq!((a.blob_id, a.media_type, a.uri), (None, None, None));
    assert_eq!(a.created_by, Some(Id::from("P1")));
    assert_eq!(a.updated_by, None);

    // L374-378: content-bearing node.
    let b: GraphNode = round_trip(&json!({
        "id": "N2",
        "graphId": "G1",
        "annotations": {},
        "blobId": "B7f3",
        "mediaType": "text/markdown",
        "uri": null,
        "created": "2026-10-20T12:00:00Z",
        "updated": "2026-10-20T12:00:00Z",
        "createdBy": "P1",
        "updatedBy": null
    }));
    let from_example: GraphNode =
        serde_json::from_value(created(&req, 1, "b", node_server_set("N2"))).unwrap();
    assert_eq!(from_example, b);
    assert_eq!(b.blob_id, Some(Id::from("B7f3")));
    assert_eq!(b.media_type.as_deref(), Some("text/markdown"));
}

#[test]
fn node_uri_holds_any_uri() {
    // L342: "Any URI may be used", https or a JMAP object URI.
    for uri in ["https://example.com/page", "jmap:graphnode:A2:N9"] {
        let mut raw = created(&parse(BUILD_REQUEST), 1, "a", node_server_set("N3"));
        raw["uri"] = json!(uri);
        let node: GraphNode = serde_json::from_value(raw).unwrap();
        assert_eq!(node.uri.as_deref(), Some(uri));
    }
}

#[test]
fn node_preserves_vendor_extras() {
    let mut raw = created(&parse(BUILD_REQUEST), 1, "b", node_server_set("N2"));
    raw["uri"] = json!(null);
    raw["annotations"] = json!({});
    raw["com.example.pinned"] = json!(true);
    let node: GraphNode = round_trip(&raw);
    assert_eq!(node.extra["com.example.pinned"], json!(true));
}

#[test]
fn edge_from_example_round_trips() {
    // L384-389, with `#a`/`#b` resolved to N1/N2.
    let req = parse(BUILD_REQUEST);
    let edge: GraphEdge =
        serde_json::from_value(created(&req, 2, "e", node_server_set("E1"))).unwrap();
    assert_eq!(edge.from_id, Id::from("N1"));
    assert_eq!(edge.to_id, Id::from("N2"));
    assert_eq!(edge.edge_type, "com.example.child");
    assert_eq!(
        serde_json::to_value(&edge).unwrap(),
        json!({
            "id": "E1",
            "graphId": "G1",
            "fromId": "N1",
            "toId": "N2",
            "type": "com.example.child",
            "annotations": {},
            "created": "2026-10-20T12:00:00Z",
            "updated": "2026-10-20T12:00:00Z",
            "createdBy": "P1",
            "updatedBy": null
        })
    );
}

#[test]
fn edge_type_defaults_to_empty_and_self_loop_allowed() {
    // L225 `type` default `""`; L223 toId "may equal `fromId`".
    let edge: GraphEdge = serde_json::from_value(json!({
        "id": "E2",
        "graphId": "G1",
        "fromId": "N1",
        "toId": "N1",
        "created": "2026-10-20T12:00:00Z",
        "updated": "2026-10-20T12:00:00Z",
        "createdBy": null,
        "updatedBy": null,
        "com.example.weight": 0.5
    }))
    .unwrap();
    assert_eq!(edge.edge_type, "");
    assert_eq!(edge.from_id, edge.to_id);
    assert_eq!(edge.extra["com.example.weight"], json!(0.5));
}

#[test]
fn edge_requires_endpoints() {
    // L219-223: `fromId` (Id) and `toId` (Id) carry no default.
    for field in ["fromId", "toId", "graphId"] {
        let mut raw = created(&parse(BUILD_REQUEST), 2, "e", node_server_set("E1"));
        raw.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<GraphEdge>(raw).is_err(),
            "missing {field} must be rejected"
        );
    }
}

#[test]
fn hop_example_filter_round_trips() {
    // L409: { "graphId": "G1", "fromId": ["N1"] }.
    let req = parse(HOP_REQUEST);
    let cond: GraphEdgeFilterCondition = round_trip(&call_args(&req, 0)["filter"]);
    assert_eq!(cond.graph_id, Id::from("G1"));
    assert_eq!(cond.from_id, Some(vec![Id::from("N1")]));
    assert_eq!((cond.to_id, cond.edge_type), (None, None));
}

#[test]
fn edge_filter_accepts_every_property() {
    // L318-328: graphId, fromId (Id[]), toId (Id[]), type (String).
    let cond: GraphEdgeFilterCondition = round_trip(&json!({
        "graphId": "G1",
        "fromId": ["N1", "N2"],
        "toId": ["N3"],
        "type": "com.example.child"
    }));
    assert_eq!(cond.edge_type.as_deref(), Some("com.example.child"));
}

#[test]
fn node_and_edge_filters_reject_operator_and_missing_graph_id() {
    // L285: "The `filter` argument MUST be a single FilterCondition, not a
    // FilterOperator, and MUST include `graphId`. A server receiving any
    // other filter ... rejects the call with an `unsupportedFilter` error."
    // L314 applies the same restriction to GraphEdge/query.
    let operator = json!({
        "operator": "AND",
        "conditions": [{ "graphId": "G1" }]
    });
    let no_graph = json!({ "fromId": ["N1"] });
    let unknown = json!({ "graphId": "G1", "com.example.where": 1 });
    for bad in [&operator, &no_graph, &unknown] {
        assert!(serde_json::from_value::<GraphEdgeFilterCondition>(bad.clone()).is_err());
    }
    for bad in [
        &operator,
        &json!({}),
        &json!({ "graphId": "G1", "fromId": ["N1"] }),
    ] {
        assert!(serde_json::from_value::<GraphNodeFilterCondition>(bad.clone()).is_err());
    }
    let cond: GraphNodeFilterCondition = round_trip(&json!({ "graphId": "G1" }));
    assert_eq!(cond, GraphNodeFilterCondition::new(Id::from("G1")));
}

#[test]
fn graph_filter_accepts_conditions_and_operators() {
    // L260-263: isSubscribed; Graph/query is a standard /query (L258), so
    // FilterOperator trees are allowed.
    let _: GraphFilter = round_trip(&json!({ "isSubscribed": true }));
    let _: GraphFilter = round_trip(&json!({
        "operator": "NOT",
        "conditions": [{ "isSubscribed": false }]
    }));
    assert!(serde_json::from_value::<GraphFilter>(json!({ "name": "x" })).is_err());
}

#[test]
fn sort_properties_match_draft() {
    // L265: Graph sorts by `name`, `created`, `updated`.
    for p in ["name", "created", "updated"] {
        let _: GraphComparator = serde_json::from_value(json!({ "property": p })).unwrap();
    }
    let c: GraphComparator = serde_json::from_value(json!({ "property": "name" })).unwrap();
    assert_eq!(c, GraphComparator::new(GraphSortProperty::Name));
    // L292, L330: GraphNode and GraphEdge sort by `created`, `updated` only.
    for p in ["created", "updated"] {
        let _: GraphNodeComparator = round_trip(&json!({ "property": p, "isAscending": false }));
        let _: GraphEdgeComparator = round_trip(&json!({ "property": p, "isAscending": true }));
    }
    assert_eq!(
        serde_json::from_value::<GraphNodeComparator>(json!({"property": "name"}))
            .unwrap()
            .property,
        GraphNodeSortProperty::Other("name".to_owned())
    );
    assert_eq!(
        serde_json::from_value::<GraphEdgeComparator>(json!({"property": "type"}))
            .unwrap()
            .property,
        GraphEdgeSortProperty::Other("type".to_owned())
    );
}

#[test]
fn projection_object_uri_filter_matches_example() {
    // draft-atwood-jmap-graph-projection-00 L128: "advertises the
    // `urn:ietf:params:jmap:graph:projection` capability".
    assert_eq!(
        jmap_graph_types::JMAP_GRAPH_PROJECTION_URI,
        "urn:ietf:params:jmap:graph:projection"
    );
    // Projection L212-213: "`objectUri` (String[]): The node's `objectUri`
    // is one of these values"; the filter of the L248-254 example verbatim.
    let cond: GraphNodeFilterCondition = round_trip(&json!({
        "graphId": "PG1",
        "objectUri": ["jmap:email:A1:M42"]
    }));
    assert_eq!(cond.graph_id, Id::from("PG1"));
    assert_eq!(cond.object_uri, Some(vec!["jmap:email:A1:M42".to_owned()]));
    // L285: the filter still MUST include `graphId`.
    assert!(serde_json::from_value::<GraphNodeFilterCondition>(
        json!({ "objectUri": ["jmap:email:A1:M42"] })
    )
    .is_err());
}

#[test]
fn projection_account_capability_round_trips() {
    // draft-atwood-jmap-graph-projection-00 at jmap-chat-spec 33702a5,
    // L136-139: the account-level value "is a JSON object with the
    // following field: `graphId` (Id): The id of the account's projected
    // Graph."
    let cap: GraphProjectionCapability = round_trip(&json!({ "graphId": "G1" }));
    assert_eq!(cap.graph_id, Id::from("G1"));
    assert!(serde_json::from_value::<GraphProjectionCapability>(json!({})).is_err());
    let raw = json!({ "graphId": "G1", "com.example.rebuiltAt": "2026-10-21T00:00:00Z" });
    let cap: GraphProjectionCapability = round_trip(&raw);
    assert_eq!(
        cap.extra["com.example.rebuiltAt"],
        json!("2026-10-21T00:00:00Z")
    );
}
