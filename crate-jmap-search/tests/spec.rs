//! Checks jmap-search against docs/specs/jmap-search.md
//! (draft-atwood-jmap-search-00), cited by section. The oracle is the draft
//! text (examples copied byte-for-byte, quoted rules) and hand-written JSON
//! objects, never the code under test.

use jmap_search::{
    Bound, Candidate, DataTypeSearchInfo, Expression, FieldTest, JMAP_SEARCH_URI,
    QUERY_TOO_COMPLEX, SearchCapability, WhereError, check_where, check_where_filter,
    filter_uses_where, where_values,
};
use jmap_types::JmapError;
use serde_json::{Value, json};

/// §Examples, "Large flagged Emails", verbatim.
const LARGE_FLAGGED: &str = r#"["Email/query", {
  "accountId": "A1",
  "filter": {
    "inMailbox": "M1",
    "where": {
      "/keywords/$flagged": true,
      "/size": { "$gt": 1000000 },
      "/subject": {
        "$prefix": "invoice",
        "$collation": "i;unicode-casemap"
      }
    }
  }
}, "0"]"#;

/// §Examples, "Chat Messages with a body type", verbatim.
const BODY_TYPE: &str = r#"["Message/query", {
  "accountId": "A1",
  "filter": {
    "chatId": "C1",
    "where": {
      "$or": [
        { "/bodyType": "text/markdown" },
        { "/reactions": { "$ne": {} } }
      ]
    }
  }
}, "0"]"#;

fn where_of(invocation: &str) -> Value {
    let v: Value = serde_json::from_str(invocation).expect("invocation JSON");
    v[1]["filter"]["where"].clone()
}

fn parse(v: Value) -> Result<Expression, WhereError> {
    Expression::parse(&v)
}

/// `expr` (parsed from hand-written JSON) evaluated on `object`.
fn eval(expr: Value, object: &Value) -> bool {
    parse(expr).expect("well-formed").matches(object)
}

fn error_type(e: JmapError) -> String {
    e.error_type.to_string()
}

fn cap(max_depth: u64, supports_text: bool) -> SearchCapability {
    SearchCapability::new(max_depth).with_data_type("Email", DataTypeSearchInfo::new(supports_text))
}

/// §Capability: "advertise the `urn:ietf:params:jmap:search` capability";
/// §Account-Level Capability Object: `dataTypes` (String[DataTypeSearchInfo])
/// with `supportsText`, and `maxDepth`; "Clients ignore fields they do not
/// recognize" (kept, not dropped, on a round trip).
#[test]
fn capability_object() {
    assert_eq!(JMAP_SEARCH_URI, "urn:ietf:params:jmap:search");
    let wire = json!({
        "dataTypes": {
            "Email": { "supportsText": true },
            "Message": { "supportsText": false, "x-vendor": 1 }
        },
        "maxDepth": 8,
        "x-other": "kept"
    });
    let cap: SearchCapability = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(cap.max_depth, 8);
    assert!(cap.data_type("Email").unwrap().supports_text);
    assert!(!cap.data_type("Message").unwrap().supports_text);
    assert!(cap.data_type("Mailbox").is_none());
    assert_eq!(serde_json::to_value(&cap).unwrap(), wire);
}

/// §Errors: "`queryTooComplex`: The query is valid, but the server will not
/// evaluate it because it exceeds a server limit".
#[test]
fn query_too_complex_name() {
    assert_eq!(QUERY_TOO_COMPLEX, "queryTooComplex");
}

/// §Examples "Large flagged Emails": a pointer through a map key
/// (§Addressable Properties: "`/keywords/$seen` ... valid pointers into an
/// Email"), `$gt`, and `$prefix` with `$collation`.
#[test]
fn large_flagged_example() {
    let expr = parse(where_of(LARGE_FLAGGED)).unwrap();
    assert_eq!(expr.fields.len(), 3);
    assert_eq!(
        expr.fields["/keywords/$flagged"],
        FieldTest::equal_to(json!(true))
    );
    assert_eq!(
        expr.fields["/size"].gt,
        Some(Bound::Number(1_000_000.into()))
    );
    let subject = &expr.fields["/subject"];
    assert_eq!(subject.prefix.as_deref(), Some("invoice"));
    assert_eq!(subject.collation.as_deref(), Some("i;unicode-casemap"));

    let email = json!({
        "id": "E1",
        "keywords": { "$flagged": true, "$seen": true },
        "size": 2_000_000,
        "subject": "INVOICE 2026-11"
    });
    assert!(expr.matches(&email));
    let small = json!({ "keywords": { "$flagged": true }, "size": 10, "subject": "Invoice" });
    assert!(!expr.matches(&small));
    let unflagged = json!({ "keywords": {}, "size": 2_000_000, "subject": "Invoice" });
    assert!(!expr.matches(&unflagged));
}

