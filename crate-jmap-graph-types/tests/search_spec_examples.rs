//! Checks the `search` module against draft-atwood-jmap-graph-search-00 at
//! jmap-chat-spec 6bcd9d8; line numbers cite that revision. The oracle is
//! the draft text (examples copied byte-for-byte, quoted rules) and
//! hand-written JSON, never the code under test.

mod common;

use jmap_graph_types::{
    Bound, CandidateKind, Direction, Expression, FieldTest, WhereError, QUERY_TOO_COMPLEX,
};
use jmap_graph_types::{GraphEdgeFilterCondition, GraphNodeFilterCondition};
use jmap_graph_types::{GraphSearchCapability, JMAP_GRAPH_SEARCH_URI};
use jmap_types::{Id, JmapError};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

/// L336-351, "Annotation tests", verbatim.
fn annotation_tests() -> String {
    common::fixture("annotation_tests.json")
}

/// L358-372, "Descendants", verbatim.
fn descendants() -> String {
    common::fixture("descendants.json")
}

/// L379-390, "Edges between matching nodes", verbatim.
fn edges_between() -> String {
    common::fixture("edges_between.json")
}

/// L406-414, "An advertised language", verbatim.
fn advertised_language() -> String {
    common::fixture("advertised_language.json")
}

fn filter<T: DeserializeOwned>(invocation: &str) -> T {
    let v: Value = serde_json::from_str(invocation).expect("invocation JSON");
    serde_json::from_value(v[1]["filter"].clone()).expect("filter condition")
}

fn node(v: Value) -> Result<Expression, WhereError> {
    Expression::parse(&v, CandidateKind::Node)
}

fn edge(v: Value) -> Result<Expression, WhereError> {
    Expression::parse(&v, CandidateKind::Edge)
}

fn error_type(e: WhereError) -> String {
    JmapError::from(e).error_type.to_string()
}

/// L126, L134-143: the capability URI and the account-level object.
#[test]
fn capability() {
    assert_eq!(JMAP_GRAPH_SEARCH_URI, "urn:ietf:params:jmap:graph:search");
    let cap: GraphSearchCapability = serde_json::from_value(json!({
        "languages": ["opencypher", "x-vendor"],
        "maxDepth": 8,
        "supportsText": false
    }))
    .unwrap();
    assert_eq!(cap.languages, ["opencypher", "x-vendor"]);
    assert_eq!(cap.max_depth, 8);
    assert!(!cap.supports_text);
}

/// L327: the `queryTooComplex` method-level error.
#[test]
fn query_too_complex_error_name() {
    assert_eq!(QUERY_TOO_COMPLEX, "queryTooComplex");
}

/// L336-351: `$in` and `$prefix` with `$collation` on two annotations.
#[test]
fn annotation_tests_example() {
    let cond: GraphNodeFilterCondition = filter(&annotation_tests());
    assert_eq!(cond.graph_id, Id::from("G1"));
    let expr = node(cond.where_.expect("where")).unwrap();
    assert_eq!(expr.fields.len(), 2);
    let status = &expr.fields["/annotations/com.example.status"];
    assert_eq!(status.in_, Some(vec![json!("open"), json!("blocked")]));
    let title = &expr.fields["/annotations/com.example.title"];
    assert_eq!(title.prefix.as_deref(), Some("draft"));
    assert_eq!(title.collation.as_deref(), Some("i;unicode-casemap"));
    assert_eq!(expr.traversal_depth(), 0);
}

/// L358-372: `$reachable` with `direction`, `type`, `maxDepth` and `node`;
/// L274: "`$reachable` counts as its `maxDepth`".
#[test]
fn descendants_example() {
    let cond: GraphNodeFilterCondition = filter(&descendants());
    let expr = node(cond.where_.expect("where")).unwrap();
    let reach = expr.reachable.as_deref().expect("$reachable");
    assert_eq!(reach.direction, Direction::In);
    assert_eq!(reach.edge_type.as_deref(), Some("com.example.child"));
    assert_eq!(reach.max_depth, 10);
    let inner = reach.node.as_ref().expect("node");
    assert_eq!(inner.fields["/id"].eq, Some(json!("N1")));
    assert_eq!(expr.traversal_depth(), 10);
}

