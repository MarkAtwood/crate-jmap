mod common;

use jmap_mail_types::{EmailAddress, EmailAddressGroup, EmailBodyValue, EmailHeader};

// Roundtrip tests compare serde_json::Value rather than the struct directly.
// This catches fields that serialize but are not reflected in PartialEq
// (e.g., a field present in JSON but missing from the struct), and avoids
// false passes from HashMap key-order non-determinism.
#[test]
fn email_address_with_name_fixture_roundtrip() {
    let json = common::fixture("email_address_with_name.json");
    let addr: EmailAddress = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(addr.name.as_deref(), Some("John Doe"));
    assert_eq!(addr.email, "john@example.com");
    let reserialized = serde_json::to_string(&addr).expect("serialize");
    let v1: serde_json::Value = serde_json::from_str(&json).expect("parse original");
    let v2: serde_json::Value = serde_json::from_str(&reserialized).expect("parse reserialized");
    assert_eq!(v1, v2);
}

#[test]
fn email_address_no_name_fixture_no_name_key() {
    let json = common::fixture("email_address_no_name.json");
    let addr: EmailAddress = serde_json::from_str(&json).expect("deserialize");
    assert!(addr.name.is_none());
    let serialized = serde_json::to_string(&addr).expect("serialize");
    assert!(
        !serialized.contains("\"name\""),
        "name must be absent when None"
    );
}

#[test]
fn email_address_group_fixture_roundtrip() {
    let json = common::fixture("email_address_group.json");
    let g: EmailAddressGroup = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(g.addresses.len(), 2);
    let serialized = serde_json::to_string(&g).expect("serialize");
    let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn email_header_fixture_roundtrip() {
    let json = common::fixture("email_header.json");
    let h: EmailHeader = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(h.name, "Content-Type");
    let serialized = serde_json::to_string(&h).expect("serialize");
    let v1: serde_json::Value = serde_json::from_str(&json).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(v1, v2);
}

#[test]
fn email_body_value_fixture_roundtrip() {
    let json = common::fixture("email_body_value.json");
    let bv: EmailBodyValue = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(bv.value, "Hello, world!");
    assert!(!bv.is_encoding_problem);
    assert!(!bv.is_truncated);
    let serialized = serde_json::to_string(&bv).expect("serialize");
    assert!(serialized.contains("\"isEncodingProblem\""));
    assert!(serialized.contains("\"isTruncated\""));
}
