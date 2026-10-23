//! draft-atwood-jmap-graph-edit-00 server rules: the GraphNode
//! `editFormat`/`snapshotSeq` extensions, `GraphNodeOp/set`, `/query`,
//! `/changes`, compaction and `GraphNode/cursor`. Line numbers cite
//! draft-atwood-jmap-graph-edit-00.md at jmap-chat-spec 6bcd9d8; each
//! assertion quotes the draft text it checks.

#[macro_use]
mod common;

use common::*;
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_server::{
    handle_graph_node_cursor, handle_graph_node_get, handle_graph_node_op_changes,
    handle_graph_node_op_get, handle_graph_node_op_query, handle_graph_node_op_query_changes,
    handle_graph_node_op_set, handle_graph_node_set, handle_graph_set,
};
use jmap_graph_types::GraphEditCapability;
use jmap_types::Id;
use serde_json::{json, Value};

const FORMAT: &str = "application/x-example-ops";

/// A backend whose [`ACCOUNT`] advertises the edit capability (L139-148)
/// with `maxOpSize` 8, `maxCursorSize` 4 and `opRetention` `retention`.
fn edit_backend_with(retention: Value) -> MemoryBackend {
    let backend = backend();
    let cap: GraphEditCapability = serde_json::from_value(json!({
        "maxOpSize": 8, "maxCursorSize": 4, "opRetention": retention
    }))
    .expect("capability fixture");
    backend.register_edit(&Id::from(ACCOUNT), cap);
    backend
}

fn edit_backend() -> MemoryBackend {
    edit_backend_with(Value::Null)
}

/// A Graph of [`ALICE`]'s shared with `share_with`.
async fn graph(backend: &MemoryBackend, share_with: Value) -> String {
    let resp = call!(
        handle_graph_set,
        backend,
        ALICE,
        json!({ "create": { "g": { "shareWith": share_with } } })
    );
    resp["created"]["g"]["id"]
        .as_str()
        .expect("graph")
        .to_owned()
}

async fn node_set(backend: &MemoryBackend, args: Value) -> Value {
    call!(handle_graph_node_set, backend, ALICE, args)
}

/// A node of `graph` with `editFormat` [`FORMAT`].
async fn edited_node(backend: &MemoryBackend, graph: &str) -> String {
    let resp = node_set(
        backend,
        json!({ "create": { "n": { "graphId": graph, "editFormat": FORMAT } } }),
    )
    .await;
    resp["created"]["n"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("node: {resp}"))
        .to_owned()
}

async fn op_set(backend: &MemoryBackend, caller: &str, create: Value) -> Value {
    call!(
        handle_graph_node_op_set,
        backend,
        caller,
        json!({ "create": create })
    )
}

/// Create one op of `node` as `caller` and return its `created` entry.
async fn op(backend: &MemoryBackend, caller: &str, node: &str, base_seq: u64) -> Value {
    let resp = op_set(
        backend,
        caller,
        json!({ "o": { "nodeId": node, "baseSeq": base_seq, "data": "AAECAwQF" } }),
    )
    .await;
    resp["created"]["o"].clone()
}

fn not_created_type(resp: &Value) -> (Value, Value) {
    let e = &resp["notCreated"]["o"];
    (e["type"].clone(), e["properties"].clone())
}

fn not_updated(resp: &Value, id: &str) -> (Value, Value) {
    let e = &resp["notUpdated"][id];
    (e["type"].clone(), e["properties"].clone())
}

/// Write a snapshot of `node` through `seq` with blob `blob`.
async fn snapshot(backend: &MemoryBackend, node: &str, blob: &str, seq: Value) -> Value {
    node_set(
        backend,
        json!({ "update": { node: {
            "blobId": blob, "mediaType": FORMAT, "snapshotSeq": seq
        } } }),
    )
    .await
}

// --- GraphNodeOp/set -----------------------------------------------------