/// L379-390: `$from` and `$to` on a GraphEdge candidate, combined with the
/// structural `type` filter; L274: each counts as one step.
#[test]
fn edges_between_matching_nodes_example() {
    let cond: GraphEdgeFilterCondition = filter(&edges_between());
    assert_eq!(cond.edge_type.as_deref(), Some("com.example.depends-on"));
    let expr = edge(cond.where_.expect("where")).unwrap();
    let from = expr.from.as_deref().expect("$from");
    let to = expr.to.as_deref().expect("$to");
    let key = "/annotations/com.example.status";
    assert_eq!(from.fields[key].eq, Some(json!("open")));
    assert_eq!(to.fields[key].eq, Some(json!("closed")));
    assert_eq!(expr.traversal_depth(), 1);
}

/// L406-414: `language` with `expression` and no `where`.
#[test]
fn advertised_language_example() {
    let cond: GraphNodeFilterCondition = filter(&advertised_language());
    assert_eq!(cond.language.as_deref(), Some("opencypher"));
    assert_eq!(cond.expression.as_deref(), Some("MATCH ... RETURN a"));
    assert!(cond.where_.is_none());
}

/// A condition without the search properties serializes without them, so
/// a request to a server without the capability does not grow them.
#[test]
fn search_properties_absent_by_default() {
    let cond = GraphNodeFilterCondition::new(Id::from("G1"));
    assert_eq!(
        serde_json::to_value(&cond).unwrap(),
        json!({ "graphId": "G1" })
    );
    let cond = GraphEdgeFilterCondition::new(Id::from("G1"));
    assert_eq!(
        serde_json::to_value(&cond).unwrap(),
        json!({ "graphId": "G1" })
    );
}

/// L170: "An empty object is always true"; L185-186: "An operator object
/// is a JSON object at least one of whose member names begins with `$`
/// [...] Any other value is shorthand for `{"$eq": value}`."
#[test]
fn plain_values_are_eq_shorthand() {
    assert_eq!(node(json!({})).unwrap(), Expression::default());
    for v in [
        json!("open"),
        json!(null),
        json!([1, 2]),
        json!({}),
        json!({ "a": 1 }),
    ] {
        let shorthand = node(json!({ "/x": v.clone() })).unwrap();
        let explicit = node(json!({ "/x": { "$eq": v.clone() } })).unwrap();
        assert_eq!(shorthand, explicit, "{v}");
        assert_eq!(shorthand.fields["/x"], FieldTest::equal_to(v));
    }
}

/// L192-217: every comparison operator with an operand of its stated type.
#[test]
fn comparison_operators() {
    let expr = node(json!({
        "/a": { "$ne": null, "$gt": 1, "$gte": "b", "$lt": 2.5, "$lte": "y" },
        "/b": { "$nin": [1, "x"], "$exists": true },
        "/c": { "$contains": "mid", "$collation": "i;ascii-casemap" }
    }))
    .unwrap();
    let a = &expr.fields["/a"];
    assert_eq!(a.ne, Some(Value::Null));
    assert_eq!(a.gt, Some(Bound::Number(1.into())));
    assert_eq!(a.gte, Some(Bound::String("b".to_owned())));
    assert!(matches!(&a.lt, Some(Bound::Number(n)) if n.as_f64() == Some(2.5)));
    assert_eq!(a.lte, Some(Bound::String("y".to_owned())));
    let b = &expr.fields["/b"];
    assert_eq!(b.nin, Some(vec![json!(1), json!("x")]));
    assert_eq!(b.exists, Some(true));
    let c = &expr.fields["/c"];
    assert_eq!(c.contains.as_deref(), Some("mid"));
    assert_eq!(c.collation.as_deref(), Some("i;ascii-casemap"));
}

/// L227-234: `$and`, `$or` and `$not`, including empty arrays.
#[test]
fn logical_operators() {
    let expr = node(json!({
        "$and": [],
        "$or": [{ "/a": 1 }, { "$not": { "/b": 2 } }]
    }))
    .unwrap();
    assert_eq!(expr.and, Some(vec![]));
    let or = expr.or.as_deref().expect("$or");
    assert_eq!(or.len(), 2);
    assert_eq!(
        or[1].not.as_deref().expect("$not").fields["/b"].eq,
        Some(json!(2))
    );
}

