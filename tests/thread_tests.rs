mod common;

use jmap_mail_types::Thread;
use jmap_types::Id;

#[test]
fn thread_deserializes_correctly() {
    let json = common::fixture("thread_example.json");
    let t: Thread = serde_json::from_str(&json).expect("deserialize thread_example.json");
    assert_eq!(t.id, Id::from("f123u4"));
    assert_eq!(t.email_ids.len(), 2);
    assert_eq!(t.email_ids[0], Id::from("eaa623"));
    assert_eq!(t.email_ids[1], Id::from("f782cbb"));
}

// Roundtrip tests compare serde_json::Value rather than the struct directly.
// This catches fields that serialize but are not reflected in PartialEq
// (e.g., a field present in JSON but missing from the struct), and avoids
// false passes from HashMap key-order non-determinism.
#[test]
fn thread_roundtrip() {
    let json = common::fixture("thread_example.json");
    let t: Thread = serde_json::from_str(&json).expect("deserialize");
    let serialized = serde_json::to_string(&t).expect("serialize");
    let original: serde_json::Value = serde_json::from_str(&json).expect("parse original");
    let reserialized: serde_json::Value =
        serde_json::from_str(&serialized).expect("parse reserialized");
    assert_eq!(original, reserialized);
}

#[test]
fn thread_email_ids_order_preserved() {
    let json = r#"{"id":"abc","emailIds":["z","a","m"]}"#;
    let t: Thread = serde_json::from_str(json).expect("deserialize");
    assert_eq!(t.email_ids[0], Id::from("z"));
    assert_eq!(t.email_ids[1], Id::from("a"));
    assert_eq!(t.email_ids[2], Id::from("m"));
}

#[test]
fn thread_wire_field_name_is_email_ids() {
    // Oracle: RFC 8621 §3 — wire name is emailIds
    let json = r#"{"id":"x","emailIds":["y"]}"#;
    let t: Thread = serde_json::from_str(json).expect("deserialize");
    let serialized = serde_json::to_string(&t).expect("serialize");
    assert!(
        serialized.contains("\"emailIds\""),
        "wire name must be emailIds, got: {}",
        serialized
    );
}

#[test]
fn thread_new_constructor() {
    use jmap_types::Id;
    let t: jmap_mail_types::Thread =
        serde_json::from_str(r#"{"id":"t1","emailIds":["e1","e2"]}"#).expect("deserialize thread");
    assert_eq!(t.id, Id::from("t1"));
    assert_eq!(t.email_ids.len(), 2);
}