/// §Examples "Chat Messages with a body type": `$or` of a plain value and
/// `$ne` with an object operand.
#[test]
fn body_type_example() {
    let expr = parse(where_of(BODY_TYPE)).unwrap();
    assert_eq!(expr.depth(), 1);
    let markdown = json!({ "bodyType": "text/markdown", "reactions": {} });
    let reacted = json!({ "bodyType": "text/plain", "reactions": { "r1": { "emoji": "x" } } });
    let neither = json!({ "bodyType": "text/plain", "reactions": {} });
    assert!(expr.matches(&markdown));
    assert!(expr.matches(&reacted));
    assert!(!expr.matches(&neither));
}

/// §Expressions: "An empty object is always true"; §Field Tests: "Any other
/// value is shorthand for `{"$eq": value}`".
#[test]
fn empty_expression_and_shorthand() {
    assert_eq!(parse(json!({})).unwrap(), Expression::default());
    assert!(eval(json!({}), &json!({ "a": 1 })));
    for v in [
        json!(1),
        json!("x"),
        json!(null),
        json!([1, 2]),
        json!({ "k": 1 }),
    ] {
        let e = parse(json!({ "/x": v.clone() })).unwrap();
        assert_eq!(e.fields["/x"], FieldTest::equal_to(v));
    }
}

/// §The where FilterCondition Property, well-formedness; §Errors: a
/// malformed `where` is `invalidArguments`.
#[test]
fn malformed_is_invalid_arguments() {
    for v in [
        // "it is a JSON object"
        json!([]),
        json!("x"),
        // "every member name is a JSON Pointer ... or an operator"
        json!({ "size": 1 }),
        json!({ "/a~2": 1 }),
        // "every operator object contains only `$`-prefixed names"
        json!({ "/a": { "$eq": 1, "b": 2 } }),
        // "every operand has the type stated for it"
        json!({ "$and": {} }),
        json!({ "$or": [1] }),
        json!({ "$not": [] }),
        json!({ "$text": 1 }),
        json!({ "/a": { "$gt": true } }),
        json!({ "/a": { "$in": "x" } }),
        json!({ "/a": { "$exists": "yes" } }),
        json!({ "/a": { "$prefix": 1 } }),
        json!({ "/a": { "$collation": 1 } }),
    ] {
        let e = parse(v.clone()).unwrap_err();
        assert!(matches!(e, WhereError::Malformed(_)), "{v}: {e}");
        assert_eq!(error_type(e.into()), "invalidArguments", "{v}");
    }
}

/// §Errors: "if `where` uses a `$`-prefixed name that is not an operator
/// defined in this document (so that ... the structural operators of
/// {{JMAP-GRAPH-SEARCH}} are unsupported here)": `unsupportedFilter`.
#[test]
fn unknown_and_structural_operators_are_unsupported() {
    for v in [
        json!({ "$regex": "a" }),
        json!({ "$edge": { "direction": "out" } }),
        json!({ "$reachable": { "direction": "in", "maxDepth": 1 } }),
        json!({ "$from": {} }),
        json!({ "$to": {} }),
        json!({ "/a": { "$elemMatch": {} } }),
        json!({ "$and": [{ "$where": "1" }] }),
    ] {
        let e = parse(v.clone()).unwrap_err();
        assert!(matches!(e, WhereError::UnknownOperator(_)), "{v}: {e}");
        assert_eq!(error_type(e.into()), "unsupportedFilter", "{v}");
    }
}

