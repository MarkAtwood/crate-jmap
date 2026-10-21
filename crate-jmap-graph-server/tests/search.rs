//! draft-atwood-jmap-graph-search-00 on GraphNode/query and GraphEdge/query.
//! Line numbers cite draft-atwood-jmap-graph-search-00.md at jmap-chat-spec
//! 6bcd9d8; each assertion quotes the text it checks. Expected id sets are
//! worked out by hand from the graphs built here.

#[macro_use]
mod common;

use common::*;
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_server::{
    handle_graph_edge_query, handle_graph_edge_set, handle_graph_node_query,
    handle_graph_node_query_changes, handle_graph_node_set, handle_graph_set,
};
use jmap_graph_types::search::GraphSearchCapability;
use jmap_types::Id;
use serde_json::{json, Value};

/// [`backend`] advertising search with `maxDepth` 10 and `supportsText`
/// `text` (L134-143).
fn search_backend(text: bool) -> MemoryBackend {
    let backend = backend();
    let cap: GraphSearchCapability = serde_json::from_value(
        json!({ "languages": ["opencypher"], "maxDepth": 10, "supportsText": text }),
    )
    .expect("search capability fixture");
    backend.register_search(&Id::from(ACCOUNT), cap);
    backend
}

async fn graph(backend: &MemoryBackend) -> String {
    let resp = call!(
        handle_graph_set,
        backend,
        ALICE,
        json!({ "create": { "g": { "name": "g" } } })
    );
    resp["created"]["g"]["id"]
        .as_str()
        .expect("graph")
        .to_owned()
}

async fn node(backend: &MemoryBackend, graph: &str, annotations: Value) -> String {
    let resp = call!(
        handle_graph_node_set,
        backend,
        ALICE,
        json!({ "create": { "n": { "graphId": graph, "annotations": annotations } } })
    );
    resp["created"]["n"]["id"]
        .as_str()
        .expect("node")
        .to_owned()
}

async fn edge(backend: &MemoryBackend, graph: &str, from: &str, to: &str, ty: &str) -> String {
    let resp = call!(
        handle_graph_edge_set,
        backend,
        ALICE,
        json!({ "create": { "e": { "graphId": graph, "fromId": from, "toId": to, "type": ty } } })
    );
    resp["created"]["e"]["id"]
        .as_str()
        .expect("edge")
        .to_owned()
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids
}

/// The sorted `ids` of a GraphNode/query with filter `{graphId, where}`.
async fn nodes_where(backend: &MemoryBackend, g: &str, w: Value) -> Vec<String> {
    let resp = call!(
        handle_graph_node_query,
        backend,
        ALICE,
        json!({ "filter": { "graphId": g, "where": w } })
    );
    sorted(serde_json::from_value(resp["ids"].clone()).expect("ids"))
}

/// The method error `type` of a GraphNode/query with `filter`.
async fn node_query_error(backend: &MemoryBackend, filter: Value) -> Value {
    call_err!(
        handle_graph_node_query,
        backend,
        ALICE,
        json!({ "filter": filter })
    )
}

/// L334-351, "Open or blocked nodes whose title starts with "Draft",
/// ignoring case": `$in` over the status and `$prefix` under
/// `i;unicode-casemap` over the title.
#[tokio::test]
async fn annotation_tests_example() {
    let backend = search_backend(false);
    let g = graph(&backend).await;
    let status = "com.example.status";
    let title = "com.example.title";
    let open_draft = node(&backend, &g, json!({ status: "open", title: "Draft plan" })).await;
    let blocked_draft = node(
        &backend,
        &g,
        json!({ status: "blocked", title: "DRAFT notes" }),
    )
    .await;
    node(
        &backend,
        &g,
        json!({ status: "closed", title: "Draft old" }),
    )
    .await;
    node(&backend, &g, json!({ status: "open", title: "Final" })).await;
    node(&backend, &g, json!({ status: "open" })).await;
    let ids = nodes_where(
        &backend,
        &g,
        json!({
            "/annotations/com.example.status": { "$in": ["open", "blocked"] },
            "/annotations/com.example.title": {
                "$prefix": "draft",
                "$collation": "i;unicode-casemap"
            }
        }),
    )
    .await;
    assert_eq!(ids, sorted(vec![open_draft, blocked_draft]));
}

