//! Checks the `edit` module against draft-atwood-jmap-graph-edit-00 at
//! jmap-chat-spec 6bcd9d8; line numbers cite that revision. The oracle is
//! the draft text (examples copied byte-for-byte, quoted property
//! definitions) and hand-written JSON, never the code under test.

use jmap_graph_types::edit::{
    GraphCursorEvent, GraphEditEvent, GraphEditStreamDisable, GraphEditStreamMessage,
    GraphNodeCursorRequest, GraphNodeCursorResponse, GraphNodeOpComparator,
    GraphNodeOpFilterCondition, GraphNodeOpSortProperty, GRAPH_EDIT_DATA_TYPES,
    GRAPH_NODE_EDIT_FORMAT_PROPERTY, GRAPH_NODE_SNAPSHOT_SEQ_PROPERTY, OPS_COMPACTED,
};
use jmap_graph_types::{GraphEditCapability, GraphNode, GraphNodeOp, JMAP_GRAPH_EDIT_URI};
use jmap_types::{Id, JmapError, JmapObject};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

/// L377-386, "Submitting an op", verbatim.
const SUBMIT_REQUEST: &str = r##"["GraphNodeOp/set", {
  "accountId": "A1",
  "create": {
    "o1": {
      "nodeId": "N1",
      "baseSeq": 41,
      "data": "AAECAwQF"
    }
  }
}, "0"]"##;

/// L392-405, the acknowledgment of [`SUBMIT_REQUEST`], verbatim.
const SUBMIT_RESPONSE: &str = r##"["GraphNodeOp/set", {
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
}, "0"]"##;

/// L413-440, "Loading a node", verbatim.
const LOAD_REQUEST: &str = r##"{
  "using": [
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:graph",
    "urn:ietf:params:jmap:graph:edit"
  ],
  "methodCalls": [
    ["GraphNode/get", {
      "accountId": "A1",
      "ids": ["N1"],
      "properties": ["blobId", "mediaType", "editFormat",
                     "snapshotSeq"]
    }, "0"],
    ["GraphNodeOp/query", {
      "accountId": "A1",
      "filter": { "nodeId": "N1", "afterSeq": 40 }
    }, "1"],
    ["GraphNodeOp/get", {
      "accountId": "A1",
      "#ids": {
        "resultOf": "1",
        "name": "GraphNodeOp/query",
        "path": "/ids"
      }
    }, "2"]
  ]
}"##;

/// L447-452, "Subscribing to a node", verbatim.
const SUBSCRIBE: &str = r##"{
  "@type": "GraphEditStreamEnable",
  "nodeIds": ["N1"],
  "dataTypes": ["ops", "cursors"],
  "id": "sub1"
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

/// The whole op of the "Submitting an op" example: the create arguments
/// (L380-384) merged with the server-set properties of the acknowledgment
/// (L397-403).
fn example_op() -> Value {
    let mut op = parse(SUBMIT_REQUEST)[1]["create"]["o1"].clone();
    let server = parse(SUBMIT_RESPONSE)[1]["created"]["o1"].clone();
    op.as_object_mut()
        .unwrap()
        .extend(server.as_object().unwrap().clone());
    op
}

fn node(extra: Value) -> GraphNode {
    let mut raw = json!({
        "id": "N1",
        "graphId": "G1",
        "created": "2026-10-20T12:00:00Z",
        "updated": "2026-10-20T12:00:00Z",
        "createdBy": null,
        "updatedBy": null
    });
    raw.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(raw).unwrap()
}

#[test]
fn capability_uri_matches_draft() {
    // L131: "advertise the `urn:ietf:params:jmap:graph:edit` capability";
    // L490 IANA Capability Name.
    assert_eq!(JMAP_GRAPH_EDIT_URI, "urn:ietf:params:jmap:graph:edit");
    assert_eq!(parse(LOAD_REQUEST)["using"][2], json!(JMAP_GRAPH_EDIT_URI));
}

#[test]
fn data_type_name_matches_registration() {
    // L508: "Type Name: GraphNodeOp".
    assert_eq!(GraphNodeOp::TYPE_NAME, "GraphNodeOp");
}

#[test]
fn ops_compacted_error_matches_registration() {
    // L261 and L519: the method-level error `opsCompacted`.
    assert_eq!(OPS_COMPACTED, "opsCompacted");
    let err: JmapError = serde_json::from_value(json!({ "type": "opsCompacted" })).unwrap();
    assert_eq!(err.error_type.to_string(), OPS_COMPACTED);
}