/// L376-406 "Submitting an op": "The response acknowledges the op and
/// gives its place in the log" with `id`, `graphId`, `seq`, `created` and
/// `createdBy`.
#[tokio::test]
async fn submitting_an_op() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    let created = op(&backend, ALICE, &n, 0).await;
    assert!(created["id"].is_string(), "{created}");
    assert_eq!(created["graphId"], json!(g), "{created}");
    // L182: "The first op of a node has `seq` 1".
    assert_eq!(created["seq"], 1, "{created}");
    assert!(created["created"].is_string(), "{created}");
    // L193-194: "The id of the Principal that created the op".
    assert_eq!(created["createdBy"], ALICE, "{created}");
    // L182: "each later op of that node has the previous op's `seq` plus
    // one, with no gaps".
    assert_eq!(op(&backend, ALICE, &n, 1).await["seq"], 2);
    assert_eq!(op(&backend, ALICE, &n, 2).await["seq"], 3);

    let got = call!(
        handle_graph_node_op_get,
        &backend,
        ALICE,
        json!({ "ids": [created["id"]] })
    );
    assert_eq!(got["list"][0]["nodeId"], json!(n), "{got}");
    assert_eq!(got["list"][0]["baseSeq"], 0, "{got}");
    assert_eq!(got["list"][0]["data"], "AAECAwQF", "{got}");
}

/// L166: "creating an op ... does not change the GraphNode, its state
/// string, or its `updated` and `updatedBy` properties."
#[tokio::test]
async fn an_op_does_not_change_the_node() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    let before = call!(
        handle_graph_node_get,
        &backend,
        ALICE,
        json!({ "ids": [n] })
    );
    op(&backend, ALICE, &n, 0).await;
    let after = call!(
        handle_graph_node_get,
        &backend,
        ALICE,
        json!({ "ids": [n] })
    );
    assert_eq!(before["state"], after["state"]);
    assert_eq!(before["list"][0]["updated"], after["list"][0]["updated"]);
}

/// L208: "The server rejects the create with an `invalidProperties` SetError
/// if `nodeId` does not identify a node the user may read whose
/// `editFormat` is not `null` (naming `nodeId`)".
#[tokio::test]
async fn op_on_an_unusable_node_names_node_id() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    let plain = node_set(&backend, json!({ "create": { "p": { "graphId": g } } })).await;
    let plain = plain["created"]["p"]["id"]
        .as_str()
        .expect("plain")
        .to_owned();
    for (caller, node) in [
        (ALICE, "missing"),
        (ALICE, plain.as_str()),
        (BOB, n.as_str()),
    ] {
        let resp = op_set(
            &backend,
            caller,
            json!({ "o": { "nodeId": node, "baseSeq": 0, "data": "AA" } }),
        )
        .await;
        assert_eq!(
            not_created_type(&resp),
            (json!("invalidProperties"), json!(["nodeId"])),
            "{caller} {node}: {resp}"
        );
    }
}

/// L206: "A create requires `mayWrite` on the node's Graph."
#[tokio::test]
async fn op_create_needs_may_write() {
    let backend = edit_backend();
    let g = graph(&backend, json!({ BOB: rights(true, false, false) })).await;
    let n = edited_node(&backend, &g).await;
    let resp = op_set(
        &backend,
        BOB,
        json!({ "o": { "nodeId": n, "baseSeq": 0, "data": "AA" } }),
    )
    .await;
    assert_eq!(resp["notCreated"]["o"]["type"], "forbidden", "{resp}");
}

/// L208: rejected "if `baseSeq` is greater than the head of the node's edit
/// log (naming `baseSeq`), or if `data` is not valid base64url (naming
/// `data`)"; L188: `data` is "encoded with base64url without padding".
/// L208: "It rejects an op whose decoded `data` exceeds `maxOpSize` with a
/// `tooLarge` SetError."
#[tokio::test]
async fn op_base_seq_and_data_checks() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    let cases = [
        (
            json!(1),
            json!("AA"),
            "invalidProperties",
            json!(["baseSeq"]),
        ),
        (
            json!(0),
            json!("AA=="),
            "invalidProperties",
            json!(["data"]),
        ),
        (
            json!(0),
            json!("AA+/"),
            "invalidProperties",
            json!(["data"]),
        ),
        // 12 base64url characters decode to 9 octets, over maxOpSize 8.
        (json!(0), json!("AAECAwQFBgcI"), "tooLarge", Value::Null),
    ];
    for (base_seq, data, ty, props) in cases {
        let resp = op_set(
            &backend,
            ALICE,
            json!({ "o": { "nodeId": n, "baseSeq": base_seq, "data": data } }),
        )
        .await;
        assert_eq!(
            not_created_type(&resp),
            (json!(ty), props),
            "{data}: {resp}"
        );
    }
    // 11 characters decode to 8 octets, exactly maxOpSize.
    let resp = op_set(
        &backend,
        ALICE,
        json!({ "o": { "nodeId": n, "baseSeq": 0, "data": "AAECAwQFBgc" } }),
    )
    .await;
    assert_eq!(resp["created"]["o"]["seq"], 1, "{resp}");
}