/// L246-252: an EdgeTest with every member; `edge` tests a GraphEdge and
/// `node` a GraphNode.
#[test]
fn edge_test_members() {
    let expr = node(json!({
        "$edge": {
            "direction": "any",
            "type": "t",
            "edge": { "/type": "t", "$to": { "/id": "N2" } },
            "node": { "$text": "hello" }
        }
    }))
    .unwrap();
    let t = expr.edge.as_deref().expect("$edge");
    assert_eq!(t.direction, Direction::Any);
    assert_eq!(t.edge_type.as_deref(), Some("t"));
    let e = t.edge.as_ref().expect("edge");
    assert_eq!(
        e.to.as_deref().expect("$to").fields["/id"].eq,
        Some(json!("N2"))
    );
    assert_eq!(
        t.node.as_ref().expect("node").text.as_deref(),
        Some("hello")
    );
}

/// L274, computed by hand: `$edge`, `$from` and `$to` are one step,
/// `$reachable` its `maxDepth`, summed along a chain and maximized across
/// chains; logical operators add nothing.
#[test]
fn traversal_depth() {
    let chained = node(json!({
        "$edge": { "direction": "out", "node": { "$reachable": { "direction": "out", "maxDepth": 3 } } }
    }))
    .unwrap();
    assert_eq!(chained.traversal_depth(), 4);
    let alternatives = node(json!({
        "$or": [
            { "$edge": { "direction": "in" } },
            { "$not": { "$reachable": { "direction": "any", "maxDepth": 2 } } }
        ]
    }))
    .unwrap();
    assert_eq!(alternatives.traversal_depth(), 2);
    let via_edge = edge(json!({
        "$from": { "$edge": { "direction": "out", "edge": { "$to": {} } } }
    }))
    .unwrap();
    assert_eq!(via_edge.traversal_depth(), 3);
}

/// L162, L177: a `$`-prefixed name the draft does not define is
/// `unsupportedFilter`; L521 lists `$regex`, `$where`, `$expr` and
/// `$elemMatch` as absent.
#[test]
fn unknown_operators_are_unsupported_filter() {
    for v in [
        json!({ "$regex": "a.*" }),
        json!({ "$where": "true" }),
        json!({ "/a": { "$elemMatch": {} } }),
        json!({ "$and": [{ "/a": { "$expr": 1 } }] }),
    ] {
        let e = node(v.clone()).unwrap_err();
        assert!(matches!(e, WhereError::UnknownOperator(_)), "{v}: {e}");
        assert_eq!(error_type(e), "unsupportedFilter", "{v}");
    }
}

/// L160-162: an expression that is not well-formed is `invalidArguments`.
#[test]
fn malformed_expressions_are_invalid_arguments() {
    let cases = [
        // "An Expression is a JSON object."
        json!([]),
        // "every member name is a JSON Pointer [...] or an operator"
        json!({ "status": "open" }),
        // RFC 6901: "~" is only "~0" or "~1".
        json!({ "/a~2": 1 }),
        // "An object that mixes `$`-prefixed and other member names"
        json!({ "/a": { "$eq": 1, "b": 2 } }),
        // "every operand has the type stated for it"
        json!({ "/a": { "$gt": true } }),
        json!({ "/a": { "$in": "x" } }),
        json!({ "/a": { "$exists": "yes" } }),
        json!({ "/a": { "$prefix": 1 } }),
        json!({ "/a": { "$collation": null } }),
        json!({ "$and": {} }),
        json!({ "$not": [] }),
        json!({ "$text": 1 }),
        // EdgeTest: required `direction`, only defined members, enum values
        json!({ "$edge": {} }),
        json!({ "$edge": { "direction": "both" } }),
        json!({ "$edge": { "direction": "out", "weight": 1 } }),
        json!({ "$edge": { "direction": "out", "$x": 1 } }),
        // ReachTest: required `maxDepth`, "at least 1", UnsignedInt
        json!({ "$reachable": { "direction": "out" } }),
        json!({ "$reachable": { "direction": "out", "maxDepth": 0 } }),
        json!({ "$reachable": { "direction": "out", "maxDepth": 1.5 } }),
        json!({ "$reachable": { "direction": "out", "maxDepth": 9007199254740992u64 } }),
        json!({ "$reachable": { "direction": "out", "maxDepth": 1, "edge": {} } }),
        // L242: `$from` and `$to` apply to a GraphEdge, not a GraphNode.
        json!({ "$from": {} }),
        json!({ "$edge": { "direction": "out", "node": { "$to": {} } } }),
    ];
    for v in cases {
        let e = node(v.clone()).unwrap_err();
        assert!(matches!(e, WhereError::Malformed(_)), "{v}: {e}");
        assert_eq!(error_type(e), "invalidArguments", "{v}");
    }
}