#[test]
fn account_capability_round_trips() {
    // L141-148: `maxOpSize` (UnsignedInt), `maxCursorSize` (UnsignedInt),
    // `opRetention` (UnsignedInt|null).
    let cap: GraphEditCapability = round_trip(&json!({
        "maxOpSize": 65536,
        "maxCursorSize": 256,
        "opRetention": 2592000
    }));
    assert_eq!(cap.max_op_size, 65536);
    assert_eq!(cap.max_cursor_size, 256);
    assert_eq!(cap.op_retention, Some(2_592_000));
    let cap: GraphEditCapability = round_trip(&json!({
        "maxOpSize": 1,
        "maxCursorSize": 1,
        "opRetention": null,
        "com.example.x": true
    }));
    assert_eq!(cap.op_retention, None);
    assert_eq!(cap.extra["com.example.x"], json!(true));
    // UnsignedInt: negative values and a missing required field fail.
    assert!(serde_json::from_value::<GraphEditCapability>(
        json!({ "maxOpSize": -1, "maxCursorSize": 1, "opRetention": null })
    )
    .is_err());
    assert!(serde_json::from_value::<GraphEditCapability>(
        json!({ "maxCursorSize": 1, "opRetention": null })
    )
    .is_err());
}

#[test]
fn node_extension_property_names_match_get_example() {
    // L423-424: the GraphNode/get `properties` list names both extensions.
    let req = parse(LOAD_REQUEST);
    let props = &req["methodCalls"][0][1]["properties"];
    assert_eq!(props[2], json!(GRAPH_NODE_EDIT_FORMAT_PROPERTY));
    assert_eq!(props[3], json!(GRAPH_NODE_SNAPSHOT_SEQ_PROPERTY));
}

#[test]
fn node_extensions_default_to_null() {
    // L156 `editFormat` (String|null, default: `null`); L159 `snapshotSeq`
    // (UnsignedInt|null, default: `null`).
    let n = node(json!({}));
    assert_eq!(n.edit_format().unwrap(), None);
    assert_eq!(n.snapshot_seq().unwrap(), None);
    let n = node(json!({ "editFormat": null, "snapshotSeq": null }));
    assert_eq!(n.edit_format().unwrap(), None);
    assert_eq!(n.snapshot_seq().unwrap(), None);
}

#[test]
fn node_extensions_read_and_round_trip() {
    // L240: a snapshot sets `blobId`, `mediaType` and `snapshotSeq`
    // together; "Typically `mediaType` is the node's `editFormat`". L442:
    // "the client already knew that `snapshotSeq` was 40".
    let n = node(json!({
        "blobId": "B1",
        "mediaType": "application/x-example-crdt",
        "editFormat": "application/x-example-crdt",
        "snapshotSeq": 40
    }));
    assert_eq!(
        n.edit_format().unwrap().as_deref(),
        Some("application/x-example-crdt")
    );
    assert_eq!(n.snapshot_seq().unwrap(), Some(40));
    let out = serde_json::to_value(&n).unwrap();
    assert_eq!(out["editFormat"], json!("application/x-example-crdt"));
    assert_eq!(out["snapshotSeq"], json!(40));
}

#[test]
fn node_extensions_reject_wrong_types() {
    // L156 String|null; L159 UnsignedInt|null.
    assert!(node(json!({ "editFormat": 7 })).edit_format().is_err());
    assert!(node(json!({ "snapshotSeq": -1 })).snapshot_seq().is_err());
    assert!(node(json!({ "snapshotSeq": "40" })).snapshot_seq().is_err());
}

#[test]
fn op_from_example_round_trips() {
    // L172-194 properties; values from the L377-405 example.
    let raw = example_op();
    let op: GraphNodeOp = serde_json::from_value(raw).unwrap();
    assert_eq!(op.id, Id::from("P42"));
    assert_eq!(op.graph_id, Id::from("G1"));
    assert_eq!(op.node_id, Id::from("N1"));
    assert_eq!(op.seq, 42);
    assert_eq!(op.base_seq, 41);
    assert_eq!(op.data, "AAECAwQF");
    assert_eq!(op.created_by, Some(Id::from("U7")));
    assert_eq!(
        serde_json::to_value(&op).unwrap(),
        json!({
            "id": "P42",
            "graphId": "G1",
            "nodeId": "N1",
            "seq": 42,
            "baseSeq": 41,
            "data": "AAECAwQF",
            "created": "2026-10-20T17:00:00Z",
            "createdBy": "U7"
        })
    );
}