/// L356-372, "All nodes below `N1` in a tree built from `com.example.child`
/// edges (from parent to child) ... A node is below `N1` if `N1` is
/// reachable from it by following `com.example.child` edges backwards".
/// L262: "The candidate itself is reachable only if a path of at least one
/// step returns to it", so N1 is not below itself; an edge of another type
/// is not followed; `maxDepth` bounds the path length.
#[tokio::test]
async fn descendants_example() {
    let backend = search_backend(false);
    let g = graph(&backend).await;
    let n1 = node(&backend, &g, json!({})).await;
    let a = node(&backend, &g, json!({})).await;
    let b = node(&backend, &g, json!({})).await;
    let other = node(&backend, &g, json!({})).await;
    edge(&backend, &g, &n1, &a, "com.example.child").await;
    edge(&backend, &g, &a, &b, "com.example.child").await;
    edge(&backend, &g, &n1, &other, "com.example.link").await;
    let below = |max: u64| {
        json!({
            "$reachable": {
                "direction": "in",
                "type": "com.example.child",
                "maxDepth": max,
                "node": { "/id": n1 }
            }
        })
    };
    assert_eq!(
        nodes_where(&backend, &g, below(10)).await,
        sorted(vec![a.clone(), b])
    );
    // "the largest number of steps followed" (L259): B is two steps below.
    assert_eq!(nodes_where(&backend, &g, below(1)).await, vec![a]);
}

/// L262: "Cycles are permitted in the graph and do not affect the result
/// ... The candidate itself is reachable only if a path of at least one
/// step returns to it."
#[tokio::test]
async fn reachable_through_a_cycle_includes_the_candidate() {
    let backend = search_backend(false);
    let g = graph(&backend).await;
    let x = node(&backend, &g, json!({ "k": "x" })).await;
    let y = node(&backend, &g, json!({ "k": "y" })).await;
    let lone = node(&backend, &g, json!({ "k": "x" })).await;
    edge(&backend, &g, &x, &y, "").await;
    edge(&backend, &g, &y, &x, "").await;
    let ids = nodes_where(
        &backend,
        &g,
        json!({ "$reachable": { "direction": "out", "maxDepth": 2, "node": { "/annotations/k": "x" } } }),
    )
    .await;
    // x reaches itself in two steps, y reaches x in one; `lone` has no path.
    assert_eq!(ids, sorted(vec![x, y]));
    assert!(!ids.contains(&lone));
}

/// L377-390, "`com.example.depends-on` edges from an open node to a closed
/// one", with the draft-atwood-jmap-graph-00 `type` filter alongside
/// `where` ("each one present must match", L147).
#[tokio::test]
async fn edges_between_matching_nodes_example() {
    let backend = search_backend(false);
    let g = graph(&backend).await;
    let status = "com.example.status";
    let open = node(&backend, &g, json!({ status: "open" })).await;
    let closed = node(&backend, &g, json!({ status: "closed" })).await;
    let wanted = edge(&backend, &g, &open, &closed, "com.example.depends-on").await;
    edge(&backend, &g, &closed, &open, "com.example.depends-on").await;
    edge(&backend, &g, &open, &closed, "com.example.blocks").await;
    let resp = call!(
        handle_graph_edge_query,
        &backend,
        ALICE,
        json!({ "filter": {
            "graphId": g,
            "type": "com.example.depends-on",
            "where": {
                "$from": { "/annotations/com.example.status": "open" },
                "$to": { "/annotations/com.example.status": "closed" }
            }
        } })
    );
    assert_eq!(resp["ids"], json!([wanted]));
    // L321: "Servers MAY return `cannotCalculateChanges` for any
    // `/queryChanges` call whose filter uses a property defined in this
    // document", so the result says so.
    assert_eq!(resp["canCalculateChanges"], json!(false));
}

