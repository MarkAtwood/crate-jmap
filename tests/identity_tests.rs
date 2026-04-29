mod common;

use jmap_mail_types::Identity;

// Roundtrip tests compare serde_json::Value rather than the struct directly.
// This catches fields that serialize but are not reflected in PartialEq
// (e.g., a field present in JSON but missing from the struct), and avoids
// false passes from HashMap key-order non-determinism.
#[test]
fn identity_full_roundtrip() {
    let json = common::fixture("identity_full.json");
    let id: Identity = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(id.name, "John Doe");
    assert_eq!(id.email, "john@example.com");
    assert!(id.reply_to.is_some());
    assert!(id.bcc.is_some());
    assert!(!id.may_delete);
    let serialized = serde_json::to_string(&id).expect("serialize");
    let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn identity_minimal_roundtrip() {
    let json = common::fixture("identity_minimal.json");
    let id: Identity = serde_json::from_str(&json).expect("deserialize");
    assert!(id.reply_to.is_none());
    assert!(id.bcc.is_none());
    let serialized = serde_json::to_string(&id).expect("serialize");
    // replyTo and bcc must not appear when absent
    assert!(
        !serialized.contains("\"replyTo\""),
        "replyTo must be absent when None"
    );
    assert!(
        !serialized.contains("\"bcc\""),
        "bcc must be absent when None"
    );
    // textSignature and htmlSignature MUST appear even when empty
    assert!(
        serialized.contains("\"textSignature\""),
        "textSignature must always be present"
    );
    assert!(
        serialized.contains("\"htmlSignature\""),
        "htmlSignature must always be present"
    );
}

#[test]
fn identity_wire_field_names() {
    // Oracle: RFC 8621 §6 — check camelCase wire names
    let json = common::fixture("identity_full.json");
    let id: Identity = serde_json::from_str(&json).expect("deserialize");
    let serialized = serde_json::to_string(&id).expect("serialize");
    assert!(serialized.contains("\"replyTo\""));
    assert!(serialized.contains("\"textSignature\""));
    assert!(serialized.contains("\"htmlSignature\""));
    assert!(serialized.contains("\"mayDelete\""));
}