#[test]
fn op_created_by_null_and_extras_round_trip() {
    // L193 `createdBy` (Id|null); L185 `baseSeq` "or 0 if none".
    let op: GraphNodeOp = round_trip(&json!({
        "id": "P1",
        "graphId": "G1",
        "nodeId": "N1",
        "seq": 1,
        "baseSeq": 0,
        "data": "",
        "created": "2026-10-20T17:00:00Z",
        "createdBy": null,
        "com.example.tag": "x"
    }));
    assert_eq!(op.created_by, None);
    assert_eq!(op.extra["com.example.tag"], json!("x"));
}

#[test]
fn op_create_arguments_are_not_a_whole_op() {
    // L208: on create only `nodeId`, `baseSeq` and `data` are given; `id`,
    // `graphId`, `seq` and `created` are server-set (L172-191).
    let create = parse(SUBMIT_REQUEST)[1]["create"]["o1"].clone();
    assert!(serde_json::from_value::<GraphNodeOp>(create).is_err());
}

#[test]
fn op_query_filter_from_example_round_trips() {
    // L428: `"filter": { "nodeId": "N1", "afterSeq": 40 }`.
    let req = parse(LOAD_REQUEST);
    let f: GraphNodeOpFilterCondition = round_trip(&req["methodCalls"][1][1]["filter"]);
    assert_eq!(f.node_id, Id::from("N1"));
    assert_eq!(f.after_seq, Some(40));
    let f: GraphNodeOpFilterCondition = round_trip(&json!({ "nodeId": "N1" }));
    assert_eq!(f, GraphNodeOpFilterCondition::new(Id::from("N1")));
}

#[test]
fn op_query_filter_rejects_other_shapes() {
    // L222: "The `filter` argument MUST be a single FilterCondition, not a
    // FilterOperator, and MUST include `nodeId`".
    for bad in [
        json!({ "afterSeq": 3 }),
        json!({ "operator": "AND", "conditions": [{ "nodeId": "N1" }] }),
        json!({ "nodeId": "N1", "graphId": "G1" }),
    ] {
        assert!(serde_json::from_value::<GraphNodeOpFilterCondition>(bad).is_err());
    }
}

#[test]
fn op_query_sort_is_seq() {
    // L230: "The only supported sort is `seq` ascending".
    let c: GraphNodeOpComparator = serde_json::from_value(json!({ "property": "seq" })).unwrap();
    assert_eq!(c, GraphNodeOpComparator::new(GraphNodeOpSortProperty::Seq));
    assert_eq!(
        serde_json::from_value::<GraphNodeOpComparator>(json!({"property": "created"}))
            .unwrap()
            .property,
        GraphNodeOpSortProperty::Other("created".to_owned())
    );
}

#[test]
fn cursor_request_and_response_round_trip() {
    // L338-347: arguments `accountId`, `nodeId`, `data`; "The response has
    // the arguments `accountId` and `nodeId` only."
    let req: GraphNodeCursorRequest =
        round_trip(&json!({ "accountId": "A1", "nodeId": "N1", "data": "AQI" }));
    assert_eq!(
        req,
        GraphNodeCursorRequest::new(Id::from("A1"), Id::from("N1"), "AQI".to_owned())
    );
    let resp: GraphNodeCursorResponse = round_trip(&json!({ "accountId": "A1", "nodeId": "N1" }));
    assert_eq!(
        resp,
        GraphNodeCursorResponse::new(Id::from("A1"), Id::from("N1"))
    );
    assert!(serde_json::from_value::<GraphNodeCursorRequest>(
        json!({ "accountId": "A1", "nodeId": "N1" })
    )
    .is_err());
}

#[test]
fn subscribe_example_round_trips() {
    // L447-452, the "Subscribing to a node" example.
    let raw = parse(SUBSCRIBE);
    let msg: GraphEditStreamMessage = round_trip(&raw);
    let GraphEditStreamMessage::Enable(e) = msg else {
        panic!("wrong variant: {msg:?}")
    };
    assert_eq!(e.id.as_deref(), Some("sub1"));
    assert!(!e.extra.contains_key("id"));
    assert_eq!(e.node_ids, [Id::from("N1")]);
    assert_eq!(e.data_types, ["ops", "cursors"]);
}

#[test]
fn data_types_vocabulary_matches_draft() {
    // L302: "`\"ops\"` for `GraphNodeOpEvent` and `\"cursors\"` for
    // `GraphCursorEvent`".
    assert_eq!(GRAPH_EDIT_DATA_TYPES, ["ops", "cursors"]);
}