/// L246-252 `$edge`: "`"out"` for edges whose `fromId` is the candidate,
/// `"in"` for edges whose `toId` is the candidate, or `"any"` for both";
/// `node` is "the node at the other end of the edge".
#[tokio::test]
async fn edge_operator_directions() {
    let backend = search_backend(false);
    let g = graph(&backend).await;
    let p = node(&backend, &g, json!({ "role": "parent" })).await;
    let c = node(&backend, &g, json!({ "role": "child" })).await;
    let alone = node(&backend, &g, json!({ "role": "child" })).await;
    edge(&backend, &g, &p, &c, "has").await;
    let to_child = |dir: &str| json!({ "$edge": { "direction": dir, "node": { "/annotations/role": "child" } } });
    assert_eq!(
        nodes_where(&backend, &g, to_child("out")).await,
        vec![p.clone()]
    );
    assert_eq!(
        nodes_where(&backend, &g, to_child("in")).await,
        Vec::<String>::new()
    );
    assert_eq!(
        nodes_where(&backend, &g, to_child("any")).await,
        vec![p.clone()]
    );
    let any_edge = json!({ "$edge": { "direction": "any", "type": "has" } });
    assert_eq!(
        nodes_where(&backend, &g, any_edge).await,
        sorted(vec![p, c])
    );
    assert!(
        !nodes_where(&backend, &g, json!({ "$edge": { "direction": "any" } }))
            .await
            .contains(&alone)
    );
}

/// L192-219: comparison operators and JSON equality.
#[tokio::test]
async fn comparison_semantics() {
    let backend = search_backend(false);
    let g = graph(&backend).await;
    let null = node(&backend, &g, json!({ "v": null })).await;
    let one = node(&backend, &g, json!({ "v": 1 })).await;
    let two_and_half = node(&backend, &g, json!({ "v": 2.5 })).await;
    let text = node(&backend, &g, json!({ "v": "1" })).await;
    let missing = node(&backend, &g, json!({})).await;
    let all = sorted(vec![
        null.clone(),
        one.clone(),
        two_and_half.clone(),
        text.clone(),
        missing.clone(),
    ]);

    // "`{"$eq": null}` is true only for an explicit `null`".
    assert_eq!(
        nodes_where(&backend, &g, json!({ "/annotations/v": { "$eq": null } })).await,
        vec![null.clone()]
    );
    // "`$exists` ... If `true`, the field is present (its value may be
    // `null`); if `false`, it is missing."
    assert_eq!(
        nodes_where(
            &backend,
            &g,
            json!({ "/annotations/v": { "$exists": false } })
        )
        .await,
        vec![missing.clone()]
    );
    // "both are numbers with the same numeric value"; "Values of different
    // types are never equal": 1.0 equals 1 but not "1".
    assert_eq!(
        nodes_where(&backend, &g, json!({ "/annotations/v": 1.0 })).await,
        vec![one.clone()]
    );
    // "`$ne` ... Not `$eq`. True when the field is missing."
    let ne = nodes_where(&backend, &g, json!({ "/annotations/v": { "$ne": 1 } })).await;
    let expected: Vec<String> = all.iter().filter(|i| **i != one).cloned().collect();
    assert_eq!(ne, expected);
    // "`$gt` ... The field and the operand are both numbers and compare
    // numerically ... Otherwise false": the string "1" is not ordered.
    assert_eq!(
        nodes_where(
            &backend,
            &g,
            json!({ "/annotations/v": { "$gt": 0, "$lte": 2.5 } })
        )
        .await,
        sorted(vec![one, two_and_half])
    );
    // "`$nin` ... Not `$in`. True when the field is missing."
    let nin = nodes_where(
        &backend,
        &g,
        json!({ "/annotations/v": { "$nin": [null, 1, 2.5, "1"] } }),
    )
    .await;
    assert_eq!(nin, vec![missing]);
    // Logical operators (L227-234): "An empty array is true" for `$and`,
    // "An empty array is false" for `$or`.
    assert_eq!(nodes_where(&backend, &g, json!({ "$and": [] })).await, all);
    assert!(nodes_where(&backend, &g, json!({ "$or": [] }))
        .await
        .is_empty());
    assert_eq!(
        nodes_where(&backend, &g, json!({ "$not": { "/annotations/v": "1" } }))
            .await
            .len(),
        4
    );
    assert!(
        !nodes_where(&backend, &g, json!({ "$not": { "/annotations/v": "1" } }))
            .await
            .contains(&text)
    );
}