/// L242: `$edge` and `$reachable` apply to a GraphNode, so they are
/// malformed on a GraphEdge candidate; inside `$from` the candidate is a
/// node again.
#[test]
fn structural_operators_follow_the_candidate_kind() {
    for v in [
        json!({ "$edge": { "direction": "out" } }),
        json!({ "$reachable": { "direction": "out", "maxDepth": 1 } }),
    ] {
        let e = edge(v.clone()).unwrap_err();
        assert!(matches!(e, WhereError::Malformed(_)), "{v}: {e}");
    }
    // An EdgeTest's `edge` expression tests a GraphEdge.
    let v = json!({ "$edge": { "direction": "out", "edge": { "$edge": { "direction": "in" } } } });
    assert!(matches!(node(v), Err(WhereError::Malformed(_))));
    assert!(edge(json!({ "$from": { "$edge": { "direction": "out" } } })).is_ok());
    assert!(node(json!({ "$edge": { "direction": "out", "edge": { "$from": {} } } })).is_ok());
}

/// `to_value` writes the operator-object form of every field test, so
/// L336-351's expression comes back as this hand-written JSON.
#[test]
fn to_value_writes_operator_objects() {
    let cond: GraphNodeFilterCondition = filter(&annotation_tests());
    let expr = node(cond.where_.expect("where")).unwrap();
    assert_eq!(
        expr.to_value(),
        json!({
            "/annotations/com.example.status": { "$in": ["open", "blocked"] },
            "/annotations/com.example.title": {
                "$prefix": "draft",
                "$collation": "i;unicode-casemap"
            }
        })
    );
    let cond: GraphNodeFilterCondition = filter(&descendants());
    let expr = node(cond.where_.expect("where")).unwrap();
    assert_eq!(
        expr.to_value(),
        json!({
            "$reachable": {
                "direction": "in",
                "type": "com.example.child",
                "maxDepth": 10,
                "node": { "/id": { "$eq": "N1" } }
            }
        })
    );
    let shorthand = node(json!({ "/x": { "$eq": { "$y": 1 } } })).unwrap();
    assert_eq!(
        shorthand.to_value(),
        json!({ "/x": { "$eq": { "$y": 1 } } })
    );
}

/// L170: "An empty object is always true." L185-186: an operator object has
/// "at least one" `$`-prefixed member, and "Any other value is shorthand
/// for `{"$eq": value}`", so `{}` cannot carry an always-true field test.
/// `to_value` drops it and the result parses back to the same meaning.
#[test]
fn always_true_field_test_is_omitted() {
    let mut expr = Expression::default();
    expr.fields.insert("/x".to_owned(), FieldTest::default());
    expr.fields
        .insert("/y".to_owned(), FieldTest::equal_to(json!("open")));
    assert_eq!(expr.to_value(), json!({ "/y": { "$eq": "open" } }));
    let back = node(expr.to_value()).unwrap();
    assert_eq!(back, node(json!({ "/y": "open" })).unwrap());

    let mut only = Expression::default();
    only.fields.insert("/x".to_owned(), FieldTest::default());
    assert_eq!(only.to_value(), json!({}));
    assert_eq!(node(only.to_value()).unwrap(), Expression::default());
}