/// §Nesting Depth: "`{}` and `{"/size": {"$gt": 10}}` have depth 0, and
/// `{"$not": {"$or": [{"/a": 1}]}}` has depth 2."
#[test]
fn nesting_depth() {
    assert_eq!(parse(json!({})).unwrap().depth(), 0);
    assert_eq!(parse(json!({"/size": {"$gt": 10}})).unwrap().depth(), 0);
    assert_eq!(
        parse(json!({"$not": {"$or": [{"/a": 1}]}}))
            .unwrap()
            .depth(),
        2
    );
    // "1 more than the largest nesting depth of the expressions that are
    // operands": an empty `$and` still has a logical operator.
    assert_eq!(parse(json!({"$and": []})).unwrap().depth(), 1);
    assert_eq!(
        parse(json!({"$and": [{}, {"$or": [{"$not": {}}]}]}))
            .unwrap()
            .depth(),
        3
    );
}

/// §Nesting Depth: "A server MUST reject an expression whose nesting depth
/// exceeds the advertised `maxDepth` with a `queryTooComplex` error".
#[test]
fn depth_over_max_is_query_too_complex() {
    let deep = json!({"$not": {"$or": [{"/a": 1}]}});
    assert!(check_where(&deep, "Email", &cap(2, false)).is_ok());
    let e = check_where(&deep, "Email", &cap(1, false)).unwrap_err();
    assert_eq!(error_type(e), "queryTooComplex");
}

/// §Errors: `unsupportedFilter` "if the type is not in the account's
/// `dataTypes`", "if the expression uses `$text` and the type's
/// `supportsText` is `false`", and "if an operator object's `$collation` is
/// not in `collationAlgorithms`".
#[test]
fn capability_checks() {
    let e = check_where(&json!({}), "Mailbox", &cap(4, true)).unwrap_err();
    assert_eq!(error_type(e), "unsupportedFilter");

    let text = json!({ "$or": [{ "$text": "hello" }] });
    assert!(check_where(&text, "Email", &cap(4, true)).is_ok());
    let e = check_where(&text, "Email", &cap(4, false)).unwrap_err();
    assert_eq!(error_type(e), "unsupportedFilter");

    for c in ["i;ascii-casemap", "i;octet", "i;unicode-casemap"] {
        let w = json!({ "/subject": { "$eq": "a", "$collation": c } });
        assert!(check_where(&w, "Email", &cap(4, false)).is_ok(), "{c}");
    }
    let w = json!({ "$not": { "/subject": { "$eq": "a", "$collation": "i;x-unknown" } } });
    let e = check_where(&w, "Email", &cap(4, false)).unwrap_err();
    assert_eq!(error_type(e), "unsupportedFilter");
}

/// §Opting In a Data Type: "A data type accepts `where` on an account
/// exactly when the server lists it in that account's `dataTypes`, and only
/// in requests whose `using` includes `urn:ietf:params:jmap:search`";
/// §The where FilterCondition Property: a FilterCondition with `where` "may
/// appear anywhere the type's `/query` accepts a FilterCondition, including
/// inside FilterOperator objects", and "A `where` of `null` is the same as
/// no `where`."
#[test]
fn filter_gate() {
    let using = vec![
        "urn:ietf:params:jmap:core".to_owned(),
        JMAP_SEARCH_URI.to_owned(),
    ];
    let core_only = vec!["urn:ietf:params:jmap:core".to_owned()];
    let nested = json!({
        "operator": "AND",
        "conditions": [
            { "inMailbox": "M1" },
            { "operator": "NOT", "conditions": [{ "where": { "/size": { "$lt": 5 } } }] }
        ]
    });
    assert!(filter_uses_where(&nested));
    assert_eq!(where_values(&nested), [&json!({ "/size": { "$lt": 5 } })]);

    let c = cap(4, false);
    assert_eq!(
        check_where_filter(Some(&nested), "Email", &using, Some(&c)),
        Ok(true)
    );
    let e = check_where_filter(Some(&nested), "Email", &core_only, Some(&c)).unwrap_err();
    assert_eq!(error_type(e), "unsupportedFilter");
    let e = check_where_filter(Some(&nested), "Email", &using, None).unwrap_err();
    assert_eq!(error_type(e), "unsupportedFilter");

    // No `where`, or a null one: nothing to check, whatever using says.
    let plain = json!({ "inMailbox": "M1", "where": null });
    assert!(!filter_uses_where(&plain));
    assert_eq!(
        check_where_filter(Some(&plain), "Email", &core_only, None),
        Ok(false)
    );
    assert_eq!(
        check_where_filter(None, "Email", &core_only, None),
        Ok(false)
    );

    // The first bad `where` anywhere in the tree is reported.
    let bad = json!({ "operator": "OR", "conditions": [{ "where": { "size": 1 } }] });
    let e = check_where_filter(Some(&bad), "Email", &using, Some(&c)).unwrap_err();
    assert_eq!(error_type(e), "invalidArguments");
}

