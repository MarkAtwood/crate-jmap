mod common;

use jmap_mail_types::SearchSnippet;

// Roundtrip tests compare serde_json::Value rather than the struct directly.
// This catches fields that serialize but are not reflected in PartialEq
// (e.g., a field present in JSON but missing from the struct), and avoids
// false passes from HashMap key-order non-determinism.
#[test]
fn snippet_with_matches_roundtrip() {
    let json = common::fixture("snippet_with_matches.json");
    let s: SearchSnippet = serde_json::from_str(&json).expect("deserialize");
    assert!(s.subject.is_some());
    assert!(s.preview.is_some());
    let serialized = serde_json::to_string(&s).expect("serialize");
    let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn snippet_no_matches_null_fields_omitted() {
    let json = common::fixture("snippet_no_matches.json");
    let s: SearchSnippet = serde_json::from_str(&json).expect("deserialize");
    assert!(s.subject.is_none());
    assert!(s.preview.is_none());
    let serialized = serde_json::to_string(&s).expect("serialize");
    // subject and preview must not appear in the serialized JSON
    assert!(
        !serialized.contains("\"subject\""),
        "subject must be absent when None"
    );
    assert!(
        !serialized.contains("\"preview\""),
        "preview must be absent when None"
    );
}

#[test]
fn snippet_has_no_id_field() {
    // Oracle: RFC 8621 §5 — SearchSnippet has NO id field
    let s: SearchSnippet =
        serde_json::from_str(r#"{"emailId":"Mx","subject":"hello","preview":"world"}"#)
            .expect("deserialize");
    let serialized = serde_json::to_string(&s).expect("serialize");
    // Must contain emailId but NOT id
    assert!(
        serialized.contains("\"emailId\""),
        "emailId must be present"
    );
    assert!(
        !serialized.contains("\"id\""),
        "id must NOT be present in SearchSnippet"
    );
}

#[test]
fn snippet_email_id_wire_name() {
    // Oracle: RFC 8621 §5 — wire name is emailId
    let json = r#"{"emailId":"Mabc123"}"#;
    let s: SearchSnippet = serde_json::from_str(json).expect("deserialize");
    let serialized = serde_json::to_string(&s).expect("serialize");
    assert!(
        serialized.contains("\"emailId\""),
        "emailId wire name must be correct"
    );
}

#[test]
fn snippet_new_constructor() {
    use jmap_types::Id;
    let s = jmap_mail_types::SearchSnippet::new(Id::from("Mabc"));
    assert_eq!(s.email_id, Id::from("Mabc"));
    assert!(s.subject.is_none());
    assert!(s.preview.is_none());
}