#[test]
fn enable_keeps_empty_and_unrecognized_data_types() {
    // L307: an empty or unrecognized-only `dataTypes` is an
    // `invalidArguments` RequestError, and "Unrecognized values alongside
    // recognized ones are ignored", so the server must see them verbatim.
    let msg: GraphEditStreamMessage =
        round_trip(&json!({ "@type": "GraphEditStreamEnable", "nodeIds": [], "dataTypes": [] }));
    assert!(matches!(msg, GraphEditStreamMessage::Enable(ref e) if e.data_types.is_empty()));
    let msg: GraphEditStreamMessage = round_trip(&json!({
        "@type": "GraphEditStreamEnable",
        "nodeIds": ["N1"],
        "dataTypes": ["ops", "presence"]
    }));
    assert!(matches!(msg, GraphEditStreamMessage::Enable(ref e)
        if e.data_types == ["ops", "presence"]));
}

#[test]
fn disable_round_trips() {
    // L311: "whose only property is `@type` with the value
    // `\"GraphEditStreamDisable\"`".
    let msg: GraphEditStreamMessage = round_trip(&json!({ "@type": "GraphEditStreamDisable" }));
    assert_eq!(
        msg,
        GraphEditStreamMessage::Disable(GraphEditStreamDisable::default())
    );
}

#[test]
fn stream_message_unknown_and_malformed() {
    let msg: GraphEditStreamMessage =
        round_trip(&json!({ "@type": "WebSocketPushEnable", "dataTypes": ["GraphNodeOp"] }));
    assert!(
        matches!(msg, GraphEditStreamMessage::Unknown { ref type_name, .. }
        if type_name == "WebSocketPushEnable")
    );
    // L298 `nodeIds` (Id[]) and L301 `dataTypes` (String[]) are required.
    assert!(serde_json::from_value::<GraphEditStreamMessage>(
        json!({ "@type": "GraphEditStreamEnable", "dataTypes": ["ops"] })
    )
    .is_err());
    assert!(
        serde_json::from_value::<GraphEditStreamMessage>(json!({ "nodeIds": ["N1"] })).is_err()
    );
}

#[test]
fn op_event_round_trips() {
    // L317-324: `@type` "GraphNodeOpEvent", `accountId`, `op` "with all its
    // properties".
    let raw = json!({ "@type": "GraphNodeOpEvent", "accountId": "A1", "op": example_op() });
    let ev: GraphEditEvent = round_trip(&raw);
    assert_eq!(ev.type_name(), "GraphNodeOpEvent");
    let GraphEditEvent::Op(e) = ev else {
        panic!("wrong variant: {ev:?}")
    };
    assert_eq!(e.account_id, Id::from("A1"));
    assert_eq!(e.op.seq, 42);
}

#[test]
fn cursor_event_round_trips() {
    // L353-366: `@type` "GraphCursorEvent", `accountId`, `nodeId`,
    // `senderId` (Id|null), `data`.
    let ev: GraphEditEvent = round_trip(&json!({
        "@type": "GraphCursorEvent",
        "accountId": "A1",
        "nodeId": "N1",
        "senderId": "U7",
        "data": "AQI"
    }));
    assert_eq!(ev.type_name(), "GraphCursorEvent");
    assert!(
        matches!(ev, GraphEditEvent::Cursor(GraphCursorEvent { ref sender_id, ref data, .. })
        if sender_id.as_ref() == Some(&Id::from("U7")) && data == "AQI")
    );
    let ev: GraphEditEvent = round_trip(&json!({
        "@type": "GraphCursorEvent",
        "accountId": "A1",
        "nodeId": "N1",
        "senderId": null,
        "data": "AQI"
    }));
    assert!(matches!(ev, GraphEditEvent::Cursor(ref c) if c.sender_id.is_none()));
}

#[test]
fn event_unknown_and_malformed() {
    let ev: GraphEditEvent = round_trip(&json!({ "@type": "StateChange", "changed": {} }));
    assert_eq!(ev.type_name(), "StateChange");
    assert!(matches!(ev, GraphEditEvent::Unknown { .. }));
    // L324 `op` (GraphNodeOp) is required.
    assert!(serde_json::from_value::<GraphEditEvent>(
        json!({ "@type": "GraphNodeOpEvent", "accountId": "A1" })
    )
    .is_err());
}