/// L208: "On create, `nodeId`, `baseSeq`, and `data` are required."
#[tokio::test]
async fn op_create_requires_its_properties() {
    let backend = edit_backend();
    let resp = op_set(&backend, ALICE, json!({ "o": {} })).await;
    assert_eq!(
        not_created_type(&resp),
        (
            json!("invalidProperties"),
            json!(["nodeId", "baseSeq", "data"])
        ),
        "{resp}"
    );
}

/// L210: "The server MUST NOT reject an op because its `baseSeq` is old."
#[tokio::test]
async fn an_old_base_seq_is_accepted() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    for base in 0..3 {
        op(&backend, ALICE, &n, base).await;
    }
    assert_eq!(op(&backend, ALICE, &n, 0).await["seq"], 4);
}

/// L206: "An update or destroy is rejected with a `forbidden` SetError".
#[tokio::test]
async fn ops_cannot_be_updated_or_destroyed() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    let id = op(&backend, ALICE, &n, 0).await["id"].clone();
    let id = id.as_str().expect("op id");
    let resp = call!(
        handle_graph_node_op_set,
        &backend,
        ALICE,
        json!({ "update": { id: { "data": "AA" } }, "destroy": [id] })
    );
    assert_eq!(resp["notUpdated"][id]["type"], "forbidden", "{resp}");
    assert_eq!(resp["notDestroyed"][id]["type"], "forbidden", "{resp}");
}

/// RFC 8620 §3.6.2 `accountNotSupportedByMethod`: "The accountId given
/// corresponds to a valid account, but the account does not support this
/// method or data type." L131: servers supporting this specification
/// "advertise the `urn:ietf:params:jmap:graph:edit` capability".
#[tokio::test]
async fn edit_methods_need_the_capability() {
    let backend = backend();
    let ty = call_err!(handle_graph_node_op_set, &backend, ALICE, json!({}));
    assert_eq!(ty, "accountNotSupportedByMethod");
    let ty = call_err!(
        handle_graph_node_cursor,
        &backend,
        ALICE,
        json!({ "nodeId": "n", "data": "AA" })
    );
    assert_eq!(ty, "accountNotSupportedByMethod");
}

// --- GraphNodeOp/query ---------------------------------------------------

/// L222: "The `filter` argument MUST be a single FilterCondition, not a
/// FilterOperator, and MUST include `nodeId`; a server receiving any other
/// filter, or no filter, rejects the call with an `unsupportedFilter`
/// error". L230: "The only supported sort is `seq` ascending".
#[tokio::test]
async fn op_query_filter_and_sort_rules() {
    let backend = edit_backend();
    for filter in [
        Value::Null,
        json!({ "operator": "AND", "conditions": [{ "nodeId": "n" }] }),
        json!({ "afterSeq": 0 }),
    ] {
        let ty = call_err!(
            handle_graph_node_op_query,
            &backend,
            ALICE,
            json!({ "filter": filter })
        );
        assert_eq!(ty, "unsupportedFilter", "{filter}");
        let ty = call_err!(
            handle_graph_node_op_query_changes,
            &backend,
            ALICE,
            json!({ "filter": filter, "sinceQueryState": "0" })
        );
        assert_eq!(ty, "unsupportedFilter", "{filter}");
    }
    for sort in [
        json!([{ "property": "seq", "isAscending": false }]),
        json!([{ "property": "created" }]),
    ] {
        let ty = call_err!(
            handle_graph_node_op_query,
            &backend,
            ALICE,
            json!({ "filter": { "nodeId": "n" }, "sort": sort })
        );
        assert_eq!(ty, "unsupportedSort", "{sort}");
    }
}

/// A misspelt property next to `nodeId` is RFC 8620 §5.5
/// `unsupportedFilter` ("The filter is syntactically valid, but the server
/// cannot process it"), not a condition on `nodeId` alone (bd:JMAP-6azm).
/// L227 names the property `afterSeq`.
#[tokio::test]
async fn op_query_misspelt_filter_property_is_unsupported_filter() {
    let backend = edit_backend();
    let ty = call_err!(
        handle_graph_node_op_query,
        &backend,
        ALICE,
        json!({ "filter": { "nodeId": "n", "afterseq": 0 } })
    );
    assert_eq!(ty, "unsupportedFilter");
    let ty = call_err!(
        handle_graph_node_op_query_changes,
        &backend,
        ALICE,
        json!({ "filter": { "nodeId": "n", "afterseq": 0 }, "sinceQueryState": "0" })
    );
    assert_eq!(ty, "unsupportedFilter");
    // Negative control: the spec property is accepted.
    call!(
        handle_graph_node_op_query,
        &backend,
        ALICE,
        json!({ "filter": { "nodeId": "n", "afterSeq": 0 } })
    );
}