/// L223: "When the field is an array and the operand ... is not an array,
/// the operator is true if it is true for at least one element of the
/// field ... so `{"$ne": "x"}` on an array field is true only if no element
/// equals `"x"`. Elements are not searched recursively."
#[tokio::test]
async fn array_fields() {
    let backend = search_backend(false);
    let g = graph(&backend).await;
    let xy = node(&backend, &g, json!({ "tags": ["x", "y"] })).await;
    let z = node(&backend, &g, json!({ "tags": ["z"] })).await;
    let nested = node(&backend, &g, json!({ "tags": [["x"]] })).await;
    assert_eq!(
        nodes_where(&backend, &g, json!({ "/annotations/tags": "x" })).await,
        vec![xy.clone()]
    );
    assert_eq!(
        nodes_where(&backend, &g, json!({ "/annotations/tags": { "$ne": "x" } })).await,
        sorted(vec![z.clone(), nested.clone()])
    );
    // An array operand compares with the whole array.
    assert_eq!(
        nodes_where(
            &backend,
            &g,
            json!({ "/annotations/tags": { "$eq": ["z"] } })
        )
        .await,
        vec![z.clone()]
    );
    // "`$in` is true if `$eq`, with this rule applied, is true for at least
    // one element of the operand": `["z"]` equals the whole field of `z`,
    // `"y"` an element of `xy`'s; `[["x"]]`'s element `["x"]` is not `"y"`.
    assert_eq!(
        nodes_where(
            &backend,
            &g,
            json!({ "/annotations/tags": { "$in": [["z"], "y"] } })
        )
        .await,
        sorted(vec![xy, z])
    );
}

/// L238: "When `$collation` is absent, strings are compared as sequences of
/// Unicode code points ... When `$collation` is present, every string
/// operator in the same operator object, including `$eq`, ... uses it."
/// Oracles for the collations: RFC 4790 §9.2.1 (`i;ascii-casemap`: "letters
/// outside ASCII are not treated case-insensitively") and the RFC 5051 §2
/// example, which converts U+01C4 to U+0044 U+007A U+030C, as it does
/// U+01C6 by way of its titlecase U+01C5.
#[tokio::test]
async fn collations() {
    let backend = search_backend(false);
    let g = graph(&backend).await;
    let abc = node(&backend, &g, json!({ "s": "ABC" })).await;
    let e_acute = node(&backend, &g, json!({ "s": "\u{c9}" })).await;
    let dz = node(&backend, &g, json!({ "s": "\u{1c4}" })).await;
    let eq = |v: &str, c: Option<&str>| match c {
        Some(c) => json!({ "/annotations/s": { "$eq": v, "$collation": c } }),
        None => json!({ "/annotations/s": { "$eq": v } }),
    };
    assert!(nodes_where(&backend, &g, eq("abc", None)).await.is_empty());
    assert_eq!(
        nodes_where(&backend, &g, eq("abc", Some("i;ascii-casemap"))).await,
        vec![abc.clone()]
    );
    assert!(
        nodes_where(&backend, &g, eq("\u{e9}", Some("i;ascii-casemap")))
            .await
            .is_empty()
    );
    assert_eq!(
        nodes_where(&backend, &g, eq("\u{e9}", Some("i;unicode-casemap"))).await,
        vec![e_acute]
    );
    assert_eq!(
        nodes_where(&backend, &g, eq("\u{1c6}", Some("i;unicode-casemap"))).await,
        vec![dz]
    );
    assert!(nodes_where(&backend, &g, eq("\u{1c6}", Some("i;octet")))
        .await
        .is_empty());
    // RFC 5051 §2: U+01C4 prepares to U+0044 U+007A U+030C ("Dz" and a
    // caron), and "dz" to "DZ" (each code point titlecased; nothing is
    // re-cased after decomposition), so it does not contain "dz".
    assert!(nodes_where(
        &backend,
        &g,
        json!({ "/annotations/s": { "$contains": "dz", "$collation": "i;unicode-casemap" } })
    )
    .await
    .is_empty());
    // Ordering by code point: "ABC" (U+0041...) sorts below "abc".
    assert_eq!(
        nodes_where(&backend, &g, json!({ "/annotations/s": { "$lt": "abc" } })).await,
        vec![abc]
    );
}

/// L278-281: `$text` "True if the candidate matches the given text. What
/// constitutes a match ... is server-defined"; this server matches a
/// string-valued annotation containing the text under `i;unicode-casemap`
/// (`GraphView` docs).
#[tokio::test]
async fn text_operator() {
    let backend = search_backend(true);
    let g = graph(&backend).await;
    let report = node(&backend, &g, json!({ "title": "Quarterly Report" })).await;
    node(&backend, &g, json!({ "title": "Minutes" })).await;
    assert_eq!(
        nodes_where(&backend, &g, json!({ "$text": "report" })).await,
        vec![report]
    );
}