/// §Comparison Operators: `$eq` "The field is present and equal to the
/// operand"; "`{"$eq": null}` is true only for an explicit `null`; a
/// missing field is tested with `$exists`"; `$ne` and `$nin` are "True when
/// the field is missing"; `$exists` "If `true`, the field is present (its
/// value may be `null`)".
#[test]
fn missing_null_and_exists() {
    let obj = json!({ "a": null, "b": 1 });
    assert!(eval(json!({ "/a": { "$eq": null } }), &obj));
    assert!(!eval(json!({ "/z": { "$eq": null } }), &obj));
    assert!(eval(json!({ "/a": { "$exists": true } }), &obj));
    assert!(eval(json!({ "/z": { "$exists": false } }), &obj));
    assert!(!eval(json!({ "/z": { "$exists": true } }), &obj));
    assert!(eval(json!({ "/z": { "$ne": 1 } }), &obj));
    assert!(eval(json!({ "/z": { "$nin": [1] } }), &obj));
    assert!(!eval(json!({ "/z": { "$in": [1] } }), &obj));
    assert!(!eval(json!({ "/z": { "$gt": 0 } }), &obj));
}

/// §Comparison Operators: equality by JSON type ("both are numbers with
/// the same numeric value"; "Values of different types are never equal and
/// never ordered"), and ordering of numbers and of strings.
#[test]
fn equality_and_ordering() {
    let obj = json!({ "n": 1, "f": 2.5, "s": "b", "o": { "x": [1, "y"] } });
    assert!(eval(json!({ "/n": 1.0 }), &obj));
    assert!(!eval(json!({ "/n": "1" }), &obj));
    assert!(!eval(json!({ "/n": true }), &obj));
    assert!(eval(json!({ "/o": { "x": [1, "y"] } }), &obj));
    assert!(!eval(json!({ "/o": { "x": [1] } }), &obj));
    assert!(eval(json!({ "/f": { "$gt": 2, "$lt": 3 } }), &obj));
    assert!(eval(json!({ "/n": { "$gte": 1, "$lte": 1 } }), &obj));
    assert!(eval(json!({ "/s": { "$gt": "a", "$lt": "c" } }), &obj));
    // A number never orders against a string.
    assert!(!eval(json!({ "/n": { "$lt": "z" } }), &obj));
    assert!(!eval(json!({ "/s": { "$gt": 0 } }), &obj));
    assert!(eval(json!({ "/s": { "$in": ["a", "b"] } }), &obj));
    assert!(!eval(json!({ "/s": { "$nin": ["a", "b"] } }), &obj));
}

/// §Arrays: a non-array operand is tested against each element; "`{"$ne":
/// "x"}` on an array field is true only if no element equals `"x"`";
/// "Elements are not searched recursively".
#[test]
fn array_rules() {
    let obj = json!({ "tags": ["red", "green"], "nested": [["red"]], "n": [1, 5] });
    assert!(eval(json!({ "/tags": "red" }), &obj));
    assert!(!eval(json!({ "/tags": { "$ne": "red" } }), &obj));
    assert!(eval(json!({ "/tags": { "$ne": "blue" } }), &obj));
    assert!(eval(json!({ "/tags": ["red", "green"] }), &obj));
    assert!(!eval(json!({ "/tags": ["green", "red"] }), &obj));
    assert!(eval(json!({ "/tags": { "$in": ["blue", "green"] } }), &obj));
    assert!(!eval(
        json!({ "/tags": { "$nin": ["blue", "green"] } }),
        &obj
    ));
    assert!(eval(json!({ "/tags": { "$prefix": "gr" } }), &obj));
    assert!(eval(json!({ "/tags": { "$contains": "ee" } }), &obj));
    assert!(eval(json!({ "/n": { "$gt": 4 } }), &obj));
    assert!(!eval(json!({ "/nested": "red" }), &obj));
}