/// L227-228 `afterSeq`: "The op's `seq` is greater than this value"; L230
/// `seq` ascending "is also the order when `sort` is absent".
#[tokio::test]
async fn op_query_after_seq_in_seq_order() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    let other = edited_node(&backend, &g).await;
    let mut ids = Vec::new();
    for base in 0..4 {
        ids.push(op(&backend, ALICE, &n, base).await["id"].clone());
        op(&backend, ALICE, &other, base).await;
    }
    let resp = call!(
        handle_graph_node_op_query,
        &backend,
        ALICE,
        json!({ "filter": { "nodeId": n, "afterSeq": 1 } })
    );
    assert_eq!(resp["ids"], json!(ids[1..]), "{resp}");
    let resp = call!(
        handle_graph_node_op_query,
        &backend,
        ALICE,
        json!({ "filter": { "nodeId": n }, "sort": [{ "property": "seq" }] })
    );
    assert_eq!(resp["ids"], json!(ids), "{resp}");
}

// --- Snapshots and compaction --------------------------------------------

/// L234: "If the server has destroyed any op of the node whose `seq` is
/// greater than `afterSeq` ..., it MUST reject the query with an
/// `opsCompacted` error". L251: the server destroys "only from the start of
/// the log" and "MUST NOT destroy an op whose `seq` is greater than
/// `snapshotSeq`"; "Destroyed ops are reported by `GraphNodeOp/changes`."
#[tokio::test]
async fn compaction_is_front_only_and_reported() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    let mut ids = Vec::new();
    for base in 0..4 {
        ids.push(op(&backend, ALICE, &n, base).await["id"].clone());
    }
    let state = call!(
        handle_graph_node_op_get,
        &backend,
        ALICE,
        json!({ "ids": [] })
    )["state"]
        .clone();
    let resp = snapshot(&backend, &n, "B1", json!(2)).await;
    assert!(resp["updated"][&n].is_object(), "{resp}");

    // A request through seq 4 stops at snapshotSeq 2.
    let account = Id::from(ACCOUNT);
    assert_eq!(backend.compact_ops(&account, &Id::from(n.as_str()), 4), 2);

    for after in [0, 1] {
        let ty = call_err!(
            handle_graph_node_op_query,
            &backend,
            ALICE,
            json!({ "filter": { "nodeId": n, "afterSeq": after } })
        );
        assert_eq!(ty, "opsCompacted", "afterSeq {after}");
    }
    let resp = call!(
        handle_graph_node_op_query,
        &backend,
        ALICE,
        json!({ "filter": { "nodeId": n, "afterSeq": 2 } })
    );
    assert_eq!(resp["ids"], json!(ids[2..]), "{resp}");

    let changes = call!(
        handle_graph_node_op_changes,
        &backend,
        ALICE,
        json!({ "sinceState": state })
    );
    assert_eq!(changes["destroyed"], json!(ids[..2]), "{changes}");
}

/// L148, L251: "When `opRetention` is not `null`, the server keeps each op
/// for at least that many seconds after its `created` time."
#[tokio::test]
async fn compaction_respects_op_retention() {
    let backend = edit_backend_with(json!(3600));
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    op(&backend, ALICE, &n, 0).await;
    snapshot(&backend, &n, "B1", json!(1)).await;
    assert_eq!(
        backend.compact_ops(&Id::from(ACCOUNT), &Id::from(n.as_str()), 1),
        0
    );
}

/// L255: "When a node is destroyed, its ops are destroyed with it; when a
/// Graph is destroyed, so are the ops of all its nodes."
#[tokio::test]
async fn destroying_a_node_or_graph_destroys_its_ops() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    let m = edited_node(&backend, &g).await;
    let on_n = op(&backend, ALICE, &n, 0).await["id"].clone();
    let on_m = op(&backend, ALICE, &m, 0).await["id"].clone();
    let state = call!(
        handle_graph_node_op_get,
        &backend,
        ALICE,
        json!({ "ids": [] })
    )["state"]
        .clone();

    node_set(&backend, json!({ "destroy": [n] })).await;
    let changes = call!(
        handle_graph_node_op_changes,
        &backend,
        ALICE,
        json!({ "sinceState": state })
    );
    assert_eq!(changes["destroyed"], json!([on_n]), "{changes}");

    call!(handle_graph_set, &backend, ALICE, json!({ "destroy": [g] }));
    let got = call!(
        handle_graph_node_op_get,
        &backend,
        ALICE,
        json!({ "ids": [on_n, on_m] })
    );
    assert_eq!(got["notFound"], json!([on_n, on_m]), "{got}");
}