/// L162 error rules and L274 `queryTooComplex`.
#[tokio::test]
async fn filter_errors() {
    let backend = search_backend(false);
    let g = graph(&backend).await;
    let with = |extra: Value| {
        let mut f = json!({ "graphId": g });
        f.as_object_mut()
            .expect("object")
            .extend(extra.as_object().expect("object").clone());
        f
    };
    let cases = [
        // "if exactly one of `language` and `expression` is present"
        (json!({ "language": "opencypher" }), "invalidArguments"),
        (
            json!({ "expression": "MATCH (a) RETURN a" }),
            "invalidArguments",
        ),
        // "`language` is not in the account's `languages` list"
        (
            json!({ "language": "gremlin", "expression": "g.V()" }),
            "unsupportedFilter",
        ),
        // "uses a `$`-prefixed name that is not an operator defined in this
        // document"
        (
            json!({ "where": { "/x": { "$regex": "a.*" } } }),
            "unsupportedFilter",
        ),
        // "structural operators appear only on the kind of candidate
        // stated" (L160): `$from` is for GraphEdge candidates.
        (json!({ "where": { "$from": {} } }), "invalidArguments"),
        // "every ReachTest `maxDepth` is at least 1" (L160)
        (
            json!({ "where": { "$reachable": { "direction": "out", "maxDepth": 0 } } }),
            "invalidArguments",
        ),
        // "the expression uses `$text` and `supportsText` is `false`"
        (json!({ "where": { "$text": "x" } }), "unsupportedFilter"),
        // "an operator object's `$collation` is not in
        // `collationAlgorithms`"
        (
            json!({ "where": { "/x": { "$eq": "a", "$collation": "i;bogus" } } }),
            "unsupportedFilter",
        ),
        // L274: "A server MUST reject an expression whose traversal depth
        // exceeds the advertised `maxDepth` with a `queryTooComplex`
        // error": `$reachable` counts as its `maxDepth` (10) plus one
        // `$edge` step is 11 > 10.
        (
            json!({ "where": { "$reachable": {
                "direction": "out", "maxDepth": 10,
                "node": { "$edge": { "direction": "out" } }
            } } }),
            "queryTooComplex",
        ),
    ];
    for (extra, want) in cases {
        let filter = with(extra);
        assert_eq!(
            node_query_error(&backend, filter.clone()).await,
            want,
            "{filter}"
        );
    }
    // Exactly at the limit is accepted (depth 10).
    let ok = call!(
        handle_graph_node_query,
        &backend,
        ALICE,
        json!({ "filter": with(json!({ "where": { "$reachable": { "direction": "out", "maxDepth": 10 } } })) })
    );
    assert_eq!(ok["ids"], json!([]));
    // GraphEdge/query: `$edge` applies "when the candidate is a GraphNode".
    let t = call_err!(
        handle_graph_edge_query,
        &backend,
        ALICE,
        json!({ "filter": with(json!({ "where": { "$edge": { "direction": "out" } } })) })
    );
    assert_eq!(t, "invalidArguments");
}

/// L321: "Servers MAY return `cannotCalculateChanges` for any
/// `/queryChanges` call whose filter uses a property defined in this
/// document."
#[tokio::test]
async fn query_changes_with_search_cannot_calculate() {
    let backend = search_backend(false);
    let g = graph(&backend).await;
    let t = call_err!(
        handle_graph_node_query_changes,
        &backend,
        ALICE,
        json!({ "filter": { "graphId": g, "where": {} }, "sinceQueryState": "0" })
    );
    assert_eq!(t, "cannotCalculateChanges");
}

/// L313-315: "The nodes and edges a search considers are exactly those the
/// user could fetch with `/get`": a user who may not read the Graph finds
/// nothing, even with the always-true empty expression (L170).
#[tokio::test]
async fn unreadable_graph_contributes_nothing() {
    let backend = search_backend(false);
    let g = graph(&backend).await;
    node(&backend, &g, json!({})).await;
    let resp = call!(
        handle_graph_node_query,
        &backend,
        BOB,
        json!({ "filter": { "graphId": g, "where": {} } })
    );
    assert_eq!(resp["ids"], json!([]));
    assert_eq!(nodes_where(&backend, &g, json!({})).await.len(), 1);
}