/// §Logical Operators: "`$and` ... An empty array is true", "`$or` ... An
/// empty array is false", "`$not` True if the expression is false".
#[test]
fn logical_operators() {
    let obj = json!({ "a": 1 });
    assert!(eval(json!({ "$and": [] }), &obj));
    assert!(!eval(json!({ "$or": [] }), &obj));
    assert!(eval(json!({ "$not": { "/a": 2 } }), &obj));
    assert!(!eval(json!({ "$not": {} }), &obj));
    assert!(eval(json!({ "$or": [{ "/a": 2 }, { "/a": 1 }] }), &obj));
    assert!(!eval(json!({ "$and": [{ "/a": 1 }, { "/a": 2 }] }), &obj));
    // "The expression is true if every one of its members is true."
    assert!(!eval(json!({ "/a": 1, "$or": [] }), &obj));
}

/// §Collation: code points without `$collation`; with it "every string
/// operator in the same operator object, including `$eq`, `$ne`, `$in`, and
/// `$nin`, uses it"; "A plain value (the `$eq` shorthand) always uses
/// code-point comparison." i;ascii-casemap folds only ASCII letters (RFC
/// 4790 §9.2.1); i;unicode-casemap folds non-ASCII letters too (RFC 5051).
#[test]
fn collations() {
    let obj = json!({ "s": "Ünïcode Mail", "id": "Abc" });
    assert!(!eval(json!({ "/id": "abc" }), &obj));
    assert!(!eval(json!({ "/id": { "$eq": "abc" } }), &obj));
    let ascii = |op: &str, v: Value| {
        let mut t = serde_json::Map::new();
        t.insert(op.to_owned(), v);
        t.insert("$collation".to_owned(), json!("i;ascii-casemap"));
        json!({ "/id": t })
    };
    assert!(eval(ascii("$eq", json!("ABC")), &obj));
    assert!(eval(ascii("$in", json!(["x", "aBc"])), &obj));
    assert!(!eval(ascii("$ne", json!("abc")), &obj));
    assert!(!eval(ascii("$nin", json!(["ABC"])), &obj));
    assert!(eval(
        json!({ "/id": { "$eq": "Abc", "$collation": "i;octet" } }),
        &obj
    ));
    assert!(!eval(
        json!({ "/id": { "$eq": "abc", "$collation": "i;octet" } }),
        &obj
    ));
    // ASCII casemap leaves Ü/ü distinct; Unicode casemap does not.
    let s = |c: &str| json!({ "/s": { "$prefix": "üNÏ", "$collation": c } });
    assert!(!eval(s("i;ascii-casemap"), &obj));
    assert!(eval(s("i;unicode-casemap"), &obj));
    assert!(eval(
        json!({ "/s": { "$contains": "MAIL", "$collation": "i;unicode-casemap" } }),
        &obj
    ));
    assert!(!eval(json!({ "/s": { "$contains": "MAIL" } }), &obj));
}

/// §Full-Text Matching: `$text` is "server-defined", decided by the
/// candidate; the view's own text match is used.
#[test]
fn text_uses_the_candidate() {
    struct Fixed(Value, bool);
    impl Candidate for Fixed {
        fn field(&self, pointer: &str) -> Option<&Value> {
            self.0.pointer(pointer)
        }
        fn matches_text(&self, _text: &str) -> bool {
            self.1
        }
    }
    let expr = parse(json!({ "$text": "anything", "/a": 1 })).unwrap();
    assert!(expr.matches(&Fixed(json!({ "a": 1 }), true)));
    assert!(!expr.matches(&Fixed(json!({ "a": 1 }), false)));
    assert!(!expr.matches(&Fixed(json!({ "a": 2 }), true)));
    // The bare-JSON view matches a string value anywhere, ignoring case.
    let obj = json!({ "subject": "Quarterly Report", "to": [{ "name": "Zoë" }] });
    assert!(eval(json!({ "$text": "report" }), &obj));
    assert!(eval(json!({ "$text": "ZOË" }), &obj));
    assert!(!eval(json!({ "$text": "subject" }), &obj));
}

/// `to_value` round-trips through `parse` (the wire form of §Field Tests).
#[test]
fn to_value_round_trips() {
    let src = json!({
        "/a": { "$eq": { "$x": 1 } },
        "/b": { "$gt": 1, "$lte": "z", "$in": [1], "$nin": [2], "$exists": true,
                "$prefix": "p", "$contains": "c", "$collation": "i;octet", "$ne": null },
        "$and": [{ "/c": 1 }],
        "$or": [],
        "$not": { "$text": "t" }
    });
    let e = parse(src).unwrap();
    assert_eq!(parse(e.to_value()).unwrap(), e);
}