// --- GraphNode extensions ------------------------------------------------

/// L156, L159: `editFormat` and `snapshotSeq` default to `null`, echoed
/// like any defaulted property (RFC 8620 §5.3).
#[tokio::test]
async fn extensions_default_to_null() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let resp = node_set(&backend, json!({ "create": { "p": { "graphId": g } } })).await;
    let created = &resp["created"]["p"];
    assert_eq!(created["editFormat"], Value::Null, "{resp}");
    assert_eq!(created["snapshotSeq"], Value::Null, "{resp}");
    assert!(
        created
            .as_object()
            .is_some_and(|m| m.contains_key("editFormat")),
        "{resp}"
    );
}

/// L157: `editFormat` "may be set on create, or changed by update from
/// `null` to a non-null value; the server rejects any other change with an
/// `invalidProperties` SetError naming `editFormat`."
#[tokio::test]
async fn edit_format_is_set_once() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let resp = node_set(&backend, json!({ "create": { "p": { "graphId": g } } })).await;
    let p = resp["created"]["p"]["id"].as_str().expect("p").to_owned();
    let resp = node_set(
        &backend,
        json!({ "update": { &p: { "editFormat": FORMAT } } }),
    )
    .await;
    assert!(resp["updated"][&p].is_object(), "{resp}");
    for value in [json!("text/other"), Value::Null] {
        let resp = node_set(
            &backend,
            json!({ "update": { &p: { "editFormat": value } } }),
        )
        .await;
        assert_eq!(
            not_updated(&resp, &p),
            (json!("invalidProperties"), json!(["editFormat"])),
            "{value}: {resp}"
        );
    }
}

/// L162: "While `editFormat` is `null`, `snapshotSeq` is `null`; the server
/// rejects any other value with an `invalidProperties` SetError naming
/// `snapshotSeq`." "The server rejects, with an `invalidProperties`
/// SetError naming `editFormat`, a create or update that would give a node
/// a non-null `editFormat` while it has a non-null `blobId` and a `null`
/// `snapshotSeq`."
#[tokio::test]
async fn edit_format_and_blob_rules() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let resp = node_set(
        &backend,
        json!({ "create": {
            "a": { "graphId": g, "snapshotSeq": 1 },
            "b": { "graphId": g, "editFormat": FORMAT, "blobId": "B1" }
        } }),
    )
    .await;
    let props = |k: &str| {
        (
            resp["notCreated"][k]["type"].clone(),
            resp["notCreated"][k]["properties"].clone(),
        )
    };
    assert_eq!(
        props("a"),
        (json!("invalidProperties"), json!(["snapshotSeq"])),
        "{resp}"
    );
    assert_eq!(
        props("b"),
        (json!("invalidProperties"), json!(["editFormat"])),
        "{resp}"
    );

    let resp = node_set(
        &backend,
        json!({ "create": { "c": { "graphId": g, "blobId": "B1" } } }),
    )
    .await;
    let c = resp["created"]["c"]["id"].as_str().expect("c").to_owned();
    let resp = node_set(
        &backend,
        json!({ "update": { &c: { "editFormat": FORMAT } } }),
    )
    .await;
    assert_eq!(
        not_updated(&resp, &c),
        (json!("invalidProperties"), json!(["editFormat"])),
        "{resp}"
    );
}

/// L164: "The server rejects a `GraphNode/set` that sets `editFormat` on
/// such a node [of a projected Graph] with a `forbidden` SetError."
#[tokio::test]
async fn projected_nodes_have_no_edit_format() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    backend.set_projected_graph(&Id::from(ACCOUNT), &Id::from(g.as_str()));
    let resp = node_set(
        &backend,
        json!({ "create": { "n": { "graphId": g, "editFormat": FORMAT } } }),
    )
    .await;
    assert_eq!(resp["notCreated"]["n"]["type"], "forbidden", "{resp}");
}

/// L244: "A `GraphNode/set` update changes `blobId` if and only if it sets
/// `snapshotSeq`. Otherwise the server rejects it with an
/// `invalidProperties` SetError naming the property that is missing."
#[tokio::test]
async fn blob_and_snapshot_seq_change_together() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    op(&backend, ALICE, &n, 0).await;
    let resp = node_set(&backend, json!({ "update": { &n: { "blobId": "B1" } } })).await;
    assert_eq!(
        not_updated(&resp, &n),
        (json!("invalidProperties"), json!(["snapshotSeq"])),
        "{resp}"
    );
    let resp = node_set(&backend, json!({ "update": { &n: { "snapshotSeq": 1 } } })).await;
    assert_eq!(
        not_updated(&resp, &n),
        (json!("invalidProperties"), json!(["blobId"])),
        "{resp}"
    );
    let resp = snapshot(&backend, &n, "B1", json!(1)).await;
    assert!(resp["updated"][&n].is_object(), "{resp}");
    let got = call!(
        handle_graph_node_get,
        &backend,
        ALICE,
        json!({ "ids": [n] })
    );
    assert_eq!(got["list"][0]["snapshotSeq"], 1, "{got}");
    assert_eq!(got["list"][0]["blobId"], "B1", "{got}");
}

/// L245: "The server MUST reject, with an `invalidProperties` SetError
/// naming `snapshotSeq`, a value of 0, a value greater than the head of the
/// edit log, a value less than the current `snapshotSeq`, or `null` when
/// the current `snapshotSeq` is not `null`."
#[tokio::test]
async fn snapshot_seq_bounds() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    for base in 0..3 {
        op(&backend, ALICE, &n, base).await;
    }
    assert!(snapshot(&backend, &n, "B2", json!(2)).await["updated"][&n].is_object());
    for seq in [json!(0), json!(4), json!(1), Value::Null] {
        let resp = snapshot(&backend, &n, "B9", seq.clone()).await;
        assert_eq!(
            not_updated(&resp, &n),
            (json!("invalidProperties"), json!(["snapshotSeq"])),
            "{seq}: {resp}"
        );
    }
    // The same and a later value are accepted.
    assert!(snapshot(&backend, &n, "B3", json!(2)).await["updated"][&n].is_object());
    assert!(snapshot(&backend, &n, "B4", json!(3)).await["updated"][&n].is_object());
}

// --- GraphNode/cursor ----------------------------------------------------

/// L347: "The response has the arguments `accountId` and `nodeId` only";
/// L351-366: the GraphCursorEvent carries `senderId`, "The id of the
/// Principal whose cursor this is", and "The cursor data as given".
#[tokio::test]
async fn cursor_is_published() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    let resp = call!(
        handle_graph_node_cursor,
        &backend,
        ALICE,
        json!({ "nodeId": n, "data": "AAEC" })
    );
    assert_eq!(resp, json!({ "accountId": ACCOUNT, "nodeId": n }));
    let events = serde_json::to_value(backend.published_cursors()).expect("events");
    assert_eq!(
        events,
        json!([{ "accountId": ACCOUNT, "nodeId": n, "senderId": ALICE, "data": "AAEC" }])
    );
}

/// L347: "It fails with an `invalidArguments` error ... if `nodeId` does not
/// identify a node the user may read whose `editFormat` is not `null`, if
/// `data` is not valid base64url, or if its decoded size exceeds
/// `maxCursorSize`".
#[tokio::test]
async fn cursor_failures_are_invalid_arguments() {
    let backend = edit_backend();
    let g = graph(&backend, Value::Null).await;
    let n = edited_node(&backend, &g).await;
    let plain = node_set(&backend, json!({ "create": { "p": { "graphId": g } } })).await;
    let plain = plain["created"]["p"]["id"].clone();
    for (caller, node, data) in [
        (ALICE, json!("missing"), "AA"),
        (ALICE, plain.clone(), "AA"),
        (BOB, json!(n), "AA"),
        (ALICE, json!(n), "AA=="),
        // 7 characters decode to 5 octets, over maxCursorSize 4.
        (ALICE, json!(n), "AAECAwQ"),
    ] {
        let ty = call_err!(
            handle_graph_node_cursor,
            &backend,
            caller,
            json!({ "nodeId": node, "data": data })
        );
        assert_eq!(ty, "invalidArguments", "{caller} {node} {data}");
    }
    assert!(backend.published_cursors().is_empty());
}
